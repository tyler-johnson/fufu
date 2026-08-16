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
        out.push(LogLine {
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
