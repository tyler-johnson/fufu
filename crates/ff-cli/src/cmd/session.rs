//! `ff session` — open, close, and inspect capture sessions.

use ff_core::Result;
use ff_core::gix;

use crate::session;

/// Run the session command. `action` is `None` for bare `ff session`,
/// `Some("start")` for `ff session start`, `Some("end")` for `ff session end`,
/// `Some("list")` for `ff session list`, `Some("diff")` for `ff session diff`.
pub fn run(action: Option<&str>, name: Option<String>, json: bool) -> Result<()> {
    let repo = ff_core::discover(".")?;

    match action {
        None => status(&repo, json),
        Some("start") => start(&repo, name, json),
        Some("end") => end(&repo, json),
        Some("list") => list(&repo, json),
        Some("diff") => diff(&repo, name, json),
        Some(other) => Err(ff_core::Error::msg(format!(
            "unknown session action: {other}"
        ))),
    }
}

/// Cap on the bounded chain reads `list` and `diff` perform — the same
/// value `ff evolog`'s `-n/--max-count` defaults to (see the `Evolog` arm in
/// `crates/ff-cli/src/cli.rs`), so a session query costs no more than an
/// evolog listing already does.
const SESSION_QUERY_LIMIT: usize = 25;

fn list(repo: &gix::Repository, json: bool) -> Result<()> {
    let spans = ff_core::spans(repo, Some(SESSION_QUERY_LIMIT))?;

    if json {
        let payload = serde_json::json!({ "spans": spans });
        crate::machine::emit("session", &payload)?;
        return Ok(());
    }

    if spans.is_empty() {
        println!("no sessions on this branch");
        return Ok(());
    }

    crate::render::init_palette(repo);
    let colored = crate::pager::color_enabled();
    let now = now_secs();
    for span in &spans {
        println!("{}", crate::render::session_span_row(span, now, colored));
    }
    Ok(())
}

fn diff(repo: &gix::Repository, name: Option<String>, json: bool) -> Result<()> {
    let name = match name {
        Some(n) => n,
        None => match session::read_current(repo) {
            Some(n) => n,
            None => {
                return Err(ff_core::Error::coded(
                    "usage/needs-session",
                    "no session open and none named",
                    vec!["ff session list".into(), "ff session diff <name>".into()],
                ));
            }
        },
    };

    let spans = ff_core::spans(repo, Some(SESSION_QUERY_LIMIT))?;
    let span = spans.into_iter().find(|s| s.name == name).ok_or_else(|| {
        ff_core::Error::coded(
            "usage/needs-session",
            format!("no session named {name} found on this branch"),
            vec!["ff session list".into()],
        )
    })?;

    let newest_oid =
        gix::ObjectId::from_hex(span.newest.as_bytes()).map_err(ff_core::Error::repo)?;
    let newest_tree = repo
        .find_commit(newest_oid)
        .map_err(ff_core::Error::repo)?
        .tree_id()
        .map_err(ff_core::Error::repo)?
        .detach();
    let start_tree = ff_core::span_start_tree(repo, &span)?;
    let change_stat = ff_core::tree_diff_stat(repo, start_tree, newest_tree)?;

    if json {
        let payload = serde_json::json!({
            "name": span.name,
            "span": span,
            "changes": change_stat.files,
            "insertions": change_stat.insertions,
            "deletions": change_stat.deletions,
        });
        crate::machine::emit("session", &payload)?;
        return Ok(());
    }

    // Spans of the same name never merge across a gap (see
    // `ff_core::session::spans`), so a repeat name is always reported as its
    // newest span here — say so, rather than let a silent pick look like the
    // only one that ever existed.
    println!(
        "session {} — newest span, {} snapshot{}",
        span.name,
        span.snapshots,
        if span.snapshots == 1 { "" } else { "s" }
    );
    if change_stat.files.is_empty() {
        println!("no changes");
    } else {
        crate::render::init_palette(repo);
        let colored = crate::pager::color_enabled();
        println!("{}", crate::render::render_diffstat(&change_stat, colored));
    }
    Ok(())
}

fn status(repo: &gix::Repository, json: bool) -> Result<()> {
    match session::current(repo) {
        Some(marker) => {
            if json {
                let payload = serde_json::json!({
                    "name": marker.name,
                    "started": marker.started,
                });
                crate::machine::emit("session", &payload)?;
            } else {
                let elapsed = elapsed_human(marker.started);
                println!("session {} — open {}", marker.name, elapsed);
            }
        }
        None => {
            if json {
                let payload = serde_json::json!({ "name": serde_json::Value::Null });
                crate::machine::emit("session", &payload)?;
            } else {
                println!("no session open");
            }
        }
    }
    Ok(())
}

fn start(repo: &gix::Repository, name: Option<String>, json: bool) -> Result<()> {
    // Capture ordering: take the pre-snapshot BEFORE writing the marker,
    // so the snapshot of the state you were in belongs to the old session.
    let prov = ff_core::Provenance::new("pre", Some("ff session start".into()));
    crate::capture::pre_best_effort(&prov);

    let name = match name {
        Some(raw) => session::normalize(&raw).unwrap_or_else(session::generate_name),
        None => session::generate_name(),
    };

    // Check what was previously open so we can report a replacement.
    let previous = session::read_current(repo);

    let marker = session::write_marker(repo, &name)?;

    if json {
        let payload = serde_json::json!({
            "name": marker.name,
            "started": marker.started,
        });
        crate::machine::emit("session", &payload)?;
    } else if let Some(prev_name) = previous {
        println!("session {} started (replaced {})", marker.name, prev_name);
    } else {
        println!("session {} started", marker.name);
    }
    Ok(())
}

fn end(repo: &gix::Repository, json: bool) -> Result<()> {
    // Capture ordering: take the pre-snapshot BEFORE clearing the marker,
    // so the final state of the work belongs to the session that produced it.
    let prov = ff_core::Provenance::new("pre", Some("ff session end".into()));
    crate::capture::pre_best_effort(&prov);

    let was_open = session::read_current(repo).is_some();
    session::remove_marker(repo)?;

    if json {
        let payload = serde_json::json!({ "name": serde_json::Value::Null });
        crate::machine::emit("session", &payload)?;
    } else if was_open {
        println!("session ended");
    } else {
        println!("no session was open");
    }
    Ok(())
}

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn elapsed_human(started: u64) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let secs = now.saturating_sub(started);
    if secs < 60 {
        format!("{secs}s ago")
    } else if secs < 3600 {
        format!("{}m ago", secs / 60)
    } else if secs < 86400 {
        format!("{}h ago", secs / 3600)
    } else {
        format!("{}d ago", secs / 86400)
    }
}
