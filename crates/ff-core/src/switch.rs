//! `ff switch` — move between branches without ceremony. A dirty tree's open
//! change stays where the capture already put it, the open commit at
//! `refs/fufu/open/<branch>`, and that commit is the park: leaving writes
//! nothing. Arriving somewhere with a parked change lays it back over the
//! tip, replays it when the tip moved, and holds the branch when the replay
//! conflicts — see [`crate::park`]. The transition itself is always
//! worktree→target-tree by construction, so the two halves (the move, then
//! the arrival) each stay differentially tested.

use crate::branch;
use crate::error::{Error, Result};
use crate::model::{ArrivalReport, HeadState, SwitchReport};
use crate::open;
use crate::ops::record::observe_refs;
use crate::ops::{OpKind, OpRecord, verb};
use crate::park;
use crate::snapshot::Provenance;
use crate::stash;
use crate::worktree;

#[derive(Debug, Clone, Default)]
pub struct SwitchOptions {
    /// Branch name, or a unique prefix of one.
    pub target: String,
    /// Clock injection for tests.
    pub now: Option<i64>,
    /// The invoking argv, recorded verbatim.
    pub argv: Vec<String>,
}

/// Resolve a switch target: exact branch name, or a unique prefix of one.
pub fn resolve_branch(repo: &gix::Repository, raw: &str) -> Result<String> {
    let names = branch_names(repo)?;
    if names.iter().any(|n| n == raw) {
        return Ok(raw.to_string());
    }
    let matches: Vec<&String> = names.iter().filter(|n| n.starts_with(raw)).collect();
    match matches.as_slice() {
        [] => Err(Error::coded(
            "branch/not-found",
            format!("no branch named {raw}"),
            vec![],
        )),
        [one] => Ok((*one).clone()),
        many => {
            let list: Vec<&str> = many.iter().map(|n| n.as_str()).collect();
            Err(Error::coded(
                "branch/ambiguous",
                format!("ambiguous branch prefix {raw}: {}", list.join(", ")),
                vec!["ff branch".into()],
            ))
        }
    }
}

/// Every local branch by short name, in the order the ref namespace lists
/// them.
pub fn branch_names(repo: &gix::Repository) -> Result<Vec<String>> {
    let mut out = Vec::new();
    let platform = repo.references().map_err(Error::repo)?;
    let iter = platform.prefixed("refs/heads/").map_err(Error::repo)?;
    for reference in iter {
        let reference = reference.map_err(|err| {
            Error::coded(
                "op/unreadable",
                format!("ref iteration failed: {err}"),
                vec![],
            )
        })?;
        let name = reference.name().as_bstr().to_string();
        if let Some(short) = name.strip_prefix("refs/heads/") {
            out.push(short.to_string());
        }
    }
    Ok(out)
}

