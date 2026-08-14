//! `ff update` — download the latest release and replace this binary.
//! `--check` is the background lane: refresh the update cache, print nothing.

use crate::selfupdate;

pub fn run(check: bool) -> ff_core::Result<()> {
    if check {
        return refresh_cache();
    }

    let exe = selfupdate::swap::resolve_exe()?;
    let updater = selfupdate::Updater {
        api_base: "https://api.github.com".into(),
        exe,
        current_version: env!("CARGO_PKG_VERSION").into(),
        official: selfupdate::OFFICIAL,
    };

    match updater.run()? {
        selfupdate::Outcome::UpToDate { current } => {
            println!("already up to date (v{current})");
        }
        selfupdate::Outcome::Updated { from, to } => {
            println!("updated {from} → {to}");
        }
    }
    Ok(())
}

fn refresh_cache() -> ff_core::Result<()> {
    let Some(path) = selfupdate::notify::state_path() else {
        return Ok(());
    };

    let mut state = selfupdate::notify::load_state(&path);

    // Stamp checked_at = now
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    state.checked_at = now;

    // Re-read cadence if we can discover a repo
    if let Ok(repo) = ff_core::discover(".") {
        state.interval_secs =
            selfupdate::notify::read_cadence_encoded(repo.config_snapshot().plumbing());
    }
    let _ = selfupdate::notify::save_state(&path, &state);

    // Fetch latest — failures are silent
    let _ = (|| -> Result<_, Box<dyn std::error::Error + Send + Sync>> {
        let agent = selfupdate::github::agent();
        let release = selfupdate::github::fetch_latest(&agent, "https://api.github.com")?;
        if selfupdate::parse_tag(&release.tag_name).is_some() {
            state.latest = Some(release.tag_name);
        }
        let _ = selfupdate::notify::save_state(&path, &state);
        Ok(())
    })();
    // Every failure is silent
    Ok(())
}
