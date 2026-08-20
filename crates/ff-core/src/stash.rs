//! Tree memory: park, arrive, drop. A park is an ordinary `git stash` entry
//! — byte-shaped like `git stash push -u -m "fufu: wip on <branch>"` — so
//! every git tool sees it, plus `refs/fufu/parked/<branch>` tracking the
//! entry by identity (the stash commit's sha, not its position). Arrival
//! resumes it when clean; a conflicting arrival leaves the entry parked and
//! reports loudly (held rewrites are Phase 4). Losing the stash entry
//! (`git stash drop/pop` outside fufu) demotes the parked ref — never lose
//! work, never guess.

use crate::error::{Error, Result};
use crate::model::HeadState;
use crate::refs::{self, EditOutcome};
use crate::snapshot::tree as snaptree;
use crate::worktree;

pub const PARKED_PREFIX: &str = "refs/fufu/parked/";
pub const STASH_REF: &str = "refs/stash";

pub fn parked_ref(branch: &str) -> String {
    format!("{PARKED_PREFIX}{branch}")
}

use crate::refs::user_signature;

/// `<short-sha> <subject>` of a commit — the description git bakes into
/// stash commit messages.
///
/// git's own abbreviation, not `crate::sha`: this string is not fufu's to
/// spell. A park is byte-shaped like `git stash push`, and `diff_stash`
/// holds the two side by side, so the width here is whatever git would have
/// written.
fn describe_commit(repo: &gix::Repository, id: gix::ObjectId) -> Result<String> {
    use gix::prelude::ObjectIdExt;
    let obj = repo.find_object(id).map_err(Error::repo)?;
    let commit = gix::objs::CommitRef::from_bytes(&obj.data).map_err(Error::repo)?;
    let subject = commit.message().summary().to_string();
    drop(commit);
    drop(obj);
    let short = id
        .attach(repo)
        .shorten()
        .map(|p| p.to_string())
        .unwrap_or_else(|_| crate::sha::short_oid(id));
    Ok(format!("{short} {subject}"))
}

fn write_commit(
    repo: &gix::Repository,
    tree: gix::ObjectId,
    parents: Vec<gix::ObjectId>,
    sig: &gix::actor::Signature,
    message: String,
) -> Result<gix::ObjectId> {
    let commit = gix::objs::Commit {
        tree,
        parents: parents.into(),
        author: sig.clone(),
        committer: sig.clone(),
        encoding: None,
        message: message.into(),
        extra_headers: Vec::new(),
    };
    Ok(repo.write_object(&commit).map_err(Error::repo)?.detach())
}

/// What park recorded.
#[derive(Debug, Clone)]
pub struct Parked {
    /// The stash (WIP) commit sha.
    pub stash: gix::ObjectId,
    pub branch: String,
    /// Paths that exceeded `fufu.maxFileSize`… never: parks are exact.
    /// (Field reserved; parks refuse nothing by size.)
    pub files: usize,
}

/// Everything a park will do, computed up front. Only object writes happen
/// during planning — invisible until [`execute_park`] moves the refs — so a
/// verb can journal the plan write-ahead.
#[derive(Debug, Clone)]
pub struct ParkPlan {
    pub branch: String,
    pub head_tree: gix::ObjectId,
    pub wip_tree: gix::ObjectId,
    pub untracked_tree: gix::ObjectId,
    pub wip_commit: gix::ObjectId,
    pub message: String,
    /// The refs/stash tip the CAS push expects.
    pub prev_stash: Option<gix::ObjectId>,
    pub files: usize,
    /// The user signature the stash reflog line will carry.
    sig: gix::actor::Signature,
    now: i64,
}

/// Park the working tree of `branch`: three commits shaped exactly like
/// `git stash push -u -m "fufu: wip on <branch>"`, refs/stash updated (CAS),
/// the parked ref written, then worktree and index reset to HEAD. Returns
/// `None` when there is nothing to park.
pub fn park(repo: &gix::Repository, head: &HeadState, now: i64) -> Result<Option<Parked>> {
    match plan_park(repo, head, now)? {
        None => Ok(None),
        Some(plan) => {
            execute_park(repo, &plan)?;
            Ok(Some(Parked {
                stash: plan.wip_commit,
                branch: plan.branch,
                files: plan.files,
            }))
        }
    }
}

