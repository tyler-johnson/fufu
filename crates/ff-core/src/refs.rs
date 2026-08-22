//! Shared ref-edit machinery. Every fufu ref write funnels through here:
//! CAS expectations, forced reflogs (custom namespaces get none by default,
//! and silently so), and the fufu identity on the reflog line.

use gix::refs::transaction::{Change, LogChange, PreviousValue, RefEdit, RefLog};

use crate::error::{Error, Result};
use crate::ops::{FUFU_EMAIL, FUFU_NAME};

/// The reflog committer for fufu's own ref writes.
pub(crate) fn committer_ref(time_str: &str) -> gix::actor::SignatureRef<'_> {
    gix::actor::SignatureRef {
        name: FUFU_NAME.into(),
        email: FUFU_EMAIL.into(),
        time: time_str,
    }
}

/// Build an update edit with a forced reflog line.
pub(crate) fn update_edit(
    name: &str,
    target: gix::ObjectId,
    expected: PreviousValue,
    message: &str,
) -> Result<RefEdit> {
    Ok(RefEdit {
        change: Change::Update {
            log: LogChange {
                mode: RefLog::AndReference,
                force_create_reflog: true,
                message: message.into(),
            },
            expected,
            new: gix::refs::Target::Object(target),
        },
        name: name.try_into().map_err(Error::repo)?,
        deref: false,
    })
}

/// Build a delete edit expecting an exact current value. The reflog file is
/// removed with the ref.
pub(crate) fn delete_edit(name: &str, expected: gix::ObjectId) -> Result<RefEdit> {
    Ok(RefEdit {
        change: Change::Delete {
            expected: PreviousValue::MustExistAndMatch(gix::refs::Target::Object(expected)),
            log: RefLog::AndReference,
        },
        name: name.try_into().map_err(Error::repo)?,
        deref: false,
    })
}

/// Whether an edit failure is contention — a held lock or a lost CAS race —
/// rather than a real error.
pub(crate) fn is_contended(err: &gix::reference::edit::Error) -> bool {
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

/// One atomic transaction over any number of edits, committed as fufu.
/// Contention is reported as a value, not an error, so callers can skip or
/// retry per their own policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EditOutcome {
    Applied,
    Contended,
}

pub(crate) fn commit_edits(
    repo: &gix::Repository,
    edits: impl IntoIterator<Item = RefEdit>,
    time: i64,
) -> Result<EditOutcome> {
    let time_str = format!("{time} +0000");
    commit_edits_as(repo, edits, committer_ref(&time_str))
}

/// Like [`commit_edits`], but with an explicit reflog committer — for lines
/// that must carry the user's identity (the stash reflog) or replay a
/// preserved one.
pub(crate) fn commit_edits_as(
    repo: &gix::Repository,
    edits: impl IntoIterator<Item = RefEdit>,
    committer: gix::actor::SignatureRef<'_>,
) -> Result<EditOutcome> {
    match repo.edit_references_as(edits, Some(committer)) {
        Ok(_) => Ok(EditOutcome::Applied),
        Err(err) if is_contended(&err) => Ok(EditOutcome::Contended),
        Err(err) => Err(Error::repo(err)),
    }
}

/// Update one ref, treating contention as a hard error (callers that can
/// gracefully skip use [`commit_edits`] directly).
pub(crate) fn write_ref(
    repo: &gix::Repository,
    name: &str,
    target: gix::ObjectId,
    expected: PreviousValue,
    time: i64,
    message: &str,
) -> Result<()> {
    match commit_edits(
        repo,
        Some(update_edit(name, target, expected, message)?),
        time,
    )? {
        EditOutcome::Applied => Ok(()),
        EditOutcome::Contended => Err(Error::coded(
            "ref/contended",
            format!("could not update {name}: contended"),
            vec![],
        )),
    }
}

/// Delete one ref that must currently equal `expected`.
pub(crate) fn delete_ref(
    repo: &gix::Repository,
    name: &str,
    expected: gix::ObjectId,
    time: i64,
) -> Result<()> {
    match commit_edits(repo, Some(delete_edit(name, expected)?), time)? {
        EditOutcome::Applied => Ok(()),
        EditOutcome::Contended => Err(Error::coded(
            "ref/contended",
            format!("could not delete {name}: contended"),
            vec![],
        )),
    }
}

