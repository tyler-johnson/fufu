//! Self-update: the explicit `ff update` dispatcher plus the passive check lane.
//!
//! fufu never writes an `ff` binary itself. The shell installer is the one
//! thing that does, and `ff update` is a dispatcher over how this copy got
//! here: it names the command that owns this binary and, for the install
//! script's own path, offers to run it.

pub mod github;
pub mod notify;

/// Release builds set FF_OFFICIAL_BUILD in CI; everything else — dev,
/// dogfood, test binaries — is unofficial and never self-updates.
pub const OFFICIAL: bool = option_env!("FF_OFFICIAL_BUILD").is_some();

/// The install script the `Script` channel runs — one per platform, since
/// each platform has one installer and never the other.
#[cfg(not(windows))]
pub const INSTALL_URL: &str =
    "https://raw.githubusercontent.com/tyler-johnson/fufu/main/install.sh";
#[cfg(windows)]
pub const INSTALL_URL: &str =
    "https://raw.githubusercontent.com/tyler-johnson/fufu/main/install.ps1";
pub const RELEASES_URL: &str = "https://github.com/tyler-johnson/fufu/releases/latest";
pub const CARGO_INSTALL: &str = "cargo install --git https://github.com/tyler-johnson/fufu ff-cli";
pub const BREW_UPGRADE: &str = "brew upgrade fufu";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Version(pub u64, pub u64, pub u64);

/// Parse a bare semver string (`major.minor.patch`) into a [`Version`].
///
/// Rejects anything with pre-release suffixes, metadata, or non-numeric parts.
pub fn parse_semver(s: &str) -> Option<Version> {
    let parts: Vec<&str> = s.split('.').collect();
    if parts.len() != 3 {
        return None;
    }
    let [a, b, c] = parts[..] else { return None };
    if a.is_empty() || b.is_empty() || c.is_empty() {
        return None;
    }
    if !a.chars().all(|ch| ch.is_ascii_digit())
        || !b.chars().all(|ch| ch.is_ascii_digit())
        || !c.chars().all(|ch| ch.is_ascii_digit())
    {
        return None;
    }
    let major = a.parse::<u64>().ok()?;
    let minor = b.parse::<u64>().ok()?;
    let patch = c.parse::<u64>().ok()?;
    Some(Version(major, minor, patch))
}

/// Parse a git-tag string (`v<major>.<minor>.<patch>`) into a [`Version`].
///
/// Returns `None` if the tag lacks a leading `v` or the version is malformed.
pub fn parse_tag(tag: &str) -> Option<Version> {
    let bare = tag.strip_prefix('v')?;
    parse_semver(bare)
}

/// Parse a declared extension's release tag: `v1.2.3` or a bare `1.2.3`.
/// fufu's own tags carry the `v` and [`parse_tag`] insists on it; an
/// extension tags releases its own way, and either spelling is a version.
pub fn parse_release(tag: &str) -> Option<Version> {
    parse_tag(tag).or_else(|| parse_semver(tag))
}

/// How this copy of fufu got onto the machine — and therefore what owns
/// updating it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstallKind {
    /// Built here: dev, dogfood, or a test binary. `cargo install`.
    Source,
    /// Under a Homebrew prefix. `brew upgrade fufu`.
    Homebrew,
    /// Sitting exactly where the install script puts it. The only kind
    /// `ff update -y` will act on.
    Script,
    /// An official build somewhere else — mise, nix, `/usr/local/bin`, a
    /// hand copy. Whatever placed it owns replacing it.
    Unmanaged,
}

/// Resolve the path of the running executable, canonicalizing symlinks.
pub fn resolve_exe() -> ff_core::Result<std::path::PathBuf> {
    let path = std::env::current_exe()
        .map_err(|err| ff_core::Error::msg(format!("cannot locate the running binary: {err}")))?;
    let path = path
        .canonicalize()
        .map_err(|err| ff_core::Error::msg(format!("cannot locate the running binary: {err}")))?;

    #[cfg(windows)]
    {
        let s = path.to_string_lossy();
        if let Some(stripped) = s.strip_prefix(r"\\?\") {
            return Ok(std::path::PathBuf::from(stripped));
        }
    }

    Ok(path)
}