/// The planning half of [`park`]: guards, scan, tree and commit writes.
pub fn plan_park(repo: &gix::Repository, head: &HeadState, now: i64) -> Result<Option<ParkPlan>> {
    let (branch, head_commit) = match head {
        HeadState::Branch { name, commit, .. } => (
            name.clone(),
            gix::ObjectId::from_hex(commit.as_bytes()).map_err(Error::repo)?,
        ),
        HeadState::Unborn { .. } => {
            return Err(Error::msg(
                "no commits yet: fufu cannot park changes before the first commit (ff commit them instead)",
            ));
        }
        HeadState::Detached { .. } => {
            return Err(Error::msg(
                "detached HEAD: fufu cannot park without a branch",
            ));
        }
    };

    // Match git stash's refusals: unmerged and intent-to-add entries cannot
    // be represented in the stash trees.
    let index = repo.index_or_empty().map_err(Error::repo)?;
    for entry in index.entries() {
        if entry.stage() != gix::index::entry::Stage::Unconflicted {
            return Err(Error::msg(
                "unmerged entries in the index: resolve the conflict before parking",
            ));
        }
        if entry
            .flags
            .contains(gix::index::entry::Flags::INTENT_TO_ADD)
        {
            return Err(Error::msg(format!(
                "intent-to-add entry ({}): stage or reset it before parking (git stash refuses these too)",
                entry.path(&index)
            )));
        }
    }
    drop(index);

    let scan = snaptree::scan(repo)?;
    if scan.is_empty() {
        return Ok(None);
    }

    let head_tree = repo.head_tree_id_or_empty().map_err(Error::repo)?.detach();
    let sig = user_signature(repo, now)?;
    let desc = describe_commit(repo, head_commit)?;

    // Index tree: what `git write-tree` would say.
    let index_tree = crate::index::tree_from_index(repo)?;

    // WIP tree: index tree + tracked worktree deltas (untracked separate).
    let (mut pipeline, index_ro) = repo.filter_pipeline(None).map_err(Error::repo)?;
    let mut editor = repo.edit_tree(index_tree).map_err(Error::repo)?;
    for path in &scan.rehash {
        match pipeline
            .worktree_file_to_object(path.as_str().into(), &index_ro)
            .map_err(Error::repo)?
        {
            Some((id, kind, _md)) => {
                editor
                    .upsert(path.as_str(), kind, id)
                    .map_err(Error::repo)?;
            }
            None => {
                editor.remove(path.as_str()).map_err(Error::repo)?;
            }
        }
    }
    for path in &scan.wt_deletes {
        editor.remove(path.as_str()).map_err(Error::repo)?;
    }
    let wip_tree = editor.write().map_err(Error::repo)?.detach();

    // Untracked tree: untracked paths only, from an empty root.
    let empty = gix::ObjectId::empty_tree(repo.object_hash());
    let mut editor = repo.edit_tree(empty).map_err(Error::repo)?;
    for path in &scan.untracked {
        if let Some((id, kind, _md)) = pipeline
            .worktree_file_to_object(path.as_str().into(), &index_ro)
            .map_err(Error::repo)?
        {
            editor
                .upsert(path.as_str(), kind, id)
                .map_err(Error::repo)?;
        }
    }
    let untracked_tree = editor.write().map_err(Error::repo)?.detach();
    drop(pipeline);

    // The three commits, shaped byte-for-byte like git's.
    let index_commit = write_commit(
        repo,
        index_tree,
        vec![head_commit],
        &sig,
        format!("index on {branch}: {desc}\n"),
    )?;
    let untracked_commit = write_commit(
        repo,
        untracked_tree,
        Vec::new(),
        &sig,
        format!("untracked files on {branch}: {desc}\n"),
    )?;
    let wip_message = format!("On {branch}: fufu: wip on {branch}");
    let wip_commit = write_commit(
        repo,
        wip_tree,
        vec![head_commit, index_commit, untracked_commit],
        &sig,
        wip_message.clone(),
    )?;

    let files = scan.rehash.len()
        + scan.untracked.len()
        + scan.wt_deletes.len()
        + scan.staged_upserts.len()
        + scan.staged_deletes.len();
    Ok(Some(ParkPlan {
        branch,
        head_tree,
        wip_tree,
        untracked_tree,
        wip_commit,
        message: wip_message,
        prev_stash: refs::ref_target(repo, STASH_REF)?,
        files,
        sig,
        now,
    }))
}