/// One reflog line, preserved verbatim for replay.
pub(crate) struct LogLine {
    /// Where the ref stood before this line, `None` when it was created here.
    /// This is what `ff redo` walks forward along: the reflog answers where
    /// you have stood, and the previous column is the only record of it.
    pub previous: Option<gix::ObjectId>,
    pub new: gix::ObjectId,
    pub name: String,
    pub email: String,
    /// Raw `<seconds> <offset>` text.
    pub time_str: String,
    pub message: String,
}

/// Read a ref's reflog, oldest→newest. Absent ref or absent log → empty.
pub(crate) fn read_ref_log(repo: &gix::Repository, name: &str) -> Result<Vec<LogLine>> {
    let Some(reference) = repo.try_find_reference(name).map_err(Error::repo)? else {
        return Ok(Vec::new());
    };
    let mut platform = reference.log_iter();
    let Some(iter) = platform.all().map_err(Error::repo)? else {
        return Ok(Vec::new());
    };
    let mut out = Vec::new();
    for line in iter {
        let line = line.map_err(Error::repo)?;
        let previous = gix::ObjectId::from_hex(line.previous_oid).map_err(Error::repo)?;
        out.push(LogLine {
            previous: (!previous.is_null()).then_some(previous),
            new: gix::ObjectId::from_hex(line.new_oid).map_err(Error::repo)?,
            name: line.signature.name.to_string(),
            email: line.signature.email.to_string(),
            time_str: line.signature.time.to_string(),
            message: line.message.to_string(),
        });
    }
    Ok(out)
}

/// Create `name` at `target` with `lines` replayed as its reflog, original
/// identities and times preserved. Each line is one transaction; the
/// previous-value column is derived from the CAS chain, so a dropped line's
/// gap closes naturally. Constraints inherited from gix: a line whose old
/// and new values are equal is silently dropped, and the first line's
/// previous value is written as null. If the last line does not land on
/// `target` (a pruned or diverged log), one final fufu-identity line closes
/// the gap.
pub(crate) fn create_ref_with_log(
    repo: &gix::Repository,
    name: &str,
    target: gix::ObjectId,
    lines: &[LogLine],
    now: i64,
    closing_message: &str,
) -> Result<()> {
    let mut expected = PreviousValue::MustNotExist;
    let mut at: Option<gix::ObjectId> = None;
    for line in lines {
        let edit = update_edit(name, line.new, expected.clone(), &line.message)?;
        let sig = gix::actor::SignatureRef {
            name: line.name.as_str().into(),
            email: line.email.as_str().into(),
            time: &line.time_str,
        };
        match commit_edits_as(repo, Some(edit), sig)? {
            EditOutcome::Applied => {}
            EditOutcome::Contended => {
                return Err(Error::coded(
                    "ref/contended",
                    format!("{name} is contended during replay"),
                    vec![],
                ));
            }
        }
        expected = PreviousValue::MustExistAndMatch(gix::refs::Target::Object(line.new));
        at = Some(line.new);
    }
    if at != Some(target) {
        write_ref(repo, name, target, expected, now, closing_message)?;
    }
    Ok(())
}

/// The user identity from config/environment, with the timestamp pinned to
/// `now`. Unset identity is fatal by design — fufu's own machinery uses the
/// fufu identity, but commits and stashes belong to the user.
pub(crate) fn user_signature(repo: &gix::Repository, now: i64) -> Result<gix::actor::Signature> {
    let sig = repo
        .committer()
        .transpose()
        .map_err(Error::repo)?
        .ok_or_else(|| {
            Error::coded(
                "identity/missing",
                "no identity configured: set user.name and user.email (git config)",
                vec![
                    "git config user.name <name>".into(),
                    "git config user.email <email>".into(),
                ],
            )
        })?;
    Ok(gix::actor::Signature {
        name: sig.name.to_owned(),
        email: sig.email.to_owned(),
        time: gix::date::Time {
            seconds: now,
            offset: 0,
        },
    })
}