/// Where the install script puts the binary: `$HOME/.local/bin/ff` on unix
/// (`install.sh:12`), `%LOCALAPPDATA%\Programs\ff\ff.exe` on windows
/// (`install.ps1:11`). Canonicalized when it exists, so the comparison in
/// [`classify_install_at`] is between two resolved paths.
///
/// `FF_INSTALL_DIR` is deliberately not consulted: an env var that happens
/// to be exported is not evidence of how this binary got here.
pub fn script_install_path() -> Option<std::path::PathBuf> {
    #[cfg(windows)]
    let path = {
        let local = std::env::var_os("LOCALAPPDATA")?;
        if local.is_empty() {
            return None;
        }
        std::path::PathBuf::from(local)
            .join("Programs")
            .join("ff")
            .join("ff.exe")
    };
    #[cfg(not(windows))]
    let path = {
        let home = std::env::var_os("HOME")?;
        if home.is_empty() {
            return None;
        }
        std::path::PathBuf::from(home)
            .join(".local")
            .join("bin")
            .join("ff")
    };

    Some(path.canonicalize().unwrap_or(path))
}

/// The testable core: classify with the install-script path injected.
///
/// Both paths are expected to be canonicalized already; an empty
/// `script_path` (no HOME) matches nothing.
pub fn classify_install_at(
    exe: &std::path::Path,
    official: bool,
    script_path: &std::path::Path,
) -> InstallKind {
    if !official {
        return InstallKind::Source;
    }
    let path = exe.to_string_lossy();
    if path.contains("/Cellar/")
        || path.contains("/opt/homebrew/")
        || path.contains("/home/linuxbrew/")
    {
        return InstallKind::Homebrew;
    }
    if !script_path.as_os_str().is_empty() && exe == script_path {
        return InstallKind::Script;
    }
    InstallKind::Unmanaged
}

/// Classify how fufu was installed based on the executable path and build flag.
pub fn classify_install(exe: &std::path::Path, official: bool) -> InstallKind {
    let script = script_install_path().unwrap_or_default();
    classify_install_at(exe, official, &script)
}

/// Where an extension's install script places its binary when its manifest
/// does not say: the directory fufu's own script uses on unix.
pub const DEFAULT_EXTENSION_BIN: &str = "~/.local/bin";

/// The directory an extension's install script places its binary in, from
/// the manifest's `update.bin` or [`DEFAULT_EXTENSION_BIN`], with a leading
/// `~` read as `HOME`. Canonicalized when it exists, so the comparison in
/// [`classify_extension_at`] is between two resolved paths. `None` when
/// the directory needs a home and the environment names none, which
/// matches nothing.
pub fn extension_bin_dir(bin: Option<&str>) -> Option<std::path::PathBuf> {
    let home = std::env::var_os("HOME").filter(|home| !home.is_empty());
    let path = extension_bin_dir_at(bin, home.as_deref().map(std::path::Path::new))?;
    Some(path.canonicalize().unwrap_or(path))
}

/// The testable core of [`extension_bin_dir`]: the shape of the directory
/// with the home injected and nothing canonicalized.
pub fn extension_bin_dir_at(
    bin: Option<&str>,
    home: Option<&std::path::Path>,
) -> Option<std::path::PathBuf> {
    let bin = bin.unwrap_or(DEFAULT_EXTENSION_BIN);
    match bin.strip_prefix('~') {
        Some(rest) if rest.is_empty() || rest.starts_with('/') || rest.starts_with('\\') => {
            let home = home?;
            let rest = rest.trim_start_matches(['/', '\\']);
            Some(if rest.is_empty() {
                home.to_path_buf()
            } else {
                home.join(rest)
            })
        }
        // `~user` is not fufu's to expand: taken as a path, as a shell
        // without that expansion would take it.
        _ => Some(std::path::PathBuf::from(bin)),
    }
}