/// The mutating half of [`park`]: refs/stash CAS push, the parked ref, then
/// the slate wiped clean (worktree and index back to HEAD).
pub fn execute_park(repo: &gix::Repository, plan: &ParkPlan) -> Result<()> {
    // refs/stash is outside the autocreate namespaces: force the reflog, or
    // the stack silently doesn't exist. The reflog line carries the user's
    // identity, as git's own stash does.
    let expected = match plan.prev_stash {
        Some(tip) => {
            gix::refs::transaction::PreviousValue::MustExistAndMatch(gix::refs::Target::Object(tip))
        }
        None => gix::refs::transaction::PreviousValue::MustNotExist,
    };
    let edit = refs::update_edit(STASH_REF, plan.wip_commit, expected, &plan.message)?;
    let time_str = format!("{} +0000", plan.now);
    let sig_ref = gix::actor::SignatureRef {
        name: plan.sig.name.as_ref(),
        email: plan.sig.email.as_ref(),
        time: &time_str,
    };
    match refs::commit_edits_as(repo, Some(edit), sig_ref) {
        Ok(EditOutcome::Applied) => {}
        Ok(EditOutcome::Contended) => {
            return Err(Error::coded(
                "ref/contended",
                "refs/stash is contended; park aborted",
                vec![],
            ));
        }
        Err(err) => return Err(err),
    }
    refs::write_ref(
        repo,
        &parked_ref(&plan.branch),
        plan.wip_commit,
        gix::refs::transaction::PreviousValue::Any,
        plan.now,
        &format!("park: wip on {}", plan.branch),
    )?;

    // Clear the slate: tracked state back to HEAD, untracked files removed,
    // index rewritten to HEAD.
    let everything = |_: &str| true;
    let empty = gix::ObjectId::empty_tree(repo.object_hash());
    worktree::apply_tree_transition(repo, plan.wip_tree, plan.head_tree, &everything)?;
    worktree::apply_tree_transition(repo, plan.untracked_tree, empty, &everything)?;
    crate::index::write_index_for_tree(repo, plan.head_tree)?;
    Ok(())
}

/// The decoded anatomy of a stash (WIP) commit.
pub struct StashCommit {
    pub id: gix::ObjectId,
    /// The HEAD the stash was taken on.
    pub base: gix::ObjectId,
    pub base_tree: gix::ObjectId,
    pub wip_tree: gix::ObjectId,
    pub index_tree: gix::ObjectId,
    /// Empty tree when the stash carries no untracked files.
    pub untracked_tree: gix::ObjectId,
}

pub fn read_stash_commit(repo: &gix::Repository, id: gix::ObjectId) -> Result<StashCommit> {
    let obj = repo.find_object(id).map_err(Error::repo)?;
    let commit = gix::objs::CommitRef::from_bytes(&obj.data).map_err(Error::repo)?;
    let wip_tree = commit.tree();
    let parents: Vec<gix::ObjectId> = commit.parents().collect();
    drop(commit);
    drop(obj);
    let (&base, rest) = parents
        .split_first()
        .ok_or_else(|| Error::msg(format!("{id} is not a stash commit: no parents")))?;
    let tree_of = |repo: &gix::Repository, id: gix::ObjectId| -> Result<gix::ObjectId> {
        Ok(repo
            .find_commit(id)
            .map_err(Error::repo)?
            .tree_id()
            .map_err(Error::repo)?
            .detach())
    };
    let base_tree = tree_of(repo, base)?;
    let index_tree = match rest.first() {
        Some(&index_commit) => tree_of(repo, index_commit)?,
        None => {
            return Err(Error::msg(format!(
                "{id} is not a stash commit: no index parent"
            )));
        }
    };
    let untracked_tree = match rest.get(1) {
        Some(&untracked_commit) => tree_of(repo, untracked_commit)?,
        None => gix::ObjectId::empty_tree(repo.object_hash()),
    };
    Ok(StashCommit {
        id,
        base,
        base_tree,
        wip_tree,
        index_tree,
        untracked_tree,
    })
}

