//! Floor 1 — continuous capture. Every working-tree state becomes an
//! ordinary git commit on a hidden per-branch ref (`refs/fufu/snap/<branch>`),
//! written natively: zero spawns, and the index is never written.

pub mod chain;
pub mod config;
pub mod message;
mod tree;

use gix::prelude::ObjectIdExt;
use gix::refs::transaction::{Change, LogChange, PreviousValue, RefEdit, RefLog};

use crate::error::{Error, Result};
use crate::model::SnapOutcome;

/// Where a snapshot came from; formatted as the commit subject
/// (`source` or `source: detail`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Provenance {
    /// `manual`, `pre`, `claude[<sess8>]`, …
    pub source: String,
    /// The command, message, or tool detail, if any.
    pub detail: Option<String>,
}

impl Provenance {
    pub fn new(source: impl Into<String>, detail: Option<String>) -> Self {
        Provenance {
            source: source.into(),
            detail,
        }
    }

    /// The raw subject line (before whitespace collapsing / capping).
    pub fn subject(&self) -> String {
        match &self.detail {
            Some(detail) if !detail.is_empty() => format!("{}: {}", self.source, detail),
            _ => self.source.clone(),
        }
    }
}

/// Injection points for tests; production uses the defaults.
#[derive(Debug, Clone, Default)]
pub struct TakeOptions {
    /// Commit/reflog timestamp; `None` = the wall clock.
    pub now: Option<i64>,
    /// Oversize cutoff in bytes; `None` = `fufu.maxFileSize` (default 50 MiB).
    pub max_file_size: Option<u64>,
}

/// Take a snapshot of the working tree. See [`take_with`].
pub fn take(repo: &gix::Repository, prov: &Provenance) -> Result<SnapOutcome> {
    take_with(repo, prov, &TakeOptions::default())
}

