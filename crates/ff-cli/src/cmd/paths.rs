//! The path-slot check the path-taking verbs share.
//!
//! A positional that names nothing on disk or in HEAD is refused before any
//! tree walk, rather than answered with an empty log or an empty patch. An
//! empty answer reads as "no changes", and the token that earned it is
//! almost always a revision or a sentence that wanted a flag: `main..HEAD`
//! in `ff diff`'s path slot, a second sha after `ff show HEAD`, a commit
//! message typed at `ff log`. So the refusal names what the slot takes, and
//! the exits lead with the flag-shaped spelling the token was reaching for.

use ff_core::{Error, Result};

/// Refuse the first of `paths` that names nothing on disk or in HEAD.
///
/// `verb` is the verb's name as typed; `slot` is its sentence about what
/// the positional takes, finishing the message "no path here matches
/// `<token>`: `ff <verb>` <slot>"; `exits` builds the lines to try next,
/// given the offending token.
///
/// "On disk or in HEAD" is `ff_core::path_exists`'s rule, so a path that
/// only an older commit knew is refused too — the same limit `ff commit`
/// and `ff log` already carry.
pub fn require(
    repo: &ff_core::gix::Repository,
    verb: &str,
    slot: &str,
    paths: &[String],
    exits: impl Fn(&str) -> Vec<String>,
) -> Result<()> {
    for token in paths {
        if !ff_core::path_exists(repo, token)? {
            return Err(Error::coded(
                "usage/no-such-path",
                format!("no path here matches {token:?}: `ff {verb}` {slot}"),
                exits(token),
            ));
        }
    }
    Ok(())
}