/// The testable core of an extension's channel: where its binary sits,
/// against the directory its install script places binaries in.
///
/// `official` is what the binary said about itself, and a source build is
/// a source build wherever it sits. A Homebrew prefix is read the way it is
/// for fufu. `Script` is the binary sitting in `bin`, and only when the
/// manifest carries an `install` recipe — without a script nothing places
/// binaries anywhere, so `bin` means nothing and a hand copy at
/// `~/.local/bin` is a hand copy. Everything else is `Unmanaged`, which
/// the walk answers with the releases page.
///
/// Both paths are expected to be canonicalized already; an empty `bin` (no
/// HOME) matches nothing.
pub fn classify_extension_at(
    exe: &std::path::Path,
    official: bool,
    has_install: bool,
    bin: &std::path::Path,
) -> InstallKind {
    if !official {
        return InstallKind::Source;
    }
    let path = exe.to_string_lossy();
    if path.contains("/Cellar/")
        || path.contains("/opt/homebrew/")
        || path.contains("/home/linuxbrew/")
    {
        return InstallKind::Homebrew;
    }
    if has_install && !bin.as_os_str().is_empty() && exe.parent() == Some(bin) {
        return InstallKind::Script;
    }
    InstallKind::Unmanaged
}

/// The block's recipe for a channel, as a person would type it, or `None`
/// when the block has none for that channel. `ff update` prints it, and
/// the passive notice names it.
pub fn recipe_for(block: &crate::manifest::Update, kind: InstallKind) -> Option<String> {
    match kind {
        InstallKind::Homebrew => block
            .brew
            .as_deref()
            .map(|formula| format!("brew upgrade {formula}")),
        InstallKind::Script => block.install.as_deref().map(install_command_for),
        InstallKind::Unmanaged => block.releases.clone(),
        InstallKind::Source => None,
    }
}

/// The GitHub repository the passive lane checks a declared extension's
/// releases against, or `None` when the extension gets no check: a
/// `source` build, a manifest with no `releases` recipe, or a page on a
/// host other than github.com. This is the one gate; the refresh and the
/// notice both read it.
pub fn release_repo(manifest: &crate::manifest::Manifest) -> Option<String> {
    if manifest.build() == crate::manifest::Build::Source {
        return None;
    }
    github::repo_from_url(manifest.update.as_ref()?.releases.as_deref()?)
}

/// Is `name` an executable on PATH? A scan, not a spawn — the zero-spawn
/// proof holds while `ff update` is only deciding what to print.
#[cfg(not(windows))]
fn on_path(name: &str) -> bool {
    let Some(path) = std::env::var_os("PATH") else {
        return false;
    };
    std::env::split_paths(&path).any(|dir| dir.join(name).is_file())
}

/// The one command that updates a `Script` install, as a person would type it.
pub fn install_command() -> String {
    install_command_for(INSTALL_URL)
}

/// The command that runs an install script at `url`, as a person would
/// type it: fufu's own, or a declared extension's `install` recipe.
pub fn install_command_for(url: &str) -> String {
    #[cfg(windows)]
    {
        format!("irm {url} | iex")
    }
    #[cfg(not(windows))]
    {
        if !on_path("curl") && on_path("wget") {
            format!("wget -qO- {url} | sh")
        } else {
            format!("curl -fsSL {url} | sh")
        }
    }
}

