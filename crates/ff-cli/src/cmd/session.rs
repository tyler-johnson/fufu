//! `ff session` — open, close, and inspect capture sessions.

use ff_core::Result;
use ff_core::gix;

use crate::session;

/// Run the session command. `action` is `None` for bare `ff session`,
/// `Some("start")` for `ff session start`, `Some("end")` for `ff session end`.
pub fn run(action: Option<&str>, name: Option<String>, json: bool) -> Result<()> {
    let repo = ff_core::discover(".")?;

    match action {
        None => status(&repo, json),
        Some("start") => start(&repo, name, json),
        Some("end") => end(&repo, json),
        Some(other) => Err(ff_core::Error::msg(format!(
            "unknown session action: {other}"
        ))),
    }
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
