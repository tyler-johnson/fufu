//! `ff done` ends an editing session opened by `ff edit`: the edited commit
//! is amended with the session's content, what waited ahead of it on the
//! branch it will replay onto, the worktree lands back on that branch, and
//! the session branch is gone. This is **one operation** — the refs, HEAD,
//! the worktree and the change `ff edit` parked all move together, so a
//! single `ff undo` takes the whole session back. That is why this fuses the
//! rewrite and the return rather than composing `ff restack` with
//! `ff switch`: two operations would need two undos, and the first of them
//! would land on a state that still holds the session open.
//!
//! The rewrite reaches [`crate::rewrite::plan`] directly, with
//! [`crate::rewrite::Change::Tree`] on the session branch's own tip and the
//! landing branch's tip as the range end — never a call into
//! [`crate::restack`], whose merge-base arithmetic is built for a branch
//! standing behind its base, not for a branch that sits *at* the commit
//! being amended: aiming restack here would replay the edited commit a
//! second time.
//!
//! `--abandon` is the escape hatch: it drops the session's uncommitted edits
//! (parked, never discarded) rather than landing them, and it works in
//! exactly the states where `ff done` refuses — a session that gained
//! commits of its own, or one whose anchor fell out of the landing branch's
//! history, both fold away without complaint under `--abandon`.

use crate::branch;
use crate::branchmeta;
use crate::error::{Error, Result};
use crate::futures;
use crate::held::{self, Held, Intent};
use crate::model::{AbandonReport, ArrivalReport, DoneOutcome, DoneReport, HeadState, HeldReport};
use crate::ops::record::{SessionTransition, observe_refs};
use crate::ops::{OpKind, OpRecord, RefTransition, StashEffect, verb};
use crate::refs;
use crate::rewrite;
use crate::snapshot::Provenance;
use crate::snapshot::tree as snaptree;
use crate::stash::{self, ArrivePlan};

/// The subject of a commit, through the object handle — the raw `CommitRef`
/// message has no summary.
fn subject(repo: &gix::Repository, commit: gix::ObjectId) -> Result<String> {
    let commit = repo.find_object(commit).map_err(Error::repo)?.into_commit();
    Ok(commit.message().map_err(Error::repo)?.summary().to_string())
}

/// The tree of a commit, resolved through whichever repository handle is
/// given.
fn tree_of(repo: &gix::Repository, commit: gix::ObjectId) -> Result<gix::ObjectId> {
    Ok(repo
        .find_object(commit)
        .map_err(Error::repo)?
        .into_commit()
        .tree_id()
        .map_err(Error::repo)?
        .detach())
}

/// A commit's full message, through the object handle — the raw `CommitRef`
/// message is what the amend compares and lands.
fn message_of(repo: &gix::Repository, commit: gix::ObjectId) -> Result<String> {
    let obj = repo.find_object(commit).map_err(Error::repo)?;
    let commit_ref = gix::objs::CommitRef::from_bytes(&obj.data).map_err(Error::repo)?;
    Ok(commit_ref.message.to_string())
}

/// A commit's first parent, or `None` for a root.
fn first_parent(repo: &gix::Repository, id: gix::ObjectId) -> Result<Option<gix::ObjectId>> {
    let obj = repo.find_object(id).map_err(Error::repo)?;
    let commit_ref = gix::objs::CommitRef::from_bytes(&obj.data).map_err(Error::repo)?;
    match commit_ref.parents.first() {
        Some(hex) => Ok(Some(gix::ObjectId::from_hex(hex).map_err(Error::repo)?)),
        None => Ok(None),
    }
}

/// The exact worktree tree: the tip's tree with the scan assembled onto it,
/// nothing size-capped out. The second result says the tree is clean.
fn open_tree(repo: &gix::Repository, tip_tree: gix::ObjectId) -> Result<(gix::ObjectId, bool)> {
    let scan = snaptree::scan(repo)?;
    if scan.is_empty() {
        return Ok((tip_tree, true));
    }
    let (tree_id, _skipped) = snaptree::assemble(repo, tip_tree, &scan, u64::MAX)?;
    Ok((tree_id, false))
}

fn session_none() -> Error {
    Error::coded(
        "session/none",
        "no editing session is running",
        vec!["ff edit <rev>".into(), "ff status".into()],
    )
}

/// A landing replay that conflicts is an outcome, not an error: record the
/// hold the caller assembled and report it. Nothing moves — the hold returns
/// from the planning half of `done`, before a single mutation, so the session
/// stays open exactly as it was.
fn hold(
    repo: &gix::Repository,
    rec: held::Recording<'_>,
    session_branch: &str,
    held: &Held,
    summary: String,
    of: usize,
) -> Result<HeldReport> {
    held::refuse_if_held(repo, session_branch, "landed")?;
    held::record(repo, rec, session_branch, held, summary)?;
    Ok(HeldReport {
        verb: "done".to_string(),
        branch: session_branch.to_string(),
        at: held.at.clone(),
        paths: held.paths.clone(),
        of,
    })
}

