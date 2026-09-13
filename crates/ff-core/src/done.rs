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
//! `--abandon` is the escape hatch: it leaves the session's uncommitted edits
//! as the session's open commit, pinned by the operation, rather than landing
//! them — never discarded — and it works in
//! exactly the states where `ff done` refuses — a session that gained
//! commits of its own, or one whose anchor fell out of the landing branch's
//! history, both fold away without complaint under `--abandon`.

use crate::absorb::cascade_after;
use crate::branch;
use crate::branchmeta;
use crate::cascade::CascadePlan;
use crate::error::{Error, Result};
use crate::futures;
use crate::held::{self, Held, Intent};
use crate::hooks;
use crate::model::{
    AbandonReport, ArrivalReport, Cascade, DoneOutcome, DoneReport, HeadState, HeldReport,
};
use crate::ops::record::{SessionTransition, observe_refs};
use crate::ops::{OpKind, OpRecord, RefTransition, verb};
use crate::park;
use crate::refs;
use crate::rewrite;
use crate::snapshot::Provenance;
use crate::snapshot::tree as snaptree;
use crate::stash;

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
    let commit_ref =
        gix::objs::CommitRef::from_bytes(&obj.data, repo.object_hash()).map_err(Error::repo)?;
    Ok(commit_ref.message.to_string())
}

/// A commit's first parent, or `None` for a root.
fn first_parent(repo: &gix::Repository, id: gix::ObjectId) -> Result<Option<gix::ObjectId>> {
    let obj = repo.find_object(id).map_err(Error::repo)?;
    let commit_ref =
        gix::objs::CommitRef::from_bytes(&obj.data, repo.object_hash()).map_err(Error::repo)?;
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
        Intent::Arrive { .. } => "switch",
    }
}

