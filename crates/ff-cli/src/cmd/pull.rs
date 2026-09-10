//! `ff pull` — line this branch up with the base beneath it and the remote
//! copy of itself. Nothing leaves the machine: sending is `ff push`.
//! `ff restack` is one of its two axes; the other is the network, which this
//! command owns and hands to the core as a number.
//!
//! Which branches the run visits is decided first, before the network is
//! paid for: the branch underfoot, the ones named, or every local branch
//! under `--all`, each with the local bases beneath it. A name that resolves
//! to nothing is refused here, ahead of the fetch.
//!
//! The three steps run in order: read the tracking ref as it stands, fetch,
//! read it again. The reason for reading it twice is the divergence rule —
//! divergence this run's fetch created is somebody else's and your commits
//! replay on top of theirs. Divergence that was already there is only yours
//! if the operation log accounts for every commit of it; anything it does
//! not recognize replays too.
//!
//! `--dry-run` runs the same three steps and the same plan, and writes none
//! of it: the fetch still goes, because without it the report cannot say
//! what the shared copy holds, and a fetch moves remote-tracking refs and
//! nothing a person stands on. What it writes is the same thing
//! `ff git fetch` writes. `--no-fetch` beside it reads what is already here.
//! Every sentence about a move then switches to the conditional, and the
//! tail says nothing was written rather than offering an undo.

use ff_core::{
    BaseAxis, BranchPull, BranchRemote, PullReport, RemoteAxis, RestackOutcome, RestackReport,
    Result,
};

use crate::ctx::Ctx;

