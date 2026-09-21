//! `ff resolve` — deal with a held rewrite. A held rewrite is a conflict
//! fufu chose not to interrupt you with; this is where you choose to deal
//! with it, and it materializes ALL of it at once: every surviving conflict
//! region lands in the working copy together, as ordinary labeled conflict
//! markers, in one editing session.
//!
//! The session is a branch, the counterpart of `ff edit`'s: an anonymous
//! branch minted at a commit carrying the marker tree, with HEAD moved onto
//! it, so the markers are a committed tree plain git can read and every
//! verb that knows an editing session knows this one. The switch that
//! moves you there parks the open change under the branch you left, and
//! the hold STAYS on that branch: it is what the session is resolving, and
//! `ff done` needs it. Two operations open a session, the mint and the
//! switch, exactly as `ff edit` spends two; `ff done` lands the fixes and
//! returns in one. Two ways out: `ff done --abandon` closes the session and
//! leaves the hold standing, so `ff resolve` can open it again; `ff resolve
//! --abandon`, from either branch, drops the hold and the session with it.

use crate::error::{Error, Result};
use crate::held::{self, verb_of};
use crate::model::{
    AbandonedHold, HeadState, HeldReport, LaidReport, ReleasedReport, ResolveOutcome, ResolveReport,
};
use crate::ops::record::{
    ChangeIdTransition, DescriptionTransition, HeldTransition, ResolveTransition,
    SessionTransition, observe_refs,
};
use crate::ops::{OpKind, OpRecord, RefTransition, verb};
use crate::park;
use crate::snapshot::Provenance;
use crate::stash;
use crate::worktree;

/// The tree HEAD's commit carries, the same way `switch` asks it.
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

/// The exact worktree tree: HEAD's tree with the scan assembled onto it,
/// nothing size-capped out — the same read `absorb` and `done` make of the
/// open change, which is what this has to reproduce.
fn open_tree(repo: &gix::Repository, tip_tree: gix::ObjectId) -> Result<gix::ObjectId> {
    let scan = crate::snapshot::tree::scan(repo)?;
    if scan.is_empty() {
        return Ok(tip_tree);
    }
    let (tree_id, _skipped) = crate::snapshot::tree::assemble(repo, tip_tree, &scan, u64::MAX)?;
    Ok(tree_id)
}

fn tree_of(repo: &gix::Repository, commit: gix::ObjectId) -> Result<gix::ObjectId> {
    Ok(repo
        .find_object(commit)
        .map_err(Error::repo)?
        .into_commit()
        .tree_id()
        .map_err(Error::repo)?
        .detach())
}

/// The refusal a second `ff resolve` on the held branch gets while its
/// session is open elsewhere, or the expiration when that session is gone.
fn open_elsewhere(repo: &gix::Repository, branch: &str, open: &held::Resolve) -> Error {
    if open.session.is_empty() {
        return held::predates_sessions(branch);
    }
    let session = &open.session;
    match crate::refs::ref_target(repo, &format!("refs/heads/{session}")) {
        Ok(Some(_)) => Error::coded(
            "held/resolving",
            format!("a resolution of {branch} is open on {session}"),
            vec![
                format!("ff switch {session}"),
                "ff resolve --abandon".into(),
                "ff status".into(),
            ],
        ),
        _ => Error::coded(
            "held/expired",
            format!("the resolution session {session} is gone: the hold is stale"),
            vec!["ff resolve --abandon".into(), "ff status".into()],
        ),
    }
}