/// The verb of a hold, in the spelling the reports carry — the same names
/// `held::Intent` serializes to.
fn verb_of(held: &Held) -> &'static str {
    match &held.intent {
        Intent::Restack { .. } => "restack",
        Intent::Done { .. } => "done",
        Intent::Absorb { .. } => "absorb",
        Intent::Lift { .. } => "lift",
    }
}

/// Map the `--abandon` delegation's outcome onto `DoneOutcome`: abandoning a
/// resolution is reported as dropping the branch's open work. A resolution
/// is not an editing session, so there is no commit being edited to name and
/// the honest absence is the empty string — which is what the renderer reads
/// to tell the two apart.
fn abandoned_as_done(outcome: crate::model::ResolveOutcome, branch: &str) -> Result<DoneOutcome> {
    match outcome {
        crate::model::ResolveOutcome::Abandoned(r) => Ok(DoneOutcome::Abandoned(AbandonReport {
            session: r.branch,
            editing: String::new(),
            subject: String::new(),
            onto: branch.to_string(),
            stashed: None,
            arrival: ArrivalReport::None,
            files: 0,
        })),
        other => Err(Error::msg(format!(
            "internal: an abandon of a resolution must abandon, got {other:?}"
        ))),
    }
}

/// What a landing verb tells the resolution arm about what it did. The four
/// reports do not share a shape, so the arm reads the same three answers off
/// each of them once and carries them in one value.
struct Landed {
    replayed: usize,
    /// The branch the landed stack sits on — `onto` for a session landing,
    /// whose own branch is gone by the time the verb returns.
    landed_on: String,
    new_tip: String,
}

/// A branch's tip, full sha, after a landing moved it.
fn tip_of(repo: &gix::Repository, branch: &str) -> Result<String> {
    Ok(refs::ref_target(repo, &format!("refs/heads/{branch}"))?
        .ok_or_else(|| Error::msg(format!("internal: {branch} is gone after its own landing")))?
        .to_string())
}

