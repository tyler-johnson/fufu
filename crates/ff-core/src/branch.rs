//! Branch operations. Phase 2 ships the rename core (the claim), list,
//! and delete; anonymous branches live under `refs/heads/ff/<petname>`.
//!
//! Rename is built by hand — gix has no native rename — as
//! create-new-with-replayed-reflog FIRST, then delete-old: a crash leaves
//! both names, never neither. The rename carries everything that makes the
//! branch a fufu change: the snap chain, the parked entry, and the
//! metadata file. Divergences from `git branch -m`, accepted and
//! documented: no trailing "Branch: renamed" reflog line (its old and new
//! values are equal, which the transaction machinery drops), and the first
//! replayed line's previous-value column is null.

use crate::error::{Error, Result};
use crate::journal::RefTransition;
use crate::model::HeadState;
use crate::refs;
use crate::snapshot::chain;
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
    let _: gix::refs::FullName = full
        .as_str()
        .try_into()
        .map_err(|err| Error::msg(format!("invalid branch name {branch:?}: {err}")))?;
    Ok(())
}

/// Refuse when `branch` is checked out in another worktree — git guards
/// this on rename/delete; gix has no such check, so fufu carries its own.
pub fn guard_other_worktrees(repo: &gix::Repository, branch: &str) -> Result<()> {
    let full = heads_ref(branch);
    for proxy in repo.worktrees().map_err(Error::repo)? {
        let head_path = repo
            .common_dir()
            .join("worktrees")
            .join(proxy.id().to_string())
            .join("HEAD");
        let Ok(contents) = std::fs::read_to_string(&head_path) else {
            continue;
        };
        if let Some(target) = contents.trim().strip_prefix("ref:")
            && target.trim() == full
        {
            let place = proxy
                .base()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|_| proxy.id().to_string());
            return Err(Error::msg(format!(
                "{branch} is checked out in another worktree ({place}); refusing"
            )));
        }
    }
    Ok(())
}

/// What a rename moved, for the journal.
#[derive(Debug, Default)]
pub struct RenameEffects {
    pub transitions: Vec<RefTransition>,
    /// HEAD symref retargeted from old to new.
    pub head_moved: Option<(String, String)>,
}

/// Rename `old` to `new`, carrying the snap chain, parked entry, and
/// metadata. This is the mechanism beneath the claim (`ff branch <name>`,
/// `ff describe -b`) and `-b` placeholder claims.
pub fn rename(repo: &gix::Repository, old: &str, new: &str, now: i64) -> Result<RenameEffects> {
    validate_name(new)?;
    let old_ref = heads_ref(old);
    let new_ref = heads_ref(new);
    let target = refs::ref_target(repo, &old_ref)?
        .ok_or_else(|| Error::msg(format!("no branch named {old}")))?;
    if refs::ref_target(repo, &new_ref)?.is_some() {
        return Err(Error::msg(format!("a branch named {new} already exists")));
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

    // 3. Snap chain replay (the timeline follows the change).
    let old_snap = format!("{}{old}", chain::SNAP_PREFIX);
    let new_snap = format!("{}{new}", chain::SNAP_PREFIX);
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

    // 6. Old branch ref goes last: a crash anywhere above leaves both
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
        refs::EditOutcome::Contended => Err(Error::msg("HEAD is contended")),
    }
}

/// List branches for `ff branch`: named and anonymous segregated, each with
/// its tip, parked marker, pending description, and upstream annotation.
pub fn list(repo: &gix::Repository) -> Result<crate::model::BranchList> {
    use crate::model::BranchInfo;
    let head = crate::head::head_state(repo)?;
    let current = crate::snapshot::chain::chain_name(&head);
    let mut named = Vec::new();
    let mut anonymous = Vec::new();
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
        let info = BranchInfo {
            name: name.clone(),
            current: name == current,
            anonymous: is_anonymous(&name),
            tip: tip.map(|id| id.to_string()),
            subject,
            parked: crate::stash::parked_entry(repo, &name)?.is_some(),
            pending_description: meta.pending_description,
            upstream,
        };
        if info.anonymous {
            anonymous.push(info);
        } else {
            named.push(info);
        }
    }
    Ok(crate::model::BranchList { named, anonymous })
}

/// `ff branch <name>` — claim the current anonymous branch, journaled.
pub fn claim_current(
    repo: &gix::Repository,
    new_name: &str,
    prov: &crate::snapshot::Provenance,
    now: Option<i64>,
    argv: Vec<String>,
) -> Result<(crate::model::ClaimReport, crate::journal::VerbContext)> {
    rename_current(repo, new_name, true, prov, now, argv)
}