/// Deal with the hold standing on the current branch: open a resolution
/// session over it, release it when the world moved out of the conflict, or
/// drop it with `abandon`. See the module docs.
pub fn resolve(
    repo: &gix::Repository,
    abandon: bool,
    prov: &Provenance,
    now: Option<i64>,
    argv: Vec<String>,
) -> Result<(ResolveOutcome, verb::VerbContext)> {
    if repo.workdir().is_none() {
        return Err(Error::coded(
            "repo/bare",
            "bare repository: nothing to resolve",
            vec![],
        ));
    }

    if let Some(op) = crate::head::operation(repo) {
        return Err(Error::coded(
            "repo/mid-operation",
            format!(
                "a {op:?} is in progress: use git status, then finish or abort that operation with Git"
            ),
            vec![],
        ));
    }

    let ctx = verb::begin_verb(repo, prov, now)?;
    let now = ctx.now;

    let head = crate::head::head_state(repo)?;
    let (branch, tip) = match &head {
        HeadState::Branch { name, commit, .. } => {
            let tip = gix::ObjectId::from_hex(commit.as_bytes()).map_err(Error::repo)?;
            (name.clone(), tip)
        }
        HeadState::Unborn { .. } => {
            return Err(Error::coded(
                "repo/detached",
                "nothing is committed yet: there is nothing to resolve",
                vec!["ff commit -m <msg>".into()],
            ));
        }
        HeadState::Detached { .. } => {
            return Err(Error::coded(
                "repo/detached",
                "detached HEAD: a resolution session needs a branch to stand on",
                vec!["ff switch <branch>".into()],
            ));
        }
    };

    // Standing on the session branch of an open resolution: the markers are
    // in this working copy. A second resolve would overwrite the very edits
    // the session exists to collect, so it refuses — and `--abandon` is
    // exactly how you get out of the first one.
    if let Some((onto, open)) = held::session_of(repo, &branch)? {
        if abandon {
            let held = held::of(repo, &onto)?;
            return abandon_hold(
                repo,
                ctx.pre_tree,
                (prov, argv, now),
                &head,
                Standing::Session {
                    session: branch,
                    session_tip: tip,
                    onto,
                    open,
                    held,
                },
                KeepHold::No,
            )
            .map(|dropped| (ResolveOutcome::Abandoned(dropped), ctx));
        }
        return Err(Error::coded(
            "held/resolving",
            format!(
                "a resolution is already open on {onto}: its conflicts are in your working copy"
            ),
            vec![
                "ff done".into(),
                "ff resolve --abandon".into(),
                "ff status".into(),
            ],
        ));
    }

    // Standing on the branch the hold stands on, whose session is open on
    // another branch: the way to the markers is a switch.
    let open = held::resolving(repo, &branch)?;
    if let Some(open) = &open
        && !abandon
    {
        return Err(open_elsewhere(repo, &branch, open));
    }

    let held = held::of(repo, &branch)?;

    if abandon {
        return abandon_hold(
            repo,
            ctx.pre_tree,
            (prov, argv, now),
            &head,
            Standing::Branch { branch, open, held },
            KeepHold::No,
        )
        .map(|dropped| (ResolveOutcome::Abandoned(dropped), ctx));
    }

    // No hold: the merge door. A branch whose commits hold a merge of its
    // base, standing behind it, takes the base in the way it already does.
    let Some(held) = held else {
        return merge_door(repo, ctx, (prov, argv, now), &head, branch, tip);
    };

    // A held arrival is laid into the open change in place: the working copy
    // is where its conflicts belong, and there is no closed commit for a
    // session to protect.
    if let held::Intent::Arrive { open, .. } = &held.intent {
        let open = gix::ObjectId::from_hex(open.as_bytes()).map_err(Error::repo)?;
        return lay_arrival(
            repo,
            ctx,
            (prov, argv, now),
            &head,
            branch,
            tip,
            held.clone(),
            open,
        );
    }

    // A held merge lands here when it is clean now, rather than being
    // released for a verb that does not exist yet.
    if let held::Intent::Merge { onto, message, .. } = &held.intent {
        let onto = onto.clone();
        let message = message.clone();
        return resolve_merge_hold(
            repo,
            ctx,
            (prov, argv, now),
            &head,
            branch,
            tip,
            held,
            &onto,
            message.as_deref(),
        );
    }

    // Ask again against the repository as it stands. A `held/expired` passes
    // straight through — the hold has outlived its meaning, and saying so is
    // the answer.
    let replan = held::replan(repo, &held)?;

    let Some(conflict) = crate::rewrite::conflict(repo, replan.target, replan.tip, &replan.change)?
    else {
        // The world moved and the rewrite is clean now. A hold is a cache
        // over "this rewrite conflicts", and a cache entry that no longer
        // describes anything is not an event — so the clear is a plain
        // metadata write with no operation, and the verb that recorded the
        // hold lands the rewrite when it is re-run.
        held::set(repo, &branch, None)?;
        return Ok((
            ResolveOutcome::Released(ReleasedReport {
                branch,
                verb: verb_of(&held),
            }),
            ctx,
        ));
    };

    // A filtered absorb or lift rewrites only the paths it selected. Changes
    // standing OUTSIDE that filter are not in the chain, and the switch
    // below would park them with the rest and the landing would spend the
    // park — work lost, so it refuses before touching anything. An
    // unfiltered rewrite selects every path, and `restack` and `done` fold
    // the whole tree into their own, so neither can lose what it did not
    // select.
    let filtered = match &held.intent {
        held::Intent::Absorb { paths, .. } | held::Intent::Lift { paths, .. }
            if !paths.is_empty() =>
        {
            Some(paths)
        }
        _ => None,
    };
    if let Some(filter) = filtered {
        let scan = crate::snapshot::tree::scan(repo)?;
        let mut outside: Vec<String> = scan
            .staged_upserts
            .iter()
            .map(|(path, _, _)| path.clone())
            .chain(scan.staged_deletes.iter().cloned())
            .chain(scan.rehash.iter().cloned())
            .chain(scan.untracked.iter().cloned())
            .chain(scan.wt_deletes.iter().cloned())
            .filter(|path| !filter.contains(path))
            .collect();
        outside.sort();
        outside.dedup();
        if !outside.is_empty() {
            return Err(Error::coded(
                "held/unsupported",
                format!(
                    "the held {} selected paths, and your open change also touches {}: \
                     resolving would overwrite what it did not select",
                    verb_of(&held),
                    crate::rewrite::join_paths(&outside),
                ),
                vec![
                    "ff commit -m <msg>".into(),
                    "ff resolve --abandon".into(),
                    "ff status".into(),
                ],
            ));
        }
    }

    let (report, ctx) = open_session(
        repo,
        (prov, argv, now),
        &head,
        Session {
            branch,
            tip,
            held: &held,
            replan: &replan,
            of: conflict.of,
            recording: None,
            reported: None,
        },
    )?;
    Ok((ResolveOutcome::Opened(report), ctx))
}