/// The resolution arm of `ff done`: the reader has fixed the markers
/// `ff resolve` laid down, so this puts each fix back into the step that
/// owned it, re-runs the chain, and lands the whole stack through the verb
/// that owns the rewrite — refs move one time, every landed commit is
/// clean, and the hold and the session are cleared inside the landing's own
/// operation.
fn finish_resolution(
    repo: &gix::Repository,
    rec: held::Recording<'_>,
    branch: &str,
    resolve: &held::Resolve,
) -> Result<DoneOutcome> {
    let hold = &resolve.hold;

    // 1. The conflicts the reader was given must still be the conflicts the
    // repository has: replan against the world as it stands, re-run the
    // chain, and compare. An answer that no longer matches its inputs is
    // not used, it is refused — the same self-invalidating key the futures
    // cache is built on.
    let open = resolve
        .open
        .as_deref()
        .map(|hex| gix::ObjectId::from_hex(hex.as_bytes()).map_err(Error::repo))
        .transpose()?;
    let plan = held::replan_at(repo, hold, open)?;
    let chain = rewrite::chain(repo, plan.target, plan.tip, &plan.change, &[])?;
    if chain.tree.to_string() != resolve.from {
        return Err(Error::coded(
            "held/moved",
            format!(
                "the repository changed while {branch} was resolving: these conflicts are not \
                 the ones you were given"
            ),
            vec![
                "ff resolve".into(),
                "ff resolve --abandon".into(),
                "ff status".into(),
            ],
        ));
    }

    // 2. The reader's fixes, as a tree. The index stands on the marker tree
    // the session laid down, so the scan is exactly what the reader changed.
    let (worktree_tree, _clean) = open_tree(repo, chain.tree)?;

    // 3. Attribute the fixes to the steps that own them. A region left
    // standing is a region not fixed: name the files, deduped, not the
    // regions — that is the list `ff status` shows.
    let attribution = rewrite::attribute(repo, &chain, worktree_tree)?;
    if !attribution.unresolved.is_empty() {
        let mut files: Vec<String> = attribution
            .unresolved
            .iter()
            .map(|r| r.path.clone())
            .collect();
        files.sort();
        files.dedup();
        let one = files.len() == 1;
        return Err(Error::coded(
            "held/unresolved",
            format!(
                "{} still {} conflict markers: fix them, then ff done",
                rewrite::join_paths(&files),
                if one { "carries" } else { "carry" }
            ),
            vec!["ff status".into(), "ff resolve --abandon".into()],
        ));
    }

    // 4. Re-run with the fixes folded into the steps that own them: this is
    // the stack the landing will write.
    let landed = rewrite::chain(
        repo,
        plan.target,
        plan.tip,
        &plan.change,
        &attribution.resolutions,
    )?;

    // The step whose tree the reader was actually shown: the last one the
    // FIRST run reached, which is the one that produced `chain.tree`. On a
    // whole chain that is the stack's tip; on a chain that stopped at a
    // tangle it is the end of the prefix, and the commits past it are not in
    // the working tree at all.
    let shown: Option<gix::ObjectId> = chain
        .steps
        .last()
        .map(|s| gix::ObjectId::from_hex(s.old.as_bytes()).map_err(Error::repo))
        .transpose()?;

    // A step still carrying a marker means the reader's fix created a
    // conflict further up the stack — one they were never shown, because the
    // run that laid the markers down did not get that far or did not produce
    // it. Nothing lands: the session stays open and the working tree stays
    // theirs, so the way forward is to edit it again and re-run `ff done`.
    // The shown step is exempt: its tree is the working tree, applied by the
    // override below, and that is the same reason `attribute` returns no
    // resolution for it.
    let mut stuck: Option<(gix::ObjectId, String)> = None;
    for step in &landed.steps {
        let id = gix::ObjectId::from_hex(step.old.as_bytes()).map_err(Error::repo)?;
        if Some(id) == shown {
            continue;
        }
        for path in &step.paths {
            if stuck.is_none() && rewrite::carries_markers(repo, step.tree, path)? {
                stuck = Some((id, step.subject.clone()));
            }
        }
    }
    // A re-run that tangles is the same refusal from the other side: two
    // conflicts land on one region, so the chain cannot even carry them
    // forward to be shown.
    if let Some(tangle) = &landed.tangled
        && stuck.is_none()
    {
        stuck = Some((
            gix::ObjectId::from_hex(tangle.old.as_bytes()).map_err(Error::repo)?,
            tangle.subject.clone(),
        ));
    }
    if let Some((id, subject)) = stuck {
        return Err(Error::coded(
            "held/unresolved",
            format!(
                "the fix leaves {} \"{}\" conflicting: nothing landed, so edit the working tree \
                 again and re-run ff done",
                crate::sha::short_oid(id),
                subject
            ),
            vec![
                "ff status".into(),
                "ff done".into(),
                "ff resolve --abandon".into(),
            ],
        ));
    }

    // 5. Decide the trees: each step takes the tree the re-run gave it, and
    // the SHOWN step takes the working tree — it is what the reader typed, so
    // nothing about the final state needs deriving, edits that fell outside
    // every region included. That override is also why `attribute` returns no
    // resolution for it: its tree IS the resolved tree.
    let mut trees: std::collections::HashMap<gix::ObjectId, gix::ObjectId> =
        std::collections::HashMap::new();
    for step in &landed.steps {
        let old = gix::ObjectId::from_hex(step.old.as_bytes()).map_err(Error::repo)?;
        trees.insert(old, step.tree);
    }
    if let Some(old) = shown {
        trees.insert(old, worktree_tree);
    }

    // What the reader fixed, counted before the landing moves anything.
    let fixed = rewrite::regions(repo, &chain)?.len();

    // 6. Land through the verb that owns the rewrite, clearing the hold and
    // the session inside the landing's own operation. The verb does the
    // rest: refs, carried branches, published count, worktree, arrival, and
    // the one operation `ff undo` reads.
    let decided = rewrite::Decided {
        trees,
        clearing: Some(rewrite::Clearing {
            branch: branch.to_string(),
            held: Some(hold.clone()),
            resolve: Some(resolve.clone()),
        }),
    };
    // The branch the landed stack sits on, and its new tip. Both come off
    // the verb's own report rather than being re-read here: a landed `done`
    // has deleted the session branch this arm was standing on, so asking the
    // repository for "the branch's tip" would ask about a ref that is gone.
    let Landed {
        replayed,
        landed_on,
        new_tip,
    } = match &hold.intent {
        Intent::Restack { branch, onto } => {
            let (outcome, _ctx) = crate::restack::restack_with(
                repo,
                Some(branch.clone()),
                Some(onto.clone()),
                rec.prov,
                (Some(rec.now), rec.argv.clone()),
                &decided,
                crate::restack::Aim::Settled,
            )?;
            match outcome {
                crate::RestackOutcome::Restacked(report) => Landed {
                    replayed: report.replayed,
                    landed_on: report.branch,
                    new_tip: report.new_tip,
                },
                other => {
                    return Err(Error::msg(format!(
                        "internal: the decided restack did not land: {other:?}"
                    )));
                }
            }
        }
        Intent::Done { .. } => {
            let (outcome, _ctx) = done_with(
                repo,
                false,
                rec.prov,
                (Some(rec.now), rec.argv.clone()),
                &decided,
            )?;
            match outcome {
                DoneOutcome::Done(report) => Landed {
                    replayed: report.replayed,
                    landed_on: report.onto,
                    new_tip: report.new_tip,
                },
                other => {
                    return Err(Error::msg(format!(
                        "internal: the decided done did not land: {other:?}"
                    )));
                }
            }
        }
        Intent::Absorb { into, paths } => {
            let into = gix::ObjectId::from_hex(into.as_bytes()).map_err(Error::repo)?;
            let (outcome, _ctx) = crate::absorb::absorb_with(
                repo,
                Some(into),
                paths.clone(),
                rec.prov,
                (Some(rec.now), rec.argv.clone()),
                &decided,
            )?;
            match outcome {
                crate::AbsorbOutcome::Absorbed(report) => Landed {
                    replayed: report.restacked,
                    new_tip: tip_of(repo, &report.branch)?,
                    landed_on: report.branch,
                },
                other => {
                    return Err(Error::msg(format!(
                        "internal: the decided absorb did not land: {other:?}"
                    )));
                }
            }
        }
        Intent::Lift { from, paths } => {
            let from = gix::ObjectId::from_hex(from.as_bytes()).map_err(Error::repo)?;
            let (outcome, _ctx) = crate::absorb::lift_with(
                repo,
                Some(from),
                paths.clone(),
                rec.prov,
                (Some(rec.now), rec.argv.clone()),
                &decided,
            )?;
            match outcome {
                crate::LiftOutcome::Lifted(report) => Landed {
                    replayed: report.restacked,
                    new_tip: tip_of(repo, &report.branch)?,
                    landed_on: report.branch,
                },
                other => {
                    return Err(Error::msg(format!(
                        "internal: the decided lift did not land: {other:?}"
                    )));
                }
            }
        }
    };

    // 7. Ask once more. A hold is a cache over "this rewrite conflicts", and
    // the landing has just changed every input to it, so the question is put
    // again against the world as it now stands: still conflicting means there
    // is more rewrite left, and it gets a fresh hold recorded exactly as a
    // verb would. A replan that no longer has an answer means the question
    // itself is gone — the session ended, the target was rewritten out from
    // under it — which is the clean answer, as is a clean verdict.
    let mut still_held: Option<HeldReport> = None;
    if let Ok(plan) = held::replan(repo, hold)
        && let Ok(Some(conflict)) = rewrite::conflict(repo, plan.target, plan.tip, &plan.change)
    {
        let held = Held {
            intent: hold.intent.clone(),
            at: conflict.at,
            paths: conflict.paths,
            time: rec.now,
        };
        let verb = verb_of(hold);
        still_held = Some(crate::done::hold(
            repo,
            rec,
            &landed_on,
            &held,
            format!("hold {verb} of {landed_on}"),
            conflict.of,
        )?);
    }

    Ok(DoneOutcome::Resolved(crate::model::ResolvedReport {
        branch: landed_on,
        verb: verb_of(hold).to_string(),
        fixed,
        replayed,
        new_tip,
        still_held,
    }))
}