/// Map the `--abandon` delegation's outcome onto `DoneOutcome`: abandoning a
/// resolution is reported as dropping the session branch and returning to
/// the branch the hold stood on. There is no commit being edited to name,
/// and the honest absence is the empty string — which is what the renderer
/// reads to tell the two apart.
fn abandoned_as_done(outcome: crate::model::ResolveOutcome) -> Result<DoneOutcome> {
    match outcome {
        crate::model::ResolveOutcome::Abandoned(r) => Ok(DoneOutcome::Abandoned(AbandonReport {
            session: r.session.unwrap_or_default(),
            editing: String::new(),
            subject: String::new(),
            onto: r.branch,
            left: None,
            arrival: r.arrival,
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
    /// What the verb's cascade did to the branches stacked above.
    cascade: Cascade,
    /// What became of the parked change waiting where HEAD landed.
    arrival: ArrivalReport,
}

/// A branch's tip, full sha, after a landing moved it.
fn tip_of(repo: &gix::Repository, branch: &str) -> Result<String> {
    Ok(refs::ref_target(repo, &format!("refs/heads/{branch}"))?
        .ok_or_else(|| Error::msg(format!("internal: {branch} is gone after its own landing")))?
        .to_string())
}

/// The resolution arm of `ff done`: the reader has fixed the markers
/// `ff resolve` laid down on the session branch `session`, so this puts
/// each fix back into the step that owned it, re-runs the chain, and lands
/// the whole stack through the verb that owns the rewrite — refs move one
/// time, every landed commit is clean, and the session branch, the hold on
/// `branch`, and the record of the session are all cleared inside the
/// landing's own operation, which also brings HEAD back.
fn finish_resolution(
    repo: &gix::Repository,
    rec: held::Recording<'_>,
    branch: &str,
    resolve: &held::Resolve,
    session: &str,
    verify: hooks::Verify,
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
    let plan = held::replan_at(repo, hold, Some(branch), open)?;
    let chain = rewrite::chain(repo, plan.target, plan.tip, &plan.change, &[])?;
    if chain.tree != resolve.from {
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

    // 2. The reader's fixes, as a tree: the session branch's working copy.
    // `open_tree` composes the scan onto its base, and that is the working
    // copy's content only when the index holds that base — so the base is
    // the session tip's tree, which is the marker commit's, equal to
    // `chain.tree` by the check above, unless something committed on the
    // session. A commit there is allowed as on any editing session: the
    // attribution below reads content, not history, so committed fixes land
    // the same way typed ones do.
    //
    // This is fufu's `rebase --continue`, which under git does run
    // `pre-commit`: the fixes on disk are about to become commit content, so
    // the gate runs over them, and a formatter's rewrite is re-read into the
    // tree that lands. The window stays open across the landing below and is
    // disarmed once the refs have moved.
    let session_tip = refs::ref_target(repo, &format!("refs/heads/{session}"))?
        .ok_or_else(|| Error::msg(format!("internal: the session branch {session} is gone")))?;
    let (mut worktree_tree, _clean) = open_tree(repo, tree_of(repo, session_tip)?)?;
    let mut window = None;
    if verify == hooks::Verify::Run && hooks::will_run(repo, &["pre-commit"])? {
        let (opened, ran) = hooks::Window::open(repo, worktree_tree, &[], verify, "done")?;
        window = Some(opened);
        if ran {
            // The window has since written the index to `worktree_tree`, so
            // that is the base the re-read owes.
            worktree_tree = open_tree(repo, worktree_tree)?.0;
        }
    }

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

    refuse_if_stuck(repo, &landed, shown)?;

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
    // The way back: planned here, once, and executed by the verb inside its
    // own operation, so the session branch's deletion and the HEAD move ride
    // the landing's record.
    let return_trip = held::Return::plan(repo, session, branch, &hold.intent, worktree_tree)?;
    let decided = rewrite::Decided {
        trees,
        clearing: Some(rewrite::Clearing {
            branch: branch.to_string(),
            held: Some(hold.clone()),
            resolve: Some(resolve.clone()),
            return_trip: Some(return_trip),
        }),
    };
    let Landed {
        replayed,
        landed_on,
        new_tip,
        cascade,
        arrival,
    } = land_decided(repo, &rec, hold, &decided, verify)?;

    // The landing moved refs and rewrote the index to match: the staged
    // index is no longer provisional. Every exit above — a declining hook,
    // `held/unresolved`, any `?` on the way — drops the window armed and gets
    // the index back byte-for-byte.
    if let Some(window) = window.take() {
        window.landed();
    }

    // 7. Ask once more. A hold is a cache over "this rewrite conflicts", and
    // the landing has just changed every input to it, so the question is put
    // again against the world as it now stands: still conflicting means there
    // is more rewrite left, and it gets a fresh hold recorded exactly as a
    // verb would. A replan that no longer has an answer means the question
    // itself is gone — the session ended, the target was rewritten out from
    // under it — which is the clean answer, as is a clean verdict.
    let mut still_held: Option<HeldReport> = None;
    if let Ok(plan) = held::replan_at(repo, hold, Some(&landed_on), None)
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
        session: session.to_string(),
        verb: verb_of(hold).to_string(),
        fixed,
        replayed,
        new_tip,
        still_held,
        arrival,
        cascade,
    }))
}

/// A step still carrying a marker means the reader's fix created a
/// conflict further up the stack — one they were never shown, because the
/// run that laid the markers down did not get that far or did not produce
/// it. Nothing lands: the session stays open and the working tree stays
/// theirs, so the way forward is to edit it again and re-run `ff done`.
/// The shown step is exempt: its tree is the working tree, applied by the
/// override in `finish_resolution`, and that is the same reason `attribute`
/// returns no resolution for it.
fn refuse_if_stuck(
    repo: &gix::Repository,
    landed: &rewrite::Chain,
    shown: Option<gix::ObjectId>,
) -> Result<()> {
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
                "the fix leaves {} \"{}\" conflicting: nothing landed, so edit the working copy \
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
    Ok(())
}

/// Land the decided stack through the verb that owns the rewrite. The
/// branch the landed stack sits on and its new tip both come off the verb's
/// own report rather than being re-read here: a landed `done` has deleted
/// the session branch this arm was standing on, so asking the repository
/// for "the branch's tip" would ask about a ref that is gone.
///
/// Each verb cascades onto the branches stacked above the one it moved,
/// decided or not, and that is the resumption: the subtree the hold stopped
/// replays from the landed tip, inside the landing's own operation. The arm
/// only carries the verb's account of it.
fn land_decided(
    repo: &gix::Repository,
    rec: &held::Recording<'_>,
    hold: &Held,
    decided: &rewrite::Decided,
    verify: hooks::Verify,
) -> Result<Landed> {
    Ok(match &hold.intent {
        Intent::Restack { branch, onto } => {
            let (outcome, _ctx, arrival) = crate::restack::restack_landing(
                repo,
                Some(branch.clone()),
                Some(onto.clone()),
                rec.prov,
                (Some(rec.now), rec.argv.clone()),
                decided,
                crate::restack::Aim::Settled,
            )?;
            match outcome {
                crate::RestackOutcome::Restacked(report) => Landed {
                    replayed: report.replayed,
                    landed_on: report.branch,
                    new_tip: report.new_tip,
                    cascade: report.cascade,
                    arrival,
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
                verify,
                rec.prov,
                (Some(rec.now), rec.argv.clone()),
                decided,
            )?;
            match outcome {
                DoneOutcome::Done(report) => Landed {
                    replayed: report.replayed,
                    landed_on: report.onto,
                    new_tip: report.new_tip,
                    cascade: report.cascade,
                    arrival: report.arrival,
                },
                other => {
                    return Err(Error::msg(format!(
                        "internal: the decided done did not land: {other:?}"
                    )));
                }
            }
        }
        Intent::Absorb { .. } | Intent::Lift { .. } => {
            let held::MoveIntent {
                verb,
                from,
                into,
                message,
                paths,
            } = hold.intent.as_move().expect("the arm matched a move");
            let from: Vec<crate::absorb::Endpoint> = from
                .iter()
                .map(|end| crate::absorb::Endpoint::parse(end))
                .collect::<Result<_>>()?;
            let into = crate::absorb::Endpoint::parse(into)?;
            let (outcome, _ctx) = crate::absorb::move_with(
                repo,
                &crate::absorb::MoveOptions {
                    verb,
                    from: Some(from),
                    into: Some(into),
                    paths: paths.to_vec(),
                    message: message.map(String::from),
                    verify,
                    now: Some(rec.now),
                    argv: rec.argv.clone(),
                },
                rec.prov,
                decided,
            )?;
            match outcome {
                crate::MoveOutcome::Moved(report) => Landed {
                    replayed: report.restacked,
                    new_tip: tip_of(repo, &report.branch)?,
                    landed_on: report.branch,
                    cascade: report.cascade,
                    arrival: ArrivalReport::None,
                },
                other => {
                    return Err(Error::msg(format!(
                        "internal: the decided {} did not land: {other:?}",
                        verb.as_str()
                    )));
                }
            }
        }
        // An arrival hold is resolved in place by `ff resolve`; it never has
        // a session to land.
        Intent::Arrive { .. } => {
            return Err(Error::msg(
                "internal: a held arrival has no resolution session to land",
            ));
        }
    })
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
    verify: hooks::Verify,
    prov: &Provenance,
    now: Option<i64>,
    argv: Vec<String>,
) -> Result<(DoneOutcome, verb::VerbContext)> {
    done_with(
        repo,
        abandon,
        verify,
        prov,
        (now, argv),
        &rewrite::Decided::none(),
    )
}

/// `done`, with some rewritten commits' trees decided in advance: those
/// skip the three-way merge and take what they are given, and the pre-flight
/// — a question about merges that are no longer going to happen — is asked
/// only when nothing is decided.
pub fn done_with(
    repo: &gix::Repository,
    abandon: bool,
    verify: hooks::Verify,
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

    // 3b. A resolution session underfoot is this verb's job to finish, not
    // the editing-session landing below. `decided.clearing` set means this
    // call IS such a landing (entered from the arm), in which case the return
    // trip rides its own operation and the ordinary path runs.
    let clearing = decided.clearing.as_ref();
    if clearing.is_none()
        && let Some(outcome) =
            resolution_underfoot(repo, &ctx, prov, &argv, &session_branch, abandon, verify)?
    {
        return Ok((outcome, ctx));
    }

    // Under a resolution landing HEAD stands on the resolution session, and
    // the editing session landing is the one the hold named.
    let session_branch = match clearing {
        Some(c) => c.branch.clone(),
        None => session_branch,
    };
    let session_tip = match clearing {
        Some(_) => {
            refs::ref_target(repo, &format!("refs/heads/{session_branch}"))?.ok_or_else(|| {
                Error::coded(
                    "branch/not-found",
                    format!("no branch named {session_branch}"),
                    vec![],
                )
            })?
        }
        None => session_tip,
    };

    let Session {
        sess,
        onto,
        onto_tip,
        anchor,
        anchor_short,
        anchor_subject,
    } = session_guards(repo, abandon, &session_branch, session_tip)?;

    let onto_ref = format!("refs/heads/{onto}");
    let session_ref = format!("refs/heads/{session_branch}");
    let session_tip_tree = tree_of(repo, session_tip)?;
    let return_trip = clearing.and_then(|c| c.return_trip.as_ref());

    // 5. The landing path: the rewrite. Planning only — no ref moves yet.
    let mut landing = LandingPlan::default();
    if !abandon {
        landing = match plan_landing(
            repo,
            &ctx,
            prov,
            &argv,
            verify,
            decided,
            &session_branch,
            anchor,
            onto_tip,
        )? {
            Planned::Held(report) => return Ok((DoneOutcome::Held(report), ctx)),
            Planned::Landing(plan) => plan,
        };
        // What the worktree actually holds, which is the assembled tree on an
        // ordinary landing and the resolution session's fixes under a
        // resolution — the amend lands the session's content either way, but
        // the transition below has to start from what is really on disk.
        landing.worktree_tree = Some(match return_trip {
            Some(ret) => ret.worktree,
            None => open_tree(repo, session_tip_tree)?.0,
        });
    }
    let LandingPlan {
        rewrite_plan,
        unchanged,
        worktree_tree,
        mut window,
    } = landing;

    // 6. The abandon path: what is uncommitted is the session's open commit,
    // left where the capture put it and pinned by this operation. A dirty
    // session with no commit to stand as it refuses, as leaving does.
    let left = if abandon && ctx.pre_tree != session_tip_tree {
        Some(
            crate::open::current(repo, &session_branch, ctx.pre_tree, Some(session_tip))?
                .ok_or_else(|| park::no_park(&head, &session_branch))?,
        )
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
        Some(plan) => match plan.rewrites.iter().find(|r| r.old == anchor) {
            Some(r) => Some(r.new.clone()),
            None if plan.dropped.iter().any(|d| d.old == anchor) => None,
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

    // The branches stacked on `onto`, planned now that its new tip is known
    // and before anything is written, so the whole cascade rides this
    // operation. A tip that did not move plans nothing, which covers every
    // abandon and every session that changed nothing. The session branch is
    // never in the child set: `futures::base_for` answers nothing for a
    // session, so the stack files it under no branch, and the deletion
    // below is its only transition on the record. HEAD stands on the
    // session branch and never on a child, so the cascade moves no file;
    // the return trip above is what moves the worktree. A hold up there is
    // that branch's, not this landing's.
    let cascade = if abandon {
        CascadePlan::default()
    } else {
        cascade_after(repo, &onto, onto_tip, new_onto_tip, now)?
    };

    // 7. The return trip, planned before anything moves. Under a resolution
    // it is the resolution's own, back from the session the fixes were made
    // on, with the editing session's park spent rather than restored.
    let arrive_plan = match return_trip {
        Some(ret) => ret.arrival(repo, new_onto_tip, new_onto_tree, now)?,
        None => park::plan_arrival(repo, &onto, new_onto_tip, new_onto_tree, now)?,
    };

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

    let mut stash_lines = stash::lines(repo)?;

    let (end_tree, end_index) = arrive_plan.end_trees(new_onto_tree);

    let summary = if abandon {
        format!("done --abandon: {anchor_short} on {onto}")
    } else {
        match cascade.report.moved.len() {
            0 => format!("done: {anchor_short} on {onto}"),
            n => format!("done: {anchor_short} on {onto}, and {n} above it"),
        }
    };
    let mut record = OpRecord::new("done", summary, now);
    record.argv = argv;
    record.head = Some((head_old, format!("ref:{onto_ref}")));
    record.refs = refs_transitions;
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
    pins.extend(left);
    // The arrival on `onto` rides the record. Under a resolution the return
    // trip folds it in beside the HEAD move from the resolution session,
    // that branch's deletion and its session's end, and the editing
    // session's spent park.
    match return_trip {
        Some(ret) => ret.fold_into(
            &mut planned,
            &mut record,
            &mut pins,
            &mut stash_lines,
            &arrive_plan,
        ),
        None => arrive_plan.fold_into(&mut planned, &mut record, &mut pins, &mut stash_lines),
    }
    // The cascade rides this record: its ref moves, rewrites, drops, and
    // holds, and its pins, so one undo takes the landing and the branches
    // above it back together.
    cascade.fold_into(&mut record, &mut planned, &mut pins);

    verb::append_op_hinted(
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
        arrive_plan.open_hint(),
        now,
    )?;

    // 8. Mutate, in order: HEAD, refs, index, worktree, arrive, metadata,
    // futures caches.
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
    // The branches above `onto` move in the same transaction as `onto`'s
    // own carried heads and the session's deletion: all of it or none.
    edits.extend(cascade.edits(&reflog_msg)?);
    edits.push(refs::delete_edit(&session_ref, session_tip)?);
    if let Some(ret) = return_trip {
        edits.extend(ret.edits()?);
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

    // The cascade's holds onto their branches, now that the refs have
    // moved, and the futures caches of every branch it carried.
    cascade.land(repo)?;

    // The session branch is gone, and its open ref with it: the open commit
    // stays pinned — by this operation when it was left, by the session's
    // own captures otherwise.
    crate::open::clear(repo, &session_branch, now)?;

    // The session has landed: the staged index is no longer provisional, and
    // putting the old one back would contradict the refs that just moved.
    if let Some(window) = window.take() {
        window.landed();
    }

    // Metadata: clear the session, leave the file and `forked_from` alone.
    // Undo restores `session` from the recorded transition, not from the
    // file, so deleting the file wholesale would come back missing
    // `forked_from`.
    let mut session_meta = branchmeta::read(repo, &session_branch)?;
    session_meta.session = None;
    branchmeta::write(repo, &session_branch, &session_meta)?;

    // Index, worktree, arrival. Under a resolution the return trip does all
    // three from the resolution session's fixes, spends the editing
    // session's park, and clears the hold and the resolution it resolved, so
    // one `ff undo` of this op takes the whole resolution back.
    let (arrival_report, files) = match return_trip {
        Some(ret) => ret.land(repo, new_onto_tree, &arrive_plan, now)?,
        None => {
            crate::index::write_index_for_tree(repo, new_onto_tree)?;
            // `from_tree` is what the worktree holds right now: the amended
            // tree on the landing path, the preamble's capture on the
            // abandon path — the edits left as the open commit, or the tip's
            // tree when nothing was open.
            let from_tree = if abandon {
                ctx.pre_tree
            } else {
                worktree_tree.expect("computed on the landing path above")
            };
            let everything = |_: &str| true;
            let transition = crate::worktree::apply_tree_transition(
                repo,
                from_tree,
                new_onto_tree,
                &everything,
            )?;
            let arrival = park::execute_arrival(repo, &onto, &arrive_plan, new_onto_tree, now)?;
            (arrival, transition.written.len() + transition.deleted.len())
        }
    };

    // Futures caches: best-effort, as restack.rs removes them.
    let _ = futures::cache::remove(repo, &onto);
    for t in &carried {
        if let Some(name) = t.name.strip_prefix("refs/heads/") {
            let _ = futures::cache::remove(repo, name);
        }
    }

    // 9. The report.
    let mut files = files;
    if let ArrivalReport::Restored { files: f, .. } = &arrival_report {
        files += f.len();
    }
    let outcome = if abandon {
        DoneOutcome::Abandoned(AbandonReport {
            session: session_branch,
            editing: anchor.to_string(),
            subject: anchor_subject,
            onto,
            left: left.map(|id| id.to_string()),
            arrival: arrival_report,
            files,
        })
    } else {
        DoneOutcome::Done(done_report(
            session_branch,
            anchor,
            anchor_subject,
            onto,
            &onto_ref,
            &carried,
            Landing {
                amended,
                replayed,
                new_tip: new_onto_tip,
                unchanged,
                published,
                published_on,
                arrival: arrival_report,
                files,
                dropped,
                cascade: cascade.report,
            },
        ))
    };
    Ok((outcome, ctx))
}

/// A resolution session underfoot: this branch was minted by `ff resolve`
/// over a held rewrite's conflicts, and finishing them is this verb's job —
/// not the editing-session landing, which would read the markers as the
/// session's own content. `None` when the branch underfoot is no
/// resolution session; the branch a hold stands on, whose session is open
/// elsewhere, is refused here too, since the way to its markers is a
/// switch.
fn resolution_underfoot(
    repo: &gix::Repository,
    ctx: &verb::VerbContext,
    prov: &Provenance,
    argv: &[String],
    session_branch: &str,
    abandon: bool,
    verify: hooks::Verify,
) -> Result<Option<DoneOutcome>> {
    let now = ctx.now;
    if let Some((onto, resolve)) = held::session_of(repo, session_branch)? {
        if abandon {
            // Abandoning a resolution is the same act whichever verb spells
            // it: one implementation, called from both doors.
            let (outcome, _ctx) =
                crate::resolve::resolve(repo, true, prov, Some(now), argv.to_vec())?;
            return Ok(Some(abandoned_as_done(outcome)?));
        }
        let rec = held::Recording {
            ctx,
            prov,
            argv: argv.to_vec(),
            now,
        };
        return Ok(Some(finish_resolution(
            repo,
            rec,
            &onto,
            &resolve,
            session_branch,
            verify,
        )?));
    }
    if let Some(open) = held::resolving(repo, session_branch)? {
        if open.session.is_empty() {
            return Err(held::predates_sessions(session_branch));
        }
        return Err(Error::coded(
            "held/resolving",
            format!(
                "a resolution of {session_branch} is open on {}",
                open.session
            ),
            vec![
                format!("ff switch {}", open.session),
                "ff resolve --abandon".into(),
                "ff status".into(),
            ],
        ));
    }
    Ok(None)
}

/// The session on the branch underfoot, once its guards have passed.
struct Session {
    sess: branchmeta::Session,
    onto: String,
    onto_tip: gix::ObjectId,
    anchor: gix::ObjectId,
    anchor_short: String,
    anchor_subject: String,
}

/// The session's metadata and the refusals about the triple it lands: the
/// branch grew since the session opened, `onto` is gone or checked out
/// elsewhere, or the anchor left `onto`'s history. The abandon path takes
/// only the `onto` guard: it drops the session whatever became of the rest.
fn session_guards(
    repo: &gix::Repository,
    abandon: bool,
    session_branch: &str,
    session_tip: gix::ObjectId,
) -> Result<Session> {
    let meta = branchmeta::read(repo, session_branch)?;
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
    let onto_tip = refs::ref_target(repo, &format!("refs/heads/{onto}"))?.ok_or_else(|| {
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

    Ok(Session {
        sess,
        onto,
        onto_tip,
        anchor,
        anchor_short,
        anchor_subject,
    })
}

/// The landing, planned: nothing has moved.
#[derive(Default)]
struct LandingPlan {
    /// The amend and the replay behind it; `None` when the session changed
    /// nothing, and on the abandon path.
    rewrite_plan: Option<rewrite::RewritePlan>,
    unchanged: bool,
    /// What the worktree holds, set by the caller once the return trip is
    /// known.
    worktree_tree: Option<gix::ObjectId>,
    /// The pre-commit gate's index window, disarmed once the refs have moved.
    /// Every `?` between the two drops it armed and puts `.git/index` back
    /// byte-for-byte.
    window: Option<hooks::Window>,
}

enum Planned {
    Landing(LandingPlan),
    /// The replay conflicts: the hold is recorded on the session branch —
    /// the branch underfoot and the one `ff resolve` will find, not `onto` —
    /// and nothing has mutated, so the session is still open for `ff
    /// resolve` or a clean retry.
    Held(HeldReport),
}

/// Plan the landing: the triple the session lands, the pre-commit gate over
/// the assembled tree, the pre-flight of the replay, and the message hooks
/// over a description the anchor does not carry. The anchor is what sits in
/// `onto`'s history and therefore what can be replayed onto; the tip only
/// says what the content became.
#[allow(clippy::too_many_arguments)]
fn plan_landing(
    repo: &gix::Repository,
    ctx: &verb::VerbContext,
    prov: &Provenance,
    argv: &[String],
    verify: hooks::Verify,
    decided: &rewrite::Decided,
    session_branch: &str,
    anchor: gix::ObjectId,
    onto_tip: gix::ObjectId,
) -> Result<Planned> {
    let now = ctx.now;
    let clearing = decided.clearing.as_ref();
    let mut landing = LandingPlan::default();
    let anchor_tree = tree_of(repo, anchor)?;
    // The triple the session lands — the same one `held::replan`
    // re-derives, so the verb and the replan cannot disagree. Under a
    // resolution the session's content comes from what the session
    // recorded, because the markers are standing in the working tree.
    let session_open = clearing
        .and_then(|c| c.resolve.as_ref())
        .and_then(|s| s.open.as_deref())
        .map(|hex| gix::ObjectId::from_hex(hex.as_bytes()).map_err(Error::repo))
        .transpose()?;
    let mut change = replan_done(repo, session_branch, session_open)?.change;

    // The pre-commit gate. An edit session's worktree is about to become
    // the anchor's content, so the gate runs over the tree the session
    // assembled, and a formatter's fixes are re-read into what gets
    // amended. A resolution landing has already run it in
    // `finish_resolution`: this re-entry must not run it a second time.
    if clearing.is_none()
        && verify == hooks::Verify::Run
        && let rewrite::Change::Tree { tree, .. } = &change
        && hooks::will_run(repo, &["pre-commit"])?
    {
        let (opened, ran) = hooks::Window::open(repo, *tree, &[], verify, "done")?;
        landing.window = Some(opened);
        if ran {
            change = replan_done(repo, session_branch, session_open)?.change;
        }
    }

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
        landing.unchanged = true;
        return Ok(Planned::Landing(landing));
    }
    // The amend and the replay behind it are one rewrite. Pre-flight it
    // with the same `change` `plan` will get: a conflict is a hold, and
    // after a clean pre-flight `plan` cannot conflict. Skipped for a
    // decided landing: its trees are already known, so the replay has
    // nothing left to conflict on.
    if decided.is_empty()
        && let Some(conflict) = rewrite::conflict(repo, anchor, onto_tip, &change)?
    {
        let held = Held {
            intent: Intent::Done {
                session: session_branch.to_string(),
            },
            at: conflict.at.clone(),
            paths: conflict.paths.clone(),
            time: now,
        };
        return Ok(Planned::Held(hold(
            repo,
            held::Recording {
                ctx,
                prov,
                argv: argv.to_vec(),
                now,
            },
            session_branch,
            &held,
            format!("hold done of {session_branch}"),
            conflict.of,
        )?));
    }
    // The session carries a description the anchor does not: a message
    // authored for a commit, so both message hooks run over it, named as an
    // amend the way git names `commit --amend` to `prepare-commit-msg`. A
    // declining `commit-msg` refuses here, before anything is planned.
    if let rewrite::Change::Tree {
        message: Some(text),
        ..
    } = &mut change
    {
        let anchor_hex = anchor.to_string();
        let before = text.to_string();
        let hooked = hooks::message_hooks(
            repo,
            &before,
            hooks::MsgSource::Commit(&anchor_hex),
            verify,
            "done",
        )?;
        // Untouched messages are left exactly as the session tip carries
        // them: cleanup is git's answer to a hook having edited the file,
        // not a rewrite of what was already a commit message.
        if hooked != before {
            *text = crate::close::normalize_message(&hooked).into();
        }
    }
    landing.rewrite_plan = Some(rewrite::plan_with(
        repo,
        anchor,
        onto_tip,
        &change,
        now,
        &decided.trees,
    )?);
    Ok(Planned::Landing(landing))
}

/// What the landing wrote, for the report.
struct Landing {
    amended: Option<String>,
    replayed: usize,
    new_tip: gix::ObjectId,
    unchanged: bool,
    published: usize,
    published_on: Option<String>,
    arrival: ArrivalReport,
    files: usize,
    dropped: Vec<rewrite::Dropped>,
    cascade: Cascade,
}

/// The landed session's report. `carried` is already sorted by ref name.
fn done_report(
    session: String,
    anchor: gix::ObjectId,
    subject: String,
    onto: String,
    onto_ref: &str,
    carried: &[RefTransition],
    landing: Landing,
) -> DoneReport {
    let moved: Vec<String> = carried
        .iter()
        .filter(|t| t.name != onto_ref)
        .map(|t| {
            t.name
                .strip_prefix("refs/heads/")
                .unwrap_or(&t.name)
                .to_string()
        })
        .collect();
    DoneReport {
        session,
        editing: anchor.to_string(),
        amended: landing.amended,
        subject,
        onto,
        replayed: landing.replayed,
        moved,
        new_tip: landing.new_tip.to_string(),
        unchanged: landing.unchanged,
        published: landing.published,
        published_on: landing.published_on,
        arrival: landing.arrival,
        files: landing.files,
        dropped: landing.dropped,
        cascade: landing.cascade,
    }
}
