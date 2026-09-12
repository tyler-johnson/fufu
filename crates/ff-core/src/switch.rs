//! `ff switch` — the one verb for moving between lines of work, and `ff
//! start` and `ff new` are spellings of it. The rule under every spelling
//! is one sentence: find the branch, else mint it. The target ladder is an
//! exact local branch, then a unique local prefix, then a branch a remote
//! holds (bare or qualified), then a revision; no target is trunk's tip. A
//! branch found is continued; a remote's branch is minted here under its
//! own name, tracking it; a revision mints an anonymous branch at it; `-b`
//! forks a branch target instead of continuing it, or names the mint.
//!
//! A dirty tree's open change stays where the capture already put it, the
//! open commit at `refs/fufu/open/<branch>`, and that commit is the park:
//! leaving writes nothing. Arriving somewhere with a parked change lays it
//! back over the tip, replays it when the tip moved, and holds the branch
//! when the replay conflicts — see [`crate::park`]. A minted branch opens
//! clean. The one target that carries anything is `@`: the fork lands under
//! the open change and the new branch receives a copy of it — the same open
//! commit, id, birth, and description — while the branch left behind keeps
//! its own, parked. No spelling produces a commit, and every invocation is
//! one operation: the mint, the copy, and the move go back together under
//! one `ff undo`.
//!
//! The transition itself is always worktree→target-tree by construction, so
//! the two halves (the move, then the arrival) each stay differentially
//! tested.

use crate::branch;
use crate::branchmeta;
use crate::error::{Error, Result};
use crate::model::{ArrivalReport, HeadState, Minted, SwitchReport};
use crate::open;
use crate::ops::record::observe_refs;
use crate::ops::{
    ChangeIdTransition, DescriptionTransition, OpKind, OpRecord, RefTransition, UpstreamTransition,
    verb,
};
use crate::park;
use crate::refs;
use crate::revset::{Rev, Revset};
use crate::snapshot::Provenance;
use crate::stash;
use crate::worktree;

