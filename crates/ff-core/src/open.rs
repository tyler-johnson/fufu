//! The open commit: the open change as a real commit object, jj's
//! working-copy commit.
//!
//! Whenever HEAD is on a branch and the branch's newest operation left a tree
//! that differs from HEAD's, that tree is a commit: the operation's tree over
//! HEAD's commit, the user as author at the change's birth and as committer
//! at the operation, the pending description as message, the change id as a
//! header. It is what the `@` row's sha names, and `ff commit` moves the
//! branch onto it rather than minting another — so the sha on the `@` row is
//! the sha on the `●` row after the close.
//!
//! It is pinned twice. `refs/fufu/open/<branch>` names the current one and
//! is deleted when there is none; every operation on the branch also carries
//! the open commit it states as its last parent slot, so the log pins it for
//! as long as the operation lives, and a branch's trash pointer keeps it
//! reachable after `ff branch -d`. The ref is derived state — moved by
//! captures, rebuilt by undo — and is deliberately not a tracked prefix in
//! the ref table, so it can never read as foreign motion.
//!
//! This module is the one writer. Every operation goes through
//! [`crate::ops::append::commit_op`], which plans the open commit for the
//! branch the op leaves you on and moves the ref in the same transaction as
//! the two ref edits; undo and redo resync from the state they land on.
//!
//! Sha reuse is the property everything else rests on: when the ref already
//! names a commit whose tree, parent, author, message and id equal the plan,
//! that sha is kept and nothing is written. A park and resume, a switch there
//! and back, an unchanged tree captured a thousand times — one sha.

use crate::branchmeta;
use crate::changeid::{self, ChangeId};
use crate::error::{Error, Result};
use crate::ops::record::OpRecord;
use crate::ops::{OpLog, walk};
use crate::refs;

/// Where each branch's open commit is named.
pub const OPEN_PREFIX: &str = "refs/fufu/open/";

/// The ref naming `branch`'s open commit.
pub fn open_ref(branch: &str) -> String {
    format!("{OPEN_PREFIX}{branch}")
}

/// Everything an open commit is, before it is written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct OpenPlan {
    pub tree: gix::ObjectId,
    /// HEAD's commit; none on an unborn branch.
    pub parent: Option<gix::ObjectId>,
    pub change_id: ChangeId,
    /// The author time.
    pub born: i64,
    /// The pending description through the close's normalization, or empty.
    pub message: String,
}

/// What a verb's record says about the destination branch's open change,
/// laid over the branch's metadata: the op is planned write-ahead, so the
/// metadata on disk still describes the world before it.
#[derive(Debug, Clone, Default)]
pub(crate) struct Overlay {
    /// The description the op leaves, when the op changes it.
    pub description: Option<Option<String>>,
    /// The id and birth the op leaves, when the op changes them.
    pub change_id: Option<(Option<String>, Option<i64>)>,
}

impl Overlay {
    /// The record's transitions that land on `branch`.
    pub(crate) fn of(record: Option<&OpRecord>, branch: &str) -> Self {
        let Some(record) = record else {
            return Overlay::default();
        };
        Overlay {
            description: record
                .description
                .as_ref()
                .filter(|d| d.branch == branch)
                .map(|d| d.new.clone()),
            change_id: record
                .change_id
                .as_ref()
                .filter(|c| c.branch == branch)
                .map(|c| (c.new.clone(), c.new_born)),
        }
    }
}

