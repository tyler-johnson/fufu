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

/// Canonicalize `path` the one way both sides of [`classify_install_at`]
/// are resolved. On windows `std::fs::canonicalize` returns a verbatim
/// `\\?\` path, which never compares equal to the ordinary spelling of the
/// same file, so it is turned back into that spelling.
pub(crate) fn canonical(path: &std::path::Path) -> std::io::Result<std::path::PathBuf> {
    let path = std::fs::canonicalize(path)?;
    #[cfg(windows)]
    let path = unverbatim(&path);
    Ok(path)
}

/// The ordinary spelling of a verbatim path: `\\?\C:\x` is `C:\x` and
/// `\\?\UNC\server\share\x` is `\\server\share\x`. Any other path comes back
/// as it is, including verbatim forms like `\\?\Volume{…}` that have no
/// ordinary spelling.
#[cfg(windows)]
fn unverbatim(path: &std::path::Path) -> std::path::PathBuf {
    use std::path::{Component, Prefix};

    let mut components = path.components();
    let Some(Component::Prefix(prefix)) = components.next() else {
        return path.to_path_buf();
    };
    let mut out = match prefix.kind() {
        Prefix::VerbatimDisk(letter) => std::path::PathBuf::from(format!("{}:", letter as char)),
        Prefix::VerbatimUNC(server, share) => {
            let mut unc = std::ffi::OsString::from(r"\\");
            unc.push(server);
            unc.push(r"\");
            unc.push(share);
            std::path::PathBuf::from(unc)
        }
        _ => return path.to_path_buf(),
    };
    out.extend(components);
    out
}

/// Resolve the path of the running executable through [`canonical`].
pub fn resolve_exe() -> ff_core::Result<std::path::PathBuf> {
    let path = std::env::current_exe()
        .map_err(|err| ff_core::Error::msg(format!("cannot locate the running binary: {err}")))?;
    canonical(&path)
        .map_err(|err| ff_core::Error::msg(format!("cannot locate the running binary: {err}")))
}

/// Where the install script puts the binary: `$HOME/.local/bin/ff` on unix
/// (`install.sh:12`), `%LOCALAPPDATA%\Programs\ff\ff.exe` on windows
/// (`install.ps1:11`). Resolved through [`canonical`] when it exists, the
/// same as [`resolve_exe`], so the comparison in [`classify_install_at`] is
/// between two paths spelled the same way.
///
/// `FF_INSTALL_DIR` is deliberately not consulted: an env var that happens
/// to be exported is not evidence of how this binary got here.
pub fn script_install_path() -> Option<std::path::PathBuf> {
    #[cfg(windows)]
    let root = std::env::var_os("LOCALAPPDATA")?;
    #[cfg(not(windows))]
    let root = std::env::var_os("HOME")?;
    if root.is_empty() {
        return None;
    }
    Some(script_install_path_under(std::path::Path::new(&root)))
}

/// [`script_install_path`] with its root injected: `LOCALAPPDATA` on
/// windows, `HOME` everywhere else.
fn script_install_path_under(root: &std::path::Path) -> std::path::PathBuf {
    #[cfg(windows)]
    let path = root.join("Programs").join("ff").join("ff.exe");
    #[cfg(not(windows))]
    let path = root.join(".local").join("bin").join("ff");

    canonical(&path).unwrap_or(path)
}

/// The testable core: classify with the install-script path injected.
///
/// Both paths are expected to have gone through [`canonical`] already; an
/// empty `script_path` (no HOME) matches nothing.
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
/// type it.
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
    // canonical — both sides of the comparison spelled the same way
    // ------------------------------------------------------------------

    #[test]
    fn classify_a_real_file_against_itself_is_script() {
        let tmp = tempfile::TempDir::new().unwrap();
        let exe = tmp.path().join("ff");
        std::fs::write(&exe, b"").unwrap();
        assert_eq!(
            classify_install_at(&canonical(&exe).unwrap(), true, &canonical(&exe).unwrap()),
            InstallKind::Script,
        );
    }

    #[test]
    fn canonical_resolves_a_parent_component() {
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::create_dir(tmp.path().join("sub")).unwrap();
        let exe = tmp.path().join("ff");
        std::fs::write(&exe, b"").unwrap();
        let roundabout = tmp.path().join("sub").join("..").join("ff");
        assert_eq!(canonical(&roundabout).unwrap(), canonical(&exe).unwrap());
    }

    #[test]
    fn script_install_path_under_a_root_without_the_file_is_the_join() {
        let tmp = tempfile::TempDir::new().unwrap();
        let path = script_install_path_under(tmp.path());
        assert!(path.starts_with(tmp.path()), "{}", path.display());
        assert!(!path.exists(), "{}", path.display());
    }

    #[cfg(windows)]
    #[test]
    fn canonical_is_not_verbatim_on_windows() {
        let tmp = tempfile::TempDir::new().unwrap();
        let exe = tmp.path().join("ff.exe");
        std::fs::write(&exe, b"").unwrap();
        let resolved = canonical(&exe).unwrap();
        assert!(
            !resolved.to_string_lossy().starts_with(r"\\?\"),
            "{}",
            resolved.display()
        );
    }

    #[cfg(windows)]
    #[test]
    fn classify_the_install_ps1_path_is_script_on_windows() {
        // #21: the exe was stripped of `\\?\` and the script path was not.
        let tmp = tempfile::TempDir::new().unwrap();
        let dir = tmp.path().join("Programs").join("ff");
        std::fs::create_dir_all(&dir).unwrap();
        let exe = dir.join("ff.exe");
        std::fs::write(&exe, b"").unwrap();

        assert_eq!(
            classify_install_at(&canonical(&exe).unwrap(), true, &canonical(&exe).unwrap()),
            InstallKind::Script,
        );
        assert_eq!(
            classify_install_at(
                &canonical(&exe).unwrap(),
                true,
                &script_install_path_under(tmp.path())
            ),
            InstallKind::Script,
        );
    }

    #[cfg(windows)]
    #[test]
    fn unverbatim_gives_the_ordinary_spelling() {
        assert_eq!(unverbatim(Path::new(r"\\?\C:\a\b")).as_os_str(), r"C:\a\b");
        assert_eq!(
            unverbatim(Path::new(r"\\?\UNC\srv\share\a")).as_os_str(),
            r"\\srv\share\a"
        );
        assert_eq!(unverbatim(Path::new(r"C:\a\b")).as_os_str(), r"C:\a\b");
        let volume = r"\\?\Volume{0b1c2d3e-0000-0000-0000-000000000000}\a";
        assert_eq!(unverbatim(Path::new(volume)).as_os_str(), volume);
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
