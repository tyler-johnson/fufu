//! Self-update: the explicit `ff update` lane plus the passive check lane.

pub mod download;
pub mod extract;
pub mod github;
pub mod notify;
pub mod swap;

/// Release builds set FF_OFFICIAL_BUILD in CI; everything else — dev,
/// dogfood, test binaries — is unofficial and never self-updates.
pub const OFFICIAL: bool = option_env!("FF_OFFICIAL_BUILD").is_some();

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstallKind {
    Official,
    Homebrew,
    Source,
}

/// Classify how fufu was installed based on the executable path and build flag.
pub fn classify_install(exe: &std::path::Path, official: bool) -> InstallKind {
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
    InstallKind::Official
}

/// Build the release asset filename for a given version, OS, and architecture.
pub fn asset_name_for(version: &str, os: &str, arch: &str) -> Option<String> {
    let os_map = match os {
        "linux" => "linux",
        "macos" => "darwin",
        "windows" => "windows",
        _ => return None,
    };
    let arch_map = match arch {
        "x86_64" => "amd64",
        "aarch64" => "arm64",
        _ => return None,
    };
    let ext = if os == "windows" { "zip" } else { "tar.gz" };
    Some(format!("ff_{version}_{os_map}_{arch_map}.{ext}"))
}

/// Build the release asset filename for the current platform.
pub fn asset_name(version: &str) -> Option<String> {
    asset_name_for(version, std::env::consts::OS, std::env::consts::ARCH)
}

/// Look up the checksum for `asset` in a checksums file.
///
/// Each line is expected to be `<hash> <filename>`. Returns the hash
/// lowercased, or `None` if the asset is not found.
pub fn checksum_for(checksums: &str, asset: &str) -> Option<String> {
    for line in checksums.lines() {
        let fields: Vec<&str> = line.split_whitespace().collect();
        if fields.len() == 2 && fields[1] == asset {
            return Some(fields[0].to_ascii_lowercase());
        }
    }
    None
}

#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    UpToDate { current: String },
    Updated { from: String, to: String },
}

/// Fields injected so tests can point api_base at a local fake server and
/// exe at a scratch binary.
pub struct Updater {
    pub api_base: String,
    pub exe: std::path::PathBuf,
    pub current_version: String,
    pub official: bool,
}

