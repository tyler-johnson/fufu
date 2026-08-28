//! `ff git` — the capture-first passthrough. Every invocation snapshots
//! first and then execs real git, verbatim; fufu never runs a write verb
//! you did not type.
//!
//! What `fufu.gitPolicy` governs here is what fufu *says*, not what it
//! runs. Under `observe` the passthrough is silent. Under `coach` (the
//! default) a git word fufu has a verb for earns one line naming it, once
//! per word. Under `strict` that word is refused outright — before the
//! capture, on `foreign.rs`'s doctrine that a refusal which captured first
//! is writing on behalf of a command that did not run.
//!
//! Which words those are lives in `rawgit`, shared with the agent hook, so
//! the two entry points cannot answer the same word differently.

use std::ffi::OsString;

use ff_core::{Error, Result};

use crate::ctx::Ctx;
use crate::gitpolicy::{LOCAL, Policy};
use crate::rawgit::{Shape, Word};

pub fn run(ctx: &Ctx, args: Vec<OsString>) -> Result<()> {
    let repo = ff_core::discover(".").ok();
    let shape = crate::rawgit::classify_argv(&args);

    // The correction, decided before anything is written. Outside a
    // repository there is no config to read and no tally to keep, so the
    // passthrough is exactly git's.
    if let (Some(repo), Shape::Write(word, _)) = (&repo, shape) {
        let policy = crate::gitpolicy::read(repo);
        // `ff git rebase` is already fufu's answer for `git rebase`, and
        // answering somebody with their own command line is not a
        // correction. Those words are tallied and otherwise left alone.
        if !word.is_passthrough() {
            match policy {
                Policy::Strict => {
                    crate::gitpolicy::record(repo, LOCAL, word.git, true);
                    return Err(refusal(word));
                }
                Policy::Coach => {
                    if crate::gitpolicy::record(repo, LOCAL, word.git, false) {
                        eprintln!("ff: tip: that's {}", word.ff);
                    }
                }
                Policy::Observe => {
                    crate::gitpolicy::record(repo, LOCAL, word.git, false);
                }
            }
        } else {
            crate::gitpolicy::record(repo, LOCAL, word.git, false);
        }
    }

    // Capture before anything runs. Loud on failure: the user asked git to
    // do something, and a skipped net deserves a notice.
    crate::capture::pre_loud(&crate::provenance::pre_git(ctx, &args));

    if let Some(repo) = &repo {
        crate::selfupdate::notify::maybe_spawn_check(repo);
        crate::autotrim::maybe_trim(repo);
    }

    let notice = repo
        .as_ref()
        .and_then(|r| crate::selfupdate::notify::pending(r, env!("CARGO_PKG_VERSION"), true));
    match notice {
        // A notice is pending and the verb tolerates child-mode: run git as a
        // child so ff regains control to speak after git's own output.
        Some(notice) if deferrable(&args) => {
            let code = super::git_exec::run_wait("git", args);
            eprintln!("{notice}");
            crate::selfupdate::notify::mark_notified();
            std::process::exit(code);
        }
        // No notice (or a non-deferrable verb): the exec fast path. The
        // notice, if any, waits for a future command.
        _ => super::git_exec::exec("git", args),
    }
}

/// What strict says instead of running it. The fufu spelling is the first
/// exit, because it is the command to type next; the tier is the second,
/// because a policy that cannot be found is a policy that reads as a bug.
fn refusal(word: &'static Word) -> Error {
    Error::coded(
        "usage/git-policy",
        format!(
            "fufu.gitPolicy is strict, and fufu has a verb for git {}: {} — {}",
            word.git, word.ff, word.why
        ),
        vec![word.ff.to_string(), "ff config gitPolicy coach".into()],
    )
}

fn deferrable(args: &[OsString]) -> bool {
    let first = args.first().and_then(|a| a.to_str());
    matches!(
        first,
        Some("status" | "diff" | "log" | "branch" | "fetch" | "pull" | "push")
    )
}