#[derive(Debug, Clone, Default)]
pub struct SwitchOptions {
    /// None: mint at trunk's tip.
    pub target: Option<String>,
    /// -m: pending description for the change being opened. Refused on a
    /// continue.
    pub message: Option<String>,
    /// -b: None = not given; Some(None) = bare (mint anonymous, fork a
    /// branch target); Some(Some(name)) = named.
    pub branch: Option<Option<String>>,
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

/// What the target resolved to: a branch here to continue, or a branch to
/// mint.
enum Target {
    Continue(String),
    Mint(MintPlan),
}

/// The branch a switch is about to mint: what to call it, where it lands,
/// and what its metadata records about where it came from.
struct MintPlan {
    /// `None` mints a petname.
    name: Option<String>,
    at: gix::ObjectId,
    /// A branch name or a short sha, for the metadata and the report. `None`
    /// on a tracking mint, which forked from nothing: it continues the
    /// remote's branch.
    forked_from: Option<String>,
    /// The branch the target named, when it named one.
    parent: Option<String>,
    /// `(origin, origin/spike)` when the mint continues a remote's branch
    /// under its own name.
    tracking: Option<(String, String)>,
    /// The target resolved to `@`.
    open: bool,
}

/// The ladder, in one place: an exact local branch, a unique local prefix,
/// a branch a remote holds, a revision. `fork` is `-b` as given: its
/// presence turns a branch target into a fork, and its name, when it has
/// one, names the mint. `under` is the commit under the open change, what
/// `@` forks at.
fn resolve_target(
    repo: &gix::Repository,
    target: Option<&str>,
    fork: Option<&Option<String>>,
    under: Option<gix::ObjectId>,
) -> Result<Target> {
    let named = fork.and_then(|name| name.clone());
    let Some(raw) = target else {
        let point = resolve_fork_point(repo, None, under)?;
        return Ok(Target::Mint(MintPlan {
            name: named,
            at: point.at,
            forked_from: Some(point.forked_from),
            parent: None,
            tracking: None,
            open: false,
        }));
    };

    // 1. A branch here. Built from the ref directly rather than through the
    // revset: the ladder says the local branch wins, so a name that is also
    // an object is the branch.
    let not_found = match resolve_branch(repo, raw) {
        Ok(name) => {
            if fork.is_none() {
                return Ok(Target::Continue(name));
            }
            let at = refs::ref_target(repo, &format!("refs/heads/{name}"))?.ok_or_else(|| {
                Error::coded(
                    "branch/not-found",
                    format!("no branch named {name}"),
                    vec![],
                )
            })?;
            return Ok(Target::Mint(MintPlan {
                name: named,
                at,
                forked_from: Some(name.clone()),
                parent: Some(name),
                tracking: None,
                open: false,
            }));
        }
        Err(err) if err.id() == "branch/not-found" => err,
        Err(err) => return Err(err),
    };

    // 2. A branch a remote holds. Under the local name it becomes a branch
    // here, tracking the remote's; under `-b` it is a fork like any other
    // branch target, with no upstream.
    if let Some(hit) = remote_branch_of(repo, raw)? {
        let local_exists = refs::ref_target(repo, &format!("refs/heads/{}", hit.local))?.is_some();
        return Ok(match (local_exists, fork) {
            (true, None) => Target::Continue(hit.local),
            (_, Some(_)) => Target::Mint(MintPlan {
                name: named,
                at: hit.tip,
                forked_from: Some(hit.short.clone()),
                parent: Some(hit.short),
                tracking: None,
                open: false,
            }),
            (false, None) => {
                branch::validate_name(&hit.local)?;
                Target::Mint(MintPlan {
                    name: Some(hit.local),
                    at: hit.tip,
                    forked_from: None,
                    parent: None,
                    tracking: Some((hit.remote, hit.short)),
                    open: false,
                })
            }
        });
    }

    // 3. A revision. A bare word that names nothing anywhere is the branch
    // rung's refusal — somebody typing a branch name with a typo in it is
    // not asking about revisions — and every other refusal (ambiguous, not
    // a point, syntax) is the revset's own.
    match resolve_fork_point(repo, Some(raw), under) {
        Ok(point) => Ok(Target::Mint(MintPlan {
            name: named,
            at: point.at,
            forked_from: Some(point.forked_from),
            parent: point.parent,
            tracking: None,
            open: point.open,
        })),
        Err(err) if err.id() == "usage/revset-unknown-revision" => Err(not_found),
        Err(err) => Err(err),
    }
}

/// One branch a remote holds, found by a target.
struct RemoteHit {
    /// `origin`
    remote: String,
    /// `origin/spike` — the spelling every report uses.
    short: String,
    /// `spike` — the name the branch takes here.
    local: String,
    tip: gix::ObjectId,
}

/// The remote rung: `raw` qualified as `<remote>/<branch>` for a configured
/// remote, else bare as `<branch>` under exactly one remote. Two or more
/// remotes holding the bare name is refused, listing the qualified
/// spellings. `<remote>/HEAD` is symbolic and is not a branch: it falls
/// through to the revision rung, as it always has.
fn remote_branch_of(repo: &gix::Repository, raw: &str) -> Result<Option<RemoteHit>> {
    let remotes: Vec<String> = repo
        .remote_names()
        .into_iter()
        .map(|name| name.to_string())
        .collect();
    for remote in &remotes {
        let Some(local) = raw.strip_prefix(&format!("{remote}/")) else {
            continue;
        };
        let full = format!("refs/remotes/{raw}");
        if refs::is_symbolic(repo, &full)? {
            continue;
        }
        if let Some(tip) = refs::ref_target(repo, &full)? {
            return Ok(Some(RemoteHit {
                remote: remote.clone(),
                short: raw.to_string(),
                local: local.to_string(),
                tip,
            }));
        }
    }
    let mut hits = Vec::new();
    for remote in &remotes {
        let full = format!("refs/remotes/{remote}/{raw}");
        if refs::is_symbolic(repo, &full)? {
            continue;
        }
        if let Some(tip) = refs::ref_target(repo, &full)? {
            hits.push(RemoteHit {
                remote: remote.clone(),
                short: format!("{remote}/{raw}"),
                local: raw.to_string(),
                tip,
            });
        }
    }
    if hits.len() > 1 {
        let list: Vec<&str> = hits.iter().map(|hit| hit.short.as_str()).collect();
        return Err(Error::coded(
            "branch/ambiguous",
            format!("ambiguous branch name {raw}: {}", list.join(", ")),
            vec!["ff branch".into()],
        ));
    }
    Ok(hits.pop())
}

/// Where a minted branch forks from.
#[derive(Debug)]
pub(crate) struct ForkPoint {
    pub at: gix::ObjectId,
    /// A branch name when the fork point came from one, else a short sha.
    pub forked_from: String,
    /// The branch the user explicitly forked from, when the target named one
    /// — local, or someone else's by way of a tracking ref. `None` for a bare
    /// (trunk) mint and for a target that resolved to a bare commit.
    pub parent: Option<String>,
    /// The target resolved to `@`, however it was spelled: the fork lands
    /// under the open change, and the caller carries a copy of it.
    pub open: bool,
}

/// Resolve the fork point, never guessing: the target is a revset that has to
/// name exactly one revision, and the revset resolver is the only thing here
/// that reads it.
///
/// `under` is the commit under the open change, what `@` forks at; `None`
/// on an unborn branch, where `@` has nothing under it and is refused.
///
/// It used to try branch names first and hand anything else to git's own
/// parser, which meant a name that was both a branch and a commit forked at
/// the branch and said nothing about the commit it ignored. That precedence
/// is the bug the revset resolver exists to refuse: it looks a base up in
/// both address spaces unconditionally and names both candidates rather than
/// ranking them. (This file mentions git's parser by description rather than
/// by name on purpose — the guard test in `revset::resolve` greps for it.)
/// The switch's own ladder runs a local branch name through the ref before
/// this, so what reaches here is what no branch claimed.
pub(crate) fn resolve_fork_point(
    repo: &gix::Repository,
    target: Option<&str>,
    under: Option<gix::ObjectId>,
) -> Result<ForkPoint> {
    match target {
        None => {
            let t = crate::trunk::trunk(repo)?;
            let at = refs::ref_target(repo, &t.full_ref)?.ok_or_else(|| {
                Error::coded(
                    "target/unresolvable",
                    format!("trunk ref {} has no target", t.full_ref),
                    vec!["ff branch".into()],
                )
            })?;
            Ok(ForkPoint {
                at,
                forked_from: t.name,
                parent: None,
                open: false,
            })
        }
        Some(raw) => {
            let point = Revset::parse(raw)?.point(repo)?;
            // Decided on the *resolved* revision rather than on the literal
            // "@": `latest(@)` and `heads(@)` were never a different request,
            // and a check on the spelling would have let them through.
            let (at, open) = match (point.rev, under) {
                (Rev::Open(_), Some(id)) => (id, true),
                (Rev::Open(_), None) => {
                    return Err(Error::coded(
                        "target/unresolvable",
                        "@ has no commit under it yet",
                        vec!["ff commit".into()],
                    ));
                }
                (Rev::Commit(id), _) => (id.object_id(), false),
            };
            // A branch name reports the branch; anything else reports the
            // commit it landed on, in the spelling `ff log` prints. The
            // resolver decides which — it already knows whether the whole
            // expression was one branch's tip.
            //
            // `point.name` is exactly a *local* branch name; a tracking ref
            // earns only `full_ref`, and it earns a name here too. Forking
            // from someone else's branch records them as the parent, so the
            // minted branch has a base to be measured against — shortened to
            // `origin/feature`, which is how every report spells it.
            let named = point.name.clone().or_else(|| {
                point
                    .full_ref
                    .as_deref()
                    .and_then(|full| full.strip_prefix("refs/remotes/"))
                    .map(str::to_string)
            });
            let parent = named.clone();
            let forked_from = named.unwrap_or_else(|| crate::sha::short_oid(at));
            Ok(ForkPoint {
                at,
                forked_from,
                parent,
                open,
            })
        }
    }
}

/// `-m`'s text through the normalization `ff describe` applies: trailing
/// whitespace dropped, and an empty message is no message.
fn normalize_message(text: Option<&str>) -> Option<String> {
    text.map(|t| t.trim_end().to_string())
        .filter(|t| !t.is_empty())
}

/// Switch to a branch, minting it first when there is none here to
/// continue. See the module docs. One entry, two arms: a branch found is
/// continued — HEAD retargeted, index and worktree rewritten, the target's
/// parked change resumed — and a branch minted is recorded in the same
/// shape, on the new branch, with the ref it mints, the HEAD move, and
/// under `@` the description and id the copy wears, so one `ff undo` takes
/// the whole of it back.
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
    if let Some(Some(name)) = &opts.branch {
        branch::validate_name(name)?;
        if refs::ref_target(repo, &format!("refs/heads/{name}"))?.is_some() {
            return Err(Error::coded(
                "branch/exists",
                format!("a branch named {name} already exists"),
                vec!["ff branch".into()],
            ));
        }
    }

