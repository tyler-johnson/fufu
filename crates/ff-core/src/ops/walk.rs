//! Decoding one operation, and the two walks over them.
//!
//! Both walks follow a message trailer, never a parent slot. That is the fix
//! for the bug the journal shipped with: its slot 1 held a pin on the first
//! entry, so `git log --first-parent refs/fufu/journal` ran straight off the
//! root into the user's own commits and kept going. A stated link cannot do
//! that — it runs out.

use std::cell::OnceCell;

use crate::error::{Error, Result};
use crate::ops::id::{CommitId, OpId};
use crate::ops::message::{self, Skeleton};
use crate::ops::record::{OpRecord, RefsTable};
use crate::ops::{OpKind, is_fufu_commit};

/// One operation, decoded as far as the commit itself goes.
///
/// Everything above is one object read. Everything below — the ref table,
/// the machine record, the index tree — is fetched on demand, because
/// `ff op log` renders none of it and captures do not have most of it.
pub struct Operation<'r> {
    repo: &'r gix::Repository,
    id: OpId,
    tree: gix::ObjectId,
    time: i64,
    subject: String,
    skipped: Vec<String>,
    skeleton: Skeleton,
    record_id: Option<CommitId>,
    pins: Vec<CommitId>,
    record: OnceCell<Option<RecordParts>>,
    refs: OnceCell<Option<RefsTable>>,
}

struct RecordParts {
    record: OpRecord,
    index_tree: gix::ObjectId,
}

impl<'r> Operation<'r> {
    pub fn id(&self) -> OpId {
        self.id
    }

    pub fn kind(&self) -> OpKind {
        self.skeleton.kind
    }

    /// The commit subject: what ran, or which agent acted. Written by fufu,
    /// never by a person — the log is a machine's account of what happened.
    pub fn summary(&self) -> &str {
        &self.subject
    }

    /// Committer time, seconds since the unix epoch.
    pub fn time(&self) -> i64 {
        self.time
    }

    pub fn session(&self) -> Option<&str> {
        self.skeleton.session.as_deref()
    }

    /// The chain the op ran on: a branch name, or `@detached`.
    pub fn branch(&self) -> Option<&str> {
        self.skeleton.branch.as_deref()
    }

    /// HEAD's commit when the op ran — real history, in the ancestry.
    pub fn base(&self) -> Option<CommitId> {
        self.skeleton.base.map(CommitId::new)
    }

    /// The previous operation anywhere in the log.
    pub fn prev(&self) -> Option<OpId> {
        self.skeleton.prev.map(OpId::new)
    }

    /// The previous operation on this op's own branch.
    pub fn prev_on_branch(&self) -> Option<OpId> {
        self.skeleton.prev_on_branch.map(OpId::new)
    }

    /// The newest op of the segment before this one, when the op names it.
    pub fn prev_segment(&self) -> Option<message::SegmentLink> {
        self.skeleton.prev_segment
    }

    /// The worktree this op carries. Free: it is the commit's own tree.
    ///
    /// It is a *plan*, not an observation — the state the world should be in
    /// once the op completes, the same contract the refs table already
    /// carries under write-ahead. A crash leaves an op whose tree never
    /// fully materialized, and the next capture records the truth.
    pub fn tree(&self) -> gix::ObjectId {
        self.tree
    }

    /// Worktree files this op's capture dropped for exceeding
    /// `fufu.maxFileSize`.
    pub fn skipped(&self) -> &[String] {
        &self.skipped
    }

    /// Commits this op pinned against gc — reachability IS the pin.
    pub fn pins(&self) -> &[CommitId] {
        &self.pins
    }

    pub fn is_capture(&self) -> bool {
        self.skeleton.kind == OpKind::Capture
    }

    /// The blob this op names as the last-seen ref table — the value a
    /// following capture copies verbatim rather than observing.
    pub(crate) fn refs_blob_oid(&self) -> Option<gix::ObjectId> {
        self.skeleton.refs_blob
    }

    /// The ref table as fufu last saw it. One blob read, cached.
    ///
    /// A capture names its predecessor's blob verbatim rather than a fresh
    /// observation, and that is deliberate: an observing capture would let
    /// `git checkout other-branch` followed by a hook capture refresh the
    /// table and erase the foreign move from the log forever. The field
    /// means *last seen by fufu*, and a capture saw nothing.
    ///
    /// `None` only where no table exists yet — the log's first ops, before
    /// reconciliation has ever observed one.
    pub fn refs(&self) -> Result<Option<&RefsTable>> {
        if self.refs.get().is_none() {
            let table = match self.skeleton.refs_blob {
                None => None,
                Some(blob) => {
                    let data = self.repo.find_object(blob).map_err(Error::repo)?.detach();
                    let text = std::str::from_utf8(&data.data).map_err(|err| {
                        Error::coded(
                            "op/unreadable",
                            format!("{}: refs blob is not utf-8: {err}", self.id),
                            vec![],
                        )
                    })?;
                    Some(RefsTable::from_blob(text)?)
                }
            };
            let _ = self.refs.set(table);
        }
        Ok(self.refs.get().and_then(Option::as_ref))
    }

