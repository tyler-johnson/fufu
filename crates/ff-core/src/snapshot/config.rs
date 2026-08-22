//! fufu's configuration reads, and the one-time gc guard written when a chain
//! is first created.

use std::io::Write as _;

use crate::error::{Error, Result};

pub const DEFAULT_MAX_FILE_SIZE: u64 = 50 * 1024 * 1024;
pub const DEFAULT_KEEP_SECS: i64 = 90 * 24 * 60 * 60;

/// `fufu.maxFileSize` in bytes; unset → 50 MiB.
pub fn max_file_size(repo: &gix::Repository) -> u64 {
    repo.config_snapshot()
        .integer("fufu.maxFileSize")
        .and_then(|v| u64::try_from(v).ok())
        .unwrap_or(DEFAULT_MAX_FILE_SIZE)
}

/// `fufu.keep` as an age cutoff in seconds; unset → 90 days.
/// Accepts compact durations (`90d`, `12w`, `36h`, `30m`, `45s`) or a bare
/// integer meaning days.
pub fn keep_secs(repo: &gix::Repository) -> Result<i64> {
    let snapshot = repo.config_snapshot();
    let Some(raw) = snapshot.string("fufu.keep") else {
        return Ok(DEFAULT_KEEP_SECS);
    };
    let raw = raw.to_string();
    parse_keep(&raw).ok_or_else(|| Error::msg(format!("invalid fufu.keep value: {raw:?}")))
}

/// Parse a retention duration: `<n>[smhdw]`, or a bare integer of days.
pub fn parse_keep(raw: &str) -> Option<i64> {
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }
    let (digits, unit) = match raw.strip_suffix(['s', 'm', 'h', 'd', 'w']) {
        Some(digits) => (digits, raw.chars().last().unwrap()),
        None => (raw, 'd'),
    };
    let n: i64 = digits.parse().ok()?;
    let secs = match unit {
        's' => n,
        'm' => n.checked_mul(60)?,
        'h' => n.checked_mul(60 * 60)?,
        'd' => n.checked_mul(24 * 60 * 60)?,
        'w' => n.checked_mul(7 * 24 * 60 * 60)?,
        _ => unreachable!(),
    };
    (secs >= 0).then_some(secs)
}

pub const GC_SUBSECTION: &str = "refs/fufu/*";
pub const GC_KEYS: [&str; 2] = ["reflogExpire", "reflogExpireUnreachable"];

/// Read a git config file losslessly (comments and formatting preserved);
/// an absent file yields an empty File carrying the given source metadata.
pub fn load_config_file(
    path: &std::path::Path,
    source: gix::config::Source,
) -> Result<gix::config::File<'static>> {
    let metadata = gix::config::file::Metadata::from(source);
    match std::fs::read(path) {
        Ok(mut bytes) => {
            gix::config::File::from_bytes_owned(&mut bytes, metadata, Default::default())
                .map_err(Error::repo)
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            Ok(gix::config::File::new(metadata))
        }
        Err(err) => Err(Error::repo(err)),
    }
}

/// Serialize and write via `<path>.lock` (git's own lock convention: create-new
/// fails if a concurrent git holds it) + atomic rename; the lock file is
/// removed on failure.
pub fn write_config_file(path: &std::path::Path, file: &gix::config::File<'_>) -> Result<()> {
    let mut bytes = Vec::new();
    file.write_to(&mut bytes).map_err(Error::repo)?;

    let lock = path.with_extension("lock");
    let mut lock_file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&lock)
        .map_err(|err| Error::msg(format!("config is locked: {err}")))?;
    let write = lock_file
        .write_all(&bytes)
        .and_then(|()| lock_file.sync_all())
        .and_then(|()| {
            drop(lock_file);
            std::fs::rename(&lock, path)
        });
    if let Err(err) = write {
        let _ = std::fs::remove_file(&lock);
        return Err(Error::repo(err));
    }
    Ok(())
}

/// Write the gc guard into the repository's config, once: without it,
/// `git gc` would expire reflogs on the custom namespace (custom namespaces
/// get git's default expiry) and then collect the chains. Existing user
/// values are never rewritten; only missing keys are appended.
pub fn ensure_gc_config(repo: &gix::Repository) -> Result<()> {
    let path = repo.common_dir().join("config");
    let mut file = load_config_file(&path, gix::config::Source::Local)?;

    let existing: Vec<&str> = GC_KEYS
        .iter()
        .copied()
        .filter(|key| {
            file.string(format!("gc.{GC_SUBSECTION}.{key}").as_str())
                .is_some()
        })
        .collect();
    if existing.len() == GC_KEYS.len() {
        return Ok(());
    }

    let mut section = file
        .section_mut_or_create_new("gc", Some(GC_SUBSECTION.into()))
        .map_err(Error::repo)?;
    for key in GC_KEYS {
        if !existing.contains(&key) {
            section.push(key.try_into().map_err(Error::repo)?, Some("never".into()));
        }
    }

    write_config_file(&path, &file)
}