    let head = crate::head::head_state(repo)?;
    let head_commit = crate::snapshot::chain::base_commit(&head)?;
    let target = resolve_target(
        repo,
        opts.target.as_deref(),
        opts.branch.as_ref(),
        head_commit,
    )?;
    if let Target::Continue(name) = &target
        && opts.message.is_some()
    {
        return Err(Error::coded(
            "switch/nothing-opened",
            format!(
                "-m describes the change a switch opens, and switching to {name} opens nothing: \
                 it resumes what is there"
            ),
            vec!["ff describe -m <msg>".into()],
        ));
    }

    // The preamble: a dirty tree's park is its open commit, and the capture
    // is what writes one — the append below finds it there when a copy
    // reuses it, and `park_of` refuses when there is none to stand.
    let ctx = verb::begin_verb(repo, prov, opts.now)?;
    let current = crate::snapshot::chain::chain_name(&head);
    match target {
        Target::Continue(name) => continue_on(repo, opts, prov, ctx, &head, current, name),
        Target::Mint(plan) => mint_and_switch(repo, opts, prov, ctx, &head, current, plan),
    }
}

/// The continue arm: the branch is here, so move there and arrive.
fn continue_on(
    repo: &gix::Repository,
    opts: &SwitchOptions,
    prov: &Provenance,
    ctx: verb::VerbContext,
    head: &HeadState,
    current: String,
    target: String,
) -> Result<(SwitchReport, verb::VerbContext)> {
    let now = ctx.now;
    if target == current {
        return Ok((
            SwitchReport {
                from: current,
                to: target,
                parked: None,
                minted: None,
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
    let target_commit = refs::ref_target(repo, &target_ref)?.ok_or_else(|| {
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

    let head_commit = crate::snapshot::chain::base_commit(head)?;
    let parked = park_of(repo, head, &current, ctx.pre_tree)?;

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

    // Mutate: retarget, index, worktree, arrive — in that order.
    move_worktree(
        repo,
        &target_ref,
        ctx.pre_tree,
        target_tree,
        target_tree,
        now,
    )?;
    let arrival = park::execute_arrival(repo, &target, &arrive, target_tree, now)?;

    Ok((
        SwitchReport {
            from: current,
            to: target,
            parked: parked.map(|id| id.to_string()),
            minted: None,
            arrival,
            pre_op: ctx.pre_op.map(|id| id.to_string()),
        },
        ctx,
    ))
}

/// The mint arm: the branch is not here, so make it and move there. No
/// arrival: the branch is fresh, and what it holds is the copy the append
/// already wrote, when the target was `@`.
fn mint_and_switch(
    repo: &gix::Repository,
    opts: &SwitchOptions,
    prov: &Provenance,
    ctx: verb::VerbContext,
    head: &HeadState,
    current: String,
    plan: MintPlan,
) -> Result<(SwitchReport, verb::VerbContext)> {
    let now = ctx.now;
    let name = match plan.name {
        Some(name) => name,
        None => crate::petname::mint(repo)?,
    };
    let head_commit = crate::snapshot::chain::base_commit(head)?;
    let parked = park_of(repo, head, &current, ctx.pre_tree)?;
    let carry = match (plan.open, parked) {
        (true, Some(open)) => Some(open),
        _ => None,
    };

    // What the opened change wears. `-m` wins; a carried copy otherwise keeps
    // the original's description, and keeps its id and birth either way —
    // it is the same change on two branches. A described change has an
    // identity from the start, as `ff describe` rules.
    let current_meta = branchmeta::read(repo, &current)?;
    let description = match (normalize_message(opts.message.as_deref()), carry) {
        (Some(text), _) => Some(text),
        (None, Some(_)) => current_meta.pending_description.clone(),
        (None, None) => None,
    };
    let (change_id, change_born) = match (carry, &description) {
        (Some(_), _) => (current_meta.change_id.clone(), current_meta.change_born),
        (None, Some(_)) => (
            Some(crate::changeid::ChangeId::mint()?.letters()),
            Some(now),
        ),
        (None, None) => (None, None),
    };

    let fork_tree = repo
        .find_commit(plan.at)
        .map_err(Error::repo)?
        .tree_id()
        .map_err(Error::repo)?
        .detach();
    let target_ref = format!("refs/heads/{name}");
    let mut planned = observe_refs(repo)?;
    let head_old = planned.head.clone();
    planned.refs.insert(target_ref.clone(), plan.at.to_string());
    planned.head = format!("ref:{target_ref}");

    // The mint stays said in the summary, whatever the spelling: `ff op log`
    // is where it is read.
    let short = crate::sha::short_oid(plan.at);
    let mut summary = format!("mint {name} at {short} and switch from {current}");
    if carry.is_some() {
        summary.push_str(", carrying the open change");
    }
    if let Some((_, tracked)) = &plan.tracking {
        summary.push_str(&format!(", tracking {tracked}"));
    }
    let mut record = OpRecord::new("switch", summary, now);
    record.argv = opts.argv.clone();
    record.refs = vec![RefTransition {
        name: target_ref.clone(),
        old: None,
        new: Some(plan.at.to_string()),
    }];
    record.head = Some((head_old, format!("ref:{target_ref}")));
    record.description = description.clone().map(|new| DescriptionTransition {
        branch: name.clone(),
        old: None,
        new: Some(new),
    });
    record.change_id = change_id.clone().map(|new| ChangeIdTransition {
        branch: name.clone(),
        old: None,
        new: Some(new),
        old_born: None,
        new_born: change_born,
    });
    // A tracking mint sets the branch's upstream, and a stale section a plain
    // `ff branch -d` left behind is what it replaces: recorded, so undo puts
    // the stale one back rather than none.
    if let Some((remote, _)) = &plan.tracking {
        record.upstream = Some(UpstreamTransition {
            branch: name.clone(),
            old: crate::snapshot::config::branch_remote(repo, &name)?,
            new: Some(remote.clone()),
        });
    }
    // The end state: the fork's tree, or the worktree as it stands when the
    // copy rides along — the index is the fork's either way.
    let end_tree = match carry {
        Some(_) => ctx.pre_tree,
        None => fork_tree,
    };
    let mut pins = vec![plan.at];
    pins.extend(parked);
    pins.extend(ctx.pre_op.map(|id| id.object_id()));
    clear_stale_open(repo, &name, now)?;
    verb::append_op_hinted(
        repo,
        OpKind::Op,
        verb::VerbOp {
            record,
            planned,
            tree: end_tree,
            index_tree: fork_tree,
            // Recorded on the destination, as a continue is: the pointer that
            // moves is the one the next capture on this worktree will read,
            // and the append writes the new branch's open commit from it.
            branch: name.clone(),
            base: head_commit,
            session: prov.session.clone(),
            pins: &pins,
        },
        carry,
        now,
    )?;

    // Mutate: the branch and its metadata, then HEAD, index, and worktree.
    mint_branch(
        repo,
        &Mint {
            name: &name,
            at: plan.at,
            forked_from: plan.forked_from.as_deref(),
            parent: plan.parent.as_deref(),
            tracking: plan.tracking.as_ref().map(|(_, short)| short.as_str()),
            opened: Opened {
                description,
                change_id,
                born: change_born,
            },
        },
        now,
    )?;
    if let Some((remote, _)) = &plan.tracking {
        crate::snapshot::config::set_branch_upstream(repo, &name, remote)?;
    }
    move_worktree(repo, &target_ref, ctx.pre_tree, fork_tree, end_tree, now)?;

    let carried = match carry {
        Some(_) => refs::ref_target(repo, &open::open_ref(&name))?.map(|id| id.to_string()),
        None => None,
    };

    Ok((
        SwitchReport {
            // The park line names where the change *was*, which is the branch
            // underfoot and not the fork source: standing on `alpha` and
            // forking from `main`, what got parked was alpha's.
            from: current,
            to: name,
            parked: parked.map(|id| id.to_string()),
            minted: Some(Minted {
                forked_from: plan.forked_from,
                parent: plan.parent,
                tracking: plan.tracking.map(|(_, short)| short),
                carried,
            }),
            arrival: ArrivalReport::None,
            pre_op: ctx.pre_op.map(|id| id.to_string()),
        },
        ctx,
    ))
}

/// The change a minted branch opens with: what `-m` said, or the copy of
/// the open change it carries. All `None` for a branch that opens clean.
#[derive(Clone, Default)]
pub(crate) struct Opened {
    pub description: Option<String>,
    pub change_id: Option<String>,
    pub born: Option<i64>,
}

/// What one minted branch is: the name and where it lands, the fork base
/// its metadata records, and the change it opens with.
#[derive(Clone)]
pub(crate) struct Mint<'a> {
    pub name: &'a str,
    pub at: gix::ObjectId,
    /// `None` on a tracking mint, which forked from nothing.
    pub forked_from: Option<&'a str>,
    pub parent: Option<&'a str>,
    /// `origin/spike` when the mint continues a remote's branch: the reflog
    /// says so instead of naming a fork.
    pub tracking: Option<&'a str>,
    pub opened: Opened,
}

/// Mint a branch at a commit, with its fork base written once. The
/// mutation only: the caller has already appended the operation that
/// records it, write-ahead, so the planned table already contains the
/// branch this is about to create.
pub(crate) fn mint_branch(repo: &gix::Repository, mint: &Mint<'_>, now: i64) -> Result<()> {
    let Mint {
        name,
        at,
        forked_from,
        parent,
        tracking,
        opened,
    } = mint;
    let reflog = match (tracking, forked_from) {
        (Some(tracked), _) => format!("branch: created from {tracked}"),
        (None, Some(from)) => format!("branch: forked from {from}"),
        (None, None) => "branch: created".to_string(),
    };
    branch::create_at(repo, name, *at, now, &reflog)?;
    branchmeta::write(
        repo,
        name,
        &branchmeta::BranchMeta {
            pending_description: opened.description.clone(),
            change_id: opened.change_id.clone(),
            change_born: opened.born,
            forked_from: forked_from.map(str::to_string),
            parent: parent.map(str::to_string),
            session: None,
            held: None,
            resolving: None,
        },
    )?;
    Ok(())
}

/// A stale open ref under a name whose branch is gone — left by a delete
/// that never resynced — would be read as the branch's park by the append;
/// the name is being minted fresh, so it holds nothing yet.
pub(crate) fn clear_stale_open(repo: &gix::Repository, name: &str, now: i64) -> Result<()> {
    if refs::ref_target(repo, &format!("refs/heads/{name}"))?.is_none() {
        open::clear(repo, name, now)?;
    }
    Ok(())
}

/// The park a verb leaving `current` leaves behind: `None` on a clean tree,
/// else the open commit the preamble's capture wrote for `pre_tree`. Nothing
/// is written for it; a dirty tree with no commit to stand as its park
/// refuses before anything moves.
pub(crate) fn park_of(
    repo: &gix::Repository,
    head: &HeadState,
    current: &str,
    pre_tree: gix::ObjectId,
) -> Result<Option<gix::ObjectId>> {
    let head_commit = crate::snapshot::chain::base_commit(head)?;
    if head_tree_of(repo, head) == Some(pre_tree) {
        return Ok(None);
    }
    Ok(Some(
        open::current(repo, current, pre_tree, head_commit)?
            .ok_or_else(|| park::no_park(head, current))?,
    ))
}

/// Retarget HEAD, rewrite the index to `index_tree`, and move the worktree
/// from `from_tree` to `end_tree` — in that order. The worktree holds the
/// pre-verb capture's tree, untracked files included, so one transition
/// clears the slate.
pub(crate) fn move_worktree(
    repo: &gix::Repository,
    target_ref: &str,
    from_tree: gix::ObjectId,
    index_tree: gix::ObjectId,
    end_tree: gix::ObjectId,
    now: i64,
) -> Result<()> {
    branch::retarget_head(repo, target_ref, now)?;
    crate::index::write_index_for_tree(repo, index_tree)?;
    let everything = |_: &str| true;
    worktree::apply_tree_transition(repo, from_tree, end_tree, &everything)?;
    Ok(())
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

#[cfg(test)]
mod tests {
    use ff_testsupport::Fixture;

    use super::*;

    fn one_commit() -> (Fixture, String) {
        let fx = Fixture::new();
        fx.write("a.txt", "a\n");
        let sha = fx.commit("init");
        (fx, sha)
    }

    fn oid(sha: &str) -> gix::ObjectId {
        gix::ObjectId::from_hex(sha.as_bytes()).unwrap()
    }

    #[test]
    fn a_branch_name_reports_the_branch_name() {
        let (fx, sha) = one_commit();
        let repo = fx.repo();
        let fork = resolve_fork_point(&repo, Some("main"), Some(oid(&sha))).expect("main resolves");
        assert_eq!(fork.forked_from, "main");
        assert_eq!(fork.at.to_string(), sha);
    }

    /// Anything that is not one branch's tip reports the commit it landed on,
    /// in the spelling `ff log` prints — which is what it reported before the
    /// revset, and what `branchmeta` has been storing all along.
    #[test]
    fn anything_else_reports_a_short_sha() {
        let (fx, sha) = one_commit();
        let repo = fx.repo();
        for target in [sha.as_str(), "main^{commit}", "HEAD"] {
            let fork = resolve_fork_point(&repo, Some(target), Some(oid(&sha))).expect("resolves");
            assert_eq!(fork.at.to_string(), sha);
            assert!(
                sha.starts_with(&fork.forked_from) && fork.forked_from.len() < sha.len(),
                "{target} reported {:?}, not a short sha of {sha}",
                fork.forked_from
            );
        }
    }

    /// Someone else's branch is a fork point with a name, and a parent: a
    /// branch minted from a tracking ref has a base to be measured against,
    /// where a short sha would have left it answering to trunk.
    #[test]
    fn a_tracking_ref_reports_and_records_the_remote_name() {
        let (fx, sha) = one_commit();
        fx.git(&["update-ref", "refs/remotes/origin/feature", &sha]);
        let repo = fx.repo();
        let fork =
            resolve_fork_point(&repo, Some("origin/feature"), Some(oid(&sha))).expect("resolves");
        assert_eq!(fork.at.to_string(), sha);
        assert_eq!(fork.forked_from, "origin/feature");
        assert_eq!(fork.parent.as_deref(), Some("origin/feature"));
    }

    /// The precedence this routing exists to delete: a name that is both a
    /// branch and an object used to fork at the branch and say nothing.
    #[test]
    fn a_name_that_is_both_refuses_and_names_both() {
        let (fx, sha) = one_commit();
        let both = &sha[..8];
        fx.git(&["branch", both]);
        let repo = fx.repo();

        let err = resolve_fork_point(&repo, Some(both), Some(oid(&sha))).expect_err("must refuse");
        assert_eq!(err.id(), "usage/revset-ambiguous");
        let text = err.to_string();
        assert!(
            text.contains(&format!("refs/heads/{both}")),
            "must name the branch: {text}"
        );
        assert!(text.contains(&sha), "must name the object: {text}");
    }

    /// However the open change is spelled: the fork lands under it, and the
    /// point says so. The decision is about the resolved revision, so a
    /// function wrapper does not smuggle it past.
    #[test]
    fn the_open_change_forks_under_itself_and_says_so() {
        let (fx, sha) = one_commit();
        let repo = fx.repo();
        for target in ["@", "latest(@)", "heads(@)"] {
            let fork = resolve_fork_point(&repo, Some(target), Some(oid(&sha))).expect("resolves");
            assert_eq!(fork.at.to_string(), sha, "{target}");
            assert!(fork.open, "{target} is the open change");
            assert_eq!(fork.parent, None, "{target}");
        }
        let fork = resolve_fork_point(&repo, Some("main"), Some(oid(&sha))).expect("resolves");
        assert!(!fork.open, "a branch name is not the open change");
    }

    /// An unborn branch has nothing under its open change to fork at.
    #[test]
    fn the_open_change_on_an_unborn_branch_is_refused() {
        let (fx, _) = one_commit();
        let repo = fx.repo();
        let err = resolve_fork_point(&repo, Some("@"), None).expect_err("must refuse");
        assert_eq!(err.id(), "target/unresolvable");
        assert_eq!(err.to_string(), "@ has no commit under it yet");
    }

    /// A target that denotes nothing is the revset's refusal now, which names
    /// the exits; `target/unresolvable` used to swallow it and name none.
    #[test]
    fn an_unresolvable_target_is_the_revsets_refusal() {
        let (fx, sha) = one_commit();
        let repo = fx.repo();
        let err = resolve_fork_point(&repo, Some("nosuchthing"), Some(oid(&sha)))
            .expect_err("must refuse");
        assert_eq!(err.id(), "usage/revset-unknown-revision");
    }

    /// A set with more than one member is not a fork point, and picking one
    /// would be the same guess the branch-first ladder used to make.
    #[test]
    fn a_target_naming_many_revisions_is_refused() {
        let fx = Fixture::new();
        fx.write("a.txt", "a\n");
        fx.commit("one");
        fx.write("a.txt", "b\n");
        fx.commit("two");
        let repo = fx.repo();
        let err = resolve_fork_point(&repo, Some("::main"), None).expect_err("must refuse");
        assert_eq!(err.id(), "usage/revset-not-a-point");
    }

    /// The remote rung reads a configured remote's tracking refs, bare or
    /// qualified, and skips the symref every clone leaves behind.
    #[test]
    fn the_remote_rung_finds_a_branch_bare_or_qualified() {
        let (fx, sha) = one_commit();
        fx.set_config("remote.origin.url", "file:///nonexistent");
        fx.set_config("remote.origin.fetch", "+refs/heads/*:refs/remotes/origin/*");
        fx.git(&["update-ref", "refs/remotes/origin/spike", &sha]);
        fx.git(&[
            "symbolic-ref",
            "refs/remotes/origin/HEAD",
            "refs/remotes/origin/spike",
        ]);
        let repo = fx.repo();
        for raw in ["spike", "origin/spike"] {
            let hit = remote_branch_of(&repo, raw)
                .expect("reads")
                .unwrap_or_else(|| panic!("{raw} is a branch origin holds"));
            assert_eq!(hit.remote, "origin");
            assert_eq!(hit.short, "origin/spike");
            assert_eq!(hit.local, "spike");
            assert_eq!(hit.tip.to_string(), sha);
        }
        assert!(remote_branch_of(&repo, "HEAD").unwrap().is_none());
        assert!(remote_branch_of(&repo, "origin/HEAD").unwrap().is_none());
        assert!(remote_branch_of(&repo, "nosuch").unwrap().is_none());
    }
}