    /// The machine record. `None` for a capture, which has none.
    pub fn record(&self) -> Result<Option<&OpRecord>> {
        Ok(self.record_parts()?.map(|parts| &parts.record))
    }

    /// The index as it stood at the op, pinned by containment in the record.
    /// `None` for a capture: it writes no index and moves no ref.
    pub fn index_tree(&self) -> Result<Option<gix::ObjectId>> {
        Ok(self.record_parts()?.map(|parts| parts.index_tree))
    }

    fn record_parts(&self) -> Result<Option<&RecordParts>> {
        if self.record.get().is_none() {
            let parts = match self.record_id {
                None => None,
                Some(record_id) => Some(read_record(self.repo, self.id, record_id)?),
            };
            let _ = self.record.set(parts);
        }
        Ok(self.record.get().and_then(Option::as_ref))
    }
}

impl std::fmt::Debug for Operation<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Operation")
            .field("id", &self.id.to_string())
            .field("kind", &self.skeleton.kind)
            .field("subject", &self.subject)
            .finish_non_exhaustive()
    }
}

/// Whether a commit is an operation — the one guard, used everywhere.
///
/// It must be this and not "does it bear the fufu identity": a *record*
/// commit bears the identity too, and restoring from one would wipe the
/// worktree and write three metadata files in its place.
pub fn is_op_commit(repo: &gix::Repository, id: gix::ObjectId) -> Result<bool> {
    let Some(obj) = repo.try_find_object(id).map_err(Error::repo)? else {
        return Ok(false);
    };
    if obj.kind != gix::objs::Kind::Commit {
        return Ok(false);
    }
    let commit = gix::objs::CommitRef::from_bytes(&obj.data).map_err(Error::repo)?;
    Ok(is_fufu_commit(&commit)
        && message::parse(&String::from_utf8_lossy(commit.message)).is_some())
}

/// Decode an operation commit. Anything that is not one is `op/not-found`:
/// the guard and the decode are the same step, so no caller can skip it.
pub fn decode(repo: &gix::Repository, id: gix::ObjectId) -> Result<Operation<'_>> {
    let obj = repo
        .try_find_object(id)
        .map_err(Error::repo)?
        .filter(|obj| obj.kind == gix::objs::Kind::Commit)
        .ok_or_else(|| not_an_op(id))?;
    let commit = gix::objs::CommitRef::from_bytes(&obj.data).map_err(Error::repo)?;
    if !is_fufu_commit(&commit) {
        return Err(not_an_op(id));
    }
    let text = String::from_utf8_lossy(commit.message).into_owned();
    let skeleton = message::parse(&text).ok_or_else(|| not_an_op(id))?;
    let tree = commit.tree();
    let time = commit.committer.time().map_err(Error::repo)?.seconds;
    let parents: Vec<gix::ObjectId> = commit.parents().collect();
    drop(commit);
    drop(obj);

    // The parent layout, read positionally: [prev?] [base?] [record?] [pins…].
    // Slot 1 is reserved for the chain and is never a pin, which is what
    // keeps a first-parent walk inside the log. Each leading slot is
    // confirmed against the trailer that claims it, so a hand-edited commit
    // degrades to "no record, everything is a pin" rather than to a record
    // read on some user's commit.
    let mut at = 0usize;
    for claimed in [skeleton.prev, skeleton.base] {
        if let Some(claimed) = claimed
            && parents.get(at) == Some(&claimed)
        {
            at += 1;
        }
    }
    let record_id = (skeleton.kind != OpKind::Capture)
        .then(|| parents.get(at).copied().map(CommitId::new))
        .flatten();
    if record_id.is_some() {
        at += 1;
    }
    let pins = parents[at.min(parents.len())..]
        .iter()
        .copied()
        .map(CommitId::new)
        .collect();

    Ok(Operation {
        repo,
        id: OpId::new(id),
        tree,
        time,
        subject: message::subject_of(&text).to_string(),
        skipped: message::skipped_of(&text),
        skeleton,
        record_id,
        pins,
        record: OnceCell::new(),
        refs: OnceCell::new(),
    })
}

fn read_record(repo: &gix::Repository, op: OpId, record_id: CommitId) -> Result<RecordParts> {
    let unreadable = |what: &str| {
        Error::coded(
            "op/unreadable",
            format!(
                "{op}: record commit {} has no {what}",
                record_id.object_id()
            ),
            vec![],
        )
    };
    let commit = repo
        .find_commit(record_id.object_id())
        .map_err(Error::repo)?;
    let tree = commit.tree().map_err(Error::repo)?;
    let mut op_blob = None;
    let mut index_tree = None;
    for entry in tree.iter() {
        let entry = entry.map_err(Error::repo)?;
        match entry.filename().to_string().as_str() {
            "op.json" => op_blob = Some(entry.object_id()),
            "index" => index_tree = Some(entry.object_id()),
            _ => {}
        }
    }
    let op_blob = op_blob.ok_or_else(|| unreadable("op.json"))?;
    let index_tree = index_tree.ok_or_else(|| unreadable("index tree"))?;
    let data = repo.find_object(op_blob).map_err(Error::repo)?.detach();
    let record: OpRecord = serde_json::from_slice(&data.data).map_err(|err| {
        Error::coded("op/unreadable", format!("{op}: bad op.json: {err}"), vec![])
    })?;
    Ok(RecordParts { record, index_tree })
}

