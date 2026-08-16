//! `ff trim` — manual retention. Reports the one log first, because that is
//! what retention acts on; a branch pointer is a *place in* the log, and the
//! only per-branch fact worth a line is a pointer that went away with its
//! branch. After any real run it nudges git's own gc (the one pragmatic spawn
//! in fufu: native writes never trigger auto-gc, so without this nothing ever
//! packs the object store — not just the objects a trim orphaned). `gc
//! --auto` is self-limiting: below git's own threshold it returns having done
//! nothing.

use ff_core::{Result, TrimOptions};

use crate::ctx::Ctx;

pub fn run(ctx: &Ctx, dry_run: bool, gone: bool) -> Result<()> {
    crate::capture::pre_best_effort(&crate::provenance::pre_ff(ctx));
    let repo = ff_core::discover(".")?;
    crate::render::init_palette(&repo);
    let report = ff_core::trim(
        &repo,
        &TrimOptions {
            now: None,
            dry_run,
            gone,
            keep_secs: None,
        },
    )?;
    if !dry_run {
        crate::autotrim::stamp(&repo);
    }
    let anything_dropped = report.log.as_ref().is_some_and(|log| log.dropped > 0);

    if ctx.json {
        crate::machine::emit("trim", &report)?;
    } else {
        match &report.log {
            None => println!("no operations yet"),
            Some(log) => {
                let total = log.dropped + log.kept;
                if log.dropped == 0 {
                    println!(
                        "nothing to drop ({} operation{} kept)",
                        log.kept,
                        if log.kept == 1 { "" } else { "s" }
                    );
                } else if dry_run {
                    println!("would drop {} of {total} operations", log.dropped);
                } else {
                    let tail = match &log.trash_ref {
                        Some(trash) => {
                            format!(" — previous tip saved at {trash} until the next trim")
                        }
                        None => String::new(),
                    };
                    if log.deleted {
                        println!(
                            "dropped all {} operations, the log removed{tail}",
                            log.dropped
                        );
                    } else {
                        println!("dropped {} of {total} operations{tail}", log.dropped);
                    }
                }
            }
        }
        // `--gone`: the branch is gone, so its way into the log goes too. The
        // operations themselves stay — one branch's cannot be excised from
        // the middle of one log — and age out on the same cutoff as
        // everything else.
        for pointer in report.pointers.iter().filter(|p| p.deleted) {
            println!(
                "  {}: branch is gone — pointer removed; its operations age out on the keep window",
                pointer.branch
            );
        }
        if anything_dropped && !dry_run {
            println!("dropped data frees after gc");
        }
    }

    if !dry_run {
        // Best effort; a machine without git skips silently.
        let _ = std::process::Command::new("git")
            .args(["gc", "--auto", "--quiet"])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
    }
    Ok(())
}