/// The refusal with nothing held: `held/none`, with `why` completing
/// "nothing is held on {branch}".
fn nothing_held(branch: &str, why: &str, exits: Vec<String>) -> Error {
    Error::coded(
        "held/none",
        format!("nothing is held on {branch}{why}"),
        exits,
    )
}

/// The merge door: with no hold on the branch, a branch whose commits hold
/// a merge of its base and that is behind it takes the base in by one
/// merge commit. A linear branch is `ff restack`'s and refuses; up to date
/// says so. A conflicting auto-merge records the hold and opens the
/// session in one operation, the hold riding the mint.
fn merge_door(
    repo: &gix::Repository,
    ctx: verb::VerbContext,
    invocation: (&Provenance, Vec<String>, i64),
    head: &HeadState,
    branch: String,
    tip: gix::ObjectId,
) -> Result<(ResolveOutcome, verb::VerbContext)> {
    let (prov, argv, now) = invocation;
    let plain = || {
        nothing_held(
            &branch,
            ": there is no pending rewrite to resolve",
            vec!["ff status".into(), "ff log".into()],
        )
    };
    let Some(pull_ref) = crate::futures::base_for(repo, &branch)? else {
        return Err(plain());
    };
    let onto = crate::restack::onto_from(repo, &pull_ref)?;
    let range = match crate::restack::measure_range(repo, &branch, tip, &onto) {
        Ok(range) => range,
        Err(err) if err.id() == "restack/unrelated" => {
            return Err(nothing_held(
                &branch,
                &format!(
                    ", and it shares no history with {}: there is nothing to resolve",
                    onto.name
                ),
                vec!["ff status".into(), "ff log".into()],
            ));
        }
        Err(err) => return Err(err),
    };
    if range.up_to_date {
        return Err(nothing_held(
            &branch,
            &format!(
                ", and it is up to date with {}: there is nothing to resolve",
                onto.name
            ),
            vec!["ff status".into(), "ff log".into()],
        ));
    }
    if range.merges_of_base(repo)?.is_empty() {
        return Err(nothing_held(
            &branch,
            &format!(
                ": it sits behind {} on a straight line, and ff restack replays it",
                onto.name
            ),
            vec!["ff restack".into(), "ff status".into()],
        ));
    }

    let plan = crate::merge::plan(repo, &branch, &onto, None)?;
    let Some(conflict) = &plan.conflict else {
        let tree = plan
            .tree
            .ok_or_else(|| Error::msg("internal: a clean merge plan carries its tree"))?;
        let report = crate::merge::commit(
            repo,
            &ctx,
            prov,
            argv,
            &plan,
            tree,
            crate::merge::Bring::Open { head, held: None },
        )?;
        return Ok((ResolveOutcome::Merged(report), ctx));
    };

    // The hold, recorded by the mint below rather than as an operation of
    // its own: the session opens on it in the same step.
    let held = held::Held {
        intent: held::Intent::Merge {
            branch: branch.clone(),
            onto: onto.full.clone(),
            message: None,
        },
        at: conflict.at.clone(),
        paths: conflict.paths.clone(),
        time: now,
    };
    let recording = HeldTransition {
        branch: branch.clone(),
        old: None,
        new: Some(held.clone()),
    };
    let reported = HeldReport {
        verb: "merge".into(),
        branch: branch.clone(),
        at: conflict.at.clone(),
        paths: conflict.paths.clone(),
        of: conflict.of,
    };
    let (report, ctx) = open_session(
        repo,
        (prov, argv, now),
        head,
        Session {
            branch,
            tip,
            held: &held,
            replan: &plan.replan,
            of: conflict.of,
            recording: Some(recording),
            reported: Some(reported),
        },
    )?;
    Ok((ResolveOutcome::Opened(report), ctx))
}

