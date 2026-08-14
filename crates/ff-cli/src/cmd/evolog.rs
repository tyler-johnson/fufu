//! `ff evolog` — strictly the snapshot chain of the open change, newest
//! first. No commit rows: commits are `ff log`'s spine, this is the
//! drill-in. Capture-first, so the newest row is this command's own
//! `pre: ff evolog` when the tree was dirty — intended, jj-like.

use std::collections::HashMap;
use std::io::Write as _;

use ff_core::{Error, EvologOptions, Result};

pub fn run(json: bool, count: usize) -> Result<()> {
    crate::capture::pre_best_effort(&crate::provenance::pre_ff());
    let repo = ff_core::discover(".")?;
    let limit = if count == 0 { None } else { Some(count) };
    let rows = ff_core::evolog(
        &repo,
        &EvologOptions {
            limit,
            ..Default::default()
        },
    )?;
    if json {
        let body = serde_json::to_string(&serde_json::json!({ "snapshots": rows }))
            .map_err(Error::repo)?;
        println!("{body}");
        return Ok(());
    }
    if rows.is_empty() {
        let branch = ff_core::open_change(&repo)?.branch;
        println!("no snapshots on {branch} yet");
        return Ok(());
    }
    let lens = prefix_lens(&repo)?;
    let now = now_secs();
    let mut out = crate::pager::LogOut::new(&repo, false);
    let colored = out.colored();
    let result = (|| -> std::io::Result<()> {
        for row in &rows {
            writeln!(out, "{}", crate::render::snap_row(row, &lens, now, colored))?;
        }
        Ok(())
    })();
    out.finish();
    result.map_err(Error::repo)
}

/// Unique-prefix lengths over the restore-resolution domain: the live AND
/// trash chains — so the bold prefix is exactly what `ff restore --at`
/// accepts unambiguously.
pub fn prefix_lens(repo: &ff_core::gix::Repository) -> Result<HashMap<String, usize>> {
    // Ids only: the domain has to be the whole chain for the bold prefix to
    // mean what it claims, so it must not also pay for rows it never shows.
    let ids = ff_core::chain_ids(
        repo,
        &EvologOptions {
            limit: None,
            chain: None,
            include_trash: true,
        },
    )?;
    Ok(crate::render::unique_prefix_lens(&ids))
}

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}