/// The plan for a branch's open commit, given the tree an op leaves and the
/// HEAD it leaves it on. `None` when the tree is HEAD's, HEAD is detached,
/// nothing wears an id, or there is no identity to author it: a capture
/// never fails over a missing `user.name`, it states `none`.
///
/// `now` is the author time only when the id has no recorded birth — an id
/// minted before births were kept — and the next capture records one.
pub(crate) fn plan(
    repo: &gix::Repository,
    branch: &str,
    tree: gix::ObjectId,
    head: Option<gix::ObjectId>,
    overlay: &Overlay,
    now: i64,
) -> Result<Option<OpenPlan>> {
    if branch == crate::snapshot::chain::DETACHED {
        return Ok(None);
    }
    let head_tree = match head {
        Some(commit) => repo
            .find_commit(commit)
            .map_err(Error::repo)?
            .tree_id()
            .map_err(Error::repo)?
            .detach(),
        None => gix::ObjectId::empty_tree(repo.object_hash()),
    };
    if tree == head_tree {
        return Ok(None);
    }
    let meta = branchmeta::read(repo, branch)?;
    // Inside an editing session the content is an amendment of the commit
    // under it, whose id and birth the landing keeps.
    let (letters, born) = match &meta.session {
        Some(session) => {
            let at = gix::ObjectId::from_hex(session.at.as_bytes()).map_err(Error::repo)?;
            let commit = repo.find_commit(at).map_err(Error::repo)?;
            let id = changeid::of_commit(&commit.data, &commit.id).letters();
            let born = commit
                .author()
                .map_err(Error::repo)?
                .time()
                .map_err(Error::repo)?
                .seconds;
            (Some(id), Some(born))
        }
        None => match &overlay.change_id {
            Some((id, born)) => (id.clone(), *born),
            None => (meta.change_id.clone(), meta.change_born),
        },
    };
    let Some(letters) = letters else {
        return Ok(None);
    };
    let change_id = ChangeId::parse(&letters).ok_or_else(|| {
        Error::msg(format!(
            "corrupt branch metadata for {branch}: change id {letters:?} is not one"
        ))
    })?;
    if refs::user_signature(repo, now).is_err() {
        return Ok(None);
    }
    let description = match &overlay.description {
        Some(text) => text.clone(),
        None => meta.pending_description.clone(),
    };
    Ok(Some(OpenPlan {
        tree,
        parent: head,
        change_id,
        born: born.unwrap_or(now),
        message: crate::close::normalize_message(description.as_deref().unwrap_or("")),
    }))
}

/// Whether `id` is a commit that says exactly what the plan says: tree,
/// parents, author (name, email, time), message and `change-id`. The
/// committer is left out on purpose — it is the one field that moves
/// between two writes of the same plan, and the reuse rule exists so it
/// never has to.
pub(crate) fn matches(repo: &gix::Repository, id: gix::ObjectId, plan: &OpenPlan) -> bool {
    let Ok(Some(obj)) = repo.try_find_object(id) else {
        return false;
    };
    if obj.kind != gix::objs::Kind::Commit {
        return false;
    }
    let Ok(commit) = gix::objs::CommitRef::from_bytes(&obj.data, repo.object_hash()) else {
        return false;
    };
    let Ok(author) = commit.author() else {
        return false;
    };
    let Ok(want) = refs::user_signature(repo, plan.born) else {
        return false;
    };
    commit.tree() == plan.tree
        && commit.parents().collect::<Vec<_>>() == plan.parent.into_iter().collect::<Vec<_>>()
        && author.name == want.name
        && author.email == want.email
        && author.time().is_ok_and(|t| t.seconds == plan.born)
        && commit.message == plan.message.as_bytes()
        && changeid::header_of(&obj.data) == Some(plan.change_id)
}

/// The open commit for `plan`: `existing` when one of them already says it,
/// else a fresh object with committer time `now`. Returns the sha.
pub(crate) fn write(
    repo: &gix::Repository,
    plan: &OpenPlan,
    existing: &[gix::ObjectId],
    now: i64,
) -> Result<gix::ObjectId> {
    if let Some(id) = existing.iter().copied().find(|id| matches(repo, *id, plan)) {
        return Ok(id);
    }
    let commit = gix::objs::Commit {
        tree: plan.tree,
        parents: plan.parent.into_iter().collect::<Vec<_>>().into(),
        author: refs::user_signature(repo, plan.born)?,
        committer: refs::user_signature(repo, now)?,
        encoding: None,
        message: plan.message.clone().into(),
        extra_headers: vec![changeid::header(&plan.change_id)],
    };
    Ok(repo.write_object(&commit).map_err(Error::repo)?.detach())
}

/// The open commit `branch`'s ref names, when it still describes the branch:
/// a commit whose tree is the branch's newest operation's and whose first
/// parent is HEAD's commit. Anything else — absent, not a commit, a tree or
/// a parent that moved under it — is `None`.
pub fn current(
    repo: &gix::Repository,
    branch: &str,
    tip_tree: gix::ObjectId,
    head: Option<gix::ObjectId>,
) -> Result<Option<gix::ObjectId>> {
    let Some(id) = refs::ref_target(repo, &open_ref(branch))? else {
        return Ok(None);
    };
    let Ok(commit) = repo.find_commit(id) else {
        return Ok(None);
    };
    let tree = commit.tree_id().map_err(Error::repo)?.detach();
    let parent = commit.parent_ids().next().map(|p| p.detach());
    Ok((tree == tip_tree && parent == head).then_some(id))
}

