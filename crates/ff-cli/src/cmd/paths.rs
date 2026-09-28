//! The path slot the path-taking verbs share.
//!
//! A positional is a path from the current directory, the way git reads one:
//! `file.ts` in `a/b` is `a/b/file.ts`, `..` climbs toward the repository
//! root and no further, and a path that lands on the root itself means the
//! whole tree, so `ff diff .` at the root is every file. Core matches
//! repo-relative paths only, so every verb converts here before anything
//! reaches it.
//!
//! A positional that names nothing on disk or in HEAD is refused before any
//! tree walk, rather than answered with an empty log or an empty patch. An
//! empty answer reads as "no changes", and the token that earned it is
//! almost always a revision or a sentence that wanted a flag: `main..HEAD`
//! in `ff diff`'s path slot, a second sha after `ff show HEAD`, a commit
//! message typed at `ff log`. So the refusal names what the slot takes, and
//! the exits lead with the flag-shaped spelling the token was reaching for.

use std::path::{Component, Path, PathBuf};

use ff_core::{Error, Result};

/// Convert `tokens` from the current directory to the repository root.
///
/// The result is what core matches: repo-relative, `/`-separated. A token
/// that lands on the root empties the result, which every verb reads as the
/// whole tree. A token outside the repository is refused. A bare repository
/// has no root to resolve against, so its tokens pass through unchanged and
/// the verb's own bare refusal leads.
pub fn from_cwd(repo: &ff_core::gix::Repository, tokens: Vec<String>) -> Result<Vec<String>> {
    let Some(resolved) = each(repo, &tokens)? else {
        return Ok(tokens);
    };
    Ok(collapse(resolved))
}

/// [`from_cwd`], then refuse the first token that names nothing on disk or
/// in HEAD.
///
/// `verb` is the verb's name as typed; `slot` is its sentence about what
/// the positional takes, finishing the message "no path here matches
/// `<token>`: `ff <verb>` <slot>"; `exits` builds the lines to try next,
/// given the offending token. The message quotes the token as typed.
///
/// "On disk or in HEAD" is `ff_core::path_exists`'s rule, so a path that
/// only an older commit knew is refused too — the same limit `ff commit`
/// and `ff log` already carry.
pub fn resolve(
    repo: &ff_core::gix::Repository,
    verb: &str,
    slot: &str,
    tokens: Vec<String>,
    exits: impl Fn(&str) -> Vec<String>,
) -> Result<Vec<String>> {
    let resolved = match each(repo, &tokens)? {
        Some(resolved) => resolved,
        None => tokens.clone(),
    };
    for (token, path) in tokens.iter().zip(&resolved) {
        if !ff_core::path_exists(repo, path)? {
            return Err(Error::coded(
                "usage/no-such-path",
                format!("no path here matches {token:?}: `ff {verb}` {slot}"),
                exits(token),
            ));
        }
    }
    Ok(collapse(resolved))
}

/// Each token resolved in order, with `""` for the root; `None` for a bare
/// repository.
fn each(repo: &ff_core::gix::Repository, tokens: &[String]) -> Result<Option<Vec<String>>> {
    let Some(workdir) = repo.workdir() else {
        return Ok(None);
    };
    if tokens.is_empty() {
        return Ok(Some(Vec::new()));
    }
    // `getcwd` is already physical, so the root is resolved to match: on
    // macOS `/tmp` is `/private/tmp`, and a lexical strip would miss. `real`
    // also drops the `\\?\` prefix Windows's canonical form carries.
    let root = ff_core::linked::path::real(workdir);
    let cwd = std::env::current_dir().map_err(Error::repo)?;
    tokens
        .iter()
        .map(|token| {
            to_repo(&root, &cwd, token)
                .or_else(|| {
                    // An absolute token spelled through a symlink strips only
                    // once it is physical too, and only one that exists can be
                    // made so.
                    let path = Path::new(token);
                    let real = ff_core::linked::path::real(path);
                    (path.is_absolute() && real != path)
                        .then(|| to_repo(&root, &cwd, real.to_str()?))
                        .flatten()
                })
                .ok_or_else(|| {
                    Error::coded(
                        "usage/no-such-path",
                        format!("{token:?} is outside this repository"),
                        vec!["ff status".into()],
                    )
                })
        })
        .collect::<Result<Vec<_>>>()
        .map(Some)
}

/// The root anywhere among the paths is the whole tree, which the verbs
/// spell as no paths at all.
fn collapse(paths: Vec<String>) -> Vec<String> {
    if paths.iter().any(String::is_empty) {
        Vec::new()
    } else {
        paths
    }
}

/// `token`, read from `cwd`, as a path relative to `root`: `/`-separated,
/// `""` for the root itself, `None` outside it.
///
/// Normalization is lexical, as git's is: `.` drops, `..` pops the
/// component before it, and symlinks are not followed.
fn to_repo(root: &Path, cwd: &Path, token: &str) -> Option<String> {
    let mut path = PathBuf::new();
    for component in cwd.join(token).components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                path.pop();
            }
            other => path.push(other),
        }
    }
    let rel = path.strip_prefix(root).ok()?;
    let parts: Vec<&str> = rel
        .components()
        .map(|component| component.as_os_str().to_str())
        .collect::<Option<_>>()?;
    Some(parts.join("/"))
}

#[cfg(test)]
mod tests {
    use super::to_repo;
    use std::path::Path;

    fn at(cwd: &str, token: &str) -> Option<String> {
        to_repo(Path::new("/repo"), Path::new(cwd), token)
    }

    #[test]
    fn a_plain_token_reads_from_the_current_directory() {
        assert_eq!(at("/repo/a/b", "file.ts").as_deref(), Some("a/b/file.ts"));
        assert_eq!(at("/repo", "file.ts").as_deref(), Some("file.ts"));
    }

    #[test]
    fn dot_segments_normalize_lexically() {
        assert_eq!(at("/repo/a/b", "./x").as_deref(), Some("a/b/x"));
        assert_eq!(at("/repo/a/b", "../x").as_deref(), Some("a/x"));
        assert_eq!(at("/repo/a/b", "../../x").as_deref(), Some("x"));
        assert_eq!(at("/repo/a/b", "c/../x").as_deref(), Some("a/b/x"));
    }

    #[test]
    fn climbing_past_the_root_is_outside() {
        assert_eq!(at("/repo/a/b", "../../.."), None);
        assert_eq!(at("/repo/a/b", "../../../x"), None);
        assert_eq!(at("/repo", "../repo2/x"), None);
    }

    #[test]
    fn an_absolute_token_replaces_the_current_directory() {
        assert_eq!(at("/repo/a/b", "/repo/c/d").as_deref(), Some("c/d"));
        assert_eq!(at("/repo/a/b", "/elsewhere/x"), None);
    }

    #[test]
    fn the_root_itself_is_the_empty_path() {
        assert_eq!(at("/repo", ".").as_deref(), Some(""));
        assert_eq!(at("/repo/a/b", "../..").as_deref(), Some(""));
        assert_eq!(at("/repo/a/b", "/repo").as_deref(), Some(""));
    }

    #[test]
    fn a_trailing_slash_drops() {
        assert_eq!(at("/repo/a", "b/").as_deref(), Some("a/b"));
        assert_eq!(at("/repo/a/b", "./").as_deref(), Some("a/b"));
    }
}
