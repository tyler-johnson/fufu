//! `ff git` — the capture-first passthrough. Invocations whose meaning maps
//! totally onto a fufu verb are translated (and hinted, once per repo);
//! everything else execs real git verbatim. The whitelist is deliberately
//! strict: any token fufu doesn't fully understand falls through to git.

use std::ffi::OsString;

use ff_core::Result;

use crate::ctx::Ctx;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Translated {
    Status,
    Log { limit: Option<usize> },
    Switch { target: String },
    Commit { message: Option<String> },
    Branch,
}

/// The translation table. Pure argv inspection, no repo access.
/// `git log -n 0` deliberately does NOT translate: git shows nothing there,
/// while `ff log -n 0` means unlimited — semantics must map exactly or not
/// at all. Same discipline everywhere: `git switch` only in its bare
/// `switch <branch>` form, `git commit` only bare or with one `-m <msg>`,
/// `git branch` only bare. Any other token falls through to git verbatim.
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
        (&"switch", [target]) if !target.starts_with('-') => Some(Translated::Switch {
            target: target.to_string(),
        }),
        (&"commit", []) => Some(Translated::Commit { message: None }),
        (&"commit", ["-m", msg]) => Some(Translated::Commit {
            message: Some(msg.to_string()),
        }),
        (&"branch", []) => Some(Translated::Branch),
        _ => None,
    }
}

fn parse_count(text: &str) -> Option<usize> {
    let n: usize = text.parse().ok()?;
    (n >= 1).then_some(n)
}

pub fn run(ctx: &Ctx, args: Vec<OsString>) -> Result<()> {
    let translated = translate(&args);
    // Capture before anything runs — translated or not. Loud on failure:
    // the user asked git to do something; a skipped net deserves a notice.
    crate::capture::pre_loud(&crate::provenance::pre_git(ctx, &args));

    let repo = ff_core::discover(".").ok();
    if let Some(repo) = &repo {
        crate::selfupdate::notify::maybe_spawn_check(repo);
        crate::autotrim::maybe_trim(repo);
    }

    match translated {
        Some(verb) => {
            hint_once(&verb);
            // Capture already happened; run the verb's inner body directly.
            // The ctx is `git`'s own, so `--json` is already off in it — the
            // translated verb speaks git's prose, not an envelope.
            let result = match verb {
                Translated::Status => crate::cmd::status::run_inner(ctx),
                Translated::Log { limit } => {
                    crate::cmd::log::run_inner(ctx, limit.unwrap_or(0), None, false, None)
                }
                // The mutating verbs own their pre-snapshot (a no-op after
                // the capture above) and their operation.
                Translated::Switch { target } => crate::cmd::switch::run(ctx, target),
                Translated::Commit { message } => {
                    crate::cmd::commit::run(ctx, message, false, None)
                }
                Translated::Branch => crate::cmd::branch::run(ctx, None, None),
            };
            if let Some(repo) = &repo
                && let Some(notice) =
                    crate::selfupdate::notify::pending(repo, env!("CARGO_PKG_VERSION"))
            {
                eprintln!("{notice}");
                crate::selfupdate::notify::mark_notified();
            }
            result
        }
        None => {
            let notice = repo
                .as_ref()
                .and_then(|r| crate::selfupdate::notify::pending(r, env!("CARGO_PKG_VERSION")));
            match notice {
                // A notice is pending and the verb tolerates child-mode: run git as a
                // child so ff regains control to speak after git's own output.
                Some(notice) if deferrable(&args) => {
                    let code = super::git_exec::run_wait(args);
                    eprintln!("{notice}");
                    crate::selfupdate::notify::mark_notified();
                    std::process::exit(code);
                }
                // No notice (or a non-deferrable verb): the exec fast path, exactly as
                // before. The notice, if any, waits for a future command.
                _ => super::git_exec::exec(args),
            }
        }
    }
}

fn deferrable(args: &[OsString]) -> bool {
    let first = args.first().and_then(|a| a.to_str());
    matches!(
        first,
        Some("status" | "diff" | "log" | "branch" | "fetch" | "pull" | "push")
    )
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
        Translated::Switch { .. } => "ff switch",
        Translated::Commit { .. } => "ff commit",
        Translated::Branch => "ff branch",
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

    #[test]
    fn phase2_whitelist_is_strict() {
        assert_eq!(
            t(&["switch", "feature"]),
            Some(Translated::Switch {
                target: "feature".into()
            })
        );
        // Flags, creation, detach: git's business.
        assert_eq!(t(&["switch", "-c", "x"]), None);
        assert_eq!(t(&["switch", "--detach", "x"]), None);
        assert_eq!(t(&["switch"]), None);

        assert_eq!(t(&["commit"]), Some(Translated::Commit { message: None }));
        assert_eq!(
            t(&["commit", "-m", "msg"]),
            Some(Translated::Commit {
                message: Some("msg".into())
            })
        );
        assert_eq!(t(&["commit", "-am", "msg"]), None);
        assert_eq!(t(&["commit", "--amend"]), None);
        assert_eq!(t(&["commit", "-m", "msg", "--no-verify"]), None);

        assert_eq!(t(&["branch"]), Some(Translated::Branch));
        assert_eq!(t(&["branch", "-d", "x"]), None);
        assert_eq!(t(&["branch", "new-name"]), None);
    }
}
