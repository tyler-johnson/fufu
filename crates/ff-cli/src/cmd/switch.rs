//! `ff switch` — branches without ceremony, under three spellings: `switch`,
//! `start`, and `new` are one verb, and the core decides whether the target
//! is a branch to continue or one to mint. Parking, minting, and arrival
//! reports are part of the verb's voice: the user should always know where
//! their work went and where it came back from. The park is the open
//! commit, so the sha the park line names is the `@` row's.

use ff_core::{ArrivalReport, Result, SwitchOptions, SwitchReport};

use crate::ctx::Ctx;

pub fn run(
    ctx: &Ctx,
    target: Option<String>,
    message: Option<String>,
    branch: Option<Option<String>>,
) -> Result<()> {
    let repo = ff_core::discover(".")?;
    let (report, verb_ctx) = ff_core::switch(
        &repo,
        &SwitchOptions {
            target,
            message,
            branch,
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
        exit_if_held(&report.arrival);
        return Ok(());
    }

    let colored = crate::pager::color_enabled();
    render_switch(&report, colored);
    Ok(())
}

/// Whether the switch's JSON should exit 3: the switch happened, and the
/// arrival held.
fn exit_if_held(arrival: &ArrivalReport) {
    if matches!(arrival, ArrivalReport::Held { .. }) {
        crate::exit::held();
    }
}

/// The switch's own rendering, shared with `ff edit`'s branch redirect so a
/// switch reads the same wherever it happens.
pub(crate) fn render_switch(report: &SwitchReport, colored: bool) {
    if report.from == report.to {
        println!("already on {}", report.to);
        return;
    }
    // `from`, never the fork source: what parked was the change open on the
    // branch underfoot, which is not the branch a mint forked from.
    if let Some(stash) = &report.parked {
        println!(
            "parked the open change on {} ({})",
            report.from,
            crate::render::paint_sha(ff_core::sha::short(stash.as_str()), colored)
        );
    }
    if let Some(minted) = &report.minted {
        match (&minted.forked_from, &minted.tracking) {
            (Some(from), _) => println!("minted {} (forked from {from})", report.to),
            (None, Some(tracking)) => println!("minted {} tracking {tracking}", report.to),
            (None, None) => println!("minted {}", report.to),
        }
    }
    println!("switched to {}", report.to);
    if let Some(sha) = report.minted.as_ref().and_then(|m| m.carried.as_deref()) {
        println!(
            "carried the open change onto {} ({})",
            report.to,
            crate::render::paint_sha(ff_core::sha::short(sha), colored)
        );
    }
    render_arrival(&report.arrival, &report.to, colored);
    println!("{}", crate::render::paint_dim("undo: ff undo", colored));
}

/// The arrival block: what became of the change parked on the target. One
/// place for the five arms, so a switch and a session landing never drift
/// apart. A held arrival sets the exit code: the switch happened, and a
/// human decision is required before the change moves.
pub(crate) fn render_arrival(arrival: &ArrivalReport, to: &str, colored: bool) {
    let folded = match arrival {
        ArrivalReport::Restored { folded, .. }
        | ArrivalReport::Held { folded, .. }
        | ArrivalReport::Landed { folded, .. } => folded.as_deref(),
        ArrivalReport::None | ArrivalReport::Invalidated { .. } => None,
    };
    match arrival {
        ArrivalReport::None => {}
        ArrivalReport::Restored { files, .. } => {
            println!("resumed the parked change ({} file(s))", files.len());
        }
        ArrivalReport::Held { paths, .. } => {
            println!("the parked change does not apply on {to}'s new tip:");
            for path in paths {
                println!(
                    "{}",
                    crate::render::paint_warn(&format!("  conflicts: {path}"), colored)
                );
            }
            println!(
                "{}",
                crate::render::paint_dim(
                    "held: ff resolve lays it into the open change with markers · ff resolve \
                     --abandon drops it",
                    colored
                )
            );
            crate::exit::held();
        }
        ArrivalReport::Landed { .. } => {
            println!("the parked change is already in {to}; nothing to resume");
        }
        ArrivalReport::Invalidated { stash } => {
            println!(
                "note: the parked change ({}) was dropped outside fufu; its entry was cleared",
                crate::render::paint_sha(ff_core::sha::short(stash.as_str()), colored)
            );
        }
    }
    if let Some(stash) = folded {
        println!(
            "{}",
            crate::render::paint_dim(
                &format!(
                    "folded its stash entry ({}) into the open commit",
                    ff_core::sha::short(stash)
                ),
                colored
            )
        );
    }
}
