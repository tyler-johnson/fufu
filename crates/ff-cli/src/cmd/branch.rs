//! `ff branch` — the branch family: list (named and anonymous segregated)
//! and delete. Naming a branch is not here; `ff describe -b` is the one
//! verb that does it.

use std::ffi::OsString;

use ff_core::{Error, Result};

use crate::cli::BranchAction;
use crate::ctx::Ctx;

pub fn run(ctx: &Ctx, action: Option<BranchAction>) -> Result<()> {
    match action {
        None => list(ctx, false),
        Some(BranchAction::List { all, .. }) => list(ctx, all),
        Some(BranchAction::Delete { target, shared }) => delete(ctx, &target, shared),
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

fn delete(ctx: &Ctx, target: &str, shared: bool) -> Result<()> {
    let repo = ff_core::discover(".")?;

    // `--shared` probes first, because its one refusal — an aliased tracking
    // ref — would remove somebody else's copy, and that must land before the
    // local delete rather than merely before the push. The wire's cwd is
    // resolved here beside it; the copy it will remove is re-read from the
    // report once the delete is done.
    let cwd = if shared {
        if let Some(probe) = &ff_core::branch::shared_copy(&repo, target)?
            && probe.aliased
        {
            return Err(Error::coded(
                "branch/aliased-copy",
                format!(
                    "{} is what {target} tracks, and it wears another branch's name: \
                     --shared would remove somebody else's copy",
                    probe.name
                ),
                vec![
                    format!("ff branch delete {target}"),
                    "ff branch list".into(),
                    "ff remote".into(),
                ],
            ));
        }
        let cwd = repo
            .workdir()
            // Uncoded on purpose: this is not a bare repository, so there is
            // a working directory, and reaching here without one is an
            // internal inconsistency rather than a state to name.
            .ok_or_else(|| ff_core::Error::msg("no working directory: internal inconsistency"))?
            .to_path_buf();
        Some(cwd)
    } else {
        None
    };

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

    // The wire, and only then its local traces: local first, so a failed
    // push degrades to the plain-delete outcome — the delete done and
    // undoable, the copy intact — and the tracking ref, the config section
    // and the published note come off only after the wire agreed.
    let shared_removed = if let (Some(cwd), Some(shared)) = (&cwd, &report.shared) {
        if !shared.tip.is_empty() {
            crate::net::push_delete(cwd, &shared.remote, &shared.remote_branch, &shared.tip)?;
            ff_core::branch::forget_shared(&repo, target, &shared.r#ref, verb_ctx.now)?;
            true
        } else {
            false
        }
    } else {
        false
    };

    if ctx.json {
        let payload = serde_json::json!({
            "deleted": report,
            "undo": "ff undo",
            "shared_removed": shared_removed,
        });
        crate::machine::emit("branch delete", &payload)?;
        return Ok(());
    }
    println!(
        "deleted {} (was {})",
        report.name,
        crate::render::paint_sha(ff_core::sha::short(report.tip.as_str()), colored)
    );
    if let Some(trash) = &report.trash_ref {
        println!("  its timeline moved to {trash}");
    }
    if report.parked_demoted.is_some() {
        println!("  its parked change stays in the stash (git stash list)");
    }
    match (&report.shared, shared) {
        (Some(shared), true) => {
            if shared_removed {
                println!(
                    "  removed the shared copy {}, and the tracking ref and upstream with it",
                    shared.name
                );
            } else {
                println!("  there was no shared copy to remove");
            }
        }
        (Some(shared), false) => {
            if shared.aliased {
                println!(
                    "  {} is what this branch tracked, and it wears another branch's name — left alone",
                    shared.name
                );
            } else if shared.tip.is_empty() {
                println!(
                    "  its upstream {} is configured and not there — nothing to remove",
                    shared.name
                );
            } else {
                // The way to it is a pair, not a verb: this branch is already
                // gone here, so `--shared` has nothing left to stand on until
                // the undo puts it back. `ff publish`'s tail says the same
                // shape for the same reason.
                println!(
                    "  the shared copy {} is still there — ff undo then ff branch delete {} --shared removes it too",
                    shared.name, report.name
                );
            }
        }
        (None, true) => println!("  there was no shared copy to remove"),
        (None, false) => {}
    }
    if shared_removed {
        println!(
            "{}",
            crate::render::paint_dim(
                "the delete left the machine — ff undo cannot reach it",
                colored
            )
        );
        println!(
            "{}",
            crate::render::paint_dim(
                "ff undo brings the branch back; ff publish sends the copy again",
                colored
            )
        );
    } else {
        println!("{}", crate::render::paint_dim("undo: ff undo", colored));
    }
    Ok(())
}

fn list(ctx: &Ctx, all: bool) -> Result<()> {
    // Listing branches as of a past operation needs that operation's ref
    // table threaded through the walk, which is the follow-up this plan
    // named rather than the flag being absent.
    ctx.refuse_past("ff branch list")?;
    let repo = ff_core::discover(".")?;
    // Reads don't reconcile here; `ff status` owns loudness.
    // Ten is the map's own default branch bound — the bound keeps a clone
    // of a large repository from scrolling your own branches off the top,
    // and `--all` is that wish spelled out.
    let remote_limit = if all { None } else { Some(10) };
    let list = ff_core::branch::list(&repo, &ff_core::BranchListOptions { remote_limit })?;
    if ctx.json {
        crate::machine::emit("branch list", &list)?;
        return Ok(());
    }
    crate::render::init_palette(&repo);
    let colored = crate::pager::color_enabled();
    // One name column across the whole listing — the widest label, not the
    // widest name, since sigil and brackets ride the column too, and the
    // remote labels ride the same column, two narrower for wanting no
    // brackets — floored so a listing of short names does not look cramped.
    let local_width = list
        .named
        .iter()
        .chain(list.anonymous.iter())
        .map(|info| crate::render::branch_label_width(&info.name))
        .max()
        .unwrap_or(0);
    let remote_width = list
        .remote_only
        .iter()
        .map(|info| crate::render::remote_label_width(&info.name))
        .max()
        .unwrap_or(0);
    let label_width = local_width.max(remote_width).max(14);
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
    if !list.remote_only.is_empty() {
        if !list.named.is_empty() || !list.anonymous.is_empty() {
            println!();
        }
        println!("{}", crate::render::paint_dim("remote only:", colored));
        for info in &list.remote_only {
            println!(
                "{}",
                crate::render::remote_branch_row(info, label_width, colored)
            );
        }
        // `remote_more` cannot be non-zero when the bucket is empty, so no
        // orphan count row is possible here.
        if list.remote_more > 0 {
            println!(
                "{}",
                crate::render::remote_more_row(list.remote_more, colored)
            );
        }
    }
    Ok(())
}
