//! `ff fold` — land the branch you are standing on into another one and
//! take the branch away. The positional names the target, trunk without it;
//! `--stay` advances a target another worktree holds, there, and keeps this
//! branch sitting on the result.

use ff_core::Result;

use crate::ctx::Ctx;

pub fn run(ctx: &Ctx, target: Option<String>, stay: bool) -> Result<()> {
    let repo = ff_core::discover(".")?;

    let (report, verb_ctx, other_reconcile) = ff_core::fold::fold(
        &repo,
        target,
        stay,
        &crate::provenance::pre_ff(ctx),
        None,
        std::env::args().collect(),
    )?;
    crate::render::init_palette(&repo);
    crate::render::reconcile_notice(&verb_ctx.reconcile);
    if let Some(other) = &other_reconcile {
        crate::render::reconcile_notice(other);
    }

    // A branch above that held needs a person before anything more moves
    // there: the same 3 a restack owes the shell, with the landed report
    // still on stdout.
    let held_above = !report.cascade.held.is_empty();
    if ctx.json {
        let payload = serde_json::json!({
            "fold": report,
            "undo": "ff undo",
        });
        crate::machine::emit("fold", &payload)?;
        if held_above {
            crate::exit::held();
        }
        return Ok(());
    }

    let colored = crate::pager::color_enabled();
    let short = |sha: &str| crate::render::paint_sha(ff_core::sha::short(sha), colored);

    if report.replayed > 0 {
        println!(
            "replayed {} commit(s) onto {}",
            report.replayed, report.target
        );
    }
    for line in crate::render::dropped_lines(&report.dropped, None, colored) {
        println!("{line}");
    }
    if !report.diverged.is_empty() {
        let sits = if report.diverged.len() == 1 {
            "sits"
        } else {
            "sit"
        };
        println!(
            "{}",
            crate::render::paint_warn(
                &format!(
                    "{} now {sits} on commits this fold replaced",
                    report.diverged.join(", ")
                ),
                colored
            )
        );
    }
    if report.advanced > 0 {
        println!(
            "{} moved ahead by {} commit(s)",
            report.target, report.advanced
        );
    } else {
        println!(
            "{} did not move: {} had nothing of its own",
            report.target, report.source
        );
    }

    if report.stay {
        if let Some(tree) = &report.moved_tree {
            let open = if tree.still_open {
                "; its change is still open"
            } else {
                ""
            };
            println!(
                "advanced {} in {} ({} file(s)){open}",
                report.target, tree.path, tree.files
            );
        }
        let open = if report.still_open {
            "; your change is still open"
        } else {
            ""
        };
        println!(
            "{} now sits on {} with nothing of its own{open}",
            report.source, report.target
        );
    } else if report.anonymous {
        println!(
            "dropped {} — an anonymous branch, nothing to lose",
            report.source
        );
    } else {
        println!("deleted {} (was {})", report.source, short(&report.old_tip));
        if let Some(trash) = &report.trash_ref {
            println!("  its timeline moved to {trash}");
        }
        if let Some(parked) = &report.parked_demoted {
            println!(
                "  its parked change ({}) is off the branch; it stays on git's stash list",
                short(parked)
            );
        }
    }

    if !report.reaimed.is_empty() {
        println!(
            "re-aimed {} at {}",
            report.reaimed.join(", "),
            report.target
        );
    }
    for line in crate::render::cascade_lines(&report.cascade, colored) {
        println!("{line}");
    }

    if !report.stay {
        let mut line = format!("now on {}", report.target);
        if report.still_open {
            line.push_str("; your change is still open");
        }
        if report.files > 0 {
            line.push_str(&format!(
                " — updated the working copy ({} file(s))",
                report.files
            ));
        }
        println!("{line}");
    }
    if let Some(parked) = &report.parked {
        if parked.applies {
            println!(
                "{} has a parked change — it still applies cleanly",
                report.target
            );
        } else {
            println!(
                "{}",
                crate::render::paint_warn(
                    &format!(
                        "{} has a parked change — it would conflict on arrival",
                        report.target
                    ),
                    colored
                )
            );
        }
    }
    if report.published > 0 {
        let upstream_name = report.published_on.as_deref().unwrap_or("the remote");
        println!(
            "{} of the rewritten commits are already on {}",
            report.published, upstream_name
        );
    }
    println!("{}", crate::render::paint_dim("undo: ff undo", colored));
    if let Some(tree) = &report.moved_tree {
        println!(
            "{}",
            crate::render::paint_dim(
                &format!("the other half: ff undo in {}", tree.path),
                colored
            )
        );
    }
    if held_above {
        crate::exit::held();
    }
    Ok(())
}
