//! `ff switch` — move between branches without ceremony. Dirty tree? The
//! open change is parked (tree memory). Arriving somewhere with a parked
//! change? It is resumed if it applies cleanly, else it stays parked and
//! the switch says so. The transition itself is always clean-tree→target
//! -tree by construction, so the two-step (clear, then transition) each
//! stay differentially tested.

use crate::branch;
use crate::error::{Error, Result};
use crate::journal::{self, OpKind, OpRecord, RefTransition, StashEffect};
use crate::model::{ArrivalReport, HeadState, SwitchReport};
use crate::snapshot::Provenance;
use crate::stash::{self, ArrivePlan};
use crate::worktree;

#[derive(Debug, Clone, Default)]
pub struct SwitchOptions {
    /// Branch name, or a unique prefix of one.
    pub target: String,
    /// Clock injection for tests.
    pub now: Option<i64>,
    /// The invoking argv, journaled verbatim.
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
        [] => Err(Error::msg(format!("no branch named {raw}"))),
        [one] => Ok((*one).clone()),
        many => {
            let list: Vec<&str> = many.iter().map(|n| n.as_str()).collect();
            Err(Error::msg(format!(
                "ambiguous branch prefix {raw}: {}",
                list.join(", ")
            )))
        }
    }
}

pub(crate) fn branch_names(repo: &gix::Repository) -> Result<Vec<String>> {
    let mut out = Vec::new();
    let platform = repo.references().map_err(Error::repo)?;
    let iter = platform.prefixed("refs/heads/").map_err(Error::repo)?;
    for reference in iter {
        let reference =
            reference.map_err(|err| Error::msg(format!("ref iteration failed: {err}")))?;
        let name = reference.name().as_bstr().to_string();
        if let Some(short) = name.strip_prefix("refs/heads/") {
            out.push(short.to_string());
        }
    }
    Ok(out)
}

/// Switch to another branch. Park if dirty, retarget HEAD, rewrite index
/// and worktree, then arrive (resume the target's parked change if any).
pub fn switch(
    repo: &gix::Repository,
    opts: &SwitchOptions,
    prov: &Provenance,
) -> Result<(SwitchReport, journal::VerbContext)> {
    if repo.workdir().is_none() {
        return Err(Error::msg("bare repository: nothing to switch"));
    }
    if let Some(op) = crate::head::operation(repo) {
        return Err(Error::msg(format!(
            "a {op:?} is in progress: finish or abort it with git before switching"
        )));
    }

    let ctx = journal::begin_verb(repo, prov, opts.now)?;
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
                pre_snapshot: ctx.pre_snapshot.clone(),
            },
            ctx,
        ));
    }
    let target_ref = format!("refs/heads/{target}");
    let target_commit = crate::refs::ref_target(repo, &target_ref)?
        .ok_or_else(|| Error::msg(format!("no branch named {target}")))?;
    let target_tree = repo
        .find_commit(target_commit)
        .map_err(Error::repo)?
        .tree_id()
        .map_err(Error::repo)?
        .detach();

    // Plan phase: park (object writes only) and arrival, before anything
    // moves — the journal entry describes the whole switch up front.
    let park_plan = stash::plan_park(repo, &head, now)?;
    let arrive_plan = stash::plan_arrival(repo, &target, target_commit, target_tree)?;

    // The planned post-switch world.
    let mut planned = journal::observe_refs(repo)?;
    let mut transitions: Vec<RefTransition> = Vec::new();
    let mut effects: Vec<StashEffect> = Vec::new();
    let head_old = planned.head.clone();
    planned.head = format!("ref:{target_ref}");

    let mut stash_lines: Vec<gix::ObjectId> = crate::refs::read_ref_log(repo, stash::STASH_REF)?
        .iter()
        .map(|l| l.new)
        .collect();
    if let Some(plan) = &park_plan {
        stash_lines.push(plan.wip_commit);
        planned
            .refs
            .insert(stash::parked_ref(&current), plan.wip_commit.to_string());
        transitions.push(RefTransition {
            name: stash::parked_ref(&current),
            old: None,
            new: Some(plan.wip_commit.to_string()),
        });
        effects.push(StashEffect::Push {
            branch: current.clone(),
            stash: plan.wip_commit.to_string(),
        });
    }
    match &arrive_plan {
        ArrivePlan::Restore { stash: sha, .. } => {
            if let Some(pos) = stash_lines.iter().rposition(|s| s == sha) {
                stash_lines.remove(pos);
            }
            planned.refs.remove(&stash::parked_ref(&target));
            transitions.push(RefTransition {
                name: stash::parked_ref(&target),
                old: Some(sha.to_string()),
                new: None,
            });
            effects.push(StashEffect::Drop {
                branch: target.clone(),
                stash: sha.to_string(),
            });
        }
        ArrivePlan::Invalidate { stash: sha } => {
            planned.refs.remove(&stash::parked_ref(&target));
            transitions.push(RefTransition {
                name: stash::parked_ref(&target),
                old: Some(sha.to_string()),
                new: None,
            });
        }
        ArrivePlan::None | ArrivePlan::Conflict { .. } => {}
    }
    match stash_lines.last() {
        Some(tip) => {
            planned.refs.insert("refs/stash".into(), tip.to_string());
        }
        None => {
            planned.refs.remove("refs/stash");
        }
    }

    let mut record = OpRecord::new(
        OpKind::Op,
        "switch",
        format!("switch from {current} to {target}"),
        now,
    );
    record.argv = opts.argv.clone();
    record.branch = Some(target.clone());
    record.pre_snapshot = ctx.pre_snapshot.clone();
    record.head = Some((head_old, format!("ref:{target_ref}")));
    record.refs = transitions;
    record.stash = effects;
    let index_tree = crate::index::tree_from_index(repo)?;
    record.index_tree = Some(index_tree.to_string());
    let mut pins = vec![target_commit];
    if let Some(plan) = &park_plan {
        pins.push(plan.wip_commit);
    }
    if let Some(pre) = &ctx.pre_snapshot {
        pins.push(gix::ObjectId::from_hex(pre.as_bytes()).map_err(Error::repo)?);
    }
    journal::append(repo, &record, &planned, index_tree, &pins, now)?;

    // Mutate: park, retarget, index, worktree, arrive — in that order.
    let parked_sha = match &park_plan {
        Some(plan) => {
            stash::execute_park(repo, plan)?;
            Some(plan.wip_commit.to_string())
        }
        None => None,
    };
    branch::retarget_head(repo, &target_ref, now)?;
    crate::index::write_index_for_tree(repo, target_tree)?;
    let from_tree = park_plan.as_ref().map(|p| p.head_tree).unwrap_or_else(|| {
        // Clean switch: the worktree matches the old HEAD tree.
        head_tree_of(repo, &head).unwrap_or(target_tree)
    });
    let everything = |_: &str| true;
    worktree::apply_tree_transition(repo, from_tree, target_tree, &everything)?;

    let arrival = stash::execute_arrival(repo, &target, &arrive_plan, target_tree, now)?;
    let arrival_report = match arrival {
        stash::Arrival::None => ArrivalReport::None,
        stash::Arrival::Restored { stash, files } => ArrivalReport::Restored { stash, files },
        stash::Arrival::Conflicted { stash, paths } => ArrivalReport::StillParked { stash, paths },
        stash::Arrival::Invalidated { stash } => ArrivalReport::Invalidated { stash },
    };

    Ok((
        SwitchReport {
            from: current,
            to: target,
            parked: parked_sha,
            arrival: arrival_report,
            pre_snapshot: ctx.pre_snapshot.clone(),
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