pub fn run(
    ctx: &Ctx,
    branches: Vec<String>,
    all: bool,
    dry_run: bool,
    no_fetch: bool,
) -> Result<()> {
    let repo = ff_core::discover(".")?;
    crate::render::init_palette(&repo);
    let colored = crate::pager::color_enabled();

    // The tracking ref as it stands before anything reaches the network,
    // and every other branch in the run beside it.
    let before = ff_core::preflight::preflight(&repo, ff_core::preflight::Verb::Pull)?;
    let scope = if all {
        ff_core::pull::Scope::All
    } else if branches.is_empty() {
        ff_core::pull::Scope::Current
    } else {
        ff_core::pull::Scope::Named(branches)
    };
    let chosen = ff_core::pull::choose(&repo, &before.branch, &scope)?;
    let others_before = ff_core::pull::read_branches(&repo, &chosen.others)?;

    let cwd = repo
        .workdir()
        // Uncoded on purpose: preflight already refused a bare repository, so
        // nobody can reach this and there is nothing to tell them.
        .ok_or_else(|| ff_core::Error::msg("no working directory: internal inconsistency"))?
        .to_path_buf();

    let mut fetched = false;
    if !no_fetch && let Some(remote) = before.remote.clone() {
        if !ctx.json {
            println!(
                "{}",
                crate::render::paint_dim(&format!("fetching from {remote}"), colored)
            );
        }
        crate::net::fetch(&cwd, &remote)?;
        fetched = true;
    }

    // And again afterwards. Re-running preflight is the honest way to read the
    // same ref twice: one function, one definition, two moments.
    let after = ff_core::preflight::preflight(&repo, ff_core::preflight::Verb::Pull)?;
    let tracking_after = after.tracking.as_ref().and_then(|t| t.tip);
    let others_after = ff_core::pull::read_branches(&repo, &chosen.others)?;
    let others = ff_core::pull::after_fetch(others_before, &others_after);

    let (report, verb_ctx) = ff_core::pull::pull(
        &repo,
        &before,
        ff_core::pull::PullOptions {
            fetched,
            tracking_after,
            current: chosen.current,
            others,
            dry_run,
            now: None,
            argv: std::env::args().collect(),
        },
        &crate::provenance::pre_ff(ctx),
    )?;
    if let Some(verb_ctx) = &verb_ctx {
        crate::render::reconcile_notice(&verb_ctx.reconcile);
    }

    // The landed reports on the branch underfoot, from either axis, gathered
    // once over both: the dropped lines and the undo hint read these; the
    // other branches say what they dropped inside their own blocks.
    let mut reports: Vec<&RestackReport> = Vec::new();
    if let RemoteAxis::Ran {
        outcome: RestackOutcome::Restacked(r),
        ..
    } = &report.remote
    {
        reports.push(&**r);
    }
    if let BaseAxis::Ran {
        outcome: RestackOutcome::Restacked(r),
        ..
    } = &report.base
    {
        reports.push(&**r);
    }

    // A hold on either axis — or on a branch stacked above, which a landed
    // axis carries in its cascade, or on any other branch — means a human
    // decision is required before anything more moves, which is exactly
    // what exit 3 says.
    let blocked = report.blocked();

    // A dry run wrote nothing, so there is nothing to undo and the hint is
    // null rather than a verb that would take back the previous operation.
    if ctx.json {
        let undo = if dry_run {
            serde_json::Value::Null
        } else {
            serde_json::Value::from("ff undo")
        };
        let payload = serde_json::json!({
            "pull": report,
            "undo": undo,
        });
        crate::machine::emit("pull", &payload)?;
        if blocked {
            crate::exit::held();
        }
        return Ok(());
    }

    // Human rendering: every report line turns `said` on, and the tail says
    // "nothing to pull" only when none of them did. Under a dry run every
    // line about a move reads in the conditional; the facts a run finds —
    // a base that moved ahead, a shared copy that is gone, a branch checked
    // out elsewhere — read the same either way, since they are as true.
    let mut said = false;
    let would = Would(dry_run);

    match &report.remote {
        RemoteAxis::NotNamed | RemoteAxis::NoRemote => {}
        RemoteAxis::Gone { name } => {
            println!("{}", gone_line(name, colored));
            said = true;
        }
        RemoteAxis::Undone { name, behind } => {
            println!("{}", undone_line(name, *behind, colored));
            said = true;
        }
        RemoteAxis::Yours { name, behind, .. } => {
            println!("{}", yours_line(name, *behind));
            said = true;
        }
        RemoteAxis::Ran { name, outcome } => {
            for line in remote_lines(name, outcome, would, colored) {
                println!("{line}");
                said = true;
            }
        }
    }

    for line in base_lines(&report.base, would, colored) {
        println!("{line}");
        said = true;
    }

    for r in &reports {
        for line in would.dropped(&r.dropped, colored) {
            println!("{line}");
            said = true;
        }
    }
    // The one worktree write is the run's: the branch underfoot may have
    // been carried by another branch's cascade, and then it has no landed
    // axis of its own to read the count from.
    if report.files > 0 {
        println!("{}", would.working_copy(report.files, report.still_open));
        said = true;
    }

    // The other half, named but not done. A branch that just lined up and
    // still holds commits its shared copy does not is exactly when pointing
    // at `ff push` is useful — and pointing is all pull does, because
    // sending is the one thing here that could not be undone. A branch
    // underfoot the run did not reach is not the branch being talked
    // about, so nothing is said of it.
    let waiting = match report.pending {
        _ if !chosen.current => None,
        ff_core::Pending::NoRemote | ff_core::Pending::Ahead(0) => None,
        ff_core::Pending::Unpublished => Some("not published yet — ff push".to_string()),
        ff_core::Pending::Ahead(n) => Some(format!("{n} commit(s) to push — ff push")),
        // The same verb clears it, pointed the other way: pushing rolls
        // the shared copy back to where the branch now stands.
        ff_core::Pending::Undone(n) => Some(format!(
            "{n} commit(s) to take off the shared copy — ff push"
        )),
    };
    if let Some(line) = waiting {
        println!("{}", crate::render::paint_dim(&line, colored));
        said = true;
    }

    // Every other branch, one block each, in report order: the name on its
    // own line, then what happened to it, indented. A branch with nothing to
    // say prints nothing, so a repository of up-to-date branches is still
    // one line.
    let mut moved_elsewhere = false;
    for b in &report.branches {
        let (name, lines, moved) = branch_lines(b, would, colored);
        moved_elsewhere |= moved;
        if lines.is_empty() {
            continue;
        }
        println!("{name}");
        for line in &lines {
            for l in line.lines() {
                println!("    {l}");
            }
        }
        said = true;
    }

    // The tail under a run that moved anything: the way back, or under a
    // dry run the one line saying there is nothing to take back, because
    // nothing was written.
    if !reports.is_empty() || moved_elsewhere {
        let tail = if dry_run {
            "nothing was written — drop --dry-run to pull"
        } else {
            "undo: ff undo"
        };
        println!("{}", crate::render::paint_dim(tail, colored));
        said = true;
    }
    if !said {
        println!("{}", crate::render::paint_dim("nothing to pull", colored));
    }

    // One closing line when a hold stands anywhere but the branch underfoot
    // alone, whose own block has already said how to pick it up: `ff resolve`
    // takes no branch, so the way to a hold elsewhere is a switch first. A
    // dry run recorded no hold, so there is nothing to switch to; it counts
    // the branches that would hold and exits the same 3, since the answer
    // a script wants is whether the run needs a person.
    if blocked {
        let held = held_branches(&report);
        if let Some(first) = held.iter().find(|b| **b != report.branch) {
            let line = if dry_run {
                format!("{} branch(es) would hold", held.len())
            } else {
                format!(
                    "{} branch(es) held — ff switch {first}, then ff resolve",
                    held.len()
                )
            };
            println!("{}", crate::render::paint_warn(&line, colored));
        }
        crate::exit::held();
    }
    Ok(())
}

