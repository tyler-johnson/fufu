//! `ff trim` — manual retention. Reports per chain; after a run that dropped
//! anything, nudges git's own gc (the one pragmatic spawn in fufu: native
//! writes never trigger auto-gc).

use ff_core::{Error, Result, TrimOptions};

pub fn run(dry_run: bool, gone: bool, json: bool) -> Result<()> {
    crate::capture::pre_best_effort(&crate::provenance::pre_ff());
    let repo = ff_core::discover(".")?;
    let report = ff_core::trim(
        &repo,
        &TrimOptions {
            now: None,
            dry_run,
            gone,
            keep_secs: None,
        },
    )?;
    let anything_dropped = report.chains.iter().any(|c| c.dropped > 0);

    if json {
        let body = serde_json::to_string(&report).map_err(Error::repo)?;
        println!("{body}");
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

    if anything_dropped && !dry_run {
        // Best effort; a machine without git skips silently.
        let _ = std::process::Command::new("git")
            .args(["gc", "--auto", "--quiet"])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
    }
    Ok(())
}
