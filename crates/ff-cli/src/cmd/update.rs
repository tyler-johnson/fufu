//! `ff update` — work out what owns this binary and name the one command
//! that updates it, running that command only on `-y` or a typed yes.
//! `--check` is the background lane: refresh the update cache, print
//! nothing.
//!
//! A source build is named as one to rebuild and nothing more. Otherwise
//! the channel is read from where the binary sits — a Homebrew prefix, the
//! directory the install script places binaries in, else anywhere — and
//! the recipe for that channel is printed. The install script's recipe is
//! the one channel that runs, after `-y` or a typed yes, and with nobody
//! there to ask the printed command is the whole answer.
//!
//! `-y` asks for the move without a prompt, and a channel fufu does not
//! drive is named as it is passed with the exit 1 at the end, so a script
//! that asked for the move is not told it happened. The install script
//! ends by running `ff hook -u` through the binary it placed, so the
//! wiring follows the move without a step here.

use crate::ctx::Ctx;
use crate::selfupdate::{self, InstallKind};

pub fn run(_ctx: &Ctx, check: bool, yes: bool) -> ff_core::Result<()> {
    if check {
        return refresh_cache();
    }

    // What `-y` asked for and could not have: a line here rather than a
    // stop, and the exit at the end says the run was not whole.
    let mut trouble: Vec<String> = Vec::new();

    fufu(yes, &mut trouble)?;

    match trouble.len() {
        0 => Ok(()),
        1 => Err(ff_core::Error::msg(trouble.remove(0))),
        _ => Err(ff_core::Error::msg(format!(
            "not everything moved:\n  {}",
            trouble.join("\n  ")
        ))),
    }
}

/// fufu's own step, unchanged: the four channels, and the install script's
/// path is the one that runs.
fn fufu(yes: bool, trouble: &mut Vec<String>) -> ff_core::Result<()> {
    let exe = selfupdate::resolve_exe()?;
    match selfupdate::classify_install(&exe, selfupdate::OFFICIAL) {
        InstallKind::Source => elsewhere(
            "ff was built from source",
            selfupdate::CARGO_INSTALL,
            yes,
            trouble,
        ),
        InstallKind::Homebrew => elsewhere(
            "ff was installed with Homebrew",
            selfupdate::BREW_UPGRADE,
            yes,
            trouble,
        ),
        InstallKind::Unmanaged => elsewhere(
            "ff was installed by something else — whatever placed this binary replaces it",
            selfupdate::RELEASES_URL,
            yes,
            trouble,
        ),
        InstallKind::Script => script(yes)?,
    }
    Ok(())
}

/// The three channels fufu does not drive. Printing is the whole job; `-y`
/// asked for an update that cannot happen here, so the run fails at its
/// end instead.
fn elsewhere(why: &str, how: &str, yes: bool, trouble: &mut Vec<String>) {
    if yes {
        trouble.push(format!("{why} — update with: {how}"));
    }
    println!("{why}.");
    println!("update with:");
    println!("  {how}");
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

    if !go(yes)? {
        return Ok(());
    }
    selfupdate::run_installer(&cmd)
}

/// Whether to run the command just printed. Not `-y`, and nobody there to
/// ask: printing the command is the whole answer, and it is not a failure.
fn go(yes: bool) -> ff_core::Result<bool> {
    if yes {
        return Ok(true);
    }
    if crate::machine::interactive() {
        return crate::machine::confirm("run it now?");
    }
    Ok(false)
}

/// The background lane's refresh: fufu's own latest release. Every failure
/// is silent, and a lookup that fails leaves the entry as it was: the next
/// check is one cadence away.
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

    let agent = selfupdate::github::agent();
    if let Ok(release) = selfupdate::github::fetch_latest(&agent, "https://api.github.com")
        && selfupdate::parse_tag(&release.tag_name).is_some()
    {
        state.latest = Some(release.tag_name);
    }
    let _ = selfupdate::notify::save_state(&path, &state);
    Ok(())
}
