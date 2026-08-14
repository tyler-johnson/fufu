use ff_core::{Error, LogOptions, Result};

pub fn run(json: bool, count: usize, commits: bool, ops: bool) -> Result<()> {
    crate::capture::pre_best_effort(&crate::provenance::pre_ff());
    if ops {
        return ops_view(json, count);
    }
    run_inner(json, count, commits)
}

/// `ff log --ops` — the operation journal, newest first, with op ids.
fn ops_view(json: bool, count: usize) -> Result<()> {
    use std::io::Write as _;
    let repo = ff_core::discover(".")?;
    let entries = ff_core::journal::read_ops(&repo, count)?;
    let mut out = crate::pager::LogOut::new(&repo, json);
    let result = (|| -> std::io::Result<()> {
        if json {
            let body = serde_json::to_string(&serde_json::json!({ "ops": entries }))
                .map_err(std::io::Error::other)?;
            writeln!(out, "{body}")?;
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
pub fn run_inner(json: bool, count: usize, commits_only: bool) -> Result<()> {
    let mut repo = ff_core::discover(".")?;
    let limit = if count == 0 { None } else { Some(count) };

    if commits_only {
        return commits_view(&mut repo, json, limit);
    }

    let open = ff_core::open_change(&repo)?;
    let commits: Vec<ff_core::LogEntry> =
        ff_core::log(&mut repo, &LogOptions { limit })?.collect::<Result<_>>()?;
    let ids: Vec<String> = commits.iter().map(|entry| entry.id.clone()).collect();
    let segments = ff_core::segment_anchors(&repo, &ids)?;

    if json {
        // `commits` key contract preserved; `id_letters` is composed at this
        // edge — the model stays hex.
        let body = serde_json::to_string(&serde_json::json!({
            "commits": commits,
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
        }))
        .map_err(Error::repo)?;
        println!("{body}");
        return Ok(());
    }

    use std::io::Write as _;
    let mut ids: Vec<String> = segments.values().cloned().collect();
    ids.extend(open.id.clone());
    let lens = crate::cmd::evolog::prefix_lens(&repo, &ids)?;
    let now = now_secs();
    let mut out = crate::pager::LogOut::new(&repo, false);
    let colored = out.colored();
    let result = (|| -> std::io::Result<()> {
        writeln!(
            out,
            "{}",
            crate::render::change_row(&open, &lens, now, colored)
        )?;
        for entry in &commits {
            let segment = segments.get(&entry.id).map(String::as_str);
            writeln!(
                out,
                "{}",
                crate::render::commit_row(entry, segment, &lens, now, colored)
            )?;
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
        let body = serde_json::to_string(&serde_json::json!({ "commits": commits }))
            .map_err(Error::repo)?;
        println!("{body}");
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
