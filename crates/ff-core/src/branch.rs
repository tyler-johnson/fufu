//! Branch operations. Phase 2 ships the rename core (`ff describe -b`),
//! list, and delete; anonymous branches live under `refs/heads/ff/<petname>`.
//!
//! Rename is built by hand — gix has no native rename — as
//! create-new-with-replayed-reflog FIRST, then delete-old: a crash leaves
//! both names, never neither. The rename carries everything that makes the
//! branch a fufu change: the snap chain, the parked entry, and the
//! metadata file. Divergences from `git branch -m`, accepted and
//! documented: no trailing "Branch: renamed" reflog line (its old and new
//! values are equal, which the transaction machinery drops), and the first
//! replayed line's previous-value column is null.

use std::collections::HashSet;

use crate::error::{Error, Result};
use crate::model::HeadState;
use crate::ops::record::observe_refs;
use crate::ops::{BRANCH_PREFIX, OpKind, OpRecord, RefTransition, verb};
use crate::refs;
use crate::stash;

/// The namespace prefix for anonymous branches.
pub const ANON_PREFIX: &str = "ff/";

pub fn is_anonymous(branch: &str) -> bool {
    branch.starts_with(ANON_PREFIX)
}

fn heads_ref(branch: &str) -> String {
    format!("refs/heads/{branch}")
}

/// Validate a proposed branch name by round-tripping it through gix's ref
/// name validation.
pub fn validate_name(branch: &str) -> Result<()> {
    let full = heads_ref(branch);
    let _: gix::refs::FullName = full.as_str().try_into().map_err(|err| {
        Error::coded(
            "branch/invalid-name",
            format!("invalid branch name {branch:?}: {err}"),
            vec![],
        )
    })?;
    Ok(())
}

/// Refuse when `branch` is checked out in another worktree — git guards
/// this on rename/delete and on opening a branch; gix has neither check, so
/// fufu carries its own.
pub fn guard_other_worktrees(repo: &gix::Repository, branch: &str) -> Result<()> {
    if let Some(holder) = crate::linked::holder_of(repo, branch)? {
        return Err(Error::coded(
            "branch/checked-out-elsewhere",
            format!(
                "'{branch}' is already used by worktree at '{}'",
                holder.path.display()
            ),
            vec![],
        ));
    }
    Ok(())
}

/// What a rename moved, for the operation record.
#[derive(Debug, Default)]
pub struct RenameEffects {
    pub transitions: Vec<RefTransition>,
    /// HEAD symref retargeted from old to new.
    pub head_moved: Option<(String, String)>,
}