/// The open commit on HEAD's branch, as the `@` row shows it: `None` when
/// the tree is clean, HEAD is detached, nothing is stated — or signing is
/// on, since the close then signs and the sha it lands is not this one.
pub fn of_head(repo: &gix::Repository) -> Result<Option<gix::ObjectId>> {
    if repo.workdir().is_none() || crate::sign::enabled(repo) {
        return Ok(None);
    }
    let head = crate::head::head_state(repo)?;
    if matches!(head, crate::model::HeadState::Detached { .. }) {
        return Ok(None);
    }
    let branch = crate::snapshot::chain::chain_name(&head);
    let log = OpLog::open(repo)?;
    let Some(tip) = log.branch_tip(&branch)? else {
        return Ok(None);
    };
    let tip_tree = log.get(tip)?.tree();
    let head_commit = crate::snapshot::chain::base_commit(&head)?;
    current(repo, &branch, tip_tree, head_commit)
}

/// Rebuild `branch`'s open commit from the state on disk — its newest
/// operation's tree, HEAD, its metadata — and move the ref to it. Undo and
/// redo's resync, after the pointer move: the landing operation's stated
/// open commit is reused when it still exists and still matches, so a step
/// back over a close puts the pre-close sha back.
pub(crate) fn sync(
    repo: &gix::Repository,
    branch: &str,
    now: i64,
) -> Result<Option<gix::ObjectId>> {
    let head = crate::head::head_state(repo)?;
    let head_commit = crate::snapshot::chain::base_commit(&head)?;
    let log = OpLog::open(repo)?;
    let tip = match log.branch_tip(branch)? {
        Some(id) => Some(walk::decode(repo, id.object_id())?),
        None => None,
    };
    let tree = match &tip {
        Some(op) => op.tree(),
        None => repo.head_tree_id_or_empty().map_err(Error::repo)?.detach(),
    };
    let name = open_ref(branch);
    let existing = refs::ref_target(repo, &name)?;
    let Some(plan) = plan(repo, branch, tree, head_commit, &Overlay::default(), now)? else {
        if let Some(existing) = existing {
            refs::delete_ref(repo, &name, existing, now)?;
        }
        return Ok(None);
    };
    let candidates: Vec<gix::ObjectId> = tip
        .as_ref()
        .and_then(|op| op.open_commit())
        .map(|id| id.object_id())
        .into_iter()
        .chain(existing)
        .collect();
    let id = write(repo, &plan, &candidates, now)?;
    if existing != Some(id) {
        move_ref(repo, &name, id, now)?;
    }
    Ok(Some(id))
}

/// Point an open ref at `id`, whatever it held. No reflog: the ref is derived
/// state, and under the gc guard's `never` expiry a line per capture would
/// pin every open commit ever written.
fn move_ref(repo: &gix::Repository, name: &str, id: gix::ObjectId, now: i64) -> Result<()> {
    let edit = refs::update_edit_unlogged(name, id, gix::refs::transaction::PreviousValue::Any)?;
    match refs::commit_edits(repo, Some(edit), now)? {
        refs::EditOutcome::Applied => Ok(()),
        refs::EditOutcome::Contended => Err(Error::coded(
            "ref/contended",
            format!("could not update {name}: contended"),
            vec![],
        )),
    }
}

/// Drop `branch`'s open ref, when it has one. The object stays pinned by
/// the operations that stated it.
pub(crate) fn clear(repo: &gix::Repository, branch: &str, now: i64) -> Result<()> {
    let name = open_ref(branch);
    if let Some(existing) = refs::ref_target(repo, &name)? {
        refs::delete_ref(repo, &name, existing, now)?;
    }
    Ok(())
}

/// Carry `old`'s open ref to `new`: the branch rename's step for it.
pub(crate) fn rename(repo: &gix::Repository, old: &str, new: &str, now: i64) -> Result<()> {
    let Some(id) = refs::ref_target(repo, &open_ref(old))? else {
        return Ok(());
    };
    move_ref(repo, &open_ref(new), id, now)?;
    refs::delete_ref(repo, &open_ref(old), id, now)
}
