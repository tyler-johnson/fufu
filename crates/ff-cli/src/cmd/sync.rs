//! `ff sync` — line this branch up with the base beneath it and the remote
//! copy of itself. Nothing leaves the machine: sending is `ff publish`.
//! `ff restack` is one of its two axes; the other is the network, which this
//! command owns and hands to the core as a number.
//!
//! The three steps run in order: read the tracking ref as it stands, fetch,
//! read it again. The reason for reading it twice is the divergence rule —
//! divergence this run's fetch created is somebody else's and your commits
//! replay on top of theirs. Divergence that was already there is only yours
//! if the operation log accounts for every commit of it; anything it does
//! not recognize replays too.

use ff_core::{
    BaseAxis, BranchRemote, BranchSync, RemoteAxis, RestackOutcome, RestackReport, Result,
    SyncReport,
};

use crate::ctx::Ctx;

pub fn run(ctx: &Ctx, no_fetch: bool) -> Result<()> {
    let repo = ff_core::discover(".")?;
    crate::render::init_palette(&repo);
    let colored = crate::pager::color_enabled();

    // The tracking ref as it stands before anything reaches the network,
    // and every other branch's beside it.
    let before = ff_core::preflight::preflight(&repo, ff_core::preflight::Verb::Sync)?;
    let others_before = ff_core::sync::other_branches(&repo, &before.branch)?;

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
    let after = ff_core::preflight::preflight(&repo, ff_core::preflight::Verb::Sync)?;
    let tracking_after = after.tracking.as_ref().and_then(|t| t.tip);
    let others_after = ff_core::sync::other_branches(&repo, &after.branch)?;
    let others = ff_core::sync::after_fetch(others_before, &others_after);

    let (report, verb_ctx) = ff_core::sync::sync(
        &repo,
        &before,
        ff_core::sync::SyncOptions {
            fetched,
            tracking_after,
            others,
            now: None,
            argv: std::env::args().collect(),
        },
        &crate::provenance::pre_ff(ctx),
    )?;
    crate::render::reconcile_notice(&verb_ctx.reconcile);

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

    if ctx.json {
        let payload = serde_json::json!({
            "sync": report,
            "undo": "ff undo",
        });
        crate::machine::emit("sync", &payload)?;
        if blocked {
            crate::exit::held();
        }
        return Ok(());
    }

    // Human rendering: every report line turns `said` on, and the tail says
    // "nothing to sync" only when none of them did.
    let mut said = false;

    match &report.remote {
        RemoteAxis::NoRemote => {}
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
            for line in remote_lines(name, outcome, colored) {
                println!("{line}");
                said = true;
            }
        }
    }

    for line in base_lines(&report.base, colored) {
        println!("{line}");
        said = true;
    }

    for r in &reports {
        if let Some(line) = crate::render::dropped_line(&r.dropped, None, colored) {
            println!("{line}");
            said = true;
        }
    }
    // The one worktree write is the run's: the branch underfoot may have
    // been carried by another branch's cascade, and then it has no landed
    // axis of its own to read the count from.
    if report.files > 0 {
        if report.still_open {
            println!(
                "updated the working tree ({} file(s)); your change is still open",
                report.files
            );
        } else {
            println!("updated the working tree ({} file(s))", report.files);
        }
        said = true;
    }

    // The other half, named but not done. A branch that just lined up and
    // still holds commits its shared copy does not is exactly when pointing
    // at `ff publish` is useful — and pointing is all sync does, because
    // sending is the one thing here that could not be undone.
    let waiting = match report.pending {
        ff_core::Pending::NoRemote | ff_core::Pending::Ahead(0) => None,
        ff_core::Pending::Unpublished => Some("not published yet — ff publish".to_string()),
        ff_core::Pending::Ahead(n) => Some(format!("{n} commit(s) to publish — ff publish")),
        // The same verb clears it, pointed the other way: publishing rolls
        // the shared copy back to where the branch now stands.
        ff_core::Pending::Undone(n) => Some(format!(
            "{n} commit(s) to take off the shared copy — ff publish"
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
        let (name, lines, moved) = branch_lines(b, colored);
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

    if !reports.is_empty() || moved_elsewhere {
        println!("{}", crate::render::paint_dim("undo: ff undo", colored));
        said = true;
    }
    if !said {
        println!("{}", crate::render::paint_dim("nothing to sync", colored));
    }

    // One closing line when a hold stands anywhere but the branch underfoot
    // alone, whose own block has already said how to pick it up: `ff resolve`
    // takes no branch, so the way to a hold elsewhere is a switch first.
    if blocked {
        let held = held_branches(&report);
        if let Some(first) = held.iter().find(|b| **b != report.branch) {
            println!(
                "{}",
                crate::render::paint_warn(
                    &format!(
                        "{} branch(es) held — ff switch {first}, then ff resolve",
                        held.len()
                    ),
                    colored
                )
            );
        }
        crate::exit::held();
    }
    Ok(())
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
fn remote_lines(name: &str, outcome: &RestackOutcome, colored: bool) -> Vec<String> {
    let mut out = Vec::new();
    match outcome {
        RestackOutcome::NothingToRestack { .. } => {}
        RestackOutcome::Restacked(r) if r.fast_forward => {
            out.push(crate::render::paint_ok(
                &format!("fast-forwarded to {name} ({} commit(s))", r.behind),
                colored,
            ));
            out.extend(crate::render::cascade_lines(&r.cascade, colored));
        }
        RestackOutcome::Restacked(r) => {
            out.push(format!("took in {} commit(s) from {name}", r.behind));
            out.push(format!("replayed {} of yours on top", r.replayed));
            out.extend(crate::render::cascade_lines(&r.cascade, colored));
        }
        RestackOutcome::Held(h) => out.push(crate::render::held_block(h, colored)),
    }
    out
}

/// What the base axis says: the base that moved and the replay onto it, the
/// hold, or why it was left alone. Nothing when the branch already sat on
/// its base, or has none.
fn base_lines(base: &BaseAxis, colored: bool) -> Vec<String> {
    let mut out = Vec::new();
    match base {
        BaseAxis::NoBase => {}
        BaseAxis::Skipped => out.push(crate::render::paint_dim(
            "the base was left alone: the first axis that conflicts stops the run",
            colored,
        )),
        // Only a branch not underfoot is refused; the branch underfoot's
        // refusal is the verb's own error, so this arm prints under a
        // branch block alone.
        BaseAxis::Refused { name, reason } => {
            out.push(crate::render::paint_warn(
                &format!("left alone: {}", crate::render::skip_reason(reason, name)),
                colored,
            ));
        }
        BaseAxis::Ran { name, outcome } => match outcome {
            RestackOutcome::NothingToRestack { .. } => {}
            RestackOutcome::Restacked(r) if r.fast_forward => {
                out.push(crate::render::paint_ok(
                    &format!("fast-forwarded to {name} — nothing to replay"),
                    colored,
                ));
                out.extend(crate::render::cascade_lines(&r.cascade, colored));
            }
            RestackOutcome::Restacked(r) => {
                out.push(format!("{name} moved ahead by {} commit(s)", r.behind));
                out.push(format!("replayed {} commit(s) onto {name}", r.replayed));
                out.extend(crate::render::cascade_lines(&r.cascade, colored));
            }
            RestackOutcome::Held(h) => out.push(crate::render::held_block(h, colored)),
        },
    }
    out
}

/// One other branch's block: its name, the lines under it, and whether
/// anything landed on it, which is what the undo hint counts. The remote
/// axis speaks first and the base axis second, the order they ran in; a
/// branch sync did not touch says why in one dim line.
fn branch_lines(b: &BranchSync, colored: bool) -> (&str, Vec<String>, bool) {
    let mut out = Vec::new();
    let mut moved = false;
    let name = match b {
        BranchSync::Elsewhere { branch, path } => {
            out.push(crate::render::paint_dim(
                &format!("checked out in {path} — skipped; run ff restack {branch} there"),
                colored,
            ));
            branch
        }
        BranchSync::Held { branch, verb } => {
            out.push(crate::render::paint_dim(
                &format!(
                    "a held {verb} stands on it — skipped; ff switch {branch}, then ff resolve"
                ),
                colored,
            ));
            branch
        }
        BranchSync::Synced {
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
                        &format!("fast-forwarded to {name} ({behind} commit(s))"),
                        colored,
                    ));
                    moved = true;
                }
                BranchRemote::Moved { name, behind, .. } => {
                    out.push(crate::render::paint_ok(
                        &format!("followed {name} after a force-push ({behind} commit(s))"),
                        colored,
                    ));
                    moved = true;
                }
                BranchRemote::Yours { name, behind, .. } => out.push(yours_line(name, *behind)),
                BranchRemote::Ran { name, outcome } => {
                    out.extend(remote_lines(name, outcome, colored));
                    if let RestackOutcome::Restacked(r) = outcome {
                        out.extend(crate::render::dropped_line(&r.dropped, None, colored));
                        moved = true;
                    }
                }
            }
            out.extend(base_lines(base, colored));
            if let BaseAxis::Ran {
                outcome: RestackOutcome::Restacked(r),
                ..
            } = base.as_ref()
            {
                out.extend(crate::render::dropped_line(&r.dropped, None, colored));
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
fn held_branches(report: &SyncReport) -> Vec<String> {
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
        if let BranchSync::Synced { remote, base, .. } = b {
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
