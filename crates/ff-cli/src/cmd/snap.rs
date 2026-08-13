//! Bare `ff` — the snapshot verb. `ff [-m <msg>]` takes a manual snapshot.

use ff_core::{Error, EvologOptions, Provenance, Result, SnapOutcome};

fn branch_of(r#ref: &str) -> &str {
    r#ref.strip_prefix("refs/fufu/snap/").unwrap_or(r#ref)
}

pub fn run(message: Option<String>, json: bool) -> Result<()> {
    let repo = ff_core::discover(".")?;
    let outcome = ff_core::take(&repo, &Provenance::new("manual", message))?;
    match &outcome {
        SnapOutcome::Created {
            id,
            short_id,
            r#ref,
            ..
        } => {
            let branch = branch_of(r#ref);
            if json {
                let body = serde_json::to_string(&serde_json::json!({
                    "outcome": "created",
                    "id": id,
                    "short_id": short_id,
                    "branch": branch,
                }))
                .map_err(Error::repo)?;
                println!("{body}");
            } else {
                println!("snapshot {short_id} on {branch}");
                println!();
                let rows = ff_core::evolog(
                    &repo,
                    &EvologOptions {
                        limit: Some(3),
                        ..Default::default()
                    },
                )?;
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs() as i64)
                    .unwrap_or(0);
                print!("{}", crate::render::timeline_human(&rows, now));
            }
        }
        SnapOutcome::NoOp { r#ref, .. } => {
            let branch = branch_of(r#ref);
            if json {
                let body = serde_json::to_string(&serde_json::json!({
                    "outcome": "noop",
                    "branch": branch,
                }))
                .map_err(Error::repo)?;
                println!("{body}");
            } else {
                println!("no changes since the last snapshot on {branch}");
            }
        }
        SnapOutcome::Contended { .. } => {
            if json {
                println!(r#"{{"outcome":"contended"}}"#);
            } else {
                println!("snapshot skipped: a concurrent ff snapshot is in progress");
            }
        }
    }
    Ok(())
}