/// Whether the run is a dry run, carried into every line about a move so
/// it can read in the conditional. The lines a real run prints are the
/// ones the tests have always pinned; a dry run's are the same facts with
/// "would" in front, and the hold and cascade blocks, which offer a way
/// out of a hold that was not recorded, are replaced with what would hold.
#[derive(Clone, Copy)]
struct Would(bool);

impl Would {
    /// The verb in the tense the run calls for: `did` after a real run,
    /// `would <bare>` under a dry one.
    fn verb(self, did: &str, bare: &str) -> String {
        if self.0 {
            format!("would {bare}")
        } else {
            did.to_string()
        }
    }

    fn working_copy(self, files: usize, still_open: bool) -> String {
        let updated = self.verb("updated", "update");
        if still_open {
            let stays = if self.0 { "stays" } else { "is still" };
            format!("{updated} the working copy ({files} file(s)); your change {stays} open")
        } else {
            format!("{updated} the working copy ({files} file(s))")
        }
    }

    /// The dropped lines, in the conditional under a dry run: the render
    /// helper's sentences open with the verb, so the tense is one word.
    fn dropped(self, dropped: &[ff_core::rewrite::Dropped], colored: bool) -> Vec<String> {
        crate::render::dropped_lines(dropped, None, colored)
            .into_iter()
            .map(|line| {
                if self.0 {
                    line.replacen("dropped ", "would drop ", 1)
                } else {
                    line
                }
            })
            .collect()
    }

    /// The block for a hold: the render helper's after a real run, with
    /// its two ways out, and under a dry run the one line saying where the
    /// replay would stop, since no hold was recorded to resolve or drop.
    fn held(self, h: &ff_core::HeldReport, colored: bool) -> String {
        if !self.0 {
            return crate::render::held_block(h, colored);
        }
        crate::render::paint_warn(
            &format!(
                "would hold: {} conflicts in {}",
                where_it_stops(&h.at),
                join_paths(&h.paths)
            ),
            colored,
        )
    }

    /// The lines for the branches stacked above a moved one: the render
    /// helper's after a real run, and under a dry run one line per branch
    /// saying what it would do, without the ways out of a hold that was
    /// not recorded.
    fn cascade(self, cascade: &ff_core::Cascade, colored: bool) -> Vec<String> {
        if !self.0 {
            return crate::render::cascade_lines(cascade, colored);
        }
        let mut out = Vec::new();
        for m in &cascade.moved {
            out.push(format!(
                "{} would follow {}: {} commit(s) to replay",
                m.branch, m.base, m.replayed
            ));
            for line in self.dropped(&m.dropped, colored) {
                out.push(format!("    {line}"));
            }
        }
        for h in &cascade.held {
            out.push(crate::render::paint_warn(
                &format!(
                    "{} would hold: {} conflicts in {}{}",
                    h.branch,
                    where_it_stops(&h.report.at),
                    join_paths(&h.report.paths),
                    left_alone(&h.left_alone)
                ),
                colored,
            ));
        }
        for s in &cascade.skipped {
            out.push(crate::render::paint_warn(
                &format!(
                    "{} would be skipped: {}{}",
                    s.branch,
                    crate::render::skip_reason(&s.reason, &s.base),
                    left_alone(&s.left_alone)
                ),
                colored,
            ));
        }
        out
    }
}