/// Rename `old` to `new`, carrying the snap chain, parked entry, metadata,
/// and the branch's upstream. This is the mechanism beneath `ff describe -b`
/// and the `-b` placeholder claims on `ff commit` and `ff start`.
pub fn rename(repo: &gix::Repository, old: &str, new: &str, now: i64) -> Result<RenameEffects> {
    validate_name(new)?;
    let old_ref = heads_ref(old);
    let new_ref = heads_ref(new);
    let target = refs::ref_target(repo, &old_ref)?.ok_or_else(|| {
        Error::coded("branch/not-found", format!("no branch named {old}"), vec![])
    })?;
    if refs::ref_target(repo, &new_ref)?.is_some() {
        return Err(Error::coded(
            "branch/exists",
            format!("a branch named {new} already exists"),
            vec!["ff branch list".into()],
        ));
    }
    guard_other_worktrees(repo, old)?;

    let mut effects = RenameEffects::default();

    // 1. New branch ref, reflog replayed with original identities/times.
    let lines = refs::read_ref_log(repo, &old_ref)?;
    refs::create_ref_with_log(
        repo,
        &new_ref,
        target,
        &lines,
        now,
        &format!("branch: renamed from {old}"),
    )?;
    effects.transitions.push(RefTransition {
        name: new_ref.clone(),
        old: None,
        new: Some(target.to_string()),
    });

    // 2. HEAD retarget when it points at the old name. deref must stay
    //    false — a dereferencing edit would move the branch, not HEAD.
    //    gix writes no reflog line for symref→symref (git does; accepted).
    let head = crate::head::head_state(repo)?;
    let head_is_old = matches!(
        &head,
        HeadState::Branch { r#ref, .. } if r#ref == &old_ref
    ) || matches!(
        &head,
        HeadState::Unborn { r#ref } if r#ref == &old_ref
    );
    if head_is_old {
        retarget_head(repo, &new_ref, now)?;
        effects.head_moved = Some((format!("ref:{old_ref}"), format!("ref:{new_ref}")));
    }

    // 3. The branch's pointer into the log follows the change, reflog and
    //    all — `ff restore --at @{n}` reads exactly those lines.
    let old_snap = format!("{BRANCH_PREFIX}{old}");
    let new_snap = format!("{BRANCH_PREFIX}{new}");
    if let Some(snap_tip) = refs::ref_target(repo, &old_snap)? {
        let lines = refs::read_ref_log(repo, &old_snap)?;
        refs::create_ref_with_log(
            repo,
            &new_snap,
            snap_tip,
            &lines,
            now,
            &format!("branch: renamed from {old}"),
        )?;
        refs::delete_ref(repo, &old_snap, snap_tip, now)?;
    }

    // 4. Parked entry follows.
    if let Some(parked) = stash::parked_entry(repo, old)? {
        refs::write_ref(
            repo,
            &stash::parked_ref(new),
            parked,
            gix::refs::transaction::PreviousValue::MustNotExist,
            now,
            &format!("branch: renamed from {old}"),
        )?;
        refs::delete_ref(repo, &stash::parked_ref(old), parked, now)?;
        effects.transitions.push(RefTransition {
            name: stash::parked_ref(new),
            old: None,
            new: Some(parked.to_string()),
        });
        effects.transitions.push(RefTransition {
            name: stash::parked_ref(old),
            old: Some(parked.to_string()),
            new: None,
        });
    }

    // 5. Metadata file follows.
    crate::branchmeta::rename(repo, old, new)?;

    // 6. The branch's upstream follows: git's own rename carries this
    //    section, and fufu's used to drop it.
    crate::snapshot::config::rename_branch_section(repo, old, new)?;

    // 7. Old branch ref goes last: a crash anywhere above leaves both
    //    names resolvable.
    refs::delete_ref(repo, &old_ref, target, now)?;
    effects.transitions.push(RefTransition {
        name: old_ref,
        old: Some(target.to_string()),
        new: None,
    });
    Ok(effects)
}

/// Point HEAD at a branch ref (symref edit, non-dereferencing).
pub(crate) fn retarget_head(repo: &gix::Repository, full_ref: &str, now: i64) -> Result<()> {
    use gix::refs::transaction::{Change, LogChange, PreviousValue, RefEdit, RefLog};
    let name: gix::refs::FullName = full_ref.try_into().map_err(Error::repo)?;
    let edit = RefEdit {
        change: Change::Update {
            log: LogChange {
                mode: RefLog::AndReference,
                force_create_reflog: false,
                message: format!("checkout: moving to {full_ref}").into(),
            },
            expected: PreviousValue::Any,
            new: gix::refs::Target::Symbolic(name),
        },
        name: "HEAD".try_into().map_err(Error::repo)?,
        deref: false,
    };
    match refs::commit_edits(repo, Some(edit), now)? {
        refs::EditOutcome::Applied => Ok(()),
        refs::EditOutcome::Contended => {
            Err(Error::coded("ref/contended", "HEAD is contended", vec![]))
        }
    }
}

/// List branches for `ff branch list`: three buckets — named branches and
/// anonymous ones segregated, each with its tip, parked marker, pending
/// description, and upstream annotation, and beside them the branches that
/// exist on a remote and no local branch tracks.
pub fn list(
    repo: &gix::Repository,
    opts: &crate::model::BranchListOptions,
) -> Result<crate::model::BranchList> {
    use crate::model::BranchInfo;
    let head = crate::head::head_state(repo)?;
    let current = crate::snapshot::chain::chain_name(&head);
    let mut named = Vec::new();
    let mut anonymous = Vec::new();
    // The tracking refs a local branch stands on, so the remote walk can
    // subtract them by ref — a branch's name is the wrong axis.
    let mut tracked: HashSet<String> = HashSet::new();
    let mut names = crate::switch::branch_names(repo)?;
    // An unborn current branch has no ref yet; show it anyway.
    if matches!(head, HeadState::Unborn { .. }) && !names.contains(&current) {
        names.push(current.clone());
    }
    names.sort();
    for name in names {
        let full = heads_ref(&name);
        let tip = refs::ref_target(repo, &full)?;
        let subject = match tip {
            Some(id) => {
                let obj = repo.find_object(id).map_err(Error::repo)?;
                let commit = gix::objs::CommitRef::from_bytes(&obj.data).map_err(Error::repo)?;
                Some(commit.message().summary().to_string())
            }
            None => None,
        };
        let meta = crate::branchmeta::read(repo, &name)?;
        let upstream = match tip {
            Some(id) => {
                let full_name: gix::refs::FullName =
                    full.as_str().try_into().map_err(Error::repo)?;
                crate::upstream::upstream_for(repo, full_name, Some(id))?
            }
            None => None,
        };
        if let Some(upstream) = upstream.as_ref() {
            tracked.insert(upstream.r#ref.clone());
        }
        // Only the row we are standing on carries the open change into the
        // simulation: another branch's row must not react to work sitting in
        // this one's tree. Errors degrade to None — one row's failed
        // simulation must never fail the whole listing.
        let open = if name == current {
            crate::futures::open_tree(repo, &name).unwrap_or(None)
        } else {
            None
        };
        // Base axis only: a listing walks every branch, and probing a remote
        // per row would make it pay a network-shaped question nobody asked.
        let future = crate::futures::base_future(repo, &name, tip, open).unwrap_or(None);
        let info = BranchInfo {
            name: name.clone(),
            current: name == current,
            anonymous: is_anonymous(&name),
            tip: tip.map(|id| id.to_string()),
            subject,
            parked: crate::stash::parked_entry(repo, &name)?.is_some(),
            pending_description: meta.pending_description,
            session: meta.session.as_ref().map(|s| s.onto.clone()),
            held: meta.held.is_some(),
            resolving: meta.resolving.is_some(),
            upstream,
            future,
        };
        if info.anonymous {
            anonymous.push(info);
        } else {
            named.push(info);
        }
    }
    // The third bucket: what a remote holds that no local branch tracks.
    let mut remote_only: Vec<crate::model::RemoteBranch> = refs::remote_branches(repo)?
        .into_iter()
        .filter(|r| !tracked.contains(&r.name))
        .map(|r| {
            // Degrade, never fail: a tracking ref whose object is missing
            // still gets a row, with the fields its commit cannot give left
            // empty.
            let (subject, tip_time) = match repo.find_commit(r.tip) {
                Ok(obj) => (
                    gix::objs::CommitRef::from_bytes(&obj.data)
                        .ok()
                        .map(|commit| commit.message().summary().to_string()),
                    obj.committer()
                        .ok()
                        .and_then(|ident| ident.time().ok())
                        .map(|time| time.seconds)
                        .unwrap_or(0),
                ),
                Err(_) => (None, 0),
            };
            crate::model::RemoteBranch {
                name: r.name,
                remote: r.remote,
                tip: r.tip.to_string(),
                subject,
                tip_time,
            }
        })
        .collect();
    // Newest tip first; equal tips keep name order — the map's rule for
    // equal committer times.
    remote_only.sort_by(|a, b| {
        b.tip_time
            .cmp(&a.tip_time)
            .then_with(|| a.name.cmp(&b.name))
    });
    let total = remote_only.len();
    let kept = opts.remote_limit.map_or(total, |limit| total.min(limit));
    remote_only.truncate(kept);
    Ok(crate::model::BranchList {
        named,
        anonymous,
        remote_only,
        remote_more: total.saturating_sub(kept),
    })
}

/// Name the current branch, recorded — `ff describe -b`, the one verb that
/// names a branch. Claiming a petname and renaming a proper name are the
/// same act on the same axis as `-m`, so there is no discipline separating
/// them: what an anonymous branch lacks is a name, not permission to have
/// one changed.
pub fn rename_current(
    repo: &gix::Repository,
    new_name: &str,
    prov: &crate::snapshot::Provenance,
    now: Option<i64>,
    argv: Vec<String>,
) -> Result<(crate::model::ClaimReport, verb::VerbContext)> {
    let ctx = verb::begin_verb(repo, prov, now)?;
    let now = ctx.now;
    let head = crate::head::head_state(repo)?;
    let (current, tip) = match &head {
        HeadState::Branch { name, commit, .. } => (
            name.clone(),
            gix::ObjectId::from_hex(commit.as_bytes()).map_err(Error::repo)?,
        ),
        _ => {
            return Err(Error::coded(
                "repo/detached",
                "not on a branch: there is no branch to name",
                vec!["ff switch <branch>".into()],
            ));
        }
    };
    validate_name(new_name)?;
    if refs::ref_target(repo, &heads_ref(new_name))?.is_some() {
        return Err(Error::coded(
            "branch/exists",
            format!("a branch named {new_name} already exists"),
            vec!["ff branch list".into()],
        ));
    }
    guard_other_worktrees(repo, &current)?;

    // Write-ahead: the rename's effects are fully known before it runs.
    let mut planned = observe_refs(repo)?;
    planned.refs.remove(&heads_ref(&current));
    planned.refs.insert(heads_ref(new_name), tip.to_string());
    planned.head = format!("ref:{}", heads_ref(new_name));
    let mut transitions = vec![
        RefTransition {
            name: heads_ref(new_name),
            old: None,
            new: Some(tip.to_string()),
        },
        RefTransition {
            name: heads_ref(&current),
            old: Some(tip.to_string()),
            new: None,
        },
    ];
    if let Some(parked) = crate::stash::parked_entry(repo, &current)? {
        let old_ref = crate::stash::parked_ref(&current);
        let new_ref = crate::stash::parked_ref(new_name);
        planned.refs.remove(&old_ref);
        planned.refs.insert(new_ref.clone(), parked.to_string());
        transitions.push(RefTransition {
            name: new_ref,
            old: None,
            new: Some(parked.to_string()),
        });
        transitions.push(RefTransition {
            name: old_ref,
            old: Some(parked.to_string()),
            new: None,
        });
    }
    let summary = if is_anonymous(&current) {
        format!("claim {current} as {new_name}")
    } else {
        format!("rename {current} to {new_name}")
    };
    let mut record = OpRecord::new("describe", summary, now);
    record.argv = argv;
    record.refs = transitions;
    record.head = Some((
        format!("ref:{}", heads_ref(&current)),
        format!("ref:{}", heads_ref(new_name)),
    ));
    verb::append_op(
        repo,
        OpKind::Op,
        verb::VerbOp {
            record,
            planned,
            // A rename moves names, never content: the working tree and the
            // index are exactly where the preamble found them.
            tree: ctx.pre_tree,
            index_tree: crate::index::tree_from_index(repo)?,
            // Recorded against the branch it RAN on, not the one it creates.
            // The rename below carries this branch's pointer into the log —
            // reflog and all — over to the new name, and an op recorded under
            // the new name would create that pointer here and leave the
            // rename with a name already taken.
            branch: current.clone(),
            base: Some(tip),
            session: prov.session.clone(),
            pins: &[tip],
        },
        now,
    )?;

    rename(repo, &current, new_name, now)?;
    Ok((
        crate::model::ClaimReport {
            from: current,
            to: new_name.to_string(),
            pre_op: ctx.pre_op.map(|id| id.to_string()),
        },
        ctx,
    ))
}

/// The shared copy `name` answered to, or `None` when it answered to
/// nothing. Reads the tracking ref and config; never refuses and never
/// guesses a remote. Callers use it to decide `--shared` before anything is
/// deleted.
pub fn shared_copy(repo: &gix::Repository, name: &str) -> Result<Option<crate::model::SharedCopy>> {
    let Some(sync_ref) = crate::futures::remote_for(repo, name)? else {
        return Ok(None);
    };
    let Some(remote) = repo
        .branch_remote_name(name, gix::remote::Direction::Fetch)
        .as_ref()
        .and_then(|n| n.as_symbol())
        .map(|n| n.to_string())
    else {
        return Ok(None);
    };
    let full: gix::refs::FullName = format!("refs/heads/{name}")
        .as_str()
        .try_into()
        .map_err(Error::repo)?;
    let remote_branch = repo
        .branch_remote_ref_name(full.as_ref(), gix::remote::Direction::Fetch)
        .and_then(|n| n.ok())
        .map(|n| n.as_ref().shorten().to_string())
        .unwrap_or_else(|| name.to_string());
    Ok(Some(crate::model::SharedCopy {
        remote,
        name: sync_ref.name,
        r#ref: sync_ref.r#ref,
        remote_branch,
        tip: sync_ref.tip,
        aliased: sync_ref.role == crate::futures::Role::RemoteAlias,
    }))
}

/// The three local traces of the shared copy, removed at once: the tracking
/// ref, the `[branch "<name>"]` section, and the published note. This runs
/// only after the wire delete returned `Ok`, and none of the three is under
/// `TRACKED_PREFIXES`, so this is deliberately the one direction `ff undo`
/// cannot walk back — and it is correct, because the shared copy is gone and
/// there is nothing left for any of them to point at. Step one is tolerant
/// of an absent ref because git's delete push already prunes the tracking
/// ref, and a hard delete there would fail the verb after the wire act
/// landed.
pub fn forget_shared(
    repo: &gix::Repository,
    name: &str,
    tracking_ref: &str,
    now: i64,
) -> Result<()> {
    if let Some(tip) = refs::ref_target(repo, tracking_ref)? {
        refs::delete_ref(repo, tracking_ref, tip, now)?;
    }
    crate::snapshot::config::remove_branch_section(repo, name)?;
    let published = format!("{}{name}", crate::published::PUBLISHED_PREFIX);
    if let Some(tip) = refs::ref_target(repo, &published)? {
        refs::delete_ref(repo, &published, tip, now)?;
    }
    Ok(())
}

/// `ff branch delete <name>` — delete a branch, recorded. The branch's pointer
/// into the log moves to trash (trim's one-deep pattern) rather than being
/// dropped, the parked entry is demoted (its stash entry survives), and the
/// tip stays pinned by the operation — so the deletion is undoable, which is
/// why there is no merged-check: nothing is lost. The branch's operations
/// themselves stay on the log; only the way in through this name goes. What
/// the branch answered to on a remote is read out and reported, and it
/// survives the delete.
pub fn delete(
    repo: &gix::Repository,
    name: &str,
    prov: &crate::snapshot::Provenance,
    now: Option<i64>,
    argv: Vec<String>,
) -> Result<(crate::model::BranchDeleteReport, verb::VerbContext)> {
    let ctx = verb::begin_verb(repo, prov, now)?;
    let now = ctx.now;
    let head = crate::head::head_state(repo)?;
    let current = crate::snapshot::chain::chain_name(&head);
    if name == current {
        return Err(Error::coded(
            "branch/is-current",
            format!("{name} is the current branch; switch away before deleting it"),
            vec!["ff switch <branch>".into()],
        ));
    }
    let full = heads_ref(name);
    let tip = refs::ref_target(repo, &full)?.ok_or_else(|| {
        Error::coded(
            "branch/not-found",
            format!("no branch named {name}"),
            vec![],
        )
    })?;
    guard_other_worktrees(repo, name)?;
    // Read the shared copy now, while the tracking ref and config are still
    // readable: the delete keeps both, and this report is the only thing
    // that will name them.
    let shared = shared_copy(repo, name)?;

    let parked = crate::stash::parked_entry(repo, name)?;
    let mut planned = observe_refs(repo)?;
    planned.refs.remove(&full);
    let mut transitions = vec![RefTransition {
        name: full.clone(),
        old: Some(tip.to_string()),
        new: None,
    }];
    if let Some(parked) = parked {
        let parked_ref = crate::stash::parked_ref(name);
        planned.refs.remove(&parked_ref);
        transitions.push(RefTransition {
            name: parked_ref,
            old: Some(parked.to_string()),
            new: None,
        });
    }
    let mut record = OpRecord::new("branch", format!("delete branch {name}"), now);
    record.argv = argv;
    record.refs = transitions;
    let mut pins = vec![tip];
    pins.extend(parked);
    verb::append_op(
        repo,
        OpKind::Op,
        verb::VerbOp {
            record,
            planned,
            // Deleting some other branch leaves this worktree untouched.
            tree: ctx.pre_tree,
            index_tree: crate::index::tree_from_index(repo)?,
            branch: current.clone(),
            base: crate::snapshot::chain::base_commit(&head)?,
            session: prov.session.clone(),
            pins: &pins,
        },
        now,
    )?;

    // The pointer to trash first (never lose the way back), then the refs.
    let snap_ref = format!("{BRANCH_PREFIX}{name}");
    let mut trash: Option<String> = None;
    if let Some(snap_tip) = refs::ref_target(repo, &snap_ref)? {
        let trash_ref = format!("refs/fufu/trash/{name}");
        refs::write_ref(
            repo,
            &trash_ref,
            snap_tip,
            gix::refs::transaction::PreviousValue::Any,
            now,
            &format!("branch: pre-delete pointer of {name}"),
        )?;
        refs::delete_ref(repo, &snap_ref, snap_tip, now)?;
        trash = Some(trash_ref);
    }
    if let Some(parked) = parked {
        refs::delete_ref(repo, &crate::stash::parked_ref(name), parked, now)?;
    }
    refs::delete_ref(repo, &full, tip, now)?;
    // The metadata file goes too.
    crate::branchmeta::write(repo, name, &crate::branchmeta::BranchMeta::default())?;
    // And its cached future — a cache, so losing it costs recomputation only.
    crate::futures::cache::remove(repo, name)?;

    Ok((
        crate::model::BranchDeleteReport {
            name: name.to_string(),
            tip: tip.to_string(),
            trash_ref: trash,
            parked_demoted: parked.map(|p| p.to_string()),
            shared,
            pre_op: ctx.pre_op.map(|id| id.to_string()),
        },
        ctx,
    ))
}

/// Create a branch at a commit (no reflog history to carry).
pub fn create_at(
    repo: &gix::Repository,
    branch: &str,
    at: gix::ObjectId,
    now: i64,
    message: &str,
) -> Result<()> {
    validate_name(branch)?;
    let full = heads_ref(branch);
    if refs::ref_target(repo, &full)?.is_some() {
        return Err(Error::coded(
            "branch/exists",
            format!("a branch named {branch} already exists"),
            vec!["ff branch list".into()],
        ));
    }
    refs::write_ref(
        repo,
        &full,
        at,
        gix::refs::transaction::PreviousValue::MustNotExist,
        now,
        message,
    )
}
