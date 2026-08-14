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
    let ids: Vec<String> = rows.iter().map(|row| row.id.clone()).collect();
    let lens = prefix_lens(&repo, &ids)?;
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
/// accepts unambiguously. The domain is still exactly the live and trash
/// chains; what changed is that the domain is materialized per chain in the
/// id index instead of re-walked, so the cost is now the number of ids on
/// screen, not the length of the chain.
pub fn prefix_lens(
    repo: &ff_core::gix::Repository,
    ids: &[String],
) -> Result<HashMap<String, usize>> {
    let chain = ff_core::snapshot::chain::chain_name(&ff_core::head_state(repo)?);
    ff_core::idindex::prefix_lens(repo, &chain, ids)
}

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}
