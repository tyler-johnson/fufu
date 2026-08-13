use ff_core::{Error, EvologOptions, LogOptions, Result};

pub fn run(json: bool, count: usize, commits: bool, ops: bool) -> Result<()> {
    crate::capture::pre_best_effort(&crate::provenance::pre_ff());
    if ops {
        return ops_view(json, count);
    }
    run_inner(json, count, commits)
}

/// `ff log --ops` — the operation journal, newest first, with op ids.
fn ops_view(json: bool, count: usize) -> Result<()> {
    let repo = ff_core::discover(".")?;
    let entries = ff_core::journal::read_ops(&repo, count)?;
    if json {
        let body =
            serde_json::to_string(&serde_json::json!({ "ops": entries })).map_err(Error::repo)?;
        println!("{body}");
        return Ok(());
    }
    if entries.is_empty() {
        println!("no operations journaled yet");
        return Ok(());
    }
    let now = now_secs();
    for op in &entries {
        let branch = op.branch.as_deref().unwrap_or("");
        println!(
            "{}  {:>8}  {:<8} {:<10} {}",
            op.short_id,
            crate::render::relative_age(now, op.time),
            op.kind,
            branch,
            op.summary
        );
    }
    println!("undo: ff undo <op>");
    Ok(())
}

/// Default view: the snapshot chain when the branch has snapshots, otherwise
/// the plain commits view (a fresh clone looks unchanged). Interim shape —
/// the change-centric spine lands with the presentation phases.
/// `--commits` forces the commits view and keeps Phase 0's exact JSON shape.
pub fn run_inner(json: bool, count: usize, commits_only: bool) -> Result<()> {
    let mut repo = ff_core::discover(".")?;
    let limit = if count == 0 { None } else { Some(count) };

    if !commits_only {
        let rows = ff_core::evolog(
            &repo,
            &EvologOptions {
                limit,
                ..Default::default()
            },
        )?;
        if !rows.is_empty() {
            if json {
                let commits: Vec<_> =
                    ff_core::log(&mut repo, &LogOptions { limit })?.collect::<Result<_>>()?;
                let body = serde_json::to_string(
                    &serde_json::json!({ "commits": commits, "timeline": rows }),
                )
                .map_err(Error::repo)?;
                println!("{body}");
            } else {
                let now = now_secs();
                for row in &rows {
                    println!("{}", crate::render::timeline_row(row, now));
                }
            }
            return Ok(());
        }
    }

    commits_view(&mut repo, json, limit)
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