/// The direct target of a ref, if the ref exists (symbolic refs error).
pub(crate) fn ref_target(repo: &gix::Repository, name: &str) -> Result<Option<gix::ObjectId>> {
    match repo.try_find_reference(name).map_err(Error::repo)? {
        Some(r) => match r.target().try_id() {
            Some(id) => Ok(Some(id.to_owned())),
            None => Err(Error::msg(format!(
                "{name} is symbolic; fufu writes only direct refs"
            ))),
        },
        None => Ok(None),
    }
}

/// The ref a branch-ish name denotes, with its target: `refs/heads/<name>`
/// first, then `refs/remotes/<name>`. Two lookups rather than gix's full
/// partial-name ladder, because the callers here hold a name they already
/// know is a branch's — someone else's counts, a tag does not.
pub(crate) fn branchish(
    repo: &gix::Repository,
    name: &str,
) -> Result<Option<(String, gix::ObjectId)>> {
    for full in [format!("refs/heads/{name}"), format!("refs/remotes/{name}")] {
        if let Some(id) = ref_target(repo, &full)? {
            return Ok(Some((full, id)));
        }
    }
    Ok(None)
}

/// Is a ref symbolic — does its target name another ref rather than an
/// object? A ref that does not exist is not symbolic. `ref_target` and
/// `branchish` both raise on a symbolic ref, where a caller may only want
/// to know "not a branch"; this one answers instead of raising.
/// `<remote>/HEAD` is the ref that makes that necessary: every clone leaves
/// one behind, and it is symbolic.
pub(crate) fn is_symbolic(repo: &gix::Repository, name: &str) -> Result<bool> {
    match repo.try_find_reference(name).map_err(Error::repo)? {
        Some(reference) => Ok(reference.target().try_name().is_some()),
        None => Ok(false),
    }
}

/// Does this remote hold any tracking ref at all?
///
/// The cheap half of "was there ever a shared copy here". A clone of a
/// non-empty remote always has some ref under `refs/remotes/<remote>/`; a
/// clone of an empty one has none, which is the fresh-clone case that used
/// to report a loss nobody had suffered. Any iteration failure answers no,
/// which routes the caller to the log's own memory rather than to a guess.
pub(crate) fn any_remote_ref(repo: &gix::Repository, remote: &str) -> Result<bool> {
    let prefix = format!("refs/remotes/{remote}/");
    let platform = repo.references().map_err(Error::repo)?;
    let Ok(mut iter) = platform.prefixed(prefix.as_str()) else {
        return Ok(false);
    };
    Ok(iter.next().is_some())
}

/// One branch a remote holds: the name every other surface uses for it, and
/// the remote it lives on.
pub(crate) struct RemoteRef {
    /// `origin/feature` — gix's own `shorten()`, the spelling every other
    /// surface uses.
    pub name: String,
    /// `origin`
    pub remote: String,
    pub tip: gix::ObjectId,
}

/// The branches a remote holds, as the tracking refs under `refs/remotes/`.
///
/// Every clone leaves one more ref behind, `<remote>/HEAD`, which is
/// symbolic and is not a branch. The walk must skip it by its target — and
/// cannot ask `branchish` or `ref_target` to do so, because both raise on a
/// symbolic ref where this walk must answer "skip". A name that does not
/// split into a remote and a branch is not a branch either.
pub(crate) fn remote_branches(repo: &gix::Repository) -> Result<Vec<RemoteRef>> {
    let platform = repo.references().map_err(Error::repo)?;
    let Ok(iter) = platform.prefixed("refs/remotes/") else {
        return Ok(Vec::new());
    };
    let mut out = Vec::new();
    for reference in iter {
        let reference = reference.map_err(|err| {
            Error::coded(
                "op/unreadable",
                format!("ref iteration failed: {err}"),
                vec![],
            )
        })?;
        let target = reference.target();
        if target.try_name().is_some() {
            continue;
        }
        let Some(tip) = target.try_id() else {
            continue;
        };
        let name = reference.name().shorten().to_string();
        let Some(remote) = name.split_once('/').map(|(remote, _)| remote.to_string()) else {
            continue;
        };
        out.push(RemoteRef {
            name,
            remote,
            tip: tip.to_owned(),
        });
    }
    Ok(out)
}
