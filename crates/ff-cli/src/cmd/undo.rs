//! `ff undo` and `ff redo` — step the whole repository back and forward
//! along the log.
//!
//! Lean by decree: no confirmation prompt, because a step is itself one step
//! away in either direction. Both take no argument and repeat; naming one
//! operation instead of a run is `ff op restore`, which shares this
//! reporting because it shares the mechanism.

use ff_core::{Result, RewindOptions};

use crate::ctx::Ctx;

pub fn run(ctx: &Ctx) -> Result<()> {
    step(ctx, &ff_core::Landing::OneRun, "undo")
}

pub fn redo(ctx: &Ctx) -> Result<()> {
    step(ctx, &ff_core::Landing::Forward, "redo")
}

fn step(ctx: &Ctx, landing: &ff_core::Landing, name: &'static str) -> Result<()> {
    let repo = ff_core::discover(".")?;
    crate::render::init_palette(&repo);
    let (report, verb_ctx) = ff_core::rewind(
        &repo,
        landing,
        &RewindOptions {
            // `--force` lives on `ff op restore`, where naming an operation is
            // already a deliberate act. Bare undo has nothing to force past:
            // it steps one run, and a run whose state was trimmed is a run
            // undo should decline rather than half-apply.
            force: false,
            now: None,
            argv: std::env::args().collect(),
        },
        &crate::provenance::pre_ff(ctx),
    )?;

    crate::render::reconcile_notice(&verb_ctx.reconcile);
    report_move(ctx, &report, name)
}

/// The one rendering all three moves share. They differ in how the landing
/// was chosen and in nothing that happens afterwards, so a second renderer
/// would be a second place for them to drift apart.
pub fn report_move(ctx: &Ctx, report: &ff_core::RewindReport, name: &'static str) -> Result<()> {
    if ctx.json {
        let payload = serde_json::json!({
            "move": report,
            "back": if report.forward { "ff undo" } else { "ff redo" },
        });
        let envelope: &'static str = match name {
            "undo" => "undo",
            "redo" => "redo",
            _ => "op restore",
        };
        crate::machine::emit(envelope, &payload)?;
        return Ok(());
    }

    let colored = crate::pager::color_enabled();

    let verb = if report.forward { "redid" } else { "undid" };
    let label = if report.stepped_kind.as_deref() == Some("foreign") {
        " (a change made outside fufu)"
    } else {
        ""
    };
    match &report.stepped_summary {
        Some(what) => println!("{verb}{label}: {what}"),
        None => println!("{verb}{label}: nothing was in the way"),
    }
    // Twelve letters: the length the ambiguity refusal lists candidates at,
    // and so the length a reader can safely copy. The whole id is on the
    // machine surface.
    let landed: String = report.landed.chars().take(12).collect();
    println!(
        "  now at {} ({})",
        crate::render::paint_id(&landed, colored),
        report.landed_summary
    );
    // What a run collapsed, say: a keystroke that moved forty operations must
    // not have to be inferred. Captures and verb operations are counted apart
    // because "and two other things" is a lie when the two changed no ref.
    if report.collapsed > 1 {
        println!("  one run of {} captures", report.collapsed);
    }
    if report.stepped_ops > 1 {
        println!("  {} operations stepped over", report.stepped_ops);
    }
    for t in &report.refs {
        let what = match (&t.old, &t.new) {
            (_, Some(new)) => {
                let sha = ff_core::sha::short(new.as_str());
                format!("→ {}", crate::render::paint_sha(sha, colored))
            }
            (Some(_), None) => "deleted".to_string(),
            (None, None) => continue,
        };
        println!("  {} {what}", t.name);
    }
    if let Some(head) = &report.head_moved {
        let display = head.strip_prefix("ref:").unwrap_or(head);
        let is_sha = display.chars().all(|c| c.is_ascii_hexdigit());
        let painted = if is_sha {
            crate::render::paint_sha(display, colored)
        } else {
            display.to_string()
        };
        println!("  HEAD → {painted}");
    }
    if !report.files.is_empty() {
        println!("  {} worktree file(s) restored", report.files.len());
    }
    for warning in &report.warnings {
        println!(
            "{}",
            crate::render::paint_warn(&format!("  warning: {warning}"), colored)
        );
    }
    let back = if report.forward { "ff undo" } else { "ff redo" };
    println!(
        "{}",
        crate::render::paint_dim(&format!("back: {back}"), colored)
    );
    Ok(())
}