/// The triple an editing session lands: the anchor takes the session's
/// assembled tree and message, and the commits that waited ahead replay
/// onto it.
///
/// It reads the session fresh — the anchor, the branch it lands on, and the
/// assembled tree are all things that could have moved since the hold was
/// recorded. It raises only the refusals about the triple not existing (no
/// session, `onto` gone, anchor unreachable); the ones about how the verb was
/// invoked stay with the verb.
pub(crate) fn replan_done(
    repo: &gix::Repository,
    session_branch: &str,
    open: Option<gix::ObjectId>,
) -> Result<held::Replan> {
    let meta = branchmeta::read(repo, session_branch)?;
    let sess = meta.session.ok_or_else(|| {
        Error::coded(
            "session/none",
            "no editing session is running",
            vec!["ff edit <rev>".into(), "ff status".into()],
        )
    })?;

    let onto = sess.onto.clone();
    let anchor = gix::ObjectId::from_hex(sess.at.as_bytes()).map_err(Error::repo)?;
    let onto_tip = refs::ref_target(repo, &format!("refs/heads/{onto}"))?.ok_or_else(|| {
        Error::coded(
            "branch/not-found",
            format!("{onto}, the branch this session replays onto, no longer exists"),
            vec!["ff switch <branch>".into()],
        )
    })?;
    // The anchor must still sit in `onto`'s history — the one thing the
    // landing can replay onto. A hold recorded earlier may outlive it.
    let bases: Vec<gix::ObjectId> = repo
        .merge_bases_many(anchor, &[onto_tip])
        .map_err(Error::repo)?
        .into_iter()
        .map(|id| id.detach())
        .collect();
    if !bases.contains(&anchor) {
        return Err(Error::coded(
            "session/unreachable",
            format!(
                "{} is no longer in {onto}'s history: this session has nothing to land onto",
                crate::sha::short_oid(anchor)
            ),
            vec!["ff done --abandon".into(), "ff log".into()],
        ));
    }

    let session_tip =
        refs::ref_target(repo, &format!("refs/heads/{session_branch}"))?.ok_or_else(|| {
            Error::coded(
                "branch/not-found",
                format!("no branch named {session_branch}"),
                vec![],
            )
        })?;
    let session_tip_tree = tree_of(repo, session_tip)?;
    // `open`, when given, is the working tree a resolution session recorded
    // before it wrote the markers over it — the session's own content, which
    // is what this otherwise reads from disk.
    let assembled = match open {
        Some(tree) => tree,
        None => open_tree(repo, session_tip_tree)?.0,
    };
    let tip_message = message_of(repo, session_tip)?;
    let anchor_message = message_of(repo, anchor)?;
    let change = rewrite::Change::Tree {
        tree: assembled,
        message: (tip_message != anchor_message).then_some(tip_message.into()),
    };
    Ok(held::Replan {
        target: anchor,
        tip: onto_tip,
        change,
    })
}