/// A standing merge hold: replan it, and land it when it is clean now,
/// release it when the base is already in, or open the session over its
/// conflicts.
#[allow(clippy::too_many_arguments)]
fn resolve_merge_hold(
    repo: &gix::Repository,
    ctx: verb::VerbContext,
    invocation: (&Provenance, Vec<String>, i64),
    head: &HeadState,
    branch: String,
    tip: gix::ObjectId,
    held: held::Held,
    onto: &str,
    message: Option<&str>,
) -> Result<(ResolveOutcome, verb::VerbContext)> {
    let (prov, argv, now) = invocation;
    // `held/expired` when the base or the branch is gone, or the branch is
    // beneath the base; the plan below repeats the same reads.
    held::replan(repo, &held)?;
    let onto = crate::restack::resolve_onto(repo, onto)?;
    let plan = crate::merge::plan(repo, &branch, &onto, message)?;
    if plan.already_in {
        // The base is in already: the hold is moot, and the clear is a plain
        // metadata write with no operation, as a released restack's is.
        held::set(repo, &branch, None)?;
        return Ok((
            ResolveOutcome::Released(ReleasedReport {
                branch,
                verb: verb_of(&held),
            }),
            ctx,
        ));
    }
    let Some(conflict) = &plan.conflict else {
        let tree = plan
            .tree
            .ok_or_else(|| Error::msg("internal: a clean merge plan carries its tree"))?;
        let report = crate::merge::commit(
            repo,
            &ctx,
            prov,
            argv,
            &plan,
            tree,
            crate::merge::Bring::Open {
                head,
                held: Some(Box::new(HeldTransition {
                    branch: branch.clone(),
                    old: Some(held.clone()),
                    new: None,
                })),
            },
        )?;
        return Ok((ResolveOutcome::Merged(report), ctx));
    };
    let (report, ctx) = open_session(
        repo,
        (prov, argv, now),
        head,
        Session {
            branch,
            tip,
            held: &held,
            replan: &plan.replan,
            of: conflict.of,
            recording: None,
            reported: None,
        },
    )?;
    Ok((ResolveOutcome::Opened(report), ctx))
}

/// What a resolution session opens over. `ff restack --resolve`, `ff pull
/// --resolve`, and `ff merge --resolve` build one too: the session is the
/// same whichever verb opens it.
pub(crate) struct Session<'a> {
    pub(crate) branch: String,
    pub(crate) tip: gix::ObjectId,
    /// The hold being resolved: standing on the branch, or about to be
    /// recorded by the mint when `recording` is set.
    pub(crate) held: &'a held::Held,
    pub(crate) replan: &'a held::Replan,
    /// The size of the whole rewrite, for the report.
    pub(crate) of: usize,
    /// The hold transition the mint records, when the verb opening the
    /// session derived the rewrite itself and found it conflicting and no
    /// other operation records the hold.
    pub(crate) recording: Option<HeldTransition>,
    /// The hold as the report names it, when this run recorded the hold —
    /// by the mint or, for a pull, by the verb's own operation — so the
    /// report reads as a hold and the shell owes a 3. `None` when the hold
    /// stood before.
    pub(crate) reported: Option<HeldReport>,
}

