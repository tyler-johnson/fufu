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

use ff_core::{Publish, Result};

use crate::ctx::Ctx;

pub fn run(ctx: &Ctx, dry_run: bool) -> Result<()> {
    let repo = ff_core::discover(".")?;
    crate::render::init_palette(&repo);
    let colored = crate::pager::color_enabled();

    let pre = ff_core::preflight::preflight(&repo, ff_core::preflight::Verb::Publish)?;
    let cwd = repo
        .workdir()
        // Uncoded on purpose: preflight already refused a bare repository, so
        // nobody can reach this and there is nothing to tell them.
        .ok_or_else(|| ff_core::Error::msg("no working directory: internal inconsistency"))?
        .to_path_buf();

    let (report, verb_ctx) = ff_core::publish::publish(
        &repo,
        &pre,
        ff_core::publish::PublishOptions {
            dry_run,
            now: None,
            argv: std::env::args().collect(),
        },
        &crate::provenance::pre_ff(ctx),
    )?;
    if let Some(verb_ctx) = &verb_ctx {
        crate::render::reconcile_notice(&verb_ctx.reconcile);
    }

    // The push, before any report line about it: the report says what
    // happened and not what was planned. A dry run is the one case where
    // those differ, and it says "would" throughout rather than pretending.
    let pushed = match &report.publish {
        Publish::Create { .. } | Publish::Push { .. } if !dry_run => {
            crate::net::push(&cwd, &report.branch, &report.publish)?;
            true
        }
        _ => false,
    };
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
            println!(
                "{}",
                crate::render::paint_dim(left_the_machine(dry_run), colored)
            );
        }
        Publish::Push {
            remote,
            remote_branch,
            lease,
            ..
        } => {
            // An empty lease is the re-creation case: the shared copy was
            // deleted and this put it back. Saying so is the difference
            // between "sent" and "somebody deleted this and you undid that".
            if lease.is_empty() {
                println!(
                    "{}",
                    crate::render::paint_ok(
                        &format!(
                            "{would}re-create{} {remote}/{remote_branch}, which is gone",
                            if dry_run { "" } else { "d" }
                        ),
                        colored
                    )
                );
            } else {
                println!(
                    "{}",
                    crate::render::paint_ok(
                        &format!(
                            "{would}publish{} {} to {remote}/{remote_branch}",
                            if dry_run { "" } else { "ed" },
                            report.branch
                        ),
                        colored
                    )
                );
            }
            println!(
                "{}",
                crate::render::paint_dim(left_the_machine(dry_run), colored)
            );
        }
    }

    if matches!(report.publish, Publish::Blocked) {
        crate::exit::held();
    }
    Ok(())
}

/// The tail line under a push. A dry run has not spent the irreversible act
/// yet, and saying it did would be the one lie this verb cannot afford.
fn left_the_machine(dry_run: bool) -> &'static str {
    if dry_run {
        "nothing was sent — drop --dry-run to send it"
    } else {
        "the push left the machine — ff undo cannot reach it"
    }
}
