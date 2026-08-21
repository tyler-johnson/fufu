//! `ff switch` — branches without ceremony. Parking and arrival reports are
//! part of the verb's voice: the user should always know where their work
//! went and where it came back from.

use ff_core::{ArrivalReport, Result, SwitchOptions, SwitchReport};

use crate::ctx::Ctx;

pub fn run(ctx: &Ctx, target: String) -> Result<()> {
    let repo = ff_core::discover(".")?;
    // A verb means a *kind*, and a kind mismatch redirects rather than
    // refuses. `ff switch <sha>` has exactly one sensible reading — you want
    // to be working there — so fufu takes it and says so. Acting is not
    // guessing: one available reading is taken and announced, and more than
    // one would be an error naming the candidates (which is what an ambiguous
    // branch prefix still gets, below).
    match ff_core::resolve_branch(&repo, &target) {
        Ok(_) => {}
        Err(err) if err.id() == "branch/not-found" && names_a_revision(&repo, &target) => {
            return switch_to_a_revision(ctx, &repo, target);
        }
        Err(err) => return Err(err),
    }
    let (report, verb_ctx) = ff_core::switch(
        &repo,
        &SwitchOptions {
            target,
            now: None,
            argv: std::env::args().collect(),
        },
        &crate::provenance::pre_ff(ctx),
    )?;

    crate::render::init_palette(&repo);
    crate::render::reconcile_notice(&verb_ctx.reconcile);

    if ctx.json {
        let payload = serde_json::json!({
            "switch": report,
            "reconcile": verb_ctx.reconcile,
            "undo": "ff undo",
        });
        crate::machine::emit("switch", &payload)?;
        return Ok(());
    }

    let colored = crate::pager::color_enabled();
    render_switch(&report, colored);
    Ok(())
}

/// The switch's own rendering, shared with `ff edit`'s branch redirect so a
/// switch reads the same wherever it happens.
pub(crate) fn render_switch(report: &SwitchReport, colored: bool) {
    if report.from == report.to {
        println!("already on {}", report.to);
        return;
    }
    if let Some(stash) = &report.parked {
        println!(
            "parked the open change on {} ({})",
            report.from,
            crate::render::paint_sha(ff_core::sha::short(stash.as_str()), colored)
        );
    }
    println!("switched to {}", report.to);
    render_arrival(&report.arrival, colored);
    println!("{}", crate::render::paint_dim("undo: ff undo", colored));
}

/// The arrival block: what became of the change parked on the target. One
/// place for the four arms, so a switch and a session landing never drift
/// apart.
pub(crate) fn render_arrival(arrival: &ArrivalReport, colored: bool) {
    match arrival {
        ArrivalReport::None => {}
        ArrivalReport::Restored { files, .. } => {
            println!("resumed the parked change ({} file(s))", files.len());
        }
        ArrivalReport::StillParked { paths, .. } => {
            println!("a parked change is waiting here but no longer applies cleanly:");
            for path in paths {
                println!(
                    "{}",
                    crate::render::paint_warn(&format!("  conflicts: {path}"), colored)
                );
            }
            println!("it stays parked; resolve by hand with git stash, or continue working");
        }
        ArrivalReport::Invalidated { stash } => {
            println!(
                "note: the parked change ({}) was dropped outside fufu; its entry was cleared",
                crate::render::paint_sha(ff_core::sha::short(stash.as_str()), colored)
            );
        }
    }
}

/// Whether the target denotes a revision. Deliberately quiet about *why* it
/// does not: the caller already holds the branch error, and that is the one
/// worth reporting when neither reading works — somebody typing a branch name
/// with a typo in it is not asking about revisions.
fn names_a_revision(repo: &ff_core::gix::Repository, target: &str) -> bool {
    use ff_core::revset::{Rev, Revset};
    matches!(
        Revset::parse(target).and_then(|set| set.point(repo)),
        Ok(point) if matches!(point.rev, Rev::Commit(_))
    )
}

/// The redirect: mint an anonymous branch at the revision and land on it,
/// which is precisely `ff start <rev>` — so it *is* `ff start <rev>`, rather
/// than a second implementation of minting that could drift from the first.
fn switch_to_a_revision(ctx: &Ctx, repo: &ff_core::gix::Repository, target: String) -> Result<()> {
    let (report, verb_ctx) = ff_core::start(
        repo,
        &ff_core::StartOptions {
            target: Some(target.clone()),
            message: None,
            branch: None,
            now: None,
            argv: std::env::args().collect(),
        },
        &crate::provenance::pre_ff(ctx),
    )?;

    crate::render::init_palette(repo);
    crate::render::reconcile_notice(&verb_ctx.reconcile);

    if ctx.json {
        let payload = serde_json::json!({
            "start": report,
            "redirected_from": target,
            "reconcile": verb_ctx.reconcile,
            "undo": "ff undo",
        });
        crate::machine::emit("switch", &payload)?;
        return Ok(());
    }

    let colored = crate::pager::color_enabled();
    // The same pairing `ff start` prints: what parked was the change open on
    // the branch underfoot, never the revision this forked at.
    if let (Some(stash), Some(from)) = (&report.parked, &report.parked_from) {
        println!(
            "parked the open change on {from} ({})",
            crate::render::paint_sha(ff_core::sha::short(stash.as_str()), colored)
        );
    }
    println!(
        "{target} is a revision, not a branch — minted {} there and switched to it",
        report.minted
    );
    println!(
        "{}",
        crate::render::paint_dim(
            &format!(
                "  ff describe -b <name>  name it   ·   ff start {target}  the verb that meant it"
            ),
            colored
        )
    );
    println!("{}", crate::render::paint_dim("undo: ff undo", colored));
    Ok(())
}
