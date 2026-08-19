//! `ff sync` — line this branch up with the base beneath it and the remote
//! copy of itself, and publish. `ff restack` is one of its two axes; the
//! other is the network, which this command owns and hands to the core as a
//! number.
//!
//! The three steps run in order: read the tracking ref as it stands, fetch,
//! read it again. The reason for reading it twice is the divergence rule —
//! divergence this run's fetch created is somebody else's and your commits
//! replay on top of theirs; divergence that was already there is a rewrite
//! of your own and it is published under a lease rather than forced.

use ff_core::{BaseAxis, Publish, RemoteAxis, RestackOutcome, RestackReport, Result};

use crate::ctx::Ctx;

pub fn run(ctx: &Ctx, push: bool, no_push: bool, no_fetch: bool) -> Result<()> {
    let repo = ff_core::discover(".")?;
    crate::render::init_palette(&repo);
    let colored = crate::pager::color_enabled();
    let push = if no_push {
        Some(false)
    } else if push {
        Some(true)
    } else {
        None
    };

    // The tracking ref as it stands before anything reaches the network.
    let before = ff_core::sync::preflight(&repo)?;

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
    let after = ff_core::sync::preflight(&repo)?;
    let tracking_after = after.tracking.as_ref().and_then(|t| t.tip);

    let (report, verb_ctx) = ff_core::sync::sync(
        &repo,
        &before,
        ff_core::sync::SyncOptions {
            push,
            fetched,
            tracking_after,
            now: None,
            argv: std::env::args().collect(),
        },
        &crate::provenance::pre_ff(ctx),
    )?;
    crate::render::reconcile_notice(&verb_ctx.reconcile);

    // The push, before any report line about it: the report says what
    // happened and not what was planned.
    let pushed = match &report.publish {
        ff_core::Publish::Create { .. } | ff_core::Publish::Push { .. } => {
            crate::net::push(&cwd, &report.branch, &report.publish)?;
            true
        }
        _ => false,
    };

    // A hold on either axis, or a blocked publish, means a human decision is
    // required before anything more moves — which is exactly what exit 3 says.
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
    ) || matches!(report.publish, Publish::Blocked);

    if ctx.json {
        let payload = serde_json::json!({
            "sync": report,
            "pushed": pushed,
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
        RemoteAxis::Yours { name, behind, .. } => {
            println!(
                "{name} still holds {behind} commit(s) this branch rewrote; nothing arrived, so they are stale copies of your own"
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

    match &report.publish {
        // A declined push is disclosed exactly when a push would have sent
        // something. Unpushed commits are precisely what sync exists to
        // send, so keeping them deliberately is news; a run with nothing to
        // send says "nothing to sync" instead — the same single clean phrase
        // ff status uses for that state — rather than pairing it with an
        // aside about a publish that had nothing to do anyway.
        Publish::Off { pending: true } => {
            println!("{}", crate::render::paint_dim("not pushed", colored));
            said = true;
        }
        Publish::Off { pending: false } => {}
        Publish::Blocked => {
            println!(
                "{}",
                crate::render::paint_warn(
                    &format!(
                        "not pushed: a rewrite is held on {} — the exit stays blocked until it lands",
                        report.branch
                    ),
                    colored
                )
            );
            said = true;
        }
        Publish::Gone => {
            println!(
                "{}",
                crate::render::paint_warn(
                    "not pushed: the remote copy is gone — ff sync --push re-creates it",
                    colored
                )
            );
            said = true;
        }
        Publish::UpToDate => {}
        Publish::Create {
            remote,
            remote_branch,
            ..
        } => {
            println!(
                "{}",
                crate::render::paint_ok(
                    &format!(
                        "created {remote}/{remote_branch} and set {} to track it",
                        report.branch
                    ),
                    colored
                )
            );
            println!(
                "{}",
                crate::render::paint_dim(
                    "the push left the machine — ff undo cannot reach it",
                    colored
                )
            );
            said = true;
        }
        Publish::Push {
            remote,
            remote_branch,
            ..
        } => {
            println!(
                "{}",
                crate::render::paint_ok(
                    &format!("pushed {} to {remote}/{remote_branch}", report.branch),
                    colored
                )
            );
            println!(
                "{}",
                crate::render::paint_dim(
                    "the push left the machine — ff undo cannot reach it",
                    colored
                )
            );
            said = true;
        }
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