/// Open the session: replay the chain with its conflicts as markers, mint
/// the session branch at a commit carrying that tree, and switch there —
/// `ff edit`'s own two operations. Returns the report and the switch's
/// context.
pub(crate) fn open_session(
    repo: &gix::Repository,
    invocation: (&Provenance, Vec<String>, i64),
    head: &HeadState,
    session: Session<'_>,
) -> Result<(ResolveReport, verb::VerbContext)> {
    let (prov, argv, now) = invocation;
    let Session {
        branch,
        tip,
        held,
        replan,
        of,
        recording,
        reported,
    } = session;

    // Replay it all the way through, carrying the conflicts as literal
    // marker content, and read back the regions standing in the result.
    let chain = crate::rewrite::chain(repo, replan.target, replan.tip, &replan.change, &[])?;
    let regions = crate::rewrite::regions(repo, &chain)?;

    // The working copy as it stands, before the switch parks it. A `done`,
    // `absorb` or `lift` plan is derived from the open change, and this is
    // the last moment it can be read from here: recording it is what lets
    // `ff done` replan to the same plan and mean "the world moved" when it
    // does not. The park keeps the change too, and the landing spends that
    // park rather than bringing it back, since the chain already carries it.
    let open = head_tree_of(repo, head)
        .map(|tip_tree| open_tree(repo, tip_tree))
        .transpose()?
        .map(|tree| tree.to_string());

    // The marker commit: the chain's tree over the branch's own tip, so the
    // session branch reads as one commit ahead of the branch it lands on,
    // the way an editing session's anchor reads as one of its commits.
    let verb_name = verb_of(held);
    let sig = crate::refs::user_signature(repo, now)?;
    let marker_commit = stash::write_commit(
        repo,
        chain.tree,
        vec![tip],
        &sig,
        format!("resolving the held {verb_name} on {branch}"),
    )?;

    let mut files: Vec<String> = regions.iter().map(|r| r.path.clone()).collect();
    files.sort();
    files.dedup();

    let session_name = crate::petname::mint(repo)?;
    let record = held::Resolve {
        hold: held.clone(),
        from: chain.tree.to_string(),
        steps: chain.steps.iter().map(|s| s.subject.clone()).collect(),
        open,
        session: session_name.clone(),
        resolutions: Vec::new(),
        files: files.clone(),
    };
    let merging = match &held.intent {
        held::Intent::Merge { .. } => chain.steps.first().map(|s| s.subject.clone()),
        _ => None,
    };
    // Mint the session branch at the marker commit, recorded, with the
    // session written on it and the resolution written here — and the hold
    // itself, when this resolve is the one recording it — then switch,
    // which parks the open change and materializes the markers, `ff edit`'s
    // own two operations.
    crate::edit::mint_session(
        repo,
        crate::edit::Mint {
            name: &session_name,
            at: marker_commit,
            onto: &branch,
            verb: "resolve",
            summary: format!(
                "resolve the held {verb_name} on {branch}: {} region(s)",
                regions.len()
            ),
            resolving: Some(ResolveTransition {
                branch: branch.clone(),
                old: None,
                new: Some(record),
            }),
            held: recording,
        },
        now,
        &argv,
        prov,
    )?;

    let (switch_report, ctx) = crate::switch::switch(
        repo,
        &crate::switch::SwitchOptions {
            target: Some(session_name.clone()),
            now: Some(now),
            argv,
            ..Default::default()
        },
        prov,
    )?;

    Ok((
        ResolveReport {
            branch,
            session: session_name,
            verb: verb_name,
            files,
            regions: regions.len(),
            steps: chain.steps.len(),
            of,
            tangled: chain.tangled.map(|t| t.subject),
            parked: switch_report.parked,
            held: reported,
            merging,
        },
        ctx,
    ))
}

