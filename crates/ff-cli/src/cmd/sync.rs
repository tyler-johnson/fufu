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

use ff_core::{BaseAxis, RemoteAxis, RestackOutcome, RestackReport, Result};

use crate::ctx::Ctx;

pub fn run(ctx: &Ctx, no_fetch: bool) -> Result<()> {
    let repo = ff_core::discover(".")?;
    crate::render::init_palette(&repo);
    let colored = crate::pager::color_enabled();

    // The tracking ref as it stands before anything reaches the network.
    let before = ff_core::preflight::preflight(&repo, ff_core::preflight::Verb::Sync)?;

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

    let (report, verb_ctx) = ff_core::sync::sync(
        &repo,
        &before,
        ff_core::sync::SyncOptions {
            fetched,
            tracking_after,
            now: None,
            argv: std::env::args().collect(),
        },
        &crate::provenance::pre_ff(ctx),
    )?;
    crate::render::reconcile_notice(&verb_ctx.reconcile);

    // A hold on either axis means a human decision is required before
    // anything more moves — which is exactly what exit 3 says.
    let blocked = matches!(
        report.remote,
        RemoteAxis::Ran {
            outcome: RestackOutcome::Held(_),
            ..
        }
    ) || matches!(
        report.base,
        BaseAxis::Ran {
            outcome: RestackOutcome::Held(_),
            ..
        }
    );

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
            println!(
                "{}",
                crate::render::paint_warn(
                    &format!("the remote copy is gone — {name} is configured but not there"),
                    colored
                )
            );
            said = true;
        }
        RemoteAxis::Undone { name, behind } => {
            println!(
                "{}",
                crate::render::paint_warn(
                    &format!(
                        "{name} still holds {behind} commit(s) you undid, so nothing was taken in"
                    ),
                    colored
                )
            );
            said = true;
        }
        RemoteAxis::Yours { name, behind, .. } => {
            println!(
                "{name} still holds {behind} commit(s) this branch rewrote; the log accounts for every one, so they are stale copies of your own"
            );
            said = true;
        }
        RemoteAxis::Ran { name, outcome } => match outcome {
            RestackOutcome::NothingToRestack { .. } => {}
            RestackOutcome::Restacked(r) if r.fast_forward => {
                println!(
                    "{}",
                    crate::render::paint_ok(
                        &format!("fast-forwarded to {name} ({} commit(s))", r.behind),
                        colored
                    )
                );
                said = true;
            }
            RestackOutcome::Restacked(r) => {
                println!("took in {} commit(s) from {name}", r.behind);
                println!("replayed {} of yours on top", r.replayed);
                said = true;
            }
            RestackOutcome::Held(h) => {
                println!("{}", crate::render::held_block(h, colored));
                said = true;
            }
        },
    }

    match &report.base {
        BaseAxis::NoBase => {}
        BaseAxis::Skipped => {
            println!(
                "{}",
                crate::render::paint_dim(
                    "the base was left alone: the first axis that conflicts stops the run",
                    colored
                )
            );
            said = true;
        }
        BaseAxis::Ran { name, outcome } => match outcome {
            RestackOutcome::NothingToRestack { .. } => {}
            RestackOutcome::Restacked(r) if r.fast_forward => {
                println!(
                    "{}",
                    crate::render::paint_ok(
                        &format!("fast-forwarded to {name} — nothing to replay"),
                        colored
                    )
                );
                said = true;
            }
            RestackOutcome::Restacked(r) => {
                println!("{name} moved ahead by {} commit(s)", r.behind);
                println!("replayed {} commit(s) onto {name}", r.replayed);
                said = true;
            }
            RestackOutcome::Held(h) => {
                println!("{}", crate::render::held_block(h, colored));
                said = true;
            }
        },
    }

    // The landed reports, from either axis, gathered once over both.
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

    for r in &reports {
        if let Some(line) = crate::render::dropped_line(&r.dropped, None, colored) {
            println!("{line}");
            said = true;
        }
    }
    let files: usize = reports.iter().map(|r| r.files).sum();
    if files > 0 {
        let still_open = reports.last().is_some_and(|r| r.still_open);
        if still_open {
            println!("updated the working tree ({files} file(s)); your change is still open");
        } else {
            println!("updated the working tree ({files} file(s))");
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

    if !reports.is_empty() {
        println!("{}", crate::render::paint_dim("undo: ff undo", colored));
        said = true;
    }
    if !said {
        println!("{}", crate::render::paint_dim("nothing to sync", colored));
    }

    if blocked {
        crate::exit::held();
    }
    Ok(())
}
