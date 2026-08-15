//! `ff evolog` — strictly the snapshot chain of the open change, newest
//! first. No commit rows: commits are `ff log`'s spine, this is the
//! drill-in. Capture-first, so the newest row is this command's own
//! `pre: ff evolog` when the tree was dirty — intended, jj-like.

use std::collections::HashMap;
use std::io::Write as _;

use ff_core::{Error, EvologOptions, Result};

/// `session` is the `--session` flag: `None` when absent, `Some("")` for the
/// bare form (group everything), `Some(name)` to narrow to one session's
/// spans.
pub fn run(json: bool, count: usize, session: Option<String>) -> Result<()> {
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

    // Every row's session, one targeted message read each — bounded by the
    // rows already fetched, not a second chain walk. JSON always wants this
    // (a plain `ff evolog --json` carries it too); human rendering only
    // spends it when --session is actually in play.
    let want_sessions = json || session.is_some();
    let row_sessions: Vec<Option<String>> = if want_sessions {
        rows.iter()
            .map(|row| ff_core::snapshot_session(&repo, &row.id))
            .collect::<Result<_>>()?
    } else {
        vec![None; rows.len()]
    };
    let narrow: Option<&str> = match &session {
        Some(name) if !name.is_empty() => Some(name.as_str()),
        _ => None,
    };

    if json {
        let mut snapshots = Vec::with_capacity(rows.len());
        for (row, sess) in rows.iter().zip(&row_sessions) {
            if let Some(target) = narrow
                && sess.as_deref() != Some(target)
            {
                continue;
            }
            let mut value = serde_json::to_value(row).map_err(Error::repo)?;
            if let serde_json::Value::Object(ref mut map) = value {
                map.insert("session".into(), serde_json::json!(sess));
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
        if session.is_none() {
            for row in &rows {
                writeln!(out, "{}", crate::render::snap_row(row, &lens, now, colored))?;
            }
            return Ok(());
        }
        // --session (bare or named): group into spans, header per span.
        // Rows outside any session render exactly as they do today, unless
        // a name narrowed the view — then only that session's spans show.
        for slot in crate::render::session_slots(&row_sessions) {
            match slot {
                crate::render::SessionSlot::Row(idx) => {
                    if narrow.is_some() {
                        continue;
                    }
                    writeln!(
                        out,
                        "{}",
                        crate::render::snap_row(&rows[idx], &lens, now, colored)
                    )?;
                }
                crate::render::SessionSlot::Span { name, rows: idxs } => {
                    if let Some(target) = narrow
                        && name != target
                    {
                        continue;
                    }
                    writeln!(
                        out,
                        "{}",
                        crate::render::session_header(&name, idxs.len(), "snapshot")
                    )?;
                    for idx in idxs {
                        writeln!(
                            out,
                            "{}",
                            crate::render::snap_row(&rows[idx], &lens, now, colored)
                        )?;
                    }
                }
            }
        }
        Ok(())
    })();
    out.finish();
    result.map_err(Error::repo)
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