/// Lay a held arrival into the open change on `branch`: the parked commit
/// `open` replayed onto the branch's tip as one step, conflicts carried as
/// markers, the tree written over the working copy and the branch's
/// identity restored from the commit. When the tip moved out of the
/// conflict since the hold, the change is simply resumed clean.
#[allow(clippy::too_many_arguments)]
fn lay_arrival(
    repo: &gix::Repository,
    ctx: verb::VerbContext,
    invocation: (&Provenance, Vec<String>, i64),
    head: &HeadState,
    branch: String,
    tip: gix::ObjectId,
    held: held::Held,
    open: gix::ObjectId,
) -> Result<(ResolveOutcome, verb::VerbContext)> {
    let (prov, argv, now) = invocation;
    let tip_tree = tree_of(repo, tip)?;
    if ctx.pre_tree != tip_tree {
        return Err(Error::coded(
            "held/unsupported",
            format!(
                "{branch} has an open change: preserve or commit it before resolving the held arrival"
            ),
            vec![
                "ff commit -m <msg>".into(),
                "ff explain held/unsupported".into(),
                "ff resolve --abandon".into(),
            ],
        ));
    }
    let commit = repo.find_object(open).map_err(Error::repo)?.into_commit();
    let meta = crate::branchmeta::read(repo, &branch)?;

    // The identity the held commit carries: its header, its author time,
    // its message. The hold took them off the branch; they come back with
    // the change.
    let change_id = crate::changeid::header_of(&commit.data).map(|id| id.letters());
    let born = commit
        .author()
        .map_err(Error::repo)?
        .time()
        .map_err(Error::repo)?
        .seconds;
    let message = commit.message_raw_sloppy().to_string();
    let description = if message.trim().is_empty() {
        None
    } else {
        Some(message.trim_end().to_string())
    };

    // The replay, one commit onto the tip. Clean now: the arrival resumes
    // and nothing needs markers.
    let replan = held::replan(repo, &held)?;
    let conflict = crate::rewrite::conflict(repo, replan.target, replan.tip, &replan.change)?;
    let (tree, regions) = match conflict {
        None => {
            let arrive = park::plan_replay(repo, &branch, open, tip, tip_tree, now)?;
            let (end_tree, _) = arrive.end_trees(tip_tree);
            (end_tree, 0usize)
        }
        Some(_) => {
            let chain =
                crate::rewrite::chain(repo, replan.target, replan.tip, &replan.change, &[])?;
            let regions = crate::rewrite::regions(repo, &chain)?;
            (chain.tree, regions.len())
        }
    };
    let clean = regions == 0;

    // The open commit the operation lands: the marker tree (or the merged
    // one) over the tip, wearing the restored identity.
    let overlay = crate::open::Overlay {
        description: Some(description.clone()),
        change_id: Some((change_id.clone(), Some(born))),
    };
    let laid = crate::open::plan(repo, &branch, tree, Some(tip), &overlay, now)?
        .map(|plan| crate::open::write(repo, &plan, &[open], now))
        .transpose()?;

    let mut planned = observe_refs(repo)?;
    let mut record = OpRecord::new(
        "resolve",
        if clean {
            format!("resume the held arrival on {branch}")
        } else {
            format!("resolve the held arrival on {branch}")
        },
        now,
    );
    record.argv = argv;
    record.held = Some(HeldTransition {
        branch: branch.clone(),
        old: Some(held.clone()),
        new: None,
    });
    if change_id.is_some() || meta.change_id.is_some() {
        record.change_id = Some(ChangeIdTransition {
            branch: branch.clone(),
            old: meta.change_id.clone(),
            new: change_id.clone(),
            old_born: meta.change_born,
            new_born: Some(born),
        });
    }
    if description.is_some() || meta.pending_description.is_some() {
        record.description = Some(DescriptionTransition {
            branch: branch.clone(),
            old: meta.pending_description.clone(),
            new: description.clone(),
        });
    }
    let mut pins = vec![open, tip];
    pins.extend(laid);
    // A tree with markers is not the parked commit's tree; the planned
    // `refs/stash` is untouched either way.
    let _ = &mut planned;
    verb::append_op_hinted(
        repo,
        OpKind::Op,
        verb::VerbOp {
            record,
            planned,
            tree,
            index_tree: tip_tree,
            branch: branch.clone(),
            base: crate::snapshot::chain::base_commit(head)?,
            session: prov.session.clone(),
            pins: &pins,
        },
        laid,
        now,
    )?;

    // Mutate: the working copy, then the metadata.
    let everything = |_: &str| true;
    let transition = worktree::apply_tree_transition(repo, tip_tree, tree, &everything)?;
    let mut files = transition.written;
    files.extend(transition.deleted);
    files.sort();
    files.dedup();
    let mut meta = crate::branchmeta::read(repo, &branch)?;
    meta.held = None;
    meta.change_id = change_id;
    meta.change_born = Some(born);
    meta.pending_description = description;
    crate::branchmeta::write(repo, &branch, &meta)?;

    let paths: Vec<String> = if clean {
        Vec::new()
    } else {
        held.paths.clone()
    };
    Ok((
        ResolveOutcome::Laid(LaidReport {
            branch,
            held: open.to_string(),
            arrival: crate::model::ArrivalReport::Restored {
                open: laid.map(|id| id.to_string()).unwrap_or_default(),
                files,
                folded: None,
            },
            paths,
            regions,
        }),
        ctx,
    ))
}

/// Where `--abandon` was typed, and what it found there.
enum Standing {
    /// HEAD on the session branch of an open resolution of `onto`.
    Session {
        session: String,
        session_tip: gix::ObjectId,
        onto: String,
        open: held::Resolve,
        held: Option<held::Held>,
    },
    /// HEAD on the branch the hold stands on, whose session, if one is
    /// recorded, is open elsewhere or gone.
    Branch {
        branch: String,
        open: Option<held::Resolve>,
        held: Option<held::Held>,
    },
}

/// Whether the hold outlives the abandon. `ff resolve --abandon` drops it;
/// `ff done --abandon` keeps it, closing only the session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum KeepHold {
    Yes,
    No,
}

/// `ff done --abandon` from the session branch: close the session and keep
/// the hold. The session branch goes, `resolving` is cleared, HEAD returns to
/// the branch the hold stands on with its parked change brought home, and
/// the hold is left standing for `ff resolve` to open again. One operation,
/// so one `ff undo` puts the session and its fixes back.
pub(crate) fn close_session(
    repo: &gix::Repository,
    ctx: &verb::VerbContext,
    invocation: (&Provenance, Vec<String>, i64),
    head: &HeadState,
    session: (String, gix::ObjectId),
    resolving: (String, held::Resolve),
) -> Result<AbandonedHold> {
    let (session, session_tip) = session;
    let (onto, open) = resolving;
    let held = held::of(repo, &onto)?;
    abandon_hold(
        repo,
        ctx.pre_tree,
        invocation,
        head,
        Standing::Session {
            session,
            session_tip,
            onto,
            open,
            held,
        },
        KeepHold::Yes,
    )
}