/// Whether `sha` is present in the stash reflog (any line's new value).
pub fn stash_contains(repo: &gix::Repository, sha: gix::ObjectId) -> Result<bool> {
    let Some(reference) = repo.try_find_reference(STASH_REF).map_err(Error::repo)? else {
        return Ok(false);
    };
    let mut platform = reference.log_iter();
    let Some(iter) = platform.rev().map_err(Error::repo)? else {
        return Ok(false);
    };
    for line in iter {
        let line = line.map_err(Error::repo)?;
        if line.new_oid == sha {
            return Ok(true);
        }
    }
    Ok(false)
}

/// The parked entry for a branch, if any.
pub fn parked_entry(repo: &gix::Repository, branch: &str) -> Result<Option<gix::ObjectId>> {
    refs::ref_target(repo, &parked_ref(branch))
}

/// The result of an arrival attempt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Arrival {
    /// No parked entry for this branch.
    None,
    /// The parked change is back in the tree (and dropped from the stash).
    Restored { stash: String, files: Vec<String> },
    /// The branch moved beneath the parked change and the merge conflicts:
    /// the entry stays parked. Held rewrites are Phase 4.
    Conflicted { stash: String, paths: Vec<String> },
    /// The stash entry vanished (dropped/popped outside fufu): the parked
    /// ref was demoted. The timeline still has the state.
    Invalidated { stash: String },
}

/// What an arrival will do, computed before any mutation — the planning is
/// pure (probe merges leave nothing in the ODB unless they are clean and
/// their result is about to be used), so a verb can journal it write-ahead.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ArrivePlan {
    None,
    Restore {
        stash: gix::ObjectId,
        target_wip: gix::ObjectId,
        target_index: gix::ObjectId,
        untracked_tree: gix::ObjectId,
    },
    Conflict {
        stash: gix::ObjectId,
        paths: Vec<String>,
    },
    Invalidate {
        stash: gix::ObjectId,
    },
}

/// Plan the arrival on `branch` against an explicit target state (the
/// switch plans before HEAD moves).
pub fn plan_arrival(
    repo: &gix::Repository,
    branch: &str,
    target_commit: gix::ObjectId,
    target_tree: gix::ObjectId,
) -> Result<ArrivePlan> {
    let Some(parked) = parked_entry(repo, branch)? else {
        return Ok(ArrivePlan::None);
    };
    if !stash_contains(repo, parked)? {
        return Ok(ArrivePlan::Invalidate { stash: parked });
    }
    let stash = read_stash_commit(repo, parked)?;

    let (target_wip, target_index) = if target_commit == stash.base {
        // Fast path: the branch didn't move while parked.
        (stash.wip_tree, stash.index_tree)
    } else {
        // The branch moved beneath the change: three-way both trees, in
        // memory first — a conflicted probe must leave zero loose objects.
        let Some(wip) = probe_merge(repo, stash.base_tree, target_tree, stash.wip_tree)? else {
            return Ok(ArrivePlan::Conflict {
                stash: parked,
                paths: crate::futures::conflict_paths(
                    repo,
                    stash.base_tree,
                    target_tree,
                    stash.wip_tree,
                )?,
            });
        };
        let Some(index) = probe_merge(repo, stash.base_tree, target_tree, stash.index_tree)? else {
            return Ok(ArrivePlan::Conflict {
                stash: parked,
                paths: crate::futures::conflict_paths(
                    repo,
                    stash.base_tree,
                    target_tree,
                    stash.index_tree,
                )?,
            });
        };
        (wip, index)
    };

    // Untracked collisions against the target tree. Disk survivors (ignored
    // files, mostly) are re-checked at execution; a late collision aborts
    // the arrival loudly rather than overwrite anything.
    let mut collisions = Vec::new();
    for (path, _, _) in tree_files(repo, stash.untracked_tree)? {
        if tree_lookup(repo, target_wip, &path)?.is_some() {
            collisions.push(path);
        }
    }
    if !collisions.is_empty() {
        return Ok(ArrivePlan::Conflict {
            stash: parked,
            paths: collisions,
        });
    }
    Ok(ArrivePlan::Restore {
        stash: parked,
        target_wip,
        target_index,
        untracked_tree: stash.untracked_tree,
    })
}