impl Updater {
    pub fn run(&self) -> ff_core::Result<Outcome> {
        // 1. Classify — must precede any network call
        match classify_install(&self.exe, self.official) {
            InstallKind::Source => {
                return Err(ff_core::Error::msg(
                    "ff was built from source — update with: cargo install --git https://github.com/tyler-johnson/fufu ff-cli",
                ));
            }
            InstallKind::Homebrew => {
                return Err(ff_core::Error::msg(
                    "ff was installed with Homebrew — update with: brew upgrade fufu",
                ));
            }
            InstallKind::Official => {}
        }

        // 2. Fetch latest release
        let agent = github::agent();
        let release = github::fetch_latest(&agent, &self.api_base)?;

        // 3. Compare versions
        let latest = parse_tag(&release.tag_name).ok_or_else(|| {
            ff_core::Error::msg(format!("unexpected release tag \"{}\"", release.tag_name))
        })?;
        let current = parse_semver(&self.current_version).ok_or_else(|| {
            ff_core::Error::msg(format!(
                "cannot parse current version \"{}\"",
                self.current_version
            ))
        })?;
        if latest <= current {
            return Ok(Outcome::UpToDate {
                current: self.current_version.clone(),
            });
        }

        // 4. Find the right asset and its checksum
        let bare = release
            .tag_name
            .strip_prefix('v')
            .unwrap_or(&release.tag_name);
        let asset_name = asset_name(bare)
            .ok_or_else(|| ff_core::Error::msg("no release asset for this platform"))?;

        let asset = release
            .assets
            .iter()
            .find(|a| a.name == asset_name)
            .ok_or_else(|| {
                ff_core::Error::msg(format!(
                    "release {} has no asset {asset_name}",
                    release.tag_name
                ))
            })?;

        let checksums_asset = release
            .assets
            .iter()
            .find(|a| a.name == "checksums.txt")
            .ok_or_else(|| {
                ff_core::Error::msg(format!("release {} has no checksums.txt", release.tag_name))
            })?;

        let checksums_text = github::fetch_text(&agent, &checksums_asset.browser_download_url)?;
        let expected_hash = checksum_for(&checksums_text, &asset_name).ok_or_else(|| {
            ff_core::Error::msg(format!("checksums.txt has no entry for {asset_name}"))
        })?;

        // 5. Temp paths in the exe's directory
        let exe_dir = self
            .exe
            .parent()
            .ok_or_else(|| ff_core::Error::msg("executable path has no parent directory"))?;
        let exe_name = self
            .exe
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .ok_or_else(|| ff_core::Error::msg("executable has no file name"))?;

        let ext = if cfg!(windows) { "zip" } else { "tar.gz" };
        let dl = exe_dir.join(format!("{exe_name}.download.{ext}"));
        let bin = exe_dir.join(format!("{exe_name}.download.bin"));

        // Remove stale files from a crashed run
        let _ = std::fs::remove_file(&dl);
        let _ = std::fs::remove_file(&bin);

        // 6. Download with hash verification
        let mut file = std::fs::File::create(&dl).map_err(|err| {
            if err.kind() == std::io::ErrorKind::PermissionDenied {
                ff_core::Error::msg(format!(
                    "cannot write {} — re-run with the needed permissions (sudo), or reinstall with the install script",
                    exe_dir.display()
                ))
            } else {
                ff_core::Error::repo(err)
            }
        })?;

        let asset_resp = github::get(&agent, &asset.browser_download_url)?;
        let status = asset_resp.status().as_u16();
        if !(200..300).contains(&status) {
            let _ = std::fs::remove_file(&dl);
            return Err(ff_core::Error::msg(format!(
                "download failed: HTTP {status}"
            )));
        }

        let actual_hash = {
            let mut reader = asset_resp.into_body().into_reader();
            download::copy_hashed(&mut reader, &mut file).map_err(ff_core::Error::repo)?
        };
        drop(file);

        if actual_hash != expected_hash {
            let _ = std::fs::remove_file(&dl);
            return Err(ff_core::Error::msg(format!(
                "checksum mismatch for {asset_name} — refusing to install (corrupt or tampered download)"
            )));
        }

        // 7. Extract the binary
        let member = if cfg!(windows) { "ff.exe" } else { "ff" };
        extract::extract_member(&dl, member, &bin)?;
        let _ = std::fs::remove_file(&dl); // best-effort cleanup

        // 8. Swap
        swap::swap(&bin, &self.exe).map_err(|err| {
            ff_core::Error::msg(format!("cannot replace {}: {err}", self.exe.display()))
        })?;

        // 9. Done
        Ok(Outcome::Updated {
            from: format!("v{}", self.current_version),
            to: release.tag_name,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn asset_name_for_platforms() {
        assert_eq!(
            asset_name_for("0.2.0", "linux", "x86_64"),
            Some("ff_0.2.0_linux_amd64.tar.gz".into()),
        );
        assert_eq!(
            asset_name_for("0.2.0", "macos", "aarch64"),
            Some("ff_0.2.0_darwin_arm64.tar.gz".into()),
        );
        assert_eq!(
            asset_name_for("0.2.0", "windows", "aarch64"),
            Some("ff_0.2.0_windows_arm64.zip".into()),
        );
        assert!(asset_name_for("0.2.0", "freebsd", "x86_64").is_none());
    }

    #[test]
    fn checksum_for_matching_and_missing() {
        let fixture = "ABC123  ff_0.1.0_linux_amd64.tar.gz\njunk extra line here\nDEF456  ff_0.1.0_darwin_arm64.tar.gz";
        assert_eq!(
            checksum_for(fixture, "ff_0.1.0_linux_amd64.tar.gz"),
            Some("abc123".into()),
        );
        assert!(checksum_for(fixture, "ff_missing.tar.gz").is_none());
    }

    #[test]
    fn classify_install_kinds() {
        assert_eq!(
            classify_install(std::path::Path::new("/usr/local/bin/ff"), false),
            InstallKind::Source,
        );
        assert_eq!(
            classify_install(std::path::Path::new("/opt/homebrew/bin/ff"), true),
            InstallKind::Homebrew,
        );
        assert_eq!(
            classify_install(
                std::path::Path::new("/home/linuxbrew/.linuxbrew/bin/ff"),
                true
            ),
            InstallKind::Homebrew,
        );
        assert_eq!(
            classify_install(
                std::path::Path::new("/usr/local/Cellar/fufu/0.1.0/bin/ff"),
                true
            ),
            InstallKind::Homebrew,
        );
        assert_eq!(
            classify_install(std::path::Path::new("/usr/local/bin/ff"), true),
            InstallKind::Official,
        );
    }
}

#[cfg(test)]
#[cfg(unix)]
mod http_tests {
    use super::*;
    use crate::selfupdate::download;
    use std::io::{Read as _, Write as _};

    fn http_response(status_line: &str, extra_headers: &[(&str, String)], body: &[u8]) -> Vec<u8> {
        let mut out = format!(
            "HTTP/1.1 {status_line}\r\nContent-Length: {}\r\nConnection: close\r\n",
            body.len()
        );
        for (k, v) in extra_headers {
            out.push_str(&format!("{k}: {v}\r\n"));
        }
        let mut bytes = out.into_bytes();
        bytes.extend_from_slice(b"\r\n");
        bytes.extend_from_slice(body);
        bytes
    }

    /// Serves until the test process exits; each connection gets one response
    /// chosen by request path. The factory receives the base URL so closures
    /// can embed it in response bodies. Returns the http://127.0.0.1:port base.
    fn spawn_server<F>(factory: F) -> String
    where
        F: FnOnce(&str) -> Box<dyn Fn(&str) -> Vec<u8> + Send>,
    {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let respond = factory(&base);
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut s) = stream else { break };
                let mut head = Vec::new();
                let mut buf = [0u8; 4096];
                loop {
                    match s.read(&mut buf) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => {
                            head.extend_from_slice(&buf[..n]);
                            if head.windows(4).any(|w| w == b"\r\n\r\n") {
                                break;
                            }
                        }
                    }
                }
                let text = String::from_utf8_lossy(&head);
                let path = text.split_whitespace().nth(1).unwrap_or("/").to_string();
                let _ = s.write_all(&respond(&path));
            }
        });
        base
    }

    /// Build a tar.gz archive containing a `ff` member with the given contents,
    /// returning (archive_bytes, sha256_hex).
    fn build_archive(bin_contents: &[u8]) -> (Vec<u8>, String) {
        let mut tar_data = Vec::new();
        {
            let gz_encoder =
                flate2::write::GzEncoder::new(&mut tar_data, flate2::Compression::default());
            let mut builder = tar::Builder::new(gz_encoder);
            let mut header = tar::Header::new_gnu();
            header.set_entry_type(tar::EntryType::Regular);
            header.set_mode(0o644);
            header.set_size(bin_contents.len() as u64);
            builder
                .append_data(&mut header, "ff", bin_contents)
                .unwrap();
            // Also add a LICENSE member
            let mut header2 = tar::Header::new_gnu();
            header2.set_entry_type(tar::EntryType::Regular);
            header2.set_mode(0o644);
            header2.set_size(7);
            builder
                .append_data(&mut header2, "LICENSE", &b"MIT\n\n"[..])
                .unwrap();
            builder.finish().unwrap();
        }

        // Compute sha256
        let sha = download::copy_hashed(&mut tar_data.as_slice(), &mut std::io::sink())
            .expect("compute sha256");
        (tar_data, sha)
    }

    #[test]
    fn happy_path() {
        let dir = tempfile::tempdir().unwrap();
        let exe_path = dir.path().join("ff");
        std::fs::write(&exe_path, b"the old binary").unwrap();

        let (archive_bytes, sha) = build_archive(b"the new binary");
        let asset_name = asset_name("9.9.9").unwrap();

        let base = spawn_server(move |base: &str| {
            let base = base.to_string();
            let handler = move |path: &str| match path {
                "/repos/tyler-johnson/fufu/releases/latest" => {
                    let json = format!(
                        r#"{{"tag_name":"v9.9.9","assets":[{{"name":"{}","browser_download_url":"{}/dl/archive"}},{{"name":"checksums.txt","browser_download_url":"{}/dl/sums"}}]}}"#,
                        asset_name, base, base
                    );
                    http_response("200 OK", &[], json.as_bytes())
                }
                "/dl/archive" => http_response("200 OK", &[], &archive_bytes),
                "/dl/sums" => {
                    let sums = format!("{sha}  {asset_name}\n");
                    http_response("200 OK", &[], sums.as_bytes())
                }
                _ => http_response("404 Not Found", &[], b"not found"),
            };
            Box::new(handler)
        });

        let updater = Updater {
            api_base: base,
            exe: exe_path.clone(),
            current_version: "0.1.0".into(),
            official: true,
        };

        let result = updater.run().unwrap();
        assert_eq!(
            result,
            Outcome::Updated {
                from: "v0.1.0".into(),
                to: "v9.9.9".into()
            }
        );

        // Binary was replaced
        assert_eq!(std::fs::read(&exe_path).unwrap(), b"the new binary");

        // Mode is 0755
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&exe_path).unwrap().permissions().mode() & 0o777,
            0o755
        );

        // No temp files remain
        assert!(!exe_path.with_extension("download.tar.gz").exists());
        assert!(!exe_path.with_extension("download.bin").exists());
    }

    #[test]
    fn redirect_hop() {
        let dir = tempfile::tempdir().unwrap();
        let exe_path = dir.path().join("ff");
        std::fs::write(&exe_path, b"the old binary").unwrap();

        let (archive_bytes, sha) = build_archive(b"the new binary");
        let asset_name = asset_name("9.9.9").unwrap();

        let base = spawn_server(move |base: &str| {
            let base = base.to_string();
            let handler = move |path: &str| match path {
                "/repos/tyler-johnson/fufu/releases/latest" => {
                    let json = format!(
                        r#"{{"tag_name":"v9.9.9","assets":[{{"name":"{}","browser_download_url":"{}/dl/archive"}},{{"name":"checksums.txt","browser_download_url":"{}/dl/sums"}}]}}"#,
                        asset_name, base, base
                    );
                    http_response("200 OK", &[], json.as_bytes())
                }
                "/dl/archive" => {
                    http_response("302 Found", &[("Location", format!("{base}/dl/real"))], b"")
                }
                "/dl/real" => http_response("200 OK", &[], &archive_bytes),
                "/dl/sums" => {
                    let sums = format!("{sha}  {asset_name}\n");
                    http_response("200 OK", &[], sums.as_bytes())
                }
                _ => http_response("404 Not Found", &[], b"not found"),
            };
            Box::new(handler)
        });

        let updater = Updater {
            api_base: base,
            exe: exe_path.clone(),
            current_version: "0.1.0".into(),
            official: true,
        };

        let result = updater.run().unwrap();
        assert!(matches!(result, Outcome::Updated { .. }));
        assert_eq!(std::fs::read(&exe_path).unwrap(), b"the new binary");
    }

    #[test]
    fn checksum_mismatch() {
        let dir = tempfile::tempdir().unwrap();
        let exe_path = dir.path().join("ff");
        std::fs::write(&exe_path, b"the old binary").unwrap();

        let (archive_bytes, _) = build_archive(b"the new binary");
        let asset_name = asset_name("9.9.9").unwrap();

        let base = spawn_server(move |base: &str| {
            let base = base.to_string();
            let handler = move |path: &str| match path {
                "/repos/tyler-johnson/fufu/releases/latest" => {
                    let json = format!(
                        r#"{{"tag_name":"v9.9.9","assets":[{{"name":"{}","browser_download_url":"{}/dl/archive"}},{{"name":"checksums.txt","browser_download_url":"{}/dl/sums"}}]}}"#,
                        asset_name, base, base
                    );
                    http_response("200 OK", &[], json.as_bytes())
                }
                "/dl/archive" => http_response("200 OK", &[], &archive_bytes),
                "/dl/sums" => {
                    let bad = format!("{}  {asset_name}\n", "0".repeat(64));
                    http_response("200 OK", &[], bad.as_bytes())
                }
                _ => http_response("404 Not Found", &[], b"not found"),
            };
            Box::new(handler)
        });

        let updater = Updater {
            api_base: base,
            exe: exe_path.clone(),
            current_version: "0.1.0".into(),
            official: true,
        };

        let err = updater.run().unwrap_err();
        assert!(err.to_string().contains("checksum mismatch"));

        // Binary unchanged
        assert_eq!(std::fs::read(&exe_path).unwrap(), b"the old binary");

        // No temp files
        assert!(!exe_path.with_extension("download.tar.gz").exists());
        assert!(!exe_path.with_extension("download.bin").exists());
    }

    #[test]
    fn up_to_date() {
        let dir = tempfile::tempdir().unwrap();
        let exe_path = dir.path().join("ff");
        std::fs::write(&exe_path, b"the old binary").unwrap();

        let base = spawn_server(move |_base: &str| {
            let handler = move |path: &str| {
                if path == "/repos/tyler-johnson/fufu/releases/latest" {
                    let json = r#"{"tag_name":"v0.1.0","assets":[]}"#;
                    http_response("200 OK", &[], json.as_bytes())
                } else {
                    http_response("404 Not Found", &[], b"not found")
                }
            };
            Box::new(handler)
        });

        let updater = Updater {
            api_base: base,
            exe: exe_path.clone(),
            current_version: "0.1.0".into(),
            official: true,
        };

        let result = updater.run().unwrap();
        assert_eq!(
            result,
            Outcome::UpToDate {
                current: "0.1.0".into()
            }
        );
        assert_eq!(std::fs::read(&exe_path).unwrap(), b"the old binary");
    }

    #[test]
    fn missing_asset() {
        let dir = tempfile::tempdir().unwrap();
        let exe_path = dir.path().join("ff");
        std::fs::write(&exe_path, b"the old binary").unwrap();

        let base = spawn_server(move |_base: &str| {
            let handler = move |path: &str| {
                if path == "/repos/tyler-johnson/fufu/releases/latest" {
                    let json = r#"{"tag_name":"v2.0.0","assets":[{"name":"checksums.txt","browser_download_url":"http://example.com/sums"}]}"#;
                    http_response("200 OK", &[], json.as_bytes())
                } else {
                    http_response("404 Not Found", &[], b"not found")
                }
            };
            Box::new(handler)
        });

        let updater = Updater {
            api_base: base,
            exe: exe_path,
            current_version: "0.1.0".into(),
            official: true,
        };

        let err = updater.run().unwrap_err();
        assert!(err.to_string().contains("has no asset"));
    }

    #[test]
    fn rate_limit() {
        let dir = tempfile::tempdir().unwrap();
        let exe_path = dir.path().join("ff");
        std::fs::write(&exe_path, b"the old binary").unwrap();

        let base = spawn_server(move |_base: &str| {
            let handler = move |path: &str| {
                if path == "/repos/tyler-johnson/fufu/releases/latest" {
                    http_response(
                        "403 Forbidden",
                        &[("X-RateLimit-Remaining", "0".into())],
                        b"{}",
                    )
                } else {
                    http_response("404 Not Found", &[], b"not found")
                }
            };
            Box::new(handler)
        });

        let updater = Updater {
            api_base: base,
            exe: exe_path,
            current_version: "0.1.0".into(),
            official: true,
        };

        let err = updater.run().unwrap_err();
        assert_eq!(
            err.to_string(),
            "GitHub API rate limit hit — try again later, or set GITHUB_TOKEN"
        );
    }

    #[test]
    fn bad_tag() {
        let dir = tempfile::tempdir().unwrap();
        let exe_path = dir.path().join("ff");
        std::fs::write(&exe_path, b"the old binary").unwrap();

        let base = spawn_server(move |_base: &str| {
            let handler = move |path: &str| {
                if path == "/repos/tyler-johnson/fufu/releases/latest" {
                    let json = r#"{"tag_name":"nightly","assets":[]}"#;
                    http_response("200 OK", &[], json.as_bytes())
                } else {
                    http_response("404 Not Found", &[], b"not found")
                }
            };
            Box::new(handler)
        });

        let updater = Updater {
            api_base: base,
            exe: exe_path,
            current_version: "0.1.0".into(),
            official: true,
        };

        let err = updater.run().unwrap_err();
        assert!(err.to_string().contains("unexpected release tag"));
    }
}