/// Take a snapshot: capture the working tree (staged + unstaged + untracked,
/// exactly `add -A`'s selection) as a commit on the branch's chain ref.
///
/// Read-only until the single CAS ref edit; the index is never written, HEAD
/// is never opened for writing. A lost CAS race or a held lock reports
/// [`SnapOutcome::Contended`] — never blocks, never retries. A crash leaves at
/// worst orphan objects for gc.
pub fn take_with(
    repo: &gix::Repository,
    prov: &Provenance,
    opts: &TakeOptions,
) -> Result<SnapOutcome> {
    if repo.workdir().is_none() {
        return Err(Error::msg(
            "bare repository: capture requires a working tree",
        ));
    }
    let head = crate::head::head_state(repo)?;
    let chain_ref_name = chain::chain_ref(&head);
    let base = chain::base_commit(&head)?;

    // Previous tip — the exact value CAS must later see again.
    let prev: Option<gix::ObjectId> = match repo
        .try_find_reference(chain_ref_name.as_str())
        .map_err(Error::repo)?
    {
        Some(r) => match r.target().try_id() {
            Some(id) => Some(id.to_owned()),
            None => {
                return Err(Error::msg(format!(
                    "{chain_ref_name} is symbolic; fufu writes only direct refs"
                )));
            }
        },
        None => None,
    };
    let prev_tree: Option<gix::ObjectId> = prev
        .map(|id| {
            repo.find_commit(id)
                .map_err(Error::repo)?
                .tree_id()
                .map_err(Error::repo)
                .map(|t| t.detach())
        })
        .transpose()?;
    let head_tree = repo.head_tree_id_or_empty().map_err(Error::repo)?.detach();

    let scan = tree::scan(repo)?;
    let (tree_id, skipped) = if scan.is_empty() {
        // Tier-1: the capture tree IS the head tree — zero object writes.
        match (prev, prev_tree) {
            (None, _) => {
                return Ok(SnapOutcome::NoOp {
                    r#ref: chain_ref_name,
                    tip: None,
                });
            }
            (Some(p), Some(pt)) if pt == head_tree => {
                return Ok(SnapOutcome::NoOp {
                    r#ref: chain_ref_name,
                    tip: Some(p.to_string()),
                });
            }
            // The user committed since the last snapshot: record the
            // post-commit state so the timeline stays continuous.
            _ => (head_tree, Vec::new()),
        }
    } else {
        let max = opts
            .max_file_size
            .unwrap_or_else(|| config::max_file_size(repo));
        tree::assemble(repo, head_tree, &scan, max)?
    };

    // Tier-2: built the tree, but it equals the previous snapshot's (or the
    // head's, when no chain exists). Orphan blobs written above are gc-able.
    let noop_against = prev_tree.unwrap_or(head_tree);
    if tree_id == noop_against {
        return Ok(SnapOutcome::NoOp {
            r#ref: chain_ref_name,
            tip: prev.map(|p| p.to_string()),
        });
    }

    let now = opts.now.unwrap_or_else(|| {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0)
    });
    let subject = message::clean_subject(&prov.subject(), message::MAX_SUBJECT);
    let msg = message::build(&prov.subject(), &skipped);
    let changed_files = count_file_changes(repo, prev_tree.unwrap_or(head_tree), tree_id)?;

    // Parent order is load-bearing: parent 1 = previous snapshot (first-parent
    // walk = timeline), parent 2 = HEAD commit (base edge).
    let parents: Vec<gix::ObjectId> = match (prev, base) {
        (Some(p), Some(b)) => vec![p, b],
        (Some(p), None) => vec![p],
        (None, Some(b)) => vec![b],
        (None, None) => Vec::new(),
    };
    let sig = gix::actor::Signature {
        name: chain::FUFU_NAME.into(),
        email: chain::FUFU_EMAIL.into(),
        time: gix::date::Time {
            seconds: now,
            offset: 0,
        },
    };
    let commit = gix::objs::Commit {
        tree: tree_id,
        parents: parents.into(),
        author: sig.clone(),
        committer: sig,
        encoding: None,
        message: msg.into(),
        extra_headers: Vec::new(),
    };
    let commit_id = repo.write_object(&commit).map_err(Error::repo)?.detach();

    // Single-ref CAS transaction. Custom namespaces get no reflog by default:
    // force_create_reflog is mandatory, and silently absent otherwise.
    let time_str = format!("{now} +0000");
    let edit = RefEdit {
        change: Change::Update {
            log: LogChange {
                mode: RefLog::AndReference,
                force_create_reflog: true,
                message: subject.clone().into(),
            },
            expected: match prev {
                Some(p) => PreviousValue::MustExistAndMatch(gix::refs::Target::Object(p)),
                None => PreviousValue::MustNotExist,
            },
            new: gix::refs::Target::Object(commit_id),
        },
        name: chain_ref_name.as_str().try_into().map_err(Error::repo)?,
        deref: false,
    };
    let committer = gix::actor::SignatureRef {
        name: chain::FUFU_NAME.into(),
        email: chain::FUFU_EMAIL.into(),
        time: &time_str,
    };
    match repo.edit_references_as(Some(edit), Some(committer)) {
        Ok(_) => {}
        Err(err) if is_contended(&err) => {
            return Ok(SnapOutcome::Contended {
                r#ref: chain_ref_name,
            });
        }
        Err(err) => return Err(Error::repo(err)),
    }

    let mut warnings = Vec::new();
    if prev.is_none() {
        // First snapshot on this chain: guard the namespace against gc, once.
        if let Err(err) = config::ensure_gc_config(repo) {
            warnings.push(format!("could not write gc config guard: {err}"));
        }
    }

    let short_id = commit_id
        .attach(repo)
        .shorten()
        .map(|p| p.to_string())
        .unwrap_or_else(|_| commit_id.to_string()[..7].to_string());
    Ok(SnapOutcome::Created {
        id: commit_id.to_string(),
        short_id,
        r#ref: chain_ref_name,
        changed_files,
        skipped_files: skipped,
        warnings,
    })
}

/// Contention is a skip, not an error: a held lock or a lost CAS race.
fn is_contended(err: &gix::reference::edit::Error) -> bool {
    use gix::refs::file::transaction::prepare::Error as Prepare;
    match err {
        gix::reference::edit::Error::FileTransactionPrepare(err) => matches!(
            err,
            Prepare::LockAcquire { .. }
                | Prepare::MustNotExist { .. }
                | Prepare::MustExist { .. }
                | Prepare::ReferenceOutOfDate { .. }
        ),
        _ => false,
    }
}

/// Count file-level changes between two trees (subtree rows excluded).
fn count_file_changes(
    repo: &gix::Repository,
    lhs: gix::ObjectId,
    rhs: gix::ObjectId,
) -> Result<usize> {
    if lhs == rhs {
        return Ok(0);
    }
    let lhs_obj = repo.find_object(lhs).map_err(Error::repo)?.detach();
    let rhs_obj = repo.find_object(rhs).map_err(Error::repo)?.detach();
    let mut recorder = gix::diff::tree::Recorder::default();
    gix::diff::tree(
        gix::objs::TreeRefIter::from_bytes(&lhs_obj.data),
        gix::objs::TreeRefIter::from_bytes(&rhs_obj.data),
        gix::diff::tree::State::default(),
        &repo.objects,
        &mut recorder,
    )
    .map_err(Error::repo)?;
    use gix::diff::tree::recorder::Change as Rec;
    Ok(recorder
        .records
        .iter()
        .filter(|change| {
            let mode = match change {
                Rec::Addition { entry_mode, .. }
                | Rec::Deletion { entry_mode, .. }
                | Rec::Modification { entry_mode, .. } => entry_mode,
            };
            !mode.is_tree()
        })
        .count())
}