/// The mutating half of an arrival. For `Restore`, the worktree must
/// currently match `current_tree` (HEAD's tree after the switch).
pub fn execute_arrival(
    repo: &gix::Repository,
    branch: &str,
    plan: &ArrivePlan,
    current_tree: gix::ObjectId,
    now: i64,
) -> Result<Arrival> {
    match plan {
        ArrivePlan::None => Ok(Arrival::None),
        ArrivePlan::Conflict { stash, paths } => Ok(Arrival::Conflicted {
            stash: stash.to_string(),
            paths: paths.clone(),
        }),
        ArrivePlan::Invalidate { stash } => {
            // The entry is gone from the stash: demote, loudly, never guess.
            refs::delete_ref(repo, &parked_ref(branch), *stash, now)?;
            Ok(Arrival::Invalidated {
                stash: stash.to_string(),
            })
        }
        ArrivePlan::Restore {
            stash,
            target_wip,
            target_index,
            untracked_tree,
        } => {
            // Late collision check: files that survived the transition
            // (ignored files) still block the untracked restoration.
            let workdir = repo
                .workdir()
                .ok_or_else(|| Error::coded("repo/bare", "bare repository: cannot arrive", vec![]))?
                .to_owned();
            let mut collisions = Vec::new();
            for (path, _, _) in tree_files(repo, *untracked_tree)? {
                if tree_lookup(repo, current_tree, &path)?.is_none() && workdir.join(&path).exists()
                {
                    collisions.push(path);
                }
            }
            if !collisions.is_empty() {
                return Ok(Arrival::Conflicted {
                    stash: stash.to_string(),
                    paths: collisions,
                });
            }

            let everything = |_: &str| true;
            let transition =
                worktree::apply_tree_transition(repo, current_tree, *target_wip, &everything)?;
            let empty = gix::ObjectId::empty_tree(repo.object_hash());
            let untracked =
                worktree::apply_tree_transition(repo, empty, *untracked_tree, &everything)?;
            crate::index::write_index_for_tree(repo, *target_index)?;

            drop_stash_entry(repo, *stash)?;
            refs::delete_ref(repo, &parked_ref(branch), *stash, now)?;

            let mut files = transition.written;
            files.extend(transition.deleted);
            files.extend(untracked.written);
            files.sort();
            files.dedup();
            Ok(Arrival::Restored {
                stash: stash.to_string(),
                files,
            })
        }
    }
}

/// Try to resume `branch`'s parked change. Call with a clean worktree that
/// matches HEAD's tree (switch guarantees this).
pub fn arrive(repo: &gix::Repository, branch: &str, now: i64) -> Result<Arrival> {
    let head_commit = match crate::head::head_state(repo)? {
        HeadState::Branch { commit, .. } => {
            gix::ObjectId::from_hex(commit.as_bytes()).map_err(Error::repo)?
        }
        _ => return Err(Error::msg("arrive requires a born branch")),
    };
    let head_tree = repo.head_tree_id_or_empty().map_err(Error::repo)?.detach();
    let plan = plan_arrival(repo, branch, head_commit, head_tree)?;
    execute_arrival(repo, branch, &plan, head_tree, now)
}

/// Three-way merge as a pure probe: `Some(tree)` when clean (re-merged
/// against the real object store so the result is usable), `None` on any
/// conflict (probed in memory; nothing touches the ODB).
fn probe_merge(
    repo: &gix::Repository,
    base: gix::ObjectId,
    ours: gix::ObjectId,
    theirs: gix::ObjectId,
) -> Result<Option<gix::ObjectId>> {
    let clean = {
        let memory = repo.clone().with_object_memory();
        let options = memory
            .tree_merge_options()
            .map_err(Error::repo)?
            .with_fail_on_conflict(Some(gix::merge::tree::TreatAsUnresolved::git()));
        let outcome = memory
            .merge_trees(base, ours, theirs, Default::default(), options)
            .map_err(Error::repo)?;
        !outcome.failed_on_first_unresolved_conflict
    };
    if !clean {
        return Ok(None);
    }
    // Clean: run the same merge against the real store so the tree persists.
    let options = repo
        .tree_merge_options()
        .map_err(Error::repo)?
        .with_fail_on_conflict(Some(gix::merge::tree::TreatAsUnresolved::git()));
    let mut outcome = repo
        .merge_trees(base, ours, theirs, Default::default(), options)
        .map_err(Error::repo)?;
    let tree = outcome.tree.write().map_err(Error::repo)?.detach();
    Ok(Some(tree))
}