/// Run the install command, stdio inherited so its own progress and
/// checksum verification reach the user.
///
/// This is a sanctioned spawn: an absolute interpreter path, running the
/// string [`install_command`] just printed, only after an explicit `-y` or
/// a typed yes.
pub fn run_installer(cmd: &str) -> ff_core::Result<()> {
    #[cfg(windows)]
    let mut command = {
        let mut c = std::process::Command::new("powershell");
        c.args(["-NoProfile", "-Command", cmd]);
        c
    };
    #[cfg(not(windows))]
    let mut command = {
        let mut c = std::process::Command::new("/bin/sh");
        c.args(["-c", cmd]);
        c
    };

    let status = command
        .stdin(std::process::Stdio::inherit())
        .stdout(std::process::Stdio::inherit())
        .stderr(std::process::Stdio::inherit())
        .status()
        .map_err(|err| ff_core::Error::msg(format!("cannot run the installer: {err}")))?;

    if !status.success() {
        return Err(ff_core::Error::msg(format!(
            "the installer failed ({status}) — run it yourself: {cmd}"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn parse_tag_valid() {
        assert_eq!(parse_tag("v0.1.0"), Some(Version(0, 1, 0)));
        assert!(parse_tag("v1.10.0") > parse_tag("v1.2.3"));
    }

    #[test]
    fn parse_tag_rejects_bad_input() {
        assert!(parse_tag("0.1.0").is_none()); // no v
        assert!(parse_tag("v1.2").is_none());
        assert!(parse_tag("v1.2.3.4").is_none());
        assert!(parse_tag("v1.2.3-rc1").is_none());
        assert!(parse_tag("v1.+2.3").is_none());
        assert!(parse_tag("").is_none());
    }

    #[test]
    fn parse_semver_bare() {
        assert_eq!(parse_semver("0.1.0"), Some(Version(0, 1, 0)));
    }

    #[test]
    fn parse_release_takes_either_spelling() {
        assert_eq!(parse_release("v0.5.0"), Some(Version(0, 5, 0)));
        assert_eq!(parse_release("0.5.0"), Some(Version(0, 5, 0)));
        assert!(parse_release("v0.5.0-rc1").is_none());
        assert!(parse_release("release-1").is_none());
    }

    /// The gate on an extension's release check: an official build with a
    /// github.com releases page, and nothing else.
    #[test]
    fn release_repo_is_the_github_page_of_an_official_build() {
        let manifest = |update: serde_json::Value, build: Option<&str>| {
            let mut value = serde_json::json!({
                "name": "tower",
                "version": "0.4.1",
                "contract": crate::machine::CONTRACT,
                "verbs": [{"name": "board", "read_only": true}],
                "undoable": true,
            });
            if !update.is_null() {
                value["update"] = update;
            }
            if let Some(build) = build {
                value["build"] = serde_json::Value::String(build.into());
            }
            crate::manifest::parse(value).expect("a manifest the page types")
        };
        let page = "https://github.com/tyler-johnson/tower/releases/latest";
        assert_eq!(
            release_repo(&manifest(serde_json::json!({"releases": page}), None)).as_deref(),
            Some("tyler-johnson/tower")
        );
        assert_eq!(
            release_repo(&manifest(
                serde_json::json!({"releases": page}),
                Some("official")
            ))
            .as_deref(),
            Some("tyler-johnson/tower")
        );
        // A source build is checked against nothing, whatever the page.
        assert_eq!(
            release_repo(&manifest(
                serde_json::json!({"releases": page}),
                Some("source")
            )),
            None
        );
        // No block, and a block with no releases recipe.
        assert_eq!(release_repo(&manifest(serde_json::Value::Null, None)), None);
        assert_eq!(
            release_repo(&manifest(
                serde_json::json!({"brew": "tyler-johnson/tap/tower"}),
                None
            )),
            None
        );
        // A page somewhere fufu has no API for.
        assert_eq!(
            release_repo(&manifest(
                serde_json::json!({"releases": "https://gitlab.com/tyler-johnson/tower/-/releases"}),
                None
            )),
            None
        );
    }

    // ------------------------------------------------------------------
    // classify_install_at — the four channels, with the script path
    // injected so the matrix is hermetic
    // ------------------------------------------------------------------

    const SCRIPT: &str = "/home/u/.local/bin/ff";

    #[test]
    fn classify_source_beats_every_path() {
        // Unofficial wins over a path that would otherwise be Script or brew.
        assert_eq!(
            classify_install_at(Path::new(SCRIPT), false, Path::new(SCRIPT)),
            InstallKind::Source,
        );
        assert_eq!(
            classify_install_at(Path::new("/opt/homebrew/bin/ff"), false, Path::new(SCRIPT)),
            InstallKind::Source,
        );
    }

    #[test]
    fn classify_homebrew_prefixes() {
        for path in [
            "/opt/homebrew/bin/ff",
            "/home/linuxbrew/.linuxbrew/bin/ff",
            "/usr/local/Cellar/fufu/0.1.0/bin/ff",
        ] {
            assert_eq!(
                classify_install_at(Path::new(path), true, Path::new(SCRIPT)),
                InstallKind::Homebrew,
                "{path}",
            );
        }
    }

    #[test]
    fn classify_script_is_exact() {
        assert_eq!(
            classify_install_at(Path::new(SCRIPT), true, Path::new(SCRIPT)),
            InstallKind::Script,
        );
        // A sibling in the same directory is not the install script's path.
        assert_eq!(
            classify_install_at(Path::new("/home/u/.local/bin/ff2"), true, Path::new(SCRIPT)),
            InstallKind::Unmanaged,
        );
    }

    #[test]
    fn classify_unmanaged_everywhere_else() {
        for path in ["/usr/local/bin/ff", "/nix/store/abc-ff/bin/ff", "/tmp/ff"] {
            assert_eq!(
                classify_install_at(Path::new(path), true, Path::new(SCRIPT)),
                InstallKind::Unmanaged,
                "{path}",
            );
        }
    }

    #[test]
    fn classify_without_a_script_path_never_matches() {
        // No HOME: the empty path must not classify an empty exe as Script,
        // and everything official stays Unmanaged.
        assert_eq!(
            classify_install_at(Path::new(""), true, Path::new("")),
            InstallKind::Unmanaged,
        );
        assert_eq!(
            classify_install_at(Path::new(SCRIPT), true, Path::new("")),
            InstallKind::Unmanaged,
        );
    }

    // ------------------------------------------------------------------
    // classify_extension_at — the same channels read for a declared
    // extension, with the manifest's bin directory injected
    // ------------------------------------------------------------------

    const BIN: &str = "/home/u/.local/bin";

    #[test]
    fn extension_source_beats_every_path() {
        assert_eq!(
            classify_extension_at(
                Path::new("/home/u/.local/bin/ff-tower"),
                false,
                true,
                Path::new(BIN)
            ),
            InstallKind::Source,
        );
        assert_eq!(
            classify_extension_at(
                Path::new("/opt/homebrew/bin/ff-tower"),
                false,
                true,
                Path::new(BIN)
            ),
            InstallKind::Source,
        );
    }

    #[test]
    fn extension_homebrew_prefixes() {
        for path in [
            "/opt/homebrew/bin/ff-tower",
            "/home/linuxbrew/.linuxbrew/bin/ff-tower",
            "/usr/local/Cellar/tower/0.4.1/bin/ff-tower",
        ] {
            assert_eq!(
                classify_extension_at(Path::new(path), true, true, Path::new(BIN)),
                InstallKind::Homebrew,
                "{path}",
            );
        }
    }

    /// The script channel is the binary's directory being the one the
    /// script places binaries in, and only when there is a script: the
    /// same directory with no install recipe is a hand copy.
    #[test]
    fn extension_script_is_the_bin_directory_with_an_install_recipe() {
        assert_eq!(
            classify_extension_at(
                Path::new("/home/u/.local/bin/ff-tower"),
                true,
                true,
                Path::new(BIN)
            ),
            InstallKind::Script,
        );
        assert_eq!(
            classify_extension_at(
                Path::new("/home/u/.local/bin/ff-tower"),
                true,
                false,
                Path::new(BIN)
            ),
            InstallKind::Unmanaged,
        );
        // A subdirectory is not the directory.
        assert_eq!(
            classify_extension_at(
                Path::new("/home/u/.local/bin/tower/ff-tower"),
                true,
                true,
                Path::new(BIN)
            ),
            InstallKind::Unmanaged,
        );
        // A manifest that names another directory is read there.
        assert_eq!(
            classify_extension_at(
                Path::new("/usr/local/bin/ff-tower"),
                true,
                true,
                Path::new("/usr/local/bin")
            ),
            InstallKind::Script,
        );
    }

    #[test]
    fn extension_unmanaged_everywhere_else() {
        for path in [
            "/usr/local/bin/ff-tower",
            "/nix/store/abc-tower/bin/ff-tower",
            "/tmp/ff-tower",
        ] {
            assert_eq!(
                classify_extension_at(Path::new(path), true, true, Path::new(BIN)),
                InstallKind::Unmanaged,
                "{path}",
            );
        }
        // No HOME: the empty bin matches nothing.
        assert_eq!(
            classify_extension_at(
                Path::new("/home/u/.local/bin/ff-tower"),
                true,
                true,
                Path::new("")
            ),
            InstallKind::Unmanaged,
        );
    }

    /// `~` is the home directory, absent is `~/.local/bin`, a bare path is
    /// taken as it is, and no home means no directory for a `~`.
    #[test]
    fn the_bin_directory_reads_a_tilde_as_home() {
        let home = Path::new("/home/u");
        let at = |bin: Option<&str>| extension_bin_dir_at(bin, Some(home));
        assert_eq!(at(None).as_deref(), Some(Path::new("/home/u/.local/bin")));
        assert_eq!(
            at(Some("~/.local/bin")).as_deref(),
            Some(Path::new("/home/u/.local/bin"))
        );
        assert_eq!(at(Some("~")).as_deref(), Some(home));
        assert_eq!(
            at(Some("~/bin/tower")).as_deref(),
            Some(Path::new("/home/u/bin/tower"))
        );
        assert_eq!(
            at(Some("/usr/local/bin")).as_deref(),
            Some(Path::new("/usr/local/bin"))
        );
        // `~user` is not fufu's to expand: taken as a relative path, as a
        // shell without that expansion would.
        assert_eq!(
            at(Some("~user/bin")).as_deref(),
            Some(Path::new("~user/bin"))
        );
        assert_eq!(extension_bin_dir_at(None, None), None);
        assert_eq!(extension_bin_dir_at(Some("~/bin"), None), None);
        assert_eq!(
            extension_bin_dir_at(Some("/usr/local/bin"), None).as_deref(),
            Some(Path::new("/usr/local/bin"))
        );
    }

    #[test]
    fn install_command_for_a_recipe_is_the_same_pipe() {
        let cmd = install_command_for("https://example.com/install.sh");
        assert!(cmd.contains("https://example.com/install.sh"), "{cmd}");
        #[cfg(not(windows))]
        assert!(cmd.ends_with(" | sh"), "{cmd}");
        #[cfg(windows)]
        assert!(cmd.ends_with(" | iex"), "{cmd}");
    }

    #[test]
    fn install_command_is_a_pipe_into_a_shell() {
        let cmd = install_command();
        assert!(cmd.contains(INSTALL_URL), "{cmd}");
        #[cfg(windows)]
        {
            assert!(cmd.starts_with("irm "), "{cmd}");
            assert!(cmd.ends_with(" | iex"), "{cmd}");
        }
        #[cfg(not(windows))]
        assert!(cmd.ends_with(" | sh"), "{cmd}");
    }
}
