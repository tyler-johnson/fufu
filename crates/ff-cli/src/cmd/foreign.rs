//! The foreign verbs: git words fufu deliberately does not have, answered
//! with where the thing they name went.
//!
//! Typing one of these is a question — "how do I do the git thing?" — and
//! the parser's "unrecognized subcommand" answers a different one. So each
//! is declared hidden in `cli.rs` and lands here, on the precedent of the
//! retired `-m` and `ff log --ops`: a word fufu chose not to have is worth
//! more than a word it never heard of.
//!
//! These never touch the repository. A refusal that captured first would be
//! writing on behalf of a command that does not exist.

use std::ffi::OsString;

use ff_core::{Error, Result};

/// The first real word after the verb, if there is one — flags skipped,
/// since a flag never names the thing an exit would act on. `ff checkout
/// main` earns `ff switch main`; `ff checkout -b` earns the placeholder.
fn subject(args: &[OsString]) -> Option<String> {
    args.iter()
        .map(|word| word.to_string_lossy().into_owned())
        .find(|word| !word.starts_with('-'))
}

/// What was typed after the verb, rejoined for a passthrough exit. Lossy on
/// purpose: this is a suggestion to read, not a command line to re-execute,
/// and quoting it would suggest otherwise.
fn tail(args: &[OsString]) -> String {
    args.iter()
        .map(|word| word.to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join(" ")
}

/// `ff git <verb> <what was typed>` — the passthrough spelled out, which is
/// the honest exit wherever fufu has no verb of its own yet.
fn passthrough(verb: &str, args: &[OsString]) -> String {
    let tail = tail(args);
    if tail.is_empty() {
        format!("ff git {verb}")
    } else {
        format!("ff git {verb} {tail}")
    }
}

fn refuse(message: String, exits: Vec<String>) -> Result<()> {
    Err(Error::coded("usage/foreign-verb", message, exits))
}

/// git's checkout did two unrelated jobs, and which one you got depended on
/// what you passed it. fufu split them, so the answer is both verbs rather
/// than a guess at which was meant.
pub fn checkout(args: &[OsString]) -> Result<()> {
    let (switch, restore) = match subject(args) {
        Some(word) => (format!("ff switch {word}"), format!("ff restore {word}")),
        None => ("ff switch <branch>".into(), "ff restore <path>".into()),
    };
    refuse(
        "ff checkout is two verbs here: ff switch moves between lines of work, ff restore brings \
         files back. git's checkout was both, and which one it did depended on what you passed it"
            .into(),
        vec![switch, restore],
    )
}

pub fn diff(args: &[OsString]) -> Result<()> {
    refuse(
        "there is no ff diff: ff status carries the diff of the open change, and ff op diff \
         compares the worktrees two operations hold"
            .into(),
        vec![
            "ff status".into(),
            "ff op diff <a> <b>".into(),
            passthrough("diff", args),
        ],
    )
}

pub fn stash(args: &[OsString]) -> Result<()> {
    // git stash is a family, and its members do not all land on one verb:
    // listing what is parked is the map, and resuming a parked change is
    // simply switching back to the branch holding it.
    let exits = match subject(args).as_deref() {
        Some("list") => vec!["ff".into(), "ff branch list".into()],
        Some("pop" | "apply") => vec!["ff switch <branch>".into()],
        _ => vec!["ff switch <branch>".into(), "ff start".into(), "ff".into()],
    };
    refuse(
        "there is no ff stash: switching parks the open change on the branch you are leaving and \
         resumes whatever was parked where you land, so there is nothing to push and nothing to \
         pop. Bare ff shows every parked change"
            .into(),
        exits,
    )
}

pub fn pull(args: &[OsString]) -> Result<()> {
    refuse(
        "there is no ff pull: ff sync is the incoming half done properly — fetch, take in what \
         arrived, replay onto your base — with no merge-versus-rebase question to get wrong. \
         Sending is ff publish, and it is a separate verb on purpose"
            .into(),
        vec![
            "ff sync".into(),
            "ff publish".into(),
            passthrough("pull", args),
        ],
    )
}

pub fn push(args: &[OsString]) -> Result<()> {
    refuse(
        "there is no ff push: ff publish sends this branch under a lease, so it refuses rather \
         than overwrites when the shared copy moved since you last looked. It is the outgoing \
         half of lining up, and ff sync is the incoming one"
            .into(),
        vec![
            "ff publish".into(),
            "ff sync".into(),
            passthrough("push", args),
        ],
    )
}

pub fn rebase(args: &[OsString]) -> Result<()> {
    refuse(
        "there is no ff rebase yet: ff status says whether this branch still replays cleanly onto \
         its base, and ff git rebase runs the real thing capture-first — snapshot taken before it \
         starts, so ff undo is enough"
            .into(),
        vec!["ff status".into(), passthrough("rebase", args)],
    )
}
