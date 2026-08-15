//! `ff trim` — manual retention. Reports per chain; after any real run,
//! nudges git's own gc (the one pragmatic spawn in fufu: native writes never
//! trigger auto-gc, so without this nothing ever packs the object store —
//! not just the objects a trim orphaned). `gc --auto` is self-limiting: below
//! git's own threshold it returns having done nothing.

use ff_core::{Result, TrimOptions};

pub fn run(dry_run: bool, gone: bool, json: bool) -> Result<()> {
    crate::capture::pre_best_effort(&crate::provenance::pre_ff());
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
    let anything_dropped = report.chains.iter().any(|c| c.dropped > 0);

    if json {
        crate::machine::emit("trim", &report)?;
    } else {
        if report.chains.is_empty() {
            println!("no snapshot chains yet");
        }
        for chain in &report.chains {
            let total = chain.dropped + chain.kept;
            if chain.dropped == 0 {
                println!(
                    "{}: nothing to drop ({} snapshot{} kept)",
                    chain.branch,
                    chain.kept,
                    if chain.kept == 1 { "" } else { "s" }
                );
            } else if dry_run {
                println!(
                    "{}: would drop {} of {} snapshots",
                    chain.branch, chain.dropped, total
                );
            } else {
                let tail = match &chain.trash_ref {
                    Some(trash) => {
                        format!(" — previous tip saved at {trash} until the next trim")
                    }
                    None => String::new(),
                };
                if chain.deleted {
                    println!(
                        "{}: dropped all {} snapshots, chain removed{}",
                        chain.branch, chain.dropped, tail
                    );
                } else {
                    println!(
                        "{}: dropped {} of {} snapshots{}",
                        chain.branch, chain.dropped, total, tail
                    );
                }
            }
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
