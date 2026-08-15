//! Session plumbing: resolve the current session name and validate names.
//!
//! Sessions are stateless: the only source is the `FF_SESSION` environment
//! variable. There is no marker file, no mode, and nothing to open or close.

use ff_core::error::{Error, Result};

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

/// The session a snapshot belongs to: the `--session` flag wins, then
/// `FF_SESSION`. Both go through the same `parse`, but only the flag is
/// fatal — the environment is ambient rather than something typed on this
/// command line, so an unusable value there is ignored (and named under
/// `FF_DEBUG`) instead of aborting the command.
///
/// Both sources are arguments: the answer belongs to one invocation, and
/// `Ctx` settles it once at startup rather than letting each caller ask.
pub fn resolve(flag: Option<&str>, env: Option<&str>) -> Result<Option<String>> {
    if let Some(raw) = flag {
        return Ok(Some(parse(raw)?));
    }
    let Some(raw) = env else {
        return Ok(None);
    };
    match parse(raw) {
        Ok(name) => Ok(Some(name)),
        Err(err) => {
            if std::env::var_os("FF_DEBUG").is_some() {
                eprintln!("ff[debug]: FF_SESSION ignored: {err}");
            }
            Ok(None)
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