/// Where a replay stops, the way the held block says it.
fn where_it_stops(at: &ff_core::futures::At) -> String {
    match at {
        ff_core::futures::At::Commit { id, subject } => format!(
            "replaying {} \"{}\"",
            ff_core::sha::short(id),
            crate::render::truncate_subject(subject)
        ),
        ff_core::futures::At::OpenChange => "your open change".to_string(),
    }
}

/// Paths the way the held block prints them: all of them up to three, then
/// the first three and a count.
fn join_paths(paths: &[String]) -> String {
    if paths.len() <= 3 {
        paths.join(", ")
    } else {
        format!("{}, and {} more", paths[..3].join(", "), paths.len() - 3)
    }
}

/// The tail a held or skipped branch's line carries for the branches above
/// it, which would stay where they stand because their base would not move.
fn left_alone(names: &[String]) -> String {
    if names.is_empty() {
        String::new()
    } else {
        format!("; above it, {} left alone", names.join(", "))
    }
}

fn gone_line(name: &str, colored: bool) -> String {
    crate::render::paint_warn(
        &format!("the remote copy is gone — {name} is configured but not there"),
        colored,
    )
}

fn undone_line(name: &str, behind: usize, colored: bool) -> String {
    crate::render::paint_warn(
        &format!("{name} still holds {behind} commit(s) you undid, so nothing was taken in"),
        colored,
    )
}

fn yours_line(name: &str, behind: usize) -> String {
    format!(
        "{name} still holds {behind} commit(s) this branch rewrote; the log accounts for every one, so they are stale copies of your own"
    )
}

/// What the remote axis says once it ran: the shared copy taken in, or the
/// hold, then what the branches stacked above did when the replay moved
/// the branch. Nothing when there was nothing to take in.
fn remote_lines(name: &str, outcome: &RestackOutcome, would: Would, colored: bool) -> Vec<String> {
    let mut out = Vec::new();
    match outcome {
        RestackOutcome::NothingToRestack { .. } => {}
        RestackOutcome::Restacked(r) if r.fast_forward => {
            out.push(crate::render::paint_ok(
                &format!(
                    "{} to {name} ({} commit(s))",
                    would.verb("fast-forwarded", "fast-forward"),
                    r.behind
                ),
                colored,
            ));
            out.extend(would.cascade(&r.cascade, colored));
        }
        RestackOutcome::Restacked(r) => {
            out.push(format!(
                "{} {} commit(s) from {name}",
                would.verb("took in", "take in"),
                r.behind
            ));
            out.push(format!(
                "{} {} of yours on top",
                would.verb("replayed", "replay"),
                r.replayed
            ));
            out.extend(would.cascade(&r.cascade, colored));
        }
        RestackOutcome::Held(h) => out.push(would.held(h, colored)),
    }
    out
}

/// What the base axis says: the base that moved and the replay onto it, the
/// hold, or why it was left alone. Nothing when the branch already sat on
/// its base, or has none.
fn base_lines(base: &BaseAxis, would: Would, colored: bool) -> Vec<String> {
    let mut out = Vec::new();
    match base {
        BaseAxis::NotNamed | BaseAxis::NoBase => {}
        BaseAxis::Skipped => out.push(crate::render::paint_dim(
            &format!(
                "the base {} left alone: the first axis that conflicts stops the run",
                would.verb("was", "be")
            ),
            colored,
        )),
        // Only a branch not underfoot is refused; the branch underfoot's
        // refusal is the verb's own error, so this arm prints under a
        // branch block alone.
        BaseAxis::Refused { name, reason } => {
            out.push(crate::render::paint_warn(
                &format!(
                    "{} alone: {}",
                    would.verb("left", "be left"),
                    crate::render::skip_reason(reason, name)
                ),
                colored,
            ));
        }
        BaseAxis::Ran { name, outcome } => match outcome {
            RestackOutcome::NothingToRestack { .. } => {}
            RestackOutcome::Restacked(r) if r.fast_forward => {
                out.push(crate::render::paint_ok(
                    &format!(
                        "{} to {name} — nothing to replay",
                        would.verb("fast-forwarded", "fast-forward")
                    ),
                    colored,
                ));
                out.extend(would.cascade(&r.cascade, colored));
            }
            RestackOutcome::Restacked(r) => {
                out.push(format!("{name} moved ahead by {} commit(s)", r.behind));
                out.push(format!(
                    "{} {} commit(s) onto {name}",
                    would.verb("replayed", "replay"),
                    r.replayed
                ));
                out.extend(would.cascade(&r.cascade, colored));
            }
            RestackOutcome::Held(h) => out.push(would.held(h, colored)),
        },
    }
    out
}

