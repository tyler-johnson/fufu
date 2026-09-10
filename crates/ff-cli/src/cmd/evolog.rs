//! `ff evolog` — the drill-in behind the letters column. Bare, or on `@`,
//! it is strictly the capture chain of the open change, newest first. No
//! commit rows: commits are `ff log`'s spine. Capture-first, so the newest
//! row is this command's own `pre: ff evolog` when the tree was dirty —
//! intended, jj-like. On a revision it is that change's history: every
//! operation on any chain that produced a commit carrying its id, and the
//! captures behind its close.

use std::collections::HashMap;
use std::io::Write as _;

use ff_core::revset::{Rev, Revset};
use ff_core::{ChangeStat, Error, EvologOptions, Result, SnapEntry};

use crate::ctx::Ctx;

pub fn run(ctx: &Ctx, rev: Option<String>, count: usize, patch: bool) -> Result<()> {
    ctx.refuse_past("ff evolog")?;
    let repo = ff_core::discover(".")?;
    let limit = if count == 0 { None } else { Some(count) };
    // The same single-member resolver `ff show` uses: a change id, a prefix
    // of the open change's id, a sha, a branch, all read the same way, and
    // an operation id typed here is refused toward `ff op show`.
    if let Some(raw) = rev.as_deref()
        && let Rev::Commit(id) = Revset::parse(raw)?.point(&repo)?.rev
    {
        return change(ctx, &repo, id.object_id(), limit, patch);
    }
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
        let snapshots = snapshot_values(&rows, &row_sessions, &row_patches)?;
        // The open change's id rides the envelope, so a script reading the
        // open change's captures knows which change they belong to.
        let payload = serde_json::json!({
            "change_id": ff_core::open_change(&repo)?.change_id,
            "snapshots": snapshots,
        });
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

/// The capture rows as JSON, each carrying its session and, under `-p`, its
/// patch. Shared by the open change's view and a change's.
fn snapshot_values(
    rows: &[SnapEntry],
    sessions: &[Option<String>],
    patches: &[Option<ChangeStat>],
) -> Result<Vec<serde_json::Value>> {
    let mut snapshots = Vec::with_capacity(rows.len());
    for ((row, sess), stat) in rows.iter().zip(sessions).zip(patches) {
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
    Ok(snapshots)
}

/// `ff evolog <rev>`: one change's history. The operations that produced a
/// commit carrying its id, on every chain, then the captures behind its
/// close under a divider. `-p` applies to the capture rows, which carry a
/// tree; an operation row names what it produced and prints nothing under
/// it.
fn change(
    ctx: &Ctx,
    repo: &ff_core::gix::Repository,
    sha: ff_core::gix::ObjectId,
    limit: Option<usize>,
    patch: bool,
) -> Result<()> {
    let history = ff_core::evolog_of(repo, sha, limit)?;
    let rows = &history.snapshots;
    let row_patches: Vec<Option<ChangeStat>> = if patch {
        let log = ff_core::ops::OpLog::open(repo)?;
        rows.iter()
            .map(|row| row_patch(repo, &log, row).map(Some))
            .collect::<Result<_>>()?
    } else {
        vec![None; rows.len()]
    };
    let row_sessions: Vec<Option<String>> = if ctx.json {
        rows.iter()
            .map(|row| crate::session::tag_of(repo, &row.id))
            .collect::<Result<_>>()?
    } else {
        vec![None; rows.len()]
    };

    if ctx.json {
        let payload = serde_json::json!({
            "change_id": history.change_id,
            "commit": history.commit,
            "operations": history.operations,
            "snapshots": snapshot_values(rows, &row_sessions, &row_patches)?,
        });
        return crate::machine::emit("evolog", &payload);
    }

    if history.operations.is_empty() && rows.is_empty() {
        println!("no operations recorded for {}", history.change_id);
        return Ok(());
    }
    let ids: Vec<String> = rows.iter().map(|row| row.id.clone()).collect();
    let lens = displayed_prefix_lens(repo, &ids)?;
    let now = now_secs();
    crate::render::init_palette(repo);
    let mut out = crate::pager::LogOut::new(repo, false);
    let colored = out.colored();
    let result = (|| -> std::io::Result<()> {
        for op in &history.operations {
            writeln!(out, "{}", crate::render::change_op_row(op, now, colored))?;
        }
        if !history.operations.is_empty() && !rows.is_empty() {
            writeln!(out, "{}", crate::render::paint_dim("captures", colored))?;
        }
        for (row, stat) in rows.iter().zip(&row_patches) {
            writeln!(out, "{}", crate::render::snap_row(row, &lens, now, colored))?;
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
/// per branch now, which is why unique prefixes run to about five hex
/// digits instead of three; the cost is still the number of ids on screen.
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
