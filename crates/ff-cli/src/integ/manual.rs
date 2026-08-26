//! `ff trigger` — the manual snapshot, and the cheapest command there is.
//!
//! Every ff command captures first, and then goes and does something. This
//! one captures and stops, which makes it the fastest way to force a
//! snapshot and the natural thing to type before something risky. `-m`
//! labels it, so a hand-taken snapshot can say why it was taken.
//!
//! It is a source like any other, so it enters the shared pipeline with a
//! synthesized event rather than through a second capture path of its own.
//! That means it rides the same throttled ambient lanes; both are
//! stamp-gated, and cost a file read on the invocations where they do not
//! fire.

use ff_core::{Error, Result};

use super::runtime;
use super::{AgentEvent, EventKind, Label};
use crate::ctx::Ctx;

pub const SOURCE: &str = "manual";

/// A hand-typed label is a subject, not a commit message: the same 64
/// characters a tool command gets.
const MAX_LABEL: usize = 64;

pub fn run(ctx: &Ctx, message: Option<String>) -> Result<()> {
    let cwd = std::env::current_dir().map_err(Error::repo)?;
    let label = message
        .map(|text| crate::provenance::truncate(text.trim(), MAX_LABEL))
        .unwrap_or_default();
    let event = AgentEvent {
        // The manual snapshot is taken *before* whatever is about to
        // happen, which is what BeforeTool means; there is no other event
        // it could honestly claim to be.
        kind: EventKind::BeforeTool,
        session: String::new(),
        cwd,
        label: Label::Text(label),
    };
    let landed = runtime::pipeline(ctx, SOURCE, &event, None)?;
    report(ctx, &landed)
}

fn report(ctx: &Ctx, landed: &runtime::Landed) -> Result<()> {
    use ff_core::CaptureOutcome;

    for warning in warnings(&landed.outcome) {
        eprintln!("ff: {warning}");
    }

    if ctx.json {
        let (op, files) = match &landed.outcome {
            CaptureOutcome::Created {
                id, changed_files, ..
            } => (Some(id.to_string()), Some(*changed_files)),
            CaptureOutcome::NoOp { tip, .. } => (tip.as_ref().map(ToString::to_string), None),
            CaptureOutcome::Contended => (None, None),
        };
        return crate::machine::emit(
            ctx.command,
            &serde_json::json!({
                "source": SOURCE,
                "captured": matches!(landed.outcome, CaptureOutcome::Created { .. }),
                "op": op,
                "files": files,
            }),
        );
    }

    let colored = crate::pager::color_enabled();
    match &landed.outcome {
        CaptureOutcome::Created {
            id, changed_files, ..
        } => {
            // The id column's convention everywhere: eight letters, with
            // the shortest prefix `ff op` resolves unambiguously in bold.
            // Prefix lengths are keyed on the raw hex, which is what the
            // id index holds.
            let hex = id.hex();
            let display = id.short(8);
            let unique =
                crate::cmd::evolog::displayed_prefix_lens(&landed.repo, std::slice::from_ref(&hex))
                    .unwrap_or_default()
                    .get(&hex)
                    .copied()
                    .unwrap_or(1);
            let files = if *changed_files == 1 {
                "1 file".to_string()
            } else {
                format!("{changed_files} files")
            };
            println!(
                "{} · {}",
                crate::render::styled_id(&display, unique, 0, colored),
                crate::render::paint_dim(&files, colored)
            );
        }
        // Not news, and not a failure: the tree at this instant is already
        // on the log, which is the state a snapshot exists to reach.
        CaptureOutcome::NoOp { .. } => println!(
            "{}",
            crate::render::paint_dim("already snapshotted — the tree has not moved", colored)
        ),
        // Somebody else is capturing this very tree right now, and their
        // operation is the one that lands.
        CaptureOutcome::Contended => println!(
            "{}",
            crate::render::paint_dim("another capture is in flight — it holds this tree", colored)
        ),
    }
    Ok(())
}

fn warnings(outcome: &ff_core::CaptureOutcome) -> &[String] {
    match outcome {
        ff_core::CaptureOutcome::Created { warnings, .. }
        | ff_core::CaptureOutcome::NoOp { warnings, .. } => warnings,
        ff_core::CaptureOutcome::Contended => &[],
    }
}
