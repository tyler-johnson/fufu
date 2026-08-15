//! Session plumbing: resolve the current session name and validate names.
//!
//! Sessions are stateless: the only source is the `FF_SESSION` environment
//! variable. There is no marker file, no mode, and nothing to open or close.

use std::sync::OnceLock;

use ff_core::error::{Error, Result};

/// The session set by `--session` on this invocation, if any. Set once from
/// main before any capture runs; read by the provenance constructors.
static OVERRIDE: OnceLock<Option<String>> = OnceLock::new();

/// Record the session override from the `--session` flag. Called once from
/// main after parsing, before any dispatch.
pub fn set_override(name: Option<String>) {
    OVERRIDE.set(name).ok();
}

/// Validate a session name. Any UTF-8 string is legal — the rules are only
/// what storing it as a commit-message trailer actually requires.
pub fn parse(raw: &str) -> Result<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(Error::coded(
            "usage/bad-session",
            "session name is empty",
            vec![],
        ));
    }
    if trimmed.chars().any(|c| c.is_control()) {
        return Err(Error::coded(
            "usage/bad-session",
            "session name cannot contain control characters or line breaks",
            vec![],
        ));
    }
    if trimmed.len() > 128 {
        return Err(Error::coded(
            "usage/bad-session",
            format!("session name is {} bytes; the limit is 128", trimmed.len()),
            vec![],
        ));
    }
    Ok(trimmed.to_string())
}

/// The session a snapshot taken right now belongs to. The `--session` flag
/// (recorded via `set_override`) wins, then `FF_SESSION`. An unusable env
/// value is ignored rather than fatal; the flag is a hard error, validated
/// before `set_override` is called.
pub fn current() -> Option<String> {
    // If the flag was explicitly provided, it wins. If not, fall through to
    // the environment variable.
    if let Some(override_val) = OVERRIDE.get()
        && let Some(name) = override_val
    {
        return Some(name.clone());
    }
    let val = std::env::var_os("FF_SESSION")?;
    let raw = val.to_string_lossy();
    match parse(&raw) {
        Ok(name) => Some(name),
        Err(err) => {
            if std::env::var_os("FF_DEBUG").is_some() {
                eprintln!("ff[debug]: FF_SESSION ignored: {err}");
            }
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_keeps_shape() {
        assert_eq!(
            parse("Refactor Parser!").unwrap(),
            "Refactor Parser!",
            "uppercase, space, and punctuation survive"
        );
    }

    #[test]
    fn parse_trims_whitespace() {
        assert_eq!(parse("  hello  ").unwrap(), "hello");
    }

    #[test]
    fn parse_rejects_empty() {
        let err = parse("   ").unwrap_err();
        assert_eq!(err.id(), "usage/bad-session");
    }

    #[test]
    fn parse_rejects_control_characters() {
        let err = parse("hello\nworld").unwrap_err();
        assert_eq!(err.id(), "usage/bad-session");
    }

    #[test]
    fn parse_rejects_over_length() {
        let long = "a".repeat(129);
        let err = parse(&long).unwrap_err();
        assert_eq!(err.id(), "usage/bad-session");

        // 128 is fine.
        let ok = "a".repeat(128);
        assert!(parse(&ok).is_ok());
    }

    #[test]
    fn parse_allows_unicode_and_emoji() {
        assert!(parse("hello 🌍/world").is_ok());
    }
}
