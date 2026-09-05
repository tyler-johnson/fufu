//! Session plumbing: resolve the current session name and validate names.
//!
//! Sessions are stateless: the only source is the `FF_SESSION` environment
//! variable or the `--session` flag. There is no marker file, no mode, and
//! nothing to open or close — a session is a tag an operation wears, so
//! whoever sets one already knows its name and there is nothing to list. It
//! rides every row of `ff op log`, and filtering by one is
//! `ff op log 'session(<name>)'` — the set language, not a flag of its own.

use ff_core::error::{Error, Result};
use ff_core::ops::{OpId, OpLog};

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
    Ok(ambient("FF_SESSION", env))
}

/// An ambient source: the environment rather than this command line, so an
/// unusable value is ignored and named under `FF_DEBUG` instead of aborting.
pub fn ambient(var: &str, raw: Option<&str>) -> Option<String> {
    let raw = raw?;
    match parse(raw) {
        Ok(name) => Some(name),
        Err(err) => {
            if std::env::var_os("FF_DEBUG").is_some() {
                eprintln!("ff[debug]: {var} ignored: {err}");
            }
            None
        }
    }
}

/// The tag one operation wears, if any. One targeted object read — the id is
/// already known from a walk the caller has done, so this never re-walks
/// anything. Rows carry their tag on the machine surface whether or not a
/// person asked; it is a property of the operation, not a view over it.
pub fn tag_of(repo: &ff_core::gix::Repository, hex: &str) -> Result<Option<String>> {
    let oid = ff_core::gix::ObjectId::from_hex(hex.as_bytes()).map_err(Error::repo)?;
    Ok(OpLog::open(repo)?
        .get(OpId::new(oid))?
        .session()
        .map(str::to_string))
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

    #[test]
    fn ambient_returns_the_parsed_name() {
        assert_eq!(
            ambient("CLAUDE_CODE_SESSION_ID", Some("  abc-123 ")).as_deref(),
            Some("abc-123")
        );
        assert_eq!(ambient("CLAUDE_CODE_SESSION_ID", None), None);
    }

    #[test]
    fn ambient_ignores_an_unusable_value() {
        assert_eq!(ambient("CLAUDE_CODE_SESSION_ID", Some("")), None);
        assert_eq!(ambient("CLAUDE_CODE_SESSION_ID", Some("   ")), None);
        assert_eq!(ambient("CLAUDE_CODE_SESSION_ID", Some("\n")), None);
        assert_eq!(ambient("CLAUDE_CODE_SESSION_ID", Some("a\tb")), None);
    }
}
