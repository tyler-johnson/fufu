//! `ff branch` — the branch family: list (named and anonymous segregated)
//! and delete. Naming a branch is not here; `ff describe -b` is the one
//! verb that does it.

use std::ffi::OsString;

use ff_core::{Error, Result};

use crate::cli::BranchAction;
use crate::ctx::Ctx;

pub fn run(ctx: &Ctx, action: Option<BranchAction>) -> Result<()> {
    match action {
        None | Some(BranchAction::List { .. }) => list(ctx),
        Some(BranchAction::Delete { target }) => delete(ctx, &target),
        Some(BranchAction::Other(words)) => Err(unknown(&words)),
    }
}

/// A removal, not a rename. `ff branch <name>` claimed the anonymous branch
/// you were standing on; that act is `ff describe -b <name>` now and takes
/// proper names too, so the redirect names the verb rather than translating
/// the invocation. Every other stray word lands here as well, which is why
/// the message also says what the family does take.
fn unknown(words: &[OsString]) -> Error {
    let typed = words
        .first()
        .map(|w| w.to_string_lossy().into_owned())
        .unwrap_or_default();
    Error::coded(
        "usage/unknown-subcommand",
        format!(
            "ff branch takes list or delete, not {typed:?} — naming the branch you are on is \
             ff describe -b"
        ),
        vec![
            "ff branch list".into(),
            format!("ff describe -b {typed}"),
            "ff branch delete <branch>".into(),
        ],
    )
}

fn delete(ctx: &Ctx, target: &str) -> Result<()> {
    let repo = ff_core::discover(".")?;
    let (report, verb_ctx) = ff_core::branch::delete(
        &repo,
        target,
        &crate::provenance::pre_ff(ctx),
        None,
        std::env::args().collect(),
    )?;
    crate::render::init_palette(&repo);
    crate::render::reconcile_notice(&verb_ctx.reconcile);
    let colored = crate::pager::color_enabled();
    if ctx.json {
        let payload = serde_json::json!({
            "deleted": report,
            "undo": "ff undo",
        });
        crate::machine::emit("branch delete", &payload)?;
        return Ok(());
    }
    println!(
        "deleted {} (was {})",
        report.name,
        crate::render::paint_sha(&report.tip[..report.tip.len().min(8)], colored)
    );
    if let Some(trash) = &report.trash_ref {
        println!("  its timeline moved to {trash}");
    }
    if report.parked_demoted.is_some() {
        println!("  its parked change stays in the stash (git stash list)");
    }
    println!("{}", crate::render::paint_dim("undo: ff undo", colored));
    Ok(())
}

fn list(ctx: &Ctx) -> Result<()> {
    // Listing branches as of a past operation needs that operation's ref
    // table threaded through the walk, which is the follow-up this plan
    // named rather than the flag being absent.
    ctx.refuse_past("ff branch list")?;
    let repo = ff_core::discover(".")?;
    // Reads don't reconcile here; `ff status` owns loudness.
    let list = ff_core::branch::list(&repo)?;
    if ctx.json {
        crate::machine::emit("branch list", &list)?;
        return Ok(());
    }
    crate::render::init_palette(&repo);
    let colored = crate::pager::color_enabled();
    // One name column across the whole listing — the widest label, not the
    // widest name, since sigil and brackets ride the column too — floored
    // so a listing of short names does not look cramped.
    let label_width = list
        .named
        .iter()
        .chain(list.anonymous.iter())
        .map(|info| crate::render::branch_label_width(&info.name))
        .max()
        .unwrap_or(14)
        .max(14);
    let mut gap = false;
    for (section, header) in [(&list.named, ""), (&list.anonymous, "anonymous:")] {
        if section.is_empty() {
            continue;
        }
        if !header.is_empty() {
            if !list.named.is_empty() {
                println!();
            }
            println!("{}", crate::render::paint_dim(header, colored));
            // The section separator is already the air; never double it.
            gap = false;
        }
        for info in section {
            if gap {
                println!();
            }
            let lines = crate::render::branch_row(info, label_width, colored);
            // A row that hung a note gets air beneath it before the next
            // branch's head line; an all-quiet listing stays single-spaced.
            gap = lines.len() > 1;
            for line in lines {
                println!("{line}");
            }
        }
    }
    Ok(())
}