/// `--abandon`: drop the hold, and the session with it if one is open — one
/// operation, so one `ff undo` puts both back. From the session branch it
/// also returns you to the branch the hold stands on; from that branch it
/// deletes the session wherever it is. Under `KeepHold::Yes` the hold is
/// left standing and only the session goes: `ff done --abandon`'s reading.
/// (Named for what it does, since the flag it serves shadows it.)
fn abandon_hold(
    repo: &gix::Repository,
    pre_tree: gix::ObjectId,
    invocation: (&Provenance, Vec<String>, i64),
    head: &HeadState,
    standing: Standing,
    keep: KeepHold,
) -> Result<AbandonedHold> {
    let (prov, argv, now) = invocation;

    // What is on the branch the hold stands on, and which session branch
    // — if any still exists — goes with it.
    let (branch, open, held, from_session) = match standing {
        Standing::Session {
            session,
            session_tip,
            onto,
            open,
            held,
        } => (onto, Some(open), held, Some((session, session_tip))),
        Standing::Branch { branch, open, held } => (branch, open, held, None),
    };
    if open.is_none() && held.is_none() {
        return Err(Error::coded(
            "held/none",
            format!("nothing is held on {branch}: there is no pending rewrite to resolve"),
            vec!["ff status".into(), "ff log".into()],
        ));
    }
    let was_resolving = open.is_some();
    let held_intent = held.as_ref().map(|h| h.intent.clone());

    // The session carries a copy of the hold it was opened on, so a hold
    // cleared underneath it can still name its verb.
    let verb = match (held.as_ref(), open.as_ref()) {
        (Some(h), _) => verb_of(h),
        (None, Some(session)) => verb_of(&session.hold),
        (None, None) => unreachable!("the empty abandon was refused above"),
    };

    // The session branch to delete: the one underfoot, or the one the
    // record names when it still exists. A record with no session, or one
    // whose branch is already gone, has nothing to delete and the records
    // are simply cleared.
    let session: Option<(String, gix::ObjectId)> = match &from_session {
        Some(s) => Some(s.clone()),
        None => match open.as_ref().map(|o| o.session.as_str()) {
            Some(name) if !name.is_empty() => {
                crate::refs::ref_target(repo, &format!("refs/heads/{name}"))?
                    .map(|tip| (name.to_string(), tip))
            }
            _ => None,
        },
    };
    let session_meta = match &session {
        Some((name, _)) => crate::branchmeta::read(repo, name)?.session,
        None => None,
    };
    // The session's own park — its open commit when HEAD is elsewhere, or a
    // legacy entry — goes with the branch: the fixes are discarded, and the
    // pre-verb capture is what keeps them.
    let session_park = match &session {
        Some((name, _)) => held::Spent::of(repo, name)?,
        None => None,
    };

    // The return trip, when HEAD is leaving the session branch: back to the
    // branch's tip, with its parked change brought home the way `switch`
    // brings one.
    let landing = match &from_session {
        Some(_) => {
            let tip = crate::refs::ref_target(repo, &format!("refs/heads/{branch}"))?.ok_or_else(
                || {
                    Error::coded(
                        "branch/not-found",
                        format!("{branch}, the branch this resolution lands on, no longer exists"),
                        vec!["ff switch <branch>".into()],
                    )
                },
            )?;
            let tree = tree_of(repo, tip)?;
            let arrive = park::plan_arrival(repo, &branch, tip, tree, now)?;
            Some((tip, tree, arrive))
        }
        None => None,
    };

    // Write the op ahead: both clears, the session's deletion, the spent
    // park, and the arrival in one record.
    let mut planned = observe_refs(repo)?;
    let mut transitions: Vec<RefTransition> = Vec::new();
    let mut pins: Vec<gix::ObjectId> = Vec::new();
    let mut stash_lines = stash::lines(repo)?;
    let head_old = planned.head.clone();
    if let Some((name, tip)) = &session {
        let session_ref = format!("refs/heads/{name}");
        planned.refs.remove(&session_ref);
        transitions.push(RefTransition {
            name: session_ref,
            old: Some(tip.to_string()),
            new: None,
        });
        pins.push(*tip);
    }
    if let Some((tip, _, _)) = &landing {
        planned.head = format!("ref:refs/heads/{branch}");
        pins.push(*tip);
    }

    // The state the op leaves the worktree in: the branch's tip, or the
    // parked change laid back over it, when HEAD comes home; the tree as it
    // stands when it does not — the tip's tree would claim a clean state a
    // dirty tree is not.
    let (end_tree, end_index) = match &landing {
        Some((_, tree, arrive)) => arrive.end_trees(*tree),
        None => (pre_tree, crate::index::tree_from_index(repo)?),
    };

    // The record names the door: `done` closes, `resolve` drops.
    let mut record = OpRecord::new(
        match keep {
            KeepHold::Yes => "done",
            KeepHold::No => "resolve",
        },
        match (keep, open.is_some()) {
            (KeepHold::Yes, _) => {
                format!("close the resolution of the held {verb} on {branch}")
            }
            (KeepHold::No, true) => {
                format!("abandon the resolution of the held {verb} on {branch}")
            }
            (KeepHold::No, false) => format!("drop the held {verb} on {branch}"),
        },
        now,
    );
    record.argv = argv;
    if landing.is_some() {
        record.head = Some((head_old, format!("ref:refs/heads/{branch}")));
    }
    record.refs = transitions;
    if let (Some((name, _)), Some(meta)) = (&session, &session_meta) {
        record.resolve_session = Some(SessionTransition {
            branch: name.clone(),
            old: Some(meta.clone()),
            new: None,
        });
    }
    if keep == KeepHold::No {
        record.held = Some(HeldTransition {
            branch: branch.clone(),
            old: held,
            new: None,
        });
    }
    record.resolving = Some(ResolveTransition {
        branch: branch.clone(),
        old: open,
        new: None,
    });
    // The hold being dropped goes after the clearing transition above, so an
    // arrival that holds again lands on the same transition: old, the hold
    // dropped; new, the one the arrival records.
    if let Some((_, _, arrive)) = &landing {
        arrive.fold_into(&mut planned, &mut record, &mut pins, &mut stash_lines);
    }
    if let Some(spent) = &session_park {
        if spent.legacy {
            stash::spend(
                &mut stash_lines,
                &mut planned,
                &mut record,
                &mut pins,
                &spent.branch,
                spent.sha,
            );
        } else {
            pins.push(spent.sha);
        }
    }
    pins.push(end_tree);
    verb::append_op_hinted(
        repo,
        OpKind::Op,
        verb::VerbOp {
            record,
            planned,
            tree: end_tree,
            index_tree: end_index,
            branch: branch.clone(),
            base: crate::snapshot::chain::base_commit(head)?,
            session: prov.session.clone(),
            pins: &pins,
        },
        landing
            .as_ref()
            .and_then(|(_, _, arrive)| arrive.open_hint()),
        now,
    )?;

    // Mutate: HEAD, refs, the spent park, index, worktree, arrive, metadata
    // — the `done` order. The fixes are discarded with the session branch;
    // the pre-verb capture is what keeps them.
    if landing.is_some() {
        crate::branch::retarget_head(repo, &format!("refs/heads/{branch}"), now)?;
    }
    let mut edits = Vec::new();
    if let Some((name, tip)) = &session {
        edits.push(crate::refs::delete_edit(
            &format!("refs/heads/{name}"),
            *tip,
        )?);
        if let Some(spent) = &session_park
            && spent.legacy
        {
            edits.push(crate::refs::delete_edit(
                &stash::parked_ref(name),
                spent.sha,
            )?);
        }
    }
    if !edits.is_empty() {
        match crate::refs::commit_edits(repo, edits, now)? {
            crate::refs::EditOutcome::Applied => {}
            crate::refs::EditOutcome::Contended => {
                let rerun = match keep {
                    KeepHold::Yes => "ff done --abandon",
                    KeepHold::No => "ff resolve --abandon",
                };
                return Err(Error::coded(
                    "ref/contended",
                    format!(
                        "refs moved while abandoning; nothing further was changed (re-run {rerun})"
                    ),
                    vec![],
                ));
            }
        }
    }
    if let Some(spent) = &session_park {
        if spent.legacy {
            stash::drop_stash_entry(repo, spent.sha)?;
        } else {
            crate::open::clear(repo, &spent.branch, now)?;
        }
    }
    if let Some((name, _)) = &session {
        // The file stays, `forked_from` and all: undo puts `session` back
        // from the recorded transition, not from the file.
        let mut meta = crate::branchmeta::read(repo, name)?;
        meta.session = None;
        crate::branchmeta::write(repo, name, &meta)?;
        let _ = crate::futures::cache::remove(repo, name);
    }
    // The clears before the arrival, which may record a hold of its own.
    if keep == KeepHold::No {
        held::set(repo, &branch, None)?;
    }
    held::set_resolving(repo, &branch, None)?;
    let arrival = match &landing {
        Some((_, tree, arrive)) => {
            crate::index::write_index_for_tree(repo, *tree)?;
            let everything = |_: &str| true;
            worktree::apply_tree_transition(repo, pre_tree, *tree, &everything)?;
            park::execute_arrival(repo, &branch, arrive, *tree, now)?
        }
        None => crate::model::ArrivalReport::None,
    };

    Ok(AbandonedHold {
        branch,
        verb,
        was_resolving,
        session: session.map(|(name, _)| name),
        returned: landing.is_some(),
        arrival,
        left: match &held_intent {
            Some(held::Intent::Arrive { open, .. }) => Some(open.clone()),
            _ => None,
        },
    })
}
