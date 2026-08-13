//! `ff git` — the capture-first passthrough. Invocations whose meaning maps
//! totally onto a fufu verb are translated (and hinted, once per repo);
//! everything else execs real git verbatim. The whitelist is deliberately
//! strict: any token fufu doesn't fully understand falls through to git.

use std::ffi::OsString;

use ff_core::Result;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Translated {
    Status,
    Log { limit: Option<usize> },
}

/// The Phase-1 translation table. Pure argv inspection, no repo access.
/// `git log -n 0` deliberately does NOT translate: git shows nothing there,
/// while `ff log -n 0` means unlimited — semantics must map exactly or not
/// at all.
pub fn translate(args: &[OsString]) -> Option<Translated> {
    let utf8: Vec<&str> = args
        .iter()
        .map(|a| a.to_str())
        .collect::<Option<Vec<_>>>()?;
    match utf8.split_first()? {
        (&"status", []) => Some(Translated::Status),
        (&"log", rest) => {
            let limit = match rest {
                [] => None,
                ["-n", k] | ["--max-count", k] => Some(parse_count(k)?),
                [flag] => {
                    let k = flag
                        .strip_prefix("--max-count=")
                        .or_else(|| flag.strip_prefix("-n"))
                        .or_else(|| flag.strip_prefix('-'))?;
                    Some(parse_count(k)?)
                }
                _ => return None,
            };
            Some(Translated::Log { limit })
        }
        _ => None,
    }
}

fn parse_count(text: &str) -> Option<usize> {
    let n: usize = text.parse().ok()?;
    (n >= 1).then_some(n)
}

pub fn run(args: Vec<OsString>) -> Result<()> {
    let translated = translate(&args);
    // Capture before anything runs — translated or not. Loud on failure:
    // the user asked git to do something; a skipped net deserves a notice.
    crate::capture::pre_loud(&crate::provenance::pre_git(&args));

    match translated {
        Some(verb) => {
            hint_once(&verb);
            // Capture already happened; run the verb's inner body directly.
            match verb {
                Translated::Status => crate::cmd::status::run_inner(false),
                Translated::Log { limit } => {
                    crate::cmd::log::run_inner(false, limit.unwrap_or(0), false)
                }
            }
        }
        None => super::git_exec::exec(args),
    }
}

/// Mention the native spelling once per repository — policy, not nag.
/// The marker is written before the hint prints, so a crash between the two
/// can only under-hint, never repeat.
fn hint_once(verb: &Translated) {
    let Ok(repo) = ff_core::discover(".") else {
        return;
    };
    let marker = repo.git_dir().join("fufu/hinted");
    if marker.exists() {
        return;
    }
    if let Some(parent) = marker.parent()
        && std::fs::create_dir_all(parent).is_err()
    {
        return;
    }
    if std::fs::write(&marker, b"").is_err() {
        return;
    }
    let spelling = match verb {
        Translated::Status => "ff status",
        Translated::Log { .. } => "ff log",
    };
    eprintln!("ff: tip: that's {spelling}");
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(args: &[&str]) -> Option<Translated> {
        let os: Vec<OsString> = args.iter().map(OsString::from).collect();
        translate(&os)
    }

    #[test]
    fn whitelist_is_strict() {
        assert_eq!(t(&["status"]), Some(Translated::Status));
        assert_eq!(t(&["status", "-s"]), None);
        assert_eq!(t(&["status", "--porcelain"]), None);
        assert_eq!(t(&["log"]), Some(Translated::Log { limit: None }));
        assert_eq!(
            t(&["log", "-n", "5"]),
            Some(Translated::Log { limit: Some(5) })
        );
        assert_eq!(t(&["log", "-5"]), Some(Translated::Log { limit: Some(5) }));
        assert_eq!(t(&["log", "-n5"]), Some(Translated::Log { limit: Some(5) }));
        assert_eq!(
            t(&["log", "--max-count=3"]),
            Some(Translated::Log { limit: Some(3) })
        );
        assert_eq!(
            t(&["log", "--max-count", "3"]),
            Some(Translated::Log { limit: Some(3) })
        );
        // Semantics that don't map exactly fall through to git.
        assert_eq!(t(&["log", "-n", "0"]), None);
        assert_eq!(t(&["log", "--oneline"]), None);
        assert_eq!(t(&["log", "-n", "5", "--oneline"]), None);
        assert_eq!(t(&["log", "main"]), None);
        assert_eq!(t(&["push"]), None);
        assert_eq!(t(&[]), None);
    }
}