/// All file entries of a tree, recursively: (path, kind, id).
fn tree_files(
    repo: &gix::Repository,
    tree: gix::ObjectId,
) -> Result<Vec<(String, gix::objs::tree::EntryKind, gix::ObjectId)>> {
    let mut out = Vec::new();
    fn walk(
        repo: &gix::Repository,
        tree: gix::ObjectId,
        prefix: &str,
        out: &mut Vec<(String, gix::objs::tree::EntryKind, gix::ObjectId)>,
    ) -> Result<()> {
        let obj = repo.find_object(tree).map_err(Error::repo)?.detach();
        for entry in gix::objs::TreeRefIter::from_bytes(&obj.data) {
            let entry = entry.map_err(Error::repo)?;
            let path = if prefix.is_empty() {
                entry.filename.to_string()
            } else {
                format!("{prefix}/{}", entry.filename)
            };
            if entry.mode.is_tree() {
                walk(repo, entry.oid.to_owned(), &path, out)?;
            } else {
                out.push((path, entry.mode.kind(), entry.oid.to_owned()));
            }
        }
        Ok(())
    }
    walk(repo, tree, "", &mut out)?;
    Ok(out)
}

/// Look up a path in a tree; `None` when absent.
fn tree_lookup(
    repo: &gix::Repository,
    tree: gix::ObjectId,
    path: &str,
) -> Result<Option<gix::ObjectId>> {
    let mut tree = repo.find_tree(tree).map_err(Error::repo)?;
    Ok(tree
        .peel_to_entry_by_path(path)
        .map_err(Error::repo)?
        .map(|e| e.object_id()))
}

/// Drop a stash entry by identity: replay the stash reflog without its line
/// (chaining rederives, matching `git reflog delete --rewrite`), retargeting
/// or deleting `refs/stash`.
pub fn drop_stash_entry(repo: &gix::Repository, sha: gix::ObjectId) -> Result<()> {
    let Some(reference) = repo.try_find_reference(STASH_REF).map_err(Error::repo)? else {
        return Err(Error::msg("no stash stack exists"));
    };
    let current = reference
        .target()
        .try_id()
        .map(|id| id.to_owned())
        .ok_or_else(|| Error::msg("refs/stash is symbolic"))?;

    // Oldest→newest lines, preserved verbatim.
    struct Line {
        new: gix::ObjectId,
        name: String,
        email: String,
        time_str: String,
        message: String,
    }
    let mut lines: Vec<Line> = Vec::new();
    {
        let mut platform = reference.log_iter();
        let Some(iter) = platform.all().map_err(Error::repo)? else {
            return Err(Error::msg("refs/stash has no reflog"));
        };
        for line in iter {
            let line = line.map_err(Error::repo)?;
            lines.push(Line {
                new: gix::ObjectId::from_hex(line.new_oid).map_err(Error::repo)?,
                name: line.signature.name.to_string(),
                email: line.signature.email.to_string(),
                // The raw `<seconds> <offset>` text, preserved verbatim.
                time_str: line.signature.time.to_string(),
                message: line.message.to_string(),
            });
        }
    }
    // Remove the NEWEST line whose new value is `sha`.
    let position = lines
        .iter()
        .rposition(|line| line.new == sha)
        .ok_or_else(|| Error::msg(format!("{sha} is not in the stash reflog")))?;
    lines.remove(position);

    // Delete, then replay survivors with their original identities and
    // times; each transaction derives the previous value from CAS.
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    refs::delete_ref(repo, STASH_REF, current, now)?;
    let mut expected = gix::refs::transaction::PreviousValue::MustNotExist;
    for line in &lines {
        let edit = refs::update_edit(STASH_REF, line.new, expected.clone(), &line.message)?;
        let sig = gix::actor::SignatureRef {
            name: line.name.as_str().into(),
            email: line.email.as_str().into(),
            time: &line.time_str,
        };
        match refs::commit_edits_as(repo, Some(edit), sig)? {
            EditOutcome::Applied => {}
            EditOutcome::Contended => {
                return Err(Error::coded(
                    "ref/contended",
                    "refs/stash is contended during replay",
                    vec![],
                ));
            }
        }
        expected = gix::refs::transaction::PreviousValue::MustExistAndMatch(
            gix::refs::Target::Object(line.new),
        );
    }
    Ok(())
}