/// Switch to another branch. A dirty tree's open commit is the park;
/// retarget HEAD, rewrite index and worktree, then arrive (resume the
/// target's parked change if any).
pub fn switch(
    repo: &gix::Repository,
    opts: &SwitchOptions,
    prov: &Provenance,
) -> Result<(SwitchReport, verb::VerbContext)> {
    if repo.workdir().is_none() {
        return Err(Error::coded(
            "repo/bare",
            "bare repository: nothing to switch",
            vec![],
        ));
    }
    if let Some(op) = crate::head::operation(repo) {
        return Err(Error::coded(
            "repo/mid-operation",
            format!("a {op:?} is in progress: finish or abort it with git before switching"),
            vec![],
        ));
    }

    let ctx = verb::begin_verb(repo, prov, opts.now)?;
    let now = ctx.now;

    let head = crate::head::head_state(repo)?;
    let current = crate::snapshot::chain::chain_name(&head);
    let target = resolve_branch(repo, &opts.target)?;
    if target == current {
        return Ok((
            SwitchReport {
                from: current,
                to: target,
                parked: None,
                arrival: ArrivalReport::None,
                pre_op: ctx.pre_op.map(|id| id.to_string()),
            },
            ctx,
        ));
    }
    // Opening a branch another worktree holds would leave two trees on one
    // branch — a state git refuses to create.
    branch::guard_other_worktrees(repo, &target)?;
    let target_ref = format!("refs/heads/{target}");
    let target_commit = crate::refs::ref_target(repo, &target_ref)?.ok_or_else(|| {
        Error::coded(
            "branch/not-found",
            format!("no branch named {target}"),
            vec![],
        )
    })?;
    let target_tree = repo
        .find_commit(target_commit)
        .map_err(Error::repo)?
        .tree_id()
        .map_err(Error::repo)?
        .detach();

    // The park: the open commit the preamble's capture wrote for the tree
    // it is leaving, when the tree is dirty. Nothing is written for it; a
    // dirty tree with no commit to stand as its park refuses before anything
    // moves.
    let head_commit = crate::snapshot::chain::base_commit(&head)?;
    let head_tree = head_tree_of(repo, &head).unwrap_or(target_tree);
    let parked = if ctx.pre_tree == head_tree {
        None
    } else {
        Some(
            open::current(repo, &current, ctx.pre_tree, head_commit)?
                .ok_or_else(|| park::no_park(&head, &current))?,
        )
    };

    // Plan phase: the arrival, before anything moves — the operation
    // describes the whole switch up front.
    let arrive = park::plan_arrival(repo, &target, target_commit, target_tree, now)?;

    // The planned post-switch world.
    let mut planned = observe_refs(repo)?;
    let head_old = planned.head.clone();
    planned.head = format!("ref:{target_ref}");

    let mut record = OpRecord::new("switch", format!("switch from {current} to {target}"), now);
    record.argv = opts.argv.clone();
    record.head = Some((head_old, format!("ref:{target_ref}")));
    let mut pins = vec![target_commit];
    pins.extend(parked);
    pins.extend(ctx.pre_op.map(|id| id.object_id()));
    let mut stash_lines = stash::lines(repo)?;
    arrive.fold_into(&mut planned, &mut record, &mut pins, &mut stash_lines);
    // The planned end state: the destination's tree, unless a parked change is
    // about to be laid back over it — in which case that is what the working
    // tree will hold, and saying "target tree" would make an undo of the next
    // operation throw the resumed change away.
    let (end_tree, end_index) = arrive.end_trees(target_tree);
    verb::append_op_hinted(
        repo,
        OpKind::Op,
        verb::VerbOp {
            record,
            planned,
            tree: end_tree,
            index_tree: end_index,
            // The destination, not the origin: the pointer that moves is the
            // one the next capture on this worktree will read.
            branch: target.clone(),
            base: head_commit,
            session: prov.session.clone(),
            pins: &pins,
        },
        arrive.open_hint(),
        now,
    )?;

    // Mutate: retarget, index, worktree, arrive — in that order. The
    // worktree holds the pre-verb capture's tree, untracked files included,
    // so one transition clears the slate.
    branch::retarget_head(repo, &target_ref, now)?;
    crate::index::write_index_for_tree(repo, target_tree)?;
    let everything = |_: &str| true;
    worktree::apply_tree_transition(repo, ctx.pre_tree, target_tree, &everything)?;
    let arrival = park::execute_arrival(repo, &target, &arrive, target_tree, now)?;

    Ok((
        SwitchReport {
            from: current,
            to: target,
            parked: parked.map(|id| id.to_string()),
            arrival,
            pre_op: ctx.pre_op.map(|id| id.to_string()),
        },
        ctx,
    ))
}

fn head_tree_of(repo: &gix::Repository, head: &HeadState) -> Option<gix::ObjectId> {
    match head {
        HeadState::Unborn { .. } => Some(gix::ObjectId::empty_tree(repo.object_hash())),
        HeadState::Branch { commit, .. } | HeadState::Detached { commit } => {
            let id = gix::ObjectId::from_hex(commit.as_bytes()).ok()?;
            repo.find_commit(id)
                .ok()?
                .tree_id()
                .ok()
                .map(|t| t.detach())
        }
    }
}
