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
