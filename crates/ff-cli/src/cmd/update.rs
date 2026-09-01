//! `ff update` — work out what owns this binary and name the one command
//! that updates it, running that command only on `-y` or a typed yes.
//! `--check` is the background lane: refresh the update cache, print nothing.

use crate::ctx::Ctx;
use crate::selfupdate::{self, InstallKind};

pub fn run(_ctx: &Ctx, check: bool, yes: bool) -> ff_core::Result<()> {
    if check {
        return refresh_cache();
    }

    let exe = selfupdate::resolve_exe()?;
    match selfupdate::classify_install(&exe, selfupdate::OFFICIAL) {
        InstallKind::Source => {
            elsewhere("ff was built from source", selfupdate::CARGO_INSTALL, yes)
        }
        InstallKind::Homebrew => elsewhere(
            "ff was installed with Homebrew",
            selfupdate::BREW_UPGRADE,
            yes,
        ),
        InstallKind::Unmanaged => elsewhere(
            "ff was installed by something else — whatever placed this binary replaces it",
            selfupdate::RELEASES_URL,
            yes,
        ),
        InstallKind::Script => script(yes),
    }
}

/// The three channels fufu does not drive. Printing is the whole job; `-y`
/// asked for an update that cannot happen here, so it fails instead.
fn elsewhere(why: &str, how: &str, yes: bool) -> ff_core::Result<()> {
    if yes {
        return Err(ff_core::Error::msg(format!("{why} — update with: {how}")));
    }
    println!("{why}.");
    println!("update with:");
    println!("  {how}");
    Ok(())
}

/// The install script's own path: the one channel `ff update` can act on.
fn script(yes: bool) -> ff_core::Result<()> {
    let current_version = env!("CARGO_PKG_VERSION");
    let current = selfupdate::parse_semver(current_version).ok_or_else(|| {
        ff_core::Error::msg(format!(
            "cannot parse current version \"{current_version}\""
        ))
    })?;

    let agent = selfupdate::github::agent();
    let release = selfupdate::github::fetch_latest(&agent, "https://api.github.com")?;
    let latest = selfupdate::parse_tag(&release.tag_name).ok_or_else(|| {
        ff_core::Error::msg(format!("unexpected release tag \"{}\"", release.tag_name))
    })?;

    if latest <= current {
        println!("already up to date (v{current_version})");
        return Ok(());
    }

    let cmd = selfupdate::install_command();
    println!(
        "ff {} is available (running v{current_version}).",
        release.tag_name
    );
    println!("update with:");
    println!("  {cmd}");

    // Not `-y`, and nobody there to ask: printing the command is the whole
    // answer, and it is not a failure.
    let go = if yes {
        true
    } else if crate::machine::interactive() {
        crate::machine::confirm("run it now?")?
    } else {
        false
    };
    if !go {
        return Ok(());
    }

    selfupdate::run_installer(&cmd)
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
            crate::cadence::read_encoded(repo.config_snapshot().plumbing(), "fufu.updateCheck");
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
