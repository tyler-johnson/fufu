//! `ff publish` — send this branch to its remote, under a lease.
//!
//! The whole of the outgoing half. It does not fetch, does not replay, and
//! does not touch a ref that is not the shared copy of the branch you are
//! standing on: `ff sync` is what takes anything in. What this command owns
//! is the network call the core deliberately will not make, and the one
//! honest line afterwards — the push left the machine, and `ff undo` cannot
//! reach across the wire to take it back.
//!
//! `--dry-run` is the answer to that line: the only way to see which push
//! this would be *before* it becomes unrecallable. It writes nothing and
//! sends nothing, so every sentence below switches to the conditional.
//!
//! What `ff undo` cannot reach, `ff publish` can: undo the commit and
//! publish again, and the lease rolls the shared copy back to where the
//! branch now stands. That is not erasure — other clones may hold the
//! commits, CI ran, a webhook fired — which is why the tail line still says
//! the push left the machine. But it is a way back, and the log recording
//! the push is what lets fufu stop pointing the other way.

use ff_core::{Publish, PushShape, Result};

use crate::ctx::Ctx;

pub fn run(ctx: &Ctx, dry_run: bool, to: Option<&str>) -> Result<()> {
    let repo = ff_core::discover(".")?;
    crate::render::init_palette(&repo);
    let colored = crate::pager::color_enabled();

    let pre = ff_core::preflight::preflight_to(&repo, ff_core::preflight::Verb::Publish, to)?;
    let cwd = repo
        .workdir()
        // Uncoded on purpose: preflight already refused a bare repository, so
        // nobody can reach this and there is nothing to tell them.
        .ok_or_else(|| ff_core::Error::msg("no working directory: internal inconsistency"))?
        .to_path_buf();

    let prov = crate::provenance::pre_ff(ctx);
    let (report, verb_ctx) = ff_core::publish::publish(
        &repo,
        &pre,
        ff_core::publish::PublishOptions {
            dry_run,
            now: None,
            argv: std::env::args().collect(),
        },
        &prov,
    )?;
    if let Some(verb_ctx) = &verb_ctx {
        crate::render::reconcile_notice(&verb_ctx.reconcile);
    }

    // The push, before any report line about it: the report says what
    // happened and not what was planned. A dry run is the one case where
    // those differ, and it says "would" throughout rather than pretending.
    let pushed = match &report.publish {
        Publish::Create { .. } | Publish::Push { .. } if !dry_run => {
            // Named before the wire agrees: a push that fails to an
            // unreachable URL still leaves `ff sync` working. Only when there
            // was no upstream at all — a branch that already tracks this
            // remote is set correctly, and rewriting it could clobber a
            // legitimately multi-valued `merge`.
            if let Some(remote) = to.filter(|_| pre.tracking.is_none()) {
                ff_core::snapshot::config::set_branch_upstream(&repo, &report.branch, remote)?;
            }
            crate::net::push(&cwd, &report.branch, &report.publish)?;
            true
        }
        _ => false,
    };
    // Recorded after the wire agreed, not before: publish moves no local ref,
    // so there is nothing a write-ahead claim could be diffed against and an
    // append-before would be a claim nothing could falsify.
    if pushed && let Some(verb_ctx) = &verb_ctx {
        ff_core::publish::record(&repo, &pre, &report, verb_ctx, &prov).map_err(|err| {
            ff_core::Error::coded(
                "publish/unrecorded",
                format!("the push landed and the operation log could not record it: {err}"),
                vec!["ff op log".into(), "ff status".into()],
            )
        })?;
    }
    let would = if dry_run { "would " } else { "" };

    if ctx.json {
        let payload = serde_json::json!({
            "publish": report,
            "pushed": pushed,
        });
        crate::machine::emit("publish", &payload)?;
        if matches!(report.publish, Publish::Blocked) {
            crate::exit::held();
        }
        return Ok(());
    }

    match &report.publish {
        Publish::NoRemote => {
            println!(
                "{}",
                crate::render::paint_dim(
                    "nowhere to publish: this repository has no remote",
                    colored
                )
            );
        }
        Publish::Blocked => {
            println!(
                "{}",
                crate::render::paint_warn(
                    &format!(
                        "not published: a rewrite is held on {} — the exit stays blocked until it lands",
                        report.branch
                    ),
                    colored
                )
            );
        }
        Publish::UpToDate => {
            println!(
                "{}",
                crate::render::paint_dim("nothing to publish", colored)
            );
        }
        Publish::Create {
            remote,
            remote_branch,
            ..
        } => {
            println!(
                "{}",
                crate::render::paint_ok(
                    &format!(
                        "{would}create{} {remote}/{remote_branch} and set {} to track it",
                        if dry_run { "" } else { "d" },
                        report.branch
                    ),
                    colored
                )
            );
            tail(dry_run, colored);
        }
        Publish::Push {
            remote,
            remote_branch,
            shape,
            ..
        } => {
            // The lease is empty for two of these and only the sentence
            // differs. It used to be the whole test, which is why a clone of
            // an empty remote was told its shared copy was gone.
            let line = match shape {
                PushShape::Recreate => format!(
                    "{would}re-create{} {remote}/{remote_branch}, which is gone",
                    if dry_run { "" } else { "d" }
                ),
                PushShape::First => format!(
                    "{would}create{} {remote}/{remote_branch}",
                    if dry_run { "" } else { "d" }
                ),
                // Not a send at all: the tip is an ancestor of what the
                // remote holds, so this takes commits off the shared copy.
                PushShape::Retract => format!(
                    "{would}roll{} {remote}/{remote_branch} back to {}",
                    if dry_run { "" } else { "ed" },
                    report.branch
                ),
                PushShape::Replace => format!(
                    "{would}publish{} {} to {remote}/{remote_branch}",
                    if dry_run { "" } else { "ed" },
                    report.branch
                ),
            };
            println!("{}", crate::render::paint_ok(&line, colored));
            tail(dry_run, colored);
        }
    }

    if matches!(report.publish, Publish::Blocked) {
        crate::exit::held();
    }
    Ok(())
}

/// The tail under a push. A dry run has not spent the irreversible act yet,
/// and saying it did would be the one lie this verb cannot afford — so it
/// gets the conditional line and nothing else. A real push gets both: what
/// left the machine, and the way back that is not a way to erase it.
fn tail(dry_run: bool, colored: bool) {
    if dry_run {
        println!(
            "{}",
            crate::render::paint_dim("nothing was sent — drop --dry-run to send it", colored)
        );
        return;
    }
    println!(
        "{}",
        crate::render::paint_dim(
            "the push left the machine — ff undo cannot reach it",
            colored
        )
    );
    println!(
        "{}",
        crate::render::paint_dim(
            "ff undo then ff publish rolls the shared copy back, under a lease",
            colored
        )
    );
}
