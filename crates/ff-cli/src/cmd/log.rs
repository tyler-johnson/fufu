use ff_core::{Error, LogOptions, Result};

use crate::ctx::Ctx;

/// `session` is the `--session` flag: `None` when absent, `Some("")` for the
/// bare form (group everything), `Some(name)` to narrow to one session's
/// spans. Meaningless for `--ops` and `--commits`, which walk operations and
/// commits rather than the snapshot chain a session lives on — reject
/// rather than silently ignore.
pub fn run(
    ctx: &Ctx,
    count: usize,
    commits: bool,
    ops: bool,
    session: Option<String>,
) -> Result<()> {
    crate::capture::pre_best_effort(&crate::provenance::pre_ff(ctx));
    if ops {
        if session.is_some() {
            return Err(session_bad_flags("--ops"));
        }
        return ops_view(ctx.json, count);
    }
    if commits && session.is_some() {
        return Err(session_bad_flags("--commits"));
    }
    run_inner(ctx, count, commits, session)
}

fn session_bad_flags(flag: &str) -> Error {
    Error::coded(
        "usage/bad-flags",
        format!(
            "--session does not work with {flag}: it walks operations or commits, not the \
             snapshot chain a session lives on"
        ),
        vec!["ff log --session".into()],
    )
}

/// `ff log --ops` — the operation journal, newest first, with op ids.
fn ops_view(json: bool, count: usize) -> Result<()> {
    use std::io::Write as _;
    let repo = ff_core::discover(".")?;
    let entries = ff_core::journal::read_ops(&repo, count)?;
    let mut out = crate::pager::LogOut::new(&repo, json);
    let result = (|| -> std::io::Result<()> {
        if json {
            let payload = serde_json::json!({ "ops": entries });
            crate::machine::write(&mut out, "log", &payload).map_err(std::io::Error::other)?;
            return Ok(());
        }
        if entries.is_empty() {
            writeln!(out, "no operations journaled yet")?;
            return Ok(());
        }
        let now = now_secs();
        for op in &entries {
            let branch = op.branch.as_deref().unwrap_or("");
            writeln!(
                out,
                "{}  {:>8}  {:<8} {:<10} {}",
                op.short_id,
                crate::render::relative_age(now, op.time),
                op.kind,
                branch,
                op.summary
            )?;
        }
        writeln!(out, "undo: ff undo <op>")?;
        Ok(())
    })();
    out.finish();
    result.map_err(Error::repo)
}

