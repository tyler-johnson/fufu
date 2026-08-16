//! Bare `ff` — the capture verb. `ff [-m <msg>]` records the working tree as
//! one operation.
//!
//! The outcome is mapped to text here rather than in core: `CaptureOutcome`
//! names an operation and a branch, and "snapshot" is this surface's word
//! for what one carries — a capture is the operation, the snapshot is its
//! tree.

use ff_core::{CaptureOutcome, EvologOptions, Provenance, Result};

use crate::ctx::Ctx;

pub fn run(ctx: &Ctx, message: Option<String>) -> Result<()> {
    let repo = ff_core::discover(".")?;
    let prov = Provenance::new("manual", message).with_session(ctx.session.clone());
    let outcome = ff_core::capture(&repo, &prov)?;
    let branch = ff_core::snapshot::chain::chain_name(&ff_core::head_state(&repo)?);
    // Warnings go to stderr before anything else and regardless of --json,
    // because the one that matters here reports that pre-cutover refs were
    // parked rather than overwritten. A receipt nobody is shown is the same
    // as no receipt, and this is the path the first run after an upgrade
    // takes.
    let warnings = match &outcome {
        CaptureOutcome::Created { warnings, .. } | CaptureOutcome::NoOp { warnings, .. } => {
            warnings.as_slice()
        }
        CaptureOutcome::Contended => &[],
    };
    for warning in warnings {
        eprintln!("ff: {warning}");
    }
    match &outcome {
        CaptureOutcome::Created { id, .. } => {
            let short = id.short(8);
            if ctx.json {
                let payload = serde_json::json!({
                    "outcome": "created",
                    "id": id.to_string(),
                    "short_id": short,
                    "branch": branch,
                });
                crate::machine::emit("snap", &payload)?;
            } else {
                println!("snapshot {short} on {branch}");
                println!();
                let rows = ff_core::evolog(
                    &repo,
                    &EvologOptions {
                        limit: Some(3),
                        ..Default::default()
                    },
                )?;
                let ids: Vec<String> = rows.iter().map(|row| row.id.clone()).collect();
                let lens = crate::cmd::evolog::displayed_prefix_lens(&repo, &ids)?;
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs() as i64)
                    .unwrap_or(0);
                crate::render::init_palette(&repo);
                use std::io::Write as _;
                let mut out = crate::pager::LogOut::unpaged();
                let colored = out.colored();
                for row in &rows {
                    let _ = writeln!(out, "{}", crate::render::snap_row(row, &lens, now, colored));
                }
                out.finish();
            }
        }
        CaptureOutcome::NoOp { .. } => {
            if ctx.json {
                let payload = serde_json::json!({
                    "outcome": "noop",
                    "branch": branch,
                });
                crate::machine::emit("snap", &payload)?;
            } else {
                println!("no changes since the last snapshot on {branch}");
            }
        }
        CaptureOutcome::Contended => {
            if ctx.json {
                let payload = serde_json::json!({ "outcome": "contended" });
                crate::machine::emit("snap", &payload)?;
            } else {
                println!("snapshot skipped: a concurrent ff snapshot is in progress");
            }
        }
    }
    crate::selfupdate::notify::maybe_spawn_check(&repo);
    crate::autotrim::maybe_trim(&repo);
    if let Some(notice) = crate::selfupdate::notify::pending(&repo, env!("CARGO_PKG_VERSION")) {
        eprintln!("{notice}");
        crate::selfupdate::notify::mark_notified();
    }
    Ok(())
}