/// End the editing session running on HEAD: land it (amend, replay, return),
/// or drop it with `abandon`. See the module docs.
pub fn done(
    repo: &gix::Repository,
    abandon: bool,
    prov: &Provenance,
    now: Option<i64>,
    argv: Vec<String>,
) -> Result<(DoneOutcome, verb::VerbContext)> {
    done_with(repo, abandon, prov, (now, argv), &rewrite::Decided::none())
}

/// `done`, with some rewritten commits' trees decided in advance: those
/// skip the three-way merge and take what they are given, and the pre-flight
/// — a question about merges that are no longer going to happen — is asked
/// only when nothing is decided.
pub fn done_with(
    repo: &gix::Repository,
    abandon: bool,
    prov: &Provenance,
    invocation: (Option<i64>, Vec<String>),
    decided: &rewrite::Decided,
) -> Result<(DoneOutcome, verb::VerbContext)> {
    let (now, argv) = invocation;
    // 1. Guards.
    if repo.workdir().is_none() {
        return Err(Error::coded(
            "repo/bare",
            "bare repository: nothing to finish",
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

    // 2.
    let ctx = verb::begin_verb(repo, prov, now)?;
    let now = ctx.now;

    // 3. The session.
    let head = crate::head::head_state(repo)?;
    let (session_branch, session_tip) = match &head {
        HeadState::Branch { name, commit, .. } => {
            let tip = gix::ObjectId::from_hex(commit.as_bytes()).map_err(Error::repo)?;
            (name.clone(), tip)
        }
        HeadState::Unborn { .. } | HeadState::Detached { .. } => return Err(session_none()),
    };

    // 3b. A resolution underfoot: `ff resolve` laid a held rewrite's
    // conflicts into this working tree, and finishing them is this verb's
    // job — not the editing-session landing below, which would read the
    // markers as the session's own content. `decided.clearing` set means
    // this call IS such a landing (entered from the arm below), in which
    // case the hold and the session are cleared by its own operation and
    // the ordinary path runs.
    let resolving = held::resolving(repo, &session_branch)?;
    if let Some(session) = &resolving
        && decided.clearing.is_none()
    {
        if abandon {
            // Abandoning a resolution is the same act whichever verb spells
            // it: one implementation, called from both doors.
            let (outcome, _ctx) = crate::resolve::resolve(repo, true, prov, Some(now), argv)?;
            return Ok((abandoned_as_done(outcome, &session_branch)?, ctx));
        }
        let rec = held::Recording {
            ctx: &ctx,
            prov,
            argv,
            now,
        };
        return Ok((finish_resolution(repo, rec, &session_branch, session)?, ctx));
    }

    let meta = branchmeta::read(repo, &session_branch)?;
    let Some(sess) = meta.session.clone() else {
        return Err(session_none());
    };

    let onto = sess.onto.clone();
    let anchor = gix::ObjectId::from_hex(sess.at.as_bytes()).map_err(Error::repo)?;
    let anchor_short = crate::sha::short_oid(anchor);
    let anchor_subject = subject(repo, anchor)?;

    // 4a. A commit landed on top of the session — the branch grew — refuses:
    // finishing would amend the anchor and leave the landed commit behind.
    // A rewrite of the anchor in place (`ff absorb`, `ff lift`, `ff describe`)
    // does not grow the branch: the rewrite copies the parent list, so the
    // first-parent comparison accepts it. Landing path only — abandon drops
    // the session anyway.
    if !abandon {
        let tip_parent = first_parent(repo, session_tip)?;
        let anchor_parent = first_parent(repo, anchor)?;
        if tip_parent != anchor_parent {
            return Err(Error::coded(
                "session/moved",
                format!(
                    "{session_branch} has commits of its own since the session opened: \
                     finishing would amend {anchor_short} and leave them behind"
                ),
                vec!["ff done --abandon".into(), "ff undo".into()],
            ));
        }
    }

    // 4b. `onto` must still exist — both paths.
    let onto_ref = format!("refs/heads/{onto}");
    let onto_tip = refs::ref_target(repo, &onto_ref)?.ok_or_else(|| {
        Error::coded(
            "branch/not-found",
            format!("{onto}, the branch this session replays onto, no longer exists"),
            vec!["ff switch <branch>".into()],
        )
    })?;
    branch::guard_other_worktrees(repo, &onto)?;

    // 4c. The edited commit must still be in `onto`'s history — landing path only.
    if !abandon {
        let bases: Vec<gix::ObjectId> = repo
            .merge_bases_many(anchor, &[onto_tip])
            .map_err(Error::repo)?
            .into_iter()
            .map(|id| id.detach())
            .collect();
        if !bases.contains(&anchor) {
            return Err(Error::coded(
                "session/unreachable",
                format!(
                    "{anchor_short} \"{anchor_subject}\" is no longer in {onto}'s history: this \
                     session has nothing to land onto"
                ),
                vec!["ff done --abandon".into(), "ff log".into()],
            ));
        }
    }

    let session_ref = format!("refs/heads/{session_branch}");
    let session_tip_tree = tree_of(repo, session_tip)?;

    // 5. The landing path: the rewrite. Planning only — no ref moves yet.
    // The anchor is what sits in `onto`'s history and therefore what can be
    // replayed onto; the tip only says what the content became.
    let mut rewrite_plan: Option<rewrite::RewritePlan> = None;
    let mut unchanged = false;
    let mut worktree_tree: Option<gix::ObjectId> = None;
    if !abandon {
        let anchor_tree = tree_of(repo, anchor)?;
        // The triple the session lands — the same one `held::replan`
        // re-derives, so the verb and the replan cannot disagree. Under a
        // resolution the session's content comes from what the session
        // recorded, because the markers are standing in the working tree.
        let session_open = resolving
            .as_ref()
            .and_then(|s| s.open.as_deref())
            .map(|hex| gix::ObjectId::from_hex(hex.as_bytes()).map_err(Error::repo))
            .transpose()?;
        let change = replan_done(repo, &session_branch, session_open)?.change;
        let (assembled, reworded) = match &change {
            rewrite::Change::Tree { tree, message } => (*tree, message.is_some()),
            other => {
                return Err(Error::msg(format!(
                    "internal: a session landing is not a tree change: {other:?}"
                )));
            }
        };
        // The amend is one tree change carrying both halves that differ
        // from the anchor: the content the worktree holds, and the tip's
        // message if a reword landed on the tip under us. A pure reword is
        // the same change with the anchor's own tree, so no merge runs
        // anywhere. Neither differs and there is nothing to land.
        if assembled == anchor_tree && !reworded {
            unchanged = true;
        } else {
            // The amend and the replay behind it are one rewrite. Pre-flight it
            // with the same `change` `plan` will get: a conflict is a hold, and
            // after a clean pre-flight `plan` cannot conflict. Returning here
            // stays on the planning side of `done` — nothing has mutated, so
            // the session is still open for `ff resolve` or a clean retry.
            // Skipped for a decided landing: its trees are already known, so
            // the replay has nothing left to conflict on.
            if decided.is_empty()
                && let Some(conflict) = rewrite::conflict(repo, anchor, onto_tip, &change)?
            {
                // The hold is recorded on the session branch: that is the branch
                // underfoot and the one `ff resolve` will find, not `onto`.
                let held = Held {
                    intent: Intent::Done {
                        session: session_branch.clone(),
                    },
                    at: conflict.at.clone(),
                    paths: conflict.paths.clone(),
                    time: now,
                };
                return Ok((
                    DoneOutcome::Held(hold(
                        repo,
                        held::Recording {
                            ctx: &ctx,
                            prov,
                            argv,
                            now,
                        },
                        &session_branch,
                        &held,
                        format!("hold done of {session_branch}"),
                        conflict.of,
                    )?),
                    ctx,
                ));
            }
            rewrite_plan = Some(rewrite::plan_with(
                repo,
                anchor,
                onto_tip,
                &change,
                now,
                &decided.trees,
            )?);
        }
        // What the worktree actually holds, which is `assembled` on an
        // ordinary landing and the marker tree the reader fixed under a
        // resolution — the amend lands the session's content either way, but
        // the transition below has to start from what is really on disk.
        worktree_tree = Some(open_tree(repo, session_tip_tree)?.0);
    }

    // 6. The abandon path: park what is uncommitted. Planning only.
    let park_plan = if abandon {
        stash::plan_park(repo, &head, now)?
    } else {
        None
    };

    // `plan.carried` holds the session branch too (the map rewrote it): that
    // entry becomes a deletion, not a ref update, below.
    let carried: Vec<RefTransition> = rewrite_plan
        .as_ref()
        .map(|plan| {
            plan.carried
                .iter()
                .filter(|t| t.name != session_ref)
                .cloned()
                .collect()
        })
        .unwrap_or_default();

    // The anchor's new identity — or the fact that the rewrite dropped it.
    // Absent from `rewrites` legitimately only when the plan names it in
    // `dropped`; anywhere else it is an ordering bug, not a drop. No plan at
    // all means the session changed nothing, and a session that changed
    // nothing did not drop its commit.
    let amended = match &rewrite_plan {
        Some(plan) => match plan.rewrites.iter().find(|r| r.old == anchor.to_string()) {
            Some(r) => Some(r.new.clone()),
            None if plan.dropped.iter().any(|d| d.old == anchor.to_string()) => None,
            None => {
                return Err(Error::msg(
                    "the session's commit was not in the rewrite plan",
                ));
            }
        },
        None => Some(anchor.to_string()),
    };

    let (new_onto_tip, replayed, published, dropped) = match &rewrite_plan {
        Some(plan) => {
            // The anchor is in `rewrites` exactly when `amended` is `Some`,
            // so the count subtracts it exactly when it is there.
            let replayed = plan
                .rewrites
                .len()
                .saturating_sub(usize::from(amended.is_some()));
            let published = rewrite::published_count(repo, &onto, plan)?;
            (plan.new_tip, replayed, published, plan.dropped.clone())
        }
        None => (onto_tip, 0, 0, Vec::new()),
    };
    let published_on = rewrite::tracking_name(repo, &onto)?;
    let new_onto_tree = tree_of(repo, new_onto_tip)?;

    // 7. The return trip, planned before anything moves.
    let arrive_plan = stash::plan_arrival(repo, &onto, new_onto_tip, new_onto_tree)?;

    let mut planned = observe_refs(repo)?;
    let head_old = planned.head.clone();
    planned.head = format!("ref:{onto_ref}");
    planned.refs.remove(&session_ref);
    for t in &carried {
        if let Some(new) = &t.new {
            planned.refs.insert(t.name.clone(), new.clone());
        }
    }

    let mut refs_transitions: Vec<RefTransition> = carried.clone();
    refs_transitions.push(RefTransition {
        name: session_ref.clone(),
        old: Some(session_tip.to_string()),
        new: None,
    });

    let mut stash_lines: Vec<gix::ObjectId> = refs::read_ref_log(repo, stash::STASH_REF)?
        .iter()
        .map(|l| l.new)
        .collect();
    let mut effects: Vec<StashEffect> = Vec::new();
    if let Some(plan) = &park_plan {
        stash_lines.push(plan.wip_commit);
        effects.push(StashEffect::Push {
            branch: session_branch.clone(),
            stash: plan.wip_commit.to_string(),
        });
        // The parked ref this park writes is demoted within the same
        // operation — the branch it names is about to be deleted — so it
        // leaves no net transition on the recorded table; only the stash
        // push above is a real, persisting change.
    }
    match &arrive_plan {
        ArrivePlan::Restore { stash: sha, .. } => {
            if let Some(pos) = stash_lines.iter().rposition(|s| s == sha) {
                stash_lines.remove(pos);
            }
            planned.refs.remove(&stash::parked_ref(&onto));
            refs_transitions.push(RefTransition {
                name: stash::parked_ref(&onto),
                old: Some(sha.to_string()),
                new: None,
            });
            effects.push(StashEffect::Drop {
                branch: onto.clone(),
                stash: sha.to_string(),
            });
        }
        ArrivePlan::Invalidate { stash: sha } => {
            planned.refs.remove(&stash::parked_ref(&onto));
            refs_transitions.push(RefTransition {
                name: stash::parked_ref(&onto),
                old: Some(sha.to_string()),
                new: None,
            });
        }
        ArrivePlan::None | ArrivePlan::Conflict { .. } => {}
    }
    match stash_lines.last() {
        Some(tip) => {
            planned
                .refs
                .insert(stash::STASH_REF.to_string(), tip.to_string());
        }
        None => {
            planned.refs.remove(stash::STASH_REF);
        }
    }

    let (end_tree, end_index) = match &arrive_plan {
        ArrivePlan::Restore {
            target_wip,
            target_index,
            ..
        } => (*target_wip, *target_index),
        _ => (new_onto_tree, new_onto_tree),
    };

    let summary = if abandon {
        format!("done --abandon: {anchor_short} on {onto}")
    } else {
        format!("done: {anchor_short} on {onto}")
    };
    let mut record = OpRecord::new("done", summary, now);
    record.argv = argv;
    record.head = Some((head_old, format!("ref:{onto_ref}")));
    record.refs = refs_transitions;
    record.stash = effects;
    if let Some(plan) = &rewrite_plan {
        record.rewrites = plan.rewrites.clone();
        record.dropped = plan.dropped.clone();
    }
    record.edit_session = Some(SessionTransition {
        branch: session_branch.clone(),
        old: Some(sess.clone()),
        new: None,
    });
    if let Some(clearing) = &decided.clearing {
        let (held, resolving) = held::clearing_transitions(clearing);
        record.held = held;
        record.resolving = resolving;
    }

    let mut pins: Vec<gix::ObjectId> = Vec::new();
    if let Some(plan) = &rewrite_plan {
        for r in &plan.rewrites {
            pins.push(gix::ObjectId::from_hex(r.new.as_bytes()).map_err(Error::repo)?);
        }
    }
    pins.push(session_tip);
    pins.push(onto_tip);
    match &arrive_plan {
        ArrivePlan::Restore { stash, .. }
        | ArrivePlan::Conflict { stash, .. }
        | ArrivePlan::Invalidate { stash } => pins.push(*stash),
        ArrivePlan::None => {}
    }
    if let Some(plan) = &park_plan {
        pins.push(plan.wip_commit);
    }

    verb::append_op(
        repo,
        OpKind::Op,
        verb::VerbOp {
            record,
            planned,
            tree: end_tree,
            index_tree: end_index,
            branch: onto.clone(),
            base: crate::snapshot::chain::base_commit(&head)?,
            session: prov.session.clone(),
            pins: &pins,
        },
        now,
    )?;

    // 8. Mutate, in order: park (abandon, if dirty), HEAD, refs, index,
    // worktree, arrive, metadata, futures caches.
    let stashed = match &park_plan {
        Some(plan) => {
            stash::execute_park(repo, plan)?;
            Some(plan.wip_commit.to_string())
        }
        None => None,
    };

    branch::retarget_head(repo, &onto_ref, now)?;

    let reflog_msg = format!("done: onto {onto}");
    let mut edits = Vec::new();
    for t in &carried {
        let (Some(old), Some(new)) = (&t.old, &t.new) else {
            continue;
        };
        let old_id = gix::ObjectId::from_hex(old.as_bytes()).map_err(Error::repo)?;
        let new_id = gix::ObjectId::from_hex(new.as_bytes()).map_err(Error::repo)?;
        edits.push(refs::update_edit(
            &t.name,
            new_id,
            gix::refs::transaction::PreviousValue::MustExistAndMatch(gix::refs::Target::Object(
                old_id,
            )),
            &reflog_msg,
        )?);
    }
    edits.push(refs::delete_edit(&session_ref, session_tip)?);
    if let Some(plan) = &park_plan {
        edits.push(refs::delete_edit(
            &stash::parked_ref(&session_branch),
            plan.wip_commit,
        )?);
    }
    match refs::commit_edits(repo, edits, now)? {
        refs::EditOutcome::Applied => {}
        refs::EditOutcome::Contended => {
            return Err(Error::coded(
                "ref/contended",
                "refs moved while finishing the session; nothing further was changed (re-run ff \
                 done)",
                vec![],
            ));
        }
    }

    crate::index::write_index_for_tree(repo, new_onto_tree)?;

    // `from_tree` is what the worktree holds right now: the amended tree on
    // the landing path, the session tip's tree on the abandon path (park
    // already reset it there, and a clean abandon never dirtied it).
    let from_tree = if abandon {
        session_tip_tree
    } else {
        worktree_tree.expect("computed on the landing path above")
    };
    let everything = |_: &str| true;
    let transition =
        crate::worktree::apply_tree_transition(repo, from_tree, new_onto_tree, &everything)?;

    let arrival = stash::execute_arrival(repo, &onto, &arrive_plan, new_onto_tree, now)?;
    let arrival_report = match arrival {
        stash::Arrival::None => ArrivalReport::None,
        stash::Arrival::Restored { stash, files } => ArrivalReport::Restored { stash, files },
        stash::Arrival::Conflicted { stash, paths } => ArrivalReport::StillParked { stash, paths },
        stash::Arrival::Invalidated { stash } => ArrivalReport::Invalidated { stash },
    };

    // Metadata: clear the session, leave the file and `forked_from` alone.
    // Undo restores `session` from the recorded transition, not from the
    // file, so deleting the file wholesale would come back missing
    // `forked_from`.
    let mut session_meta = branchmeta::read(repo, &session_branch)?;
    session_meta.session = None;
    branchmeta::write(repo, &session_branch, &session_meta)?;

    // A resolution landing: clear the hold and the session it resolved, so
    // one `ff undo` of this op takes the whole resolution back.
    if let Some(clearing) = &decided.clearing {
        held::set(repo, &clearing.branch, None)?;
        held::set_resolving(repo, &clearing.branch, None)?;
    }

    // Futures caches: best-effort, as restack.rs removes them.
    let _ = futures::cache::remove(repo, &onto);
    for t in &carried {
        if let Some(name) = t.name.strip_prefix("refs/heads/") {
            let _ = futures::cache::remove(repo, name);
        }
    }

    // 9. The report.
    let mut files = transition.written.len() + transition.deleted.len();
    if let ArrivalReport::Restored { files: f, .. } = &arrival_report {
        files += f.len();
    }

    if abandon {
        Ok((
            DoneOutcome::Abandoned(AbandonReport {
                session: session_branch,
                editing: anchor.to_string(),
                subject: anchor_subject,
                onto,
                stashed,
                arrival: arrival_report,
                files,
            }),
            ctx,
        ))
    } else {
        let moved: Vec<String> = carried
            .iter()
            .filter(|t| t.name != onto_ref)
            .map(|t| {
                t.name
                    .strip_prefix("refs/heads/")
                    .unwrap_or(&t.name)
                    .to_string()
            })
            .collect(); // carried is already sorted by ref name
        Ok((
            DoneOutcome::Done(DoneReport {
                session: session_branch,
                editing: anchor.to_string(),
                amended,
                subject: anchor_subject,
                onto,
                replayed,
                moved,
                new_tip: new_onto_tip.to_string(),
                unchanged,
                published,
                published_on,
                arrival: arrival_report,
                files,
                dropped,
            }),
            ctx,
        ))
    }
}