/// One other branch's block: its name, the lines under it, and whether
/// anything landed on it, which is what the undo hint counts. The remote
/// axis speaks first and the base axis second, the order they ran in; a
/// branch pull did not touch says why in one dim line.
fn branch_lines(b: &BranchPull, would: Would, colored: bool) -> (&str, Vec<String>, bool) {
    let mut out = Vec::new();
    let mut moved = false;
    let name = match b {
        BranchPull::Elsewhere { branch, path } => {
            out.push(crate::render::paint_dim(
                &format!("checked out in {path} — skipped; run ff restack {branch} there"),
                colored,
            ));
            branch
        }
        BranchPull::Held { branch, verb } => {
            out.push(crate::render::paint_dim(
                &format!(
                    "a held {verb} stands on it — skipped; ff switch {branch}, then ff resolve"
                ),
                colored,
            ));
            branch
        }
        BranchPull::Pulled {
            branch,
            remote,
            base,
        } => {
            match remote {
                BranchRemote::NoRemote
                | BranchRemote::NotFetched { .. }
                | BranchRemote::UpToDate { .. } => {}
                BranchRemote::Gone { name } => out.push(gone_line(name, colored)),
                BranchRemote::Undone { name, behind } => {
                    out.push(undone_line(name, *behind, colored))
                }
                BranchRemote::Moved {
                    name,
                    fast_forward: true,
                    behind,
                    ..
                } => {
                    out.push(crate::render::paint_ok(
                        &format!(
                            "{} to {name} ({behind} commit(s))",
                            would.verb("fast-forwarded", "fast-forward")
                        ),
                        colored,
                    ));
                    moved = true;
                }
                BranchRemote::Moved { name, behind, .. } => {
                    out.push(crate::render::paint_ok(
                        &format!(
                            "{} {name} after a force-push ({behind} commit(s))",
                            would.verb("followed", "follow")
                        ),
                        colored,
                    ));
                    moved = true;
                }
                BranchRemote::Yours { name, behind, .. } => out.push(yours_line(name, *behind)),
                BranchRemote::Ran { name, outcome } => {
                    out.extend(remote_lines(name, outcome, would, colored));
                    if let RestackOutcome::Restacked(r) = outcome {
                        out.extend(would.dropped(&r.dropped, colored));
                        moved = true;
                    }
                }
            }
            out.extend(base_lines(base, would, colored));
            if let BaseAxis::Ran {
                outcome: RestackOutcome::Restacked(r),
                ..
            } = base.as_ref()
            {
                out.extend(would.dropped(&r.dropped, colored));
                moved = true;
            }
            branch
        }
    };
    (name, out, moved)
}

/// Every branch a hold stands on after this run, in report order: the branch
/// underfoot when either of its axes held, the branches its cascades held,
/// then each other branch that held on its own axes or in its cascades.
fn held_branches(report: &PullReport) -> Vec<String> {
    fn of(outcome: &RestackOutcome, out: &mut Vec<String>) {
        match outcome {
            RestackOutcome::Held(h) => out.push(h.branch.clone()),
            RestackOutcome::Restacked(r) => {
                out.extend(r.cascade.held.iter().map(|h| h.branch.clone()))
            }
            RestackOutcome::NothingToRestack { .. } => {}
        }
    }
    let mut out = Vec::new();
    if let RemoteAxis::Ran { outcome, .. } = &report.remote {
        of(outcome, &mut out);
    }
    if let BaseAxis::Ran { outcome, .. } = &report.base {
        of(outcome, &mut out);
    }
    for b in &report.branches {
        if let BranchPull::Pulled { remote, base, .. } = b {
            if let BranchRemote::Ran { outcome, .. } = remote {
                of(outcome, &mut out);
            }
            if let BaseAxis::Ran { outcome, .. } = base.as_ref() {
                of(outcome, &mut out);
            }
        }
    }
    out.dedup();
    out
}