/// Rename the current branch, journaled. `require_anonymous` is the claim
/// discipline (`ff branch <name>`); `ff describe -b` lifts it — the one
/// rename that may touch proper names.
pub fn rename_current(
    repo: &gix::Repository,
    new_name: &str,
    require_anonymous: bool,
    prov: &crate::snapshot::Provenance,
    now: Option<i64>,
    argv: Vec<String>,
) -> Result<(crate::model::ClaimReport, crate::journal::VerbContext)> {
    let ctx = crate::journal::begin_verb(repo, prov, now)?;
    let now = ctx.now;
    let head = crate::head::head_state(repo)?;
    let (current, tip) = match &head {
        HeadState::Branch { name, commit, .. } => (
            name.clone(),
            gix::ObjectId::from_hex(commit.as_bytes()).map_err(Error::repo)?,
        ),
        _ => return Err(Error::msg("not on a branch: nothing to claim")),
    };
    if require_anonymous && !is_anonymous(&current) {
        return Err(Error::msg(format!(
            "{current} already has a proper name; use ff describe -b {new_name} to rename it"
        )));
    }
    validate_name(new_name)?;
    if refs::ref_target(repo, &heads_ref(new_name))?.is_some() {
        return Err(Error::msg(format!(
            "a branch named {new_name} already exists"
        )));
    }
    guard_other_worktrees(repo, &current)?;

    // Write-ahead: the rename's effects are fully known before it runs.
    let mut planned = crate::journal::observe_refs(repo)?;
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
    let mut record =
        crate::journal::OpRecord::new(crate::journal::OpKind::Op, "branch", summary, now);
    record.argv = argv;
    record.branch = Some(new_name.to_string());
    record.pre_snapshot = ctx.pre_snapshot.clone();
    record.refs = transitions;
    record.head = Some((
        format!("ref:{}", heads_ref(&current)),
        format!("ref:{}", heads_ref(new_name)),
    ));
    let index_tree = crate::index::tree_from_index(repo)?;
    record.index_tree = Some(index_tree.to_string());
    crate::journal::append(repo, &record, &planned, index_tree, &[tip], now)?;

    rename(repo, &current, new_name, now)?;
    Ok((
        crate::model::ClaimReport {
            from: current,
            to: new_name.to_string(),
            pre_snapshot: ctx.pre_snapshot.clone(),
        },
        ctx,
    ))
}

/// `ff branch -d <name>` — delete a branch, journaled. The snap chain moves
/// to trash (trim's one-deep pattern), the parked entry is demoted (its
/// stash entry survives), and the tip stays pinned by the journal — so the
/// deletion is undoable, which is why there is no merged-check: nothing is
/// lost.
pub fn delete(
    repo: &gix::Repository,
    name: &str,
    prov: &crate::snapshot::Provenance,
    now: Option<i64>,
    argv: Vec<String>,
) -> Result<(
    crate::model::BranchDeleteReport,
    crate::journal::VerbContext,
)> {
    let ctx = crate::journal::begin_verb(repo, prov, now)?;
    let now = ctx.now;
    let head = crate::head::head_state(repo)?;
    let current = crate::snapshot::chain::chain_name(&head);
    if name == current {
        return Err(Error::msg(format!(
            "{name} is the current branch; switch away before deleting it"
        )));
    }
    let full = heads_ref(name);
    let tip = refs::ref_target(repo, &full)?
        .ok_or_else(|| Error::msg(format!("no branch named {name}")))?;
    guard_other_worktrees(repo, name)?;

    let parked = crate::stash::parked_entry(repo, name)?;
    let mut planned = crate::journal::observe_refs(repo)?;
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
    let mut record = crate::journal::OpRecord::new(
        crate::journal::OpKind::Op,
        "branch",
        format!("delete branch {name}"),
        now,
    );
    record.argv = argv;
    record.branch = Some(current);
    record.pre_snapshot = ctx.pre_snapshot.clone();
    record.refs = transitions;
    let index_tree = crate::index::tree_from_index(repo)?;
    record.index_tree = Some(index_tree.to_string());
    let mut pins = vec![tip];
    pins.extend(parked);
    crate::journal::append(repo, &record, &planned, index_tree, &pins, now)?;

    // Snap chain to trash first (never lose the timeline), then the refs.
    let snap_ref = format!("{}{name}", chain::SNAP_PREFIX);
    let mut trash: Option<String> = None;
    if let Some(snap_tip) = refs::ref_target(repo, &snap_ref)? {
        let trash_ref = chain::trash_ref(name);
        refs::write_ref(
            repo,
            &trash_ref,
            snap_tip,
            gix::refs::transaction::PreviousValue::Any,
            now,
            &format!("branch: pre-delete chain of {name}"),
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

    Ok((
        crate::model::BranchDeleteReport {
            name: name.to_string(),
            tip: tip.to_string(),
            trash_ref: trash,
            parked_demoted: parked.map(|p| p.to_string()),
            pre_snapshot: ctx.pre_snapshot.clone(),
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
        return Err(Error::msg(format!(
            "a branch named {branch} already exists"
        )));
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
