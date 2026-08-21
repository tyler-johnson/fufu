//! `ff evolog` — strictly the snapshot chain of the open change, newest
//! first. No commit rows: commits are `ff log`'s spine, this is the
//! drill-in. Capture-first, so the newest row is this command's own
//! `pre: ff evolog` when the tree was dirty — intended, jj-like.

use std::collections::HashMap;
use std::io::Write as _;

use ff_core::{ChangeStat, Error, EvologOptions, Result, SnapEntry};

use crate::ctx::Ctx;

pub fn run(ctx: &Ctx, count: usize, patch: bool) -> Result<()> {
    ctx.refuse_past("ff evolog")?;
    let repo = ff_core::discover(".")?;
    let limit = if count == 0 { None } else { Some(count) };
    let rows = ff_core::evolog(
        &repo,
        &EvologOptions {
            limit,
            ..Default::default()
        },
    )?;

    // Every row's tag, one targeted message read each — bounded by the rows
    // already fetched, not a second chain walk. Only the machine surface
    // spends it: a tag is a property of the operation, not a view over rows.
    // Each row's patch is its own tree against the previous capture's on
    // this branch — assembled here rather than in `EvologOptions`, for the
    // same reason `session` is: it is a property of the operation, not of
    // the walk, and the walk has no business growing a field per view.
    let row_patches: Vec<Option<ChangeStat>> = if patch {
        let log = ff_core::ops::OpLog::open(&repo)?;
        rows.iter()
            .map(|row| row_patch(&repo, &log, row).map(Some))
            .collect::<Result<_>>()?
    } else {
        vec![None; rows.len()]
    };

    let row_sessions: Vec<Option<String>> = if ctx.json {
        rows.iter()
            .map(|row| crate::session::tag_of(&repo, &row.id))
            .collect::<Result<_>>()?
    } else {
        vec![None; rows.len()]
    };

    if ctx.json {
        let mut snapshots = Vec::with_capacity(rows.len());
        for ((row, sess), stat) in rows.iter().zip(&row_sessions).zip(&row_patches) {
            let mut value = serde_json::to_value(row).map_err(Error::repo)?;
            if let serde_json::Value::Object(ref mut map) = value {
                map.insert("session".into(), serde_json::json!(sess));
                if let Some(stat) = stat {
                    map.insert(
                        "changes".into(),
                        serde_json::to_value(&stat.files).map_err(Error::repo)?,
                    );
                    map.insert("insertions".into(), serde_json::json!(stat.insertions));
                    map.insert("deletions".into(), serde_json::json!(stat.deletions));
                }
            }
            snapshots.push(value);
        }
        let payload = serde_json::json!({ "snapshots": snapshots });
        crate::machine::emit("evolog", &payload)?;
        return Ok(());
    }

    if rows.is_empty() {
        let branch = ff_core::open_change(&repo)?.branch;
        println!("no snapshots on {branch} yet");
        return Ok(());
    }
    let ids: Vec<String> = rows.iter().map(|row| row.id.clone()).collect();
    let lens = displayed_prefix_lens(&repo, &ids)?;
    let now = now_secs();
    crate::render::init_palette(&repo);
    let mut out = crate::pager::LogOut::new(&repo, false);
    let colored = out.colored();
    let result = (|| -> std::io::Result<()> {
        for (row, stat) in rows.iter().zip(&row_patches) {
            writeln!(out, "{}", crate::render::snap_row(row, &lens, now, colored))?;
            // Furniture above, format below — the row, then what it did.
            let Some(stat) = stat else { continue };
            if stat.files.is_empty() {
                continue;
            }
            write!(out, "{}", crate::render::patch_block(&stat.files, colored))?;
            writeln!(out)?;
        }
        Ok(())
    })();
    out.finish();
    result.map_err(Error::repo)
}

/// What one capture changed: its tree against the previous capture's on the
/// same branch, or against nothing when it is the first.
fn row_patch(
    repo: &ff_core::gix::Repository,
    log: &ff_core::ops::OpLog<'_>,
    row: &SnapEntry,
) -> Result<ChangeStat> {
    let tree = |hex: &str| -> Result<ff_core::gix::ObjectId> {
        let oid = ff_core::gix::ObjectId::from_hex(hex.as_bytes()).map_err(Error::repo)?;
        Ok(log.get(ff_core::OpId::new(oid))?.tree())
    };
    let before = match &row.prev {
        Some(prev) => tree(prev)?,
        None => ff_core::gix::ObjectId::empty_tree(repo.object_hash()),
    };
    ff_core::tree_diff(
        repo,
        before,
        tree(&row.id)?,
        &ff_core::DiffOptions {
            hunks: true,
            paths: Vec::new(),
        },
    )
}

/// Unique-prefix lengths for the ids a view is about to print, or nothing at
/// all when the view cannot show them.
///
/// The bold prefix is the only consumer of these lengths, and `styled_id`
/// ignores them outright when color is off — so a piped or `NO_COLOR` run
/// would be computing a table it then throws away. Skipping it keeps such a
/// run read-only against the id index too: no rebuild, no write into `.git`,
/// which is what a fresh clone or a read-only checkout would otherwise pay
/// (~9ms here) to render nothing.
///
/// The empty map is not a fallback: every renderer already defaults a missing
/// id to a 1-character prefix, and that value is discarded uncolored.
pub fn displayed_prefix_lens(
    repo: &ff_core::gix::Repository,
    ids: &[String],
) -> Result<HashMap<String, usize>> {
    if !crate::pager::color_enabled() {
        return Ok(HashMap::new());
    }
    prefix_lens(repo, ids)
}

/// Unique-prefix lengths over the restore-resolution domain: the live AND
/// trashed operation log — so the bold prefix is exactly what `ff restore
/// --at` accepts unambiguously. The domain is one log rather than one chain
/// per branch now, which is why unique prefixes run to about five letters
/// instead of three; the cost is still the number of ids on screen.
pub fn prefix_lens(
    repo: &ff_core::gix::Repository,
    ids: &[String],
) -> Result<HashMap<String, usize>> {
    ff_core::ops::index::prefix_lens(repo, ids)
}

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}