/// Default view, jj-style: the open change (`@`) as the spine's head, then
/// the commit walk (`●` rows) with each commit's chain-segment tip beside
/// it. `--commits` forces the plain commits view and keeps Phase 0's exact
/// JSON shape.
pub fn run_inner(
    ctx: &Ctx,
    count: usize,
    commits_only: bool,
    session: Option<String>,
) -> Result<()> {
    let mut repo = ff_core::discover(".")?;
    let limit = if count == 0 { None } else { Some(count) };

    if commits_only {
        return commits_view(&mut repo, ctx.json, limit);
    }

    let open = ff_core::open_change(&repo)?;
    let commits: Vec<ff_core::LogEntry> =
        ff_core::log(&mut repo, &LogOptions { limit })?.collect::<Result<_>>()?;
    let ids: Vec<String> = commits.iter().map(|entry| entry.id.clone()).collect();
    let segments = ff_core::segment_anchors(&repo, &ids)?;

    // Each displayed commit's session is the session (if any) its own
    // chain-segment anchor snapshot carried — "the snapshot" a commit row
    // corresponds to, per `segment_anchors`. One targeted message read per
    // anchor already found, bounded by the commits already fetched: no
    // second chain walk. JSON always wants this; human rendering only
    // spends it when --session is in play.
    let want_sessions = ctx.json || session.is_some();
    let row_sessions: Vec<Option<String>> = if want_sessions {
        commits
            .iter()
            .map(|entry| match segments.get(&entry.id) {
                Some(anchor) => ff_core::snapshot_session(&repo, anchor),
                None => Ok(None),
            })
            .collect::<Result<_>>()?
    } else {
        vec![None; commits.len()]
    };
    let narrow: Option<&str> = match &session {
        Some(name) if !name.is_empty() => Some(name.as_str()),
        _ => None,
    };

    if ctx.json {
        // `commits` key contract preserved; `id_letters` is composed at this
        // edge — the model stays hex. Every row now also carries `session`
        // (null when the anchor snapshot has none); a name narrows the
        // array to matching rows, the bare flag leaves it untouched.
        let mut commit_values = Vec::with_capacity(commits.len());
        for (entry, sess) in commits.iter().zip(&row_sessions) {
            if let Some(target) = narrow
                && sess.as_deref() != Some(target)
            {
                continue;
            }
            let mut value = serde_json::to_value(entry).map_err(Error::repo)?;
            if let serde_json::Value::Object(ref mut map) = value {
                map.insert("session".into(), serde_json::json!(sess));
            }
            commit_values.push(value);
        }
        let payload = serde_json::json!({
            "commits": commit_values,
            "open": {
                "branch": open.branch,
                "id": open.id,
                "id_letters": open.id.as_deref().map(ff_core::snapid::encode),
                "base": open.base,
                "subject": open.subject,
                "time": open.time,
                "clean": open.clean,
                "pending": open.pending,
                "pending_short": open.pending.as_deref().map(|p| &p[..7]),
            },
        });
        crate::machine::emit("log", &payload)?;
        return Ok(());
    }

    use std::io::Write as _;
    crate::render::init_palette(&repo);
    let mut ids: Vec<String> = segments.values().cloned().collect();
    ids.extend(open.id.clone());
    let lens = crate::cmd::evolog::displayed_prefix_lens(&repo, &ids)?;
    let now = now_secs();
    let mut out = crate::pager::LogOut::new(&repo, false);
    let colored = out.colored();
    let result = (|| -> std::io::Result<()> {
        let change_display = crate::render::ChangeRowDisplay {
            subject: open.subject.as_deref(),
            born: open.base.is_some(),
            clean: open.clean,
            id: open.id.as_deref(),
            pending: open.pending.as_deref(),
            time: open.time,
        };
        writeln!(
            out,
            "{}",
            crate::render::change_row(&change_display, &lens, now, colored)
        )?;

        let write_commit_row = |out: &mut crate::pager::LogOut, entry: &ff_core::LogEntry| {
            let segment = segments.get(&entry.id).map(String::as_str);
            let commit_display = crate::render::CommitRowDisplay {
                id: &entry.id,
                subject: &entry.subject,
                time: entry.time,
            };
            writeln!(
                out,
                "{}",
                crate::render::commit_row(&commit_display, segment, &lens, now, colored)
            )
        };

        if session.is_none() {
            for entry in &commits {
                write_commit_row(&mut out, entry)?;
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
                    write_commit_row(&mut out, &commits[idx])?;
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
                        crate::render::session_header(&name, idxs.len(), "commit")
                    )?;
                    for idx in idxs {
                        write_commit_row(&mut out, &commits[idx])?;
                    }
                }
            }
        }
        Ok(())
    })();
    out.finish();
    result.map_err(Error::repo)
}

/// Phase 0's commits view, byte-stable: `{"commits":[...]}`.
fn commits_view(
    repo: &mut ff_core::gix::Repository,
    json: bool,
    limit: Option<usize>,
) -> Result<()> {
    let entries = ff_core::log(repo, &LogOptions { limit })?;
    if json {
        let commits: Vec<_> = entries.collect::<Result<_>>()?;
        // Envelope object so future fields can be added without breaking consumers.
        let payload = serde_json::json!({ "commits": commits });
        crate::machine::emit("log", &payload)?;
    } else {
        let now = now_secs();
        for entry in entries {
            let entry = entry?;
            println!("{}", crate::render::log_row(&entry, now));
        }
    }
    Ok(())
}

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}
