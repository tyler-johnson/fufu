//! Removing a linked worktree: fufu tears down the checkout and the
//! administrative directory, and touches nothing else.

use std::path::PathBuf;

use crate::error::{Error, Result};
use crate::ops::record::observe_refs;
use crate::ops::{CaptureOutcome, OpKind, OpRecord, verb};

/// A linked worktree fufu removed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Removed {
    pub id: String,
    /// Where the checkout stood, or `None` when the entry no longer named
    /// one — a worktree somebody deleted by hand leaves an administrative
    /// entry behind, and tearing that down is the normal way it gets cleaned
    /// up.
    pub path: Option<PathBuf>,
}

/// Delete a linked worktree's checkout and its administrative directory, and
/// nothing else.
///
/// This captures nothing and refuses nothing about the tree's contents; it
/// is the raw teardown, and the capture-before-courage belongs to the verb in
/// a later brief, so nobody wires it to a verb without one.
///
/// The chain at `refs/fufu/wt/<id>/ops` survives a removal on purpose — that
/// is the previous landing's guarantee, and the reason a deleted bay's work
/// stays reachable.
pub fn teardown(repo: &gix::Repository, id: &str) -> Result<Removed> {
    if id == crate::linked::MAIN_ID {
        return Err(Error::coded(
            "worktree/is-main",
            "the main worktree cannot be removed",
            vec!["git worktree list".into()],
        ));
    }

    let proxy = repo
        .worktrees()
        .map_err(Error::repo)?
        .into_iter()
        .find(|proxy| proxy.id() == id)
        .ok_or_else(|| {
            Error::coded(
                "worktree/not-found",
                format!("no linked worktree named {id}"),
                vec!["git worktree list".into()],
            )
        })?;

    // The checkout path, if the entry still names one. An entry whose
    // checkout is already gone is still torn down — that is the normal way a
    // stale entry gets cleaned up.
    let path = proxy.base().ok();

    if let Some(checkout) = &path {
        remove_dir_all_lenient(checkout)?;
    }
    remove_dir_all_lenient(&repo.common_dir().join("worktrees").join(id))?;

    Ok(Removed {
        id: id.to_string(),
        path,
    })
}

/// Remove a linked worktree as a recorded operation.
///
/// The earn over `git worktree remove`: the tree is captured into its own
/// chain before it is torn down, so a dirty bay's work survives the removal,
/// and the chain — with that capture at its tip — stays addressable after
/// the worktree is gone. That capture is what makes the removal recoverable
/// rather than merely recorded.
pub fn remove_worktree(
    repo: &gix::Repository,
    id: &str,
    prov: &crate::snapshot::Provenance,
    now: Option<i64>,
    argv: Vec<String>,
) -> Result<(
    crate::model::WorktreeRemoveReport,
    crate::ops::verb::VerbContext,
)> {
    let ctx = verb::begin_verb(repo, prov, now)?;
    let now = ctx.now;

    if id == crate::linked::MAIN_ID {
        return Err(Error::coded(
            "worktree/is-main",
            "the main worktree cannot be removed",
            vec!["git worktree list".into()],
        ));
    }
    if id == crate::linked::id(repo) {
        return Err(Error::coded(
            "worktree/is-current",
            format!("{id} is the worktree you are in; run this from another one"),
            vec!["ff worktree list".into()],
        ));
    }

    let proxy = repo
        .worktrees()
        .map_err(Error::repo)?
        .into_iter()
        .find(|proxy| proxy.id() == id)
        .ok_or_else(|| {
            Error::coded(
                "worktree/not-found",
                format!("no linked worktree named {id}"),
                vec!["git worktree list".into()],
            )
        })?;

    // What the effect will name, read before anything is destroyed.
    let branch = super::head_branch(&proxy.git_dir().join("HEAD"));
    let path = proxy.base().ok();

    // The capture, and the whole point of the verb: this is why it needs no
    // --force. git demands one for a dirty worktree because it has nowhere
    // to put the work, and fufu captured it a line ago — the tree's own
    // chain now holds it.
    let capture = match proxy.into_repo() {
        Ok(wt) => match crate::ops::capture_with(
            &wt,
            prov,
            &crate::snapshot::TakeOptions {
                now: Some(now),
                max_file_size: None,
            },
        )? {
            CaptureOutcome::Created { id, .. } => Some(id.to_string()),
            // A clean bay with a chain already holding its tree needs
            // nothing new; a bay with no operations at all yields `None`,
            // which is honest.
            CaptureOutcome::NoOp { tip, .. } => tip.map(|tip| tip.to_string()),
            CaptureOutcome::Contended => {
                return Err(Error::coded(
                    "worktree/busy",
                    format!("something is running in {id}: its operation log is locked"),
                    vec!["ff worktree list".into()],
                ));
            }
        },
        // The checkout is already gone from disk: nothing to capture, and
        // that is not an error. This is the stale-entry cleanup path.
        Err(_) => None,
    };

    // Here the append comes before the teardown, the ordinary write-ahead
    // way: everything the effect names is known before anything is
    // destroyed, so there is no reason to invert the order the add verb
    // must. A removal moves no ref of its own, and must not — the chain at
    // `refs/fufu/wt/<id>/ops` survives on purpose.
    let head = crate::head::head_state(repo)?;
    let chain = crate::snapshot::chain::chain_name(&head);
    let mut planned = observe_refs(repo)?;
    // The branch the removed worktree held becomes this worktree's to record
    // the moment the checkout is gone, because nobody holds it any more and
    // `observe_refs` excludes only branches somebody does. `planned` is the
    // planned END state, so it must say so here rather than leave the next
    // reconcile to find a branch it has no record of and call it a foreign
    // create — an absorption that would then stand between `ff undo` and the
    // removal it is trying to reverse.
    if let Some(branch) = &branch {
        let full = format!("refs/heads/{branch}");
        if let Some(tip) = crate::refs::ref_target(repo, &full)? {
            planned.refs.insert(full, tip.to_string());
        }
    }
    let mut record = OpRecord::new("worktree", format!("remove worktree {id}"), now);
    record.argv = argv;
    record.worktree = vec![crate::ops::record::WorktreeEffect::Remove {
        id: id.to_string(),
        path: path
            .as_ref()
            .map(|path| path.display().to_string())
            .unwrap_or_default(),
        branch: branch.clone(),
        capture: capture.clone(),
    }];
    let pins: Vec<gix::ObjectId> = Vec::new();
    verb::append_op(
        repo,
        OpKind::Op,
        verb::VerbOp {
            record,
            planned,
            // Removing a worktree leaves this worktree untouched.
            tree: ctx.pre_tree,
            index_tree: crate::index::tree_from_index(repo)?,
            branch: chain,
            base: crate::snapshot::chain::base_commit(&head)?,
            session: prov.session.clone(),
            pins: &pins,
        },
        now,
    )?;

    let removed = teardown(repo, id)?;

    Ok((
        crate::model::WorktreeRemoveReport {
            id: removed.id,
            path: removed.path,
            branch,
            capture,
            chain: crate::ops::ops_ref(id),
        },
        ctx,
    ))
}

/// `remove_dir_all`, where `NotFound` is success, not an error.
fn remove_dir_all_lenient(path: &std::path::Path) -> Result<()> {
    match std::fs::remove_dir_all(path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(Error::repo(err)),
    }
}
