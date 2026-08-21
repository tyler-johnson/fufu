//! Bare `ff` — the map. `ff [-n <count>] [--all]` draws the local branches
//! as a skeleton: the tips, the merges, the forks — the answer to "where did
//! I leave that idea?".
//!
//! Capture still comes first, like every other verb: what retired is
//! *typing* a snapshot, not taking one. The auto-trim lane no longer rides
//! this command line specially — the table on `Command` decides which
//! command lines carry it, and this one is just a row in it.

use ff_core::{Error, Result};

use crate::ctx::Ctx;

pub fn run(ctx: &Ctx, branches: Option<usize>, all: bool) -> Result<()> {
    // The past-state view is what `--at-op` would need here, and it does not
    // exist yet.
    // Bare `ff` declares no `--at` flags today, so this cannot fire yet — it
    // is here so the day the root grows them the refusal is already correct.
    ctx.refuse_past("ff")?;
    let repo = ff_core::discover(".")?;
    // 0 means all; `--all` is the same wish spelled out.
    let limit = if all {
        None
    } else {
        match branches {
            None => Some(10),
            Some(0) => None,
            Some(n) => Some(n),
        }
    };
    let map = ff_core::map::map(&repo, &ff_core::MapOptions { branches: limit })?;
    if ctx.json {
        // The `Map` serializes itself: `{"rows": [...], "truncated": bool}`.
        crate::machine::emit("map", &map)?;
    } else {
        human(&repo, &map)?;
    }
    Ok(())
}

/// The human surface: the map's rows over the lane renderer, through the
/// same pager and palette the log family uses, so the two read as siblings.
fn human(repo: &ff_core::gix::Repository, map: &ff_core::Map) -> Result<()> {
    use std::io::Write as _;

    crate::render::init_palette(repo);
    // Each commit row's op id is its chain-segment anchor — the same one
    // `ff log` and `ff status` show beside that commit. One walk over the
    // rows already fetched, exactly as `cmd/log.rs` runs it.
    let commit_ids: Vec<String> = map
        .rows
        .iter()
        .filter_map(|row| match &row.node {
            ff_core::MapNode::Commit { id, .. } => Some(id.clone()),
            _ => None,
        })
        .collect();
    let segments = ff_core::segment_anchors(repo, &commit_ids)?;
    // Every id the map can display gets priced together, so two rows never
    // abbreviate the same operation to different lengths.
    let mut ids: Vec<String> = segments.values().cloned().collect();
    ids.extend(map.rows.iter().filter_map(|row| match &row.node {
        ff_core::MapNode::Open { id, .. } => id.clone(),
        _ => None,
    }));
    let lens = crate::cmd::evolog::displayed_prefix_lens(repo, &ids)?;
    let now = now_secs();
    let mut out = crate::pager::LogOut::new(repo, false);
    let colored = out.colored();
    let payloads: Vec<crate::render::MapPayload> = map
        .rows
        .iter()
        .map(|row| crate::render::map_payload(&row.node, &segments, &lens, now, colored))
        .collect();
    let rows: Vec<crate::graph::GraphRow> = payloads
        .iter()
        .zip(&map.rows)
        .map(|(p, row)| crate::graph::GraphRow {
            parents: &row.parents,
            glyph: &p.glyph,
            payload: &p.lines,
        })
        .collect();
    let lines = crate::graph::render(&rows, colored);
    let result = (|| -> std::io::Result<()> {
        for line in lines {
            writeln!(out, "{line}")?;
        }
        Ok(())
    })();
    out.finish();
    result.map_err(Error::repo)
}

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}