fn not_an_op(id: gix::ObjectId) -> Error {
    Error::coded(
        "op/not-found",
        format!("{id} is not a fufu operation"),
        vec!["ff op log".into()],
    )
}

/// One *run*: the granularity `ff undo` moves by, and the one `ff evolog`
/// collapses to.
///
/// A capture is a machine's granularity and a person's undo is not, so a run
/// is the longest stretch of adjacent captures carrying the same session,
/// ending at the first operation that is not one. The session is only the
/// equality test — no session compares equal to no session — so a run is a
/// fact about adjacency, never a range a tag defines, which is what keeps
/// sessions tags.
///
/// Only captures group. A verb's operation is a decision somebody made, so it
/// is always its own run and always ends one; that is also what keeps undo
/// from stepping past a commit by accident.
#[derive(Debug, Clone)]
pub struct Run {
    /// The newest operation in the run — where it was entered from.
    pub tip: OpId,
    /// The oldest operation in the run.
    pub oldest: OpId,
    /// How many operations the run collapsed. A keystroke that moved forty
    /// operations must not have to be inferred, so this is reported.
    pub len: usize,
    /// The operation before the run: where undo lands. `None` at the floor.
    pub prev: Option<OpId>,
    /// The tag every member shares. `None` both for an untagged capture run
    /// and for a verb operation, which never groups in any case.
    pub session: Option<String>,
    /// What the run collapsed: captures, or the one operation it consists of.
    pub kind: OpKind,
}

impl Run {
    /// Whether the run collapsed more than the operation it started from.
    pub fn collapsed(&self) -> bool {
        self.len > 1
    }
}

/// The run `tip` belongs to, walking backwards from it.
///
/// Note what this does *not* special-case: a verb's own pre-capture sits at
/// the tip when the verb ran on a dirty tree, so `ff undo` finds its own
/// capture at the head of the run it is about to undo, and takes it along.
/// That is deliberate — DESIGN puts the pre-undo capture at the head of the
/// abandoned branch precisely so redo hands your held work back first.
pub fn run_at(repo: &gix::Repository, tip: OpId) -> Result<Run> {
    let op = decode(repo, tip.object_id())?;
    let kind = op.kind();
    let session = op.session().map(str::to_string);
    if kind != OpKind::Capture {
        return Ok(Run {
            tip,
            oldest: tip,
            len: 1,
            prev: op.prev(),
            session,
            kind,
        });
    }
    let (mut oldest, mut len, mut prev) = (tip, 1usize, op.prev());
    while let Some(id) = prev {
        let older = decode(repo, id.object_id())?;
        // `Option == Option` is the whole rule: `None == None` groups two
        // untagged captures, and no tag ever bridges to an untagged one.
        if !older.is_capture() || older.session().map(str::to_string) != session {
            break;
        }
        oldest = id;
        len += 1;
        prev = older.prev();
    }
    Ok(Run {
        tip,
        oldest,
        len,
        prev,
        session,
        kind,
    })
}

/// Which link a walk follows. Both are stated in the message; neither is a
/// parent slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Follow {
    /// `fufu-prev` — every operation, newest first.
    Log,
    /// `fufu-prev-branch` — one branch's operations, newest first.
    Branch,
}

/// A lazy walk. Nothing is decoded until it is asked for, so a caller can
/// `.take(25)` or filter the captures out without paying for the rest — the
/// discipline an eager `read_ops(repo, limit)` left to whoever remembered.
pub(crate) struct OpWalk<'r> {
    repo: &'r gix::Repository,
    next: Option<gix::ObjectId>,
    follow: Follow,
    /// A failure reading the ref the walk starts from. Carried rather than
    /// swallowed: an empty iterator and an unreadable log look identical to
    /// a caller, and only one of them is news.
    start: Option<Error>,
}

impl<'r> OpWalk<'r> {
    pub(crate) fn new(
        repo: &'r gix::Repository,
        from: Result<Option<OpId>>,
        follow: Follow,
    ) -> Self {
        let (next, start) = match from {
            Ok(from) => (from.map(|id| id.object_id()), None),
            Err(err) => (None, Some(err)),
        };
        OpWalk {
            repo,
            next,
            follow,
            start,
        }
    }
}

impl<'r> Iterator for OpWalk<'r> {
    type Item = Result<Operation<'r>>;

    fn next(&mut self) -> Option<Self::Item> {
        if let Some(err) = self.start.take() {
            return Some(Err(err));
        }
        let id = self.next.take()?;
        match decode(self.repo, id) {
            Err(err) => Some(Err(err)),
            Ok(op) => {
                self.next = match self.follow {
                    Follow::Log => op.prev(),
                    Follow::Branch => op.prev_on_branch(),
                }
                .map(|id| id.object_id());
                Some(Ok(op))
            }
        }
    }
}
