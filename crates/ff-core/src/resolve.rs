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
//! returns in one. The way out is `--abandon`, from either branch.

use crate::error::{Error, Result};
use crate::held::{self, verb_of};
use crate::model::{AbandonedHold, HeadState, ReleasedReport, ResolveOutcome, ResolveReport};
use crate::ops::record::{HeldTransition, ResolveTransition, SessionTransition, observe_refs};
use crate::ops::{OpKind, OpRecord, RefTransition, StashEffect, verb};
use crate::snapshot::Provenance;
use crate::stash::{self, ArrivePlan};
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
                "a {op:?} is in progress: finish it with git (git rebase --abort / git merge \
                 --abort); fufu owns merges in a later phase"
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
                ctx,
                (prov, argv, now),
                &head,
                Standing::Session {
                    session: branch,
                    session_tip: tip,
                    onto,
                    open,
                    held,
                },
            );
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
            ctx,
            (prov, argv, now),
            &head,
            Standing::Branch { branch, open, held },
        );
    }

    let held = held.ok_or_else(|| {
        Error::coded(
            "held/none",
            format!("nothing is held on {branch}: there is no pending rewrite to resolve"),
            vec!["ff status".into(), "ff log".into()],
        )
    })?;

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
    // The size of the whole rewrite, tangled commit included: the label
    // names the stack, not this attempt.
    let of = conflict.of;

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
    let open = head_tree_of(repo, &head)
        .map(|tip_tree| open_tree(repo, tip_tree))
        .transpose()?
        .map(|tree| tree.to_string());

    // The marker commit: the chain's tree over the branch's own tip, so the
    // session branch reads as one commit ahead of the branch it lands on,
    // the way an editing session's anchor reads as one of its commits.
    let verb_name = verb_of(&held);
    let sig = crate::refs::user_signature(repo, now)?;
    let marker_commit = stash::write_commit(
        repo,
        chain.tree,
        vec![tip],
        &sig,
        format!("resolving the held {verb_name} on {branch}"),
    )?;

    let session_name = crate::petname::mint(repo)?;
    let session = held::Resolve {
        hold: held.clone(),
        from: chain.tree.to_string(),
        steps: chain.steps.iter().map(|s| s.subject.clone()).collect(),
        open,
        session: session_name.clone(),
    };

    // Mint the session branch at the marker commit, recorded, with the
    // session written on it and the resolution written here — then switch,
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
                new: Some(session),
            }),
        },
        now,
        &argv,
        prov,
    )?;

    let (switch_report, ctx) = crate::switch::switch(
        repo,
        &crate::switch::SwitchOptions {
            target: session_name.clone(),
            now: Some(now),
            argv,
        },
        prov,
    )?;

    let mut files: Vec<String> = regions.iter().map(|r| r.path.clone()).collect();
    files.sort();
    files.dedup();

    Ok((
        ResolveOutcome::Opened(ResolveReport {
            branch,
            session: session_name,
            verb: verb_name,
            files,
            regions: regions.len(),
            steps: chain.steps.len(),
            of,
            tangled: chain.tangled.map(|t| t.subject),
            parked: switch_report.parked,
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

/// `--abandon`: drop the hold, and the session with it if one is open — one
/// operation, so one `ff undo` puts both back. From the session branch it
/// also returns you to the branch the hold stands on; from that branch it
/// deletes the session wherever it is. (Named for what it does, since the
/// flag it serves shadows it.)
fn abandon_hold(
    repo: &gix::Repository,
    ctx: verb::VerbContext,
    invocation: (&Provenance, Vec<String>, i64),
    head: &HeadState,
    standing: Standing,
) -> Result<(ResolveOutcome, verb::VerbContext)> {
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
    let session_park = match &session {
        Some((name, _)) => stash::parked_entry(repo, name)?,
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
            let arrive = stash::plan_arrival(repo, &branch, tip, tree)?;
            Some((tip, tree, arrive))
        }
        None => None,
    };

    // Write the op ahead: both clears, the session's deletion, and the
    // stash effects in one record.
    let mut planned = observe_refs(repo)?;
    let mut transitions: Vec<RefTransition> = Vec::new();
    let mut effects: Vec<StashEffect> = Vec::new();
    let mut pins: Vec<gix::ObjectId> = Vec::new();
    let mut stash_lines: Vec<gix::ObjectId> = crate::refs::read_ref_log(repo, stash::STASH_REF)?
        .iter()
        .map(|l| l.new)
        .collect();
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
        if let Some(sha) = session_park {
            if let Some(pos) = stash_lines.iter().rposition(|s| *s == sha) {
                stash_lines.remove(pos);
            }
            planned.refs.remove(&stash::parked_ref(name));
            transitions.push(RefTransition {
                name: stash::parked_ref(name),
                old: Some(sha.to_string()),
                new: None,
            });
            effects.push(StashEffect::Drop {
                branch: name.clone(),
                stash: sha.to_string(),
            });
            pins.push(sha);
        }
    }
    if let Some((tip, _, arrive)) = &landing {
        planned.head = format!("ref:refs/heads/{branch}");
        pins.push(*tip);
        match arrive {
            ArrivePlan::Restore { stash: sha, .. } => {
                if let Some(pos) = stash_lines.iter().rposition(|s| s == sha) {
                    stash_lines.remove(pos);
                }
                planned.refs.remove(&stash::parked_ref(&branch));
                transitions.push(RefTransition {
                    name: stash::parked_ref(&branch),
                    old: Some(sha.to_string()),
                    new: None,
                });
                effects.push(StashEffect::Drop {
                    branch: branch.clone(),
                    stash: sha.to_string(),
                });
                pins.push(*sha);
            }
            ArrivePlan::Invalidate { stash: sha } => {
                planned.refs.remove(&stash::parked_ref(&branch));
                transitions.push(RefTransition {
                    name: stash::parked_ref(&branch),
                    old: Some(sha.to_string()),
                    new: None,
                });
                pins.push(*sha);
            }
            ArrivePlan::Conflict { stash, .. } => pins.push(*stash),
            ArrivePlan::None => {}
        }
    }
    match stash_lines.last() {
        Some(t) => {
            planned
                .refs
                .insert(stash::STASH_REF.to_string(), t.to_string());
        }
        None => {
            planned.refs.remove(stash::STASH_REF);
        }
    }

    // The state the op leaves the worktree in: the branch's tip, or the
    // parked change laid back over it, when HEAD comes home; the tree as it
    // stands when it does not — the tip's tree would claim a clean state a
    // dirty tree is not.
    let (end_tree, end_index) = match &landing {
        Some((_, tree, arrive)) => held::Return::end_trees(arrive, *tree),
        None => (ctx.pre_tree, crate::index::tree_from_index(repo)?),
    };

    let mut record = OpRecord::new(
        "resolve",
        if open.is_some() {
            format!("abandon the resolution of the held {verb} on {branch}")
        } else {
            format!("drop the held {verb} on {branch}")
        },
        now,
    );
    record.argv = argv;
    if landing.is_some() {
        record.head = Some((head_old, format!("ref:refs/heads/{branch}")));
    }
    record.refs = transitions;
    record.stash = effects;
    if let (Some((name, _)), Some(meta)) = (&session, &session_meta) {
        record.resolve_session = Some(SessionTransition {
            branch: name.clone(),
            old: Some(meta.clone()),
            new: None,
        });
    }
    record.held = Some(HeldTransition {
        branch: branch.clone(),
        old: held,
        new: None,
    });
    record.resolving = Some(ResolveTransition {
        branch: branch.clone(),
        old: open,
        new: None,
    });
    pins.push(end_tree);
    verb::append_op(
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
        if let Some(sha) = session_park {
            edits.push(crate::refs::delete_edit(&stash::parked_ref(name), sha)?);
        }
    }
    if !edits.is_empty() {
        match crate::refs::commit_edits(repo, edits, now)? {
            crate::refs::EditOutcome::Applied => {}
            crate::refs::EditOutcome::Contended => {
                return Err(Error::coded(
                    "ref/contended",
                    "refs moved while abandoning; nothing further was changed (re-run ff \
                     resolve --abandon)",
                    vec![],
                ));
            }
        }
    }
    if let Some(sha) = session_park {
        stash::drop_stash_entry(repo, sha)?;
    }
    let arrival = match &landing {
        Some((_, tree, arrive)) => {
            crate::index::write_index_for_tree(repo, *tree)?;
            let everything = |_: &str| true;
            worktree::apply_tree_transition(repo, ctx.pre_tree, *tree, &everything)?;
            stash::execute_arrival(repo, &branch, arrive, *tree, now)?.into()
        }
        None => crate::model::ArrivalReport::None,
    };
    if let Some((name, _)) = &session {
        // The file stays, `forked_from` and all: undo puts `session` back
        // from the recorded transition, not from the file.
        let mut meta = crate::branchmeta::read(repo, name)?;
        meta.session = None;
        crate::branchmeta::write(repo, name, &meta)?;
        let _ = crate::futures::cache::remove(repo, name);
    }
    held::set(repo, &branch, None)?;
    held::set_resolving(repo, &branch, None)?;

    Ok((
        ResolveOutcome::Abandoned(AbandonedHold {
            branch,
            verb,
            was_resolving,
            session: session.map(|(name, _)| name),
            returned: landing.is_some(),
            arrival,
        }),
        ctx,
    ))
}