/// Doctor's consented `--fix` front door: unlike the lazy append-only guard
/// above, this unconditionally overwrites both gc keys to `never` in the local
/// config file, so a manually-tweaked value is corrected rather than left alone.
pub fn force_gc_config(repo: &gix::Repository) -> Result<()> {
    let path = repo.common_dir().join("config");
    let mut file = load_config_file(&path, gix::config::Source::Local)?;

    for key in GC_KEYS {
        file.set_raw_value_by("gc", Some(GC_SUBSECTION.into()), key, "never")
            .map_err(Error::repo)?;
    }

    write_config_file(&path, &file)
}

/// Carry a branch's upstream across a rename: the `[branch "<old>"]`
/// section's keys and values move verbatim to `[branch "<new>"]` and the
/// old section is removed, so nothing is left naming a branch that no
/// longer exists. `merge` rides across unchanged because it names the
/// branch on the remote side, which this rename did not touch — the
/// renamed branch keeps the shared copy it already had. A branch with no
/// section (the common case: the anonymous claims of `ff commit -b` and
/// `ff start -b`) returns without writing the file at all.
pub fn rename_branch_section(repo: &gix::Repository, old: &str, new: &str) -> Result<()> {
    let path = repo.common_dir().join("config");
    let mut file = load_config_file(&path, gix::config::Source::Local)?;

    // Collect the old section's contents into owned data so this borrow on
    // `file` ends before the mutations below.
    let moved: Vec<(String, gix::bstr::BString)> = match file.section("branch", Some(old.into())) {
        Ok(section) => {
            // The names are collapsed first: `value_names` yields one entry
            // per *occurrence* while `values` answers with every value under
            // a name, so a key written twice — which `merge` legitimately is,
            // for an octopus upstream — would otherwise come out squared.
            let mut names: Vec<String> = Vec::new();
            for name in section.value_names() {
                let name = name.as_ref().to_string();
                if !names.contains(&name) {
                    names.push(name);
                }
            }
            names
                .into_iter()
                .flat_map(|name| {
                    section
                        .values(&name)
                        .into_iter()
                        .map(move |value| (name.clone(), value.into_owned()))
                })
                .collect()
        }
        Err(_) => return Ok(()),
    };

    // Any section already under the new name can only be stale — the
    // rename refused a branch of that name — so replace it rather than
    // appending into it, which would leave duplicate keys.
    let _ = file.remove_section("branch", Some(new.into()));
    let mut section = file
        .section_mut_or_create_new("branch", Some(new.into()))
        .map_err(Error::repo)?;
    for (name, value) in moved {
        section.push(name.try_into().map_err(Error::repo)?, Some(value.as_ref()));
    }
    let _ = file.remove_section("branch", Some(old.into()));

    write_config_file(&path, &file)
}

/// Give a branch an upstream: `remote` and `merge` under
/// `[branch "<branch>"]` and nothing else. The remote side is always
/// `refs/heads/<branch>` — the shared copy lives under the branch's own
/// name — so it is derived here rather than taken as an argument. The
/// section is removed and created fresh, not appended into: appending into
/// a stale section would leave a second `merge` and the config reader would
/// silently pick one of the two. The `value_names()`/`values()` squaring
/// trap documented above cannot arise here — this writes two known keys
/// instead of carrying an unknown set across.
pub fn set_branch_upstream(repo: &gix::Repository, branch: &str, remote: &str) -> Result<()> {
    let path = repo.common_dir().join("config");
    let mut file = load_config_file(&path, gix::config::Source::Local)?;

    let _ = file.remove_section("branch", Some(branch.into()));
    let mut section = file
        .section_mut_or_create_new("branch", Some(branch.into()))
        .map_err(Error::repo)?;
    section.push(
        "remote".try_into().map_err(Error::repo)?,
        Some(gix::bstr::BStr::new(remote.as_bytes())),
    );
    let merge = format!("refs/heads/{branch}");
    section.push(
        "merge".try_into().map_err(Error::repo)?,
        Some(gix::bstr::BStr::new(merge.as_bytes())),
    );

    write_config_file(&path, &file)
}

#[cfg(test)]
mod tests {
    use super::parse_keep;

    #[test]
    fn keep_durations() {
        assert_eq!(parse_keep("90d"), Some(90 * 86_400));
        assert_eq!(parse_keep("2w"), Some(14 * 86_400));
        assert_eq!(parse_keep("36h"), Some(36 * 3_600));
        assert_eq!(parse_keep("30m"), Some(1_800));
        assert_eq!(parse_keep("45s"), Some(45));
        assert_eq!(parse_keep("7"), Some(7 * 86_400));
        assert_eq!(parse_keep("0d"), Some(0));
        assert_eq!(parse_keep("x"), None);
        assert_eq!(parse_keep(""), None);
        assert_eq!(parse_keep("-1d"), None);
    }
}
