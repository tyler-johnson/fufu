//! `ff update` — work out what owns this binary and name the one command
//! that updates it, running that command only on `-y` or a typed yes; then
//! the same over every declared extension, by the recipes its manifest
//! carries. `--check` is the background lane: refresh the update cache,
//! print nothing.
//!
//! The walk is fufu first, then each declared extension in registry order,
//! and the rules are the ones fufu applies to itself. A source build is
//! named as one to rebuild and nothing more. Otherwise the channel is read
//! from where the binary sits — a Homebrew prefix, the directory its
//! install script places binaries in, else anywhere — and the block's
//! recipe for that channel is printed. The install script's recipe is the
//! one channel that runs, after `-y` or a typed yes, and with nobody there
//! to ask the printed command is the whole answer. A channel with no
//! recipe, or a manifest with no block, is named as one fufu cannot move.
//!
//! `-y` is one answer for the whole walk: every install recipe runs without
//! asking, and anything the walk could not move — fufu on a channel it does
//! not drive, an extension with no recipe for its channel — is named as it
//! is passed and the exit is 1 at the end, so a script that asked for
//! everything to move is not told it did. After a move that ran, the walk
//! ends with `ff hook -u`, which re-asks every declared manifest and
//! re-records it before re-running the installs, so the skills on disk are
//! the ones the new binary names.

use crate::ctx::Ctx;
use crate::manifest::Build;
use crate::registry::Declared;
use crate::selfupdate::{self, InstallKind};

pub fn run(ctx: &Ctx, check: bool, yes: bool) -> ff_core::Result<()> {
    if check {
        return refresh_cache();
    }

    // What `-y` asked for and could not have, and what was run and failed:
    // each is a line here rather than a stop, so the walk reaches every
    // extension, and the exit at the end says the run was not whole.
    let mut trouble: Vec<String> = Vec::new();

    fufu(yes, &mut trouble)?;

    let registry = crate::registry::read();
    if let Some(why) = &registry.unreadable {
        eprintln!(
            "{}",
            crate::render::paint_warn(
                &format!("ff: the registry does not read as one: {why}"),
                crate::pager::color_enabled()
            )
        );
    }
    let mut moved = false;
    for declared in registry.declared() {
        println!();
        moved |= extension(declared, yes, &mut trouble)?;
    }

    // A move replaced a binary whose record names the skills it used to
    // have. `ff hook -u` is the refresh: the manifest pass re-asks and
    // re-records every declared extension, then the installs re-run from
    // the records. fufu's own move needs none of this here, because the
    // install script ends by running it through the binary it placed.
    if moved {
        println!();
        crate::integ::hook(ctx, Vec::new(), false, false, false, true, None)?;
    }

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

/// The three channels fufu does not drive, for fufu and for an extension
/// alike. Printing is the whole job; `-y` asked for an update that cannot
/// happen here, so the run fails at its end instead.
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

/// One declared extension, by the rules fufu applies to itself. Answers
/// whether a move ran.
///
/// No release is checked here. fufu's own script channel compares versions
/// before it prints, and an extension's `releases` recipe is a page rather
/// than an API, so the lookup that would say "already up to date" is not
/// this walk's; the install recipe is printed and offered as it stands.
fn extension(declared: &Declared, yes: bool, trouble: &mut Vec<String>) -> ff_core::Result<bool> {
    let name = declared.name();
    let label = format!("ff-{name} {}", declared.manifest.version);
    let unmoved = |trouble: &mut Vec<String>, why: String| {
        if yes {
            trouble.push(why);
        }
    };

    // The fresh PATH walk, as every run of a declared extension is: a
    // binary that moved directories is found, and one that was uninstalled
    // is named with the path its record carries.
    let Some(path) = declared.resolve() else {
        println!("{label} is not on PATH any more.");
        println!("  recorded at {}", declared.path.display());
        unmoved(
            trouble,
            format!("ff-{name} is not on PATH any more — ff extension -d {name} forgets it"),
        );
        return Ok(false);
    };
    let path = path.canonicalize().unwrap_or(path);

    // A source build is a source build wherever it sits, whatever the
    // block says, and whatever the language: only the binary knows how it
    // was built, and it said.
    if declared.manifest.build() == Build::Source {
        println!("{label} was built from source — rebuild it the way you built it.");
        unmoved(
            trouble,
            format!("ff-{name} was built from source — rebuild it the way you built it"),
        );
        return Ok(false);
    }

    let Some(block) = &declared.manifest.update else {
        println!("{label} is one fufu cannot move: its manifest carries no update recipes.");
        println!("  at {}", path.display());
        unmoved(
            trouble,
            format!("ff-{name} is one fufu cannot move: its manifest carries no update recipes"),
        );
        return Ok(false);
    };

    let bin = selfupdate::extension_bin_dir(block.bin.as_deref()).unwrap_or_default();
    let kind = selfupdate::classify_extension_at(&path, true, block.install.is_some(), &bin);
    let (why, recipe) = match kind {
        InstallKind::Homebrew => ("was installed with Homebrew", "brew"),
        InstallKind::Script => ("sits where its install script puts it", "install"),
        InstallKind::Unmanaged => (
            "was installed by something else — whatever placed this binary replaces it",
            "releases",
        ),
        // Ruled out above: the classifier was told the build is official.
        InstallKind::Source => unreachable!("a source build never reaches the block"),
    };
    let Some(how) = selfupdate::recipe_for(block, kind) else {
        println!("{label} {why}, and its manifest has no {recipe} recipe, so fufu cannot move it.");
        println!("  at {}", path.display());
        unmoved(
            trouble,
            format!("ff-{name} {why}, and its manifest has no {recipe} recipe"),
        );
        return Ok(false);
    };

    if kind != InstallKind::Script {
        elsewhere(&format!("{label} {why}"), &how, yes, trouble);
        return Ok(false);
    }

    println!("{label} {why}.");
    println!("update with:");
    println!("  {how}");
    if !go(yes)? {
        return Ok(false);
    }
    // The installer's failure is this extension's alone: the rest of the
    // walk goes on, and the exit at the end says so.
    match selfupdate::run_installer(&how) {
        Ok(()) => Ok(true),
        Err(err) => {
            eprintln!("ff: {err}");
            trouble.push(format!("ff-{name}: {err}"));
            Ok(false)
        }
    }
}

/// The background lane's refresh: fufu's own latest release, then one per
/// declared extension the gate in [`selfupdate::release_repo`] lets
/// through. Every failure is silent, and a lookup that fails leaves the
/// entry as it was: the next check is one cadence away.
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

    let registry = crate::registry::read();
    selfupdate::notify::refresh_extensions(&mut state, registry.declared(), |repo| {
        selfupdate::github::fetch_latest_of(&agent, "https://api.github.com", repo)
            .ok()
            .map(|release| release.tag_name)
    });
    let _ = selfupdate::notify::save_state(&path, &state);
    Ok(())
}
