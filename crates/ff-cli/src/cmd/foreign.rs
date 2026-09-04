//! The foreign verbs: git's and jj's words fufu deliberately does not have,
//! answered with where the thing they name went.
//!
//! Typing one of these is a question — "how do I do the git thing?", or the
//! jj thing — and the parser's "unrecognized subcommand" answers a different
//! one. So each is declared hidden in `cli.rs` and lands here, on the
//! precedent of the retired `-m` and `ff log --ops`: a word fufu chose not
//! to have is worth more than a word it never heard of. A jj word whose
//! meaning already *is* a fufu verb — `new`, `bookmark`, `squash`, `rebase`
//! — is an alias on that verb rather than an answer here.
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

/// The one on this list that is a *position* rather than a gap: principle 12
/// names rebase over merge outright, and the replay verbs are what fufu has
/// instead. So the answer is where the act went, not an apology for a verb
/// that has not been written.
pub fn merge(args: &[OsString]) -> Result<()> {
    refuse(
        "there is no ff merge: fufu replays rather than merges, so ff restack --onto puts a \
         branch on top of the work you wanted in and ff sync does that same replay on the way \
         in. A merge commit is what a forge makes when work lands, not something a branch \
         collects locally. ff git merge still runs the real thing capture-first"
            .into(),
        vec![
            "ff sync".into(),
            "ff restack --onto <branch>".into(),
            passthrough("merge", args),
        ],
    )
}

/// Reads stay git's, and saying so is the answer. What earns the entry is the
/// second half: blame reads *history*, and the work fufu is holding for you is
/// the part that is not history yet — so the honest pointer is both.
pub fn blame(args: &[OsString]) -> Result<()> {
    refuse(
        "there is no ff blame: reading history is still git's, and ff git blame runs it \
         capture-first. What blame cannot see is the part fufu holds — every operation behind \
         this file since you last committed, which is ff evolog, and the whole worktree at any \
         one of them, which is ff restore --at-op"
            .into(),
        vec![
            passthrough("blame", args),
            "ff evolog".into(),
            "ff restore <path> --at-op <op>".into(),
        ],
    )
}

/// Making a tag is git's; *losing* one is not, and that is the half worth
/// saying. `refs/tags/` is in `TRACKED_PREFIXES`, so a tag rides every
/// operation's ref table and `ff undo` puts back one that was deleted.
pub fn tag(args: &[OsString]) -> Result<()> {
    refuse(
        "there is no ff tag: tags are git's to make, and ff git tag makes one capture-first. \
         fufu records refs/tags/ in every operation's ref table, so a tag you moved or deleted \
         is on the operation log and ff undo puts it back — which is the half git has no \
         answer for"
            .into(),
        vec![
            passthrough("tag", args),
            "ff undo".into(),
            "ff op log".into(),
        ],
    )
}

/// jj's abandon drops a change wherever it stands, and fufu has no one
/// verb for that because the thing dropped is a different thing at each
/// stage: the open change is files, a session or a held rewrite is a
/// pending act, and a closed commit is history. Each has its own drop.
pub fn abandon(args: &[OsString]) -> Result<()> {
    let exits = match subject(args) {
        Some(rev) => vec![
            format!("ff lift --from {rev}"),
            "ff restore --all".into(),
            "ff done --abandon".into(),
        ],
        None => vec![
            "ff restore --all".into(),
            "ff done --abandon".into(),
            "ff lift --from <rev>".into(),
        ],
    };
    refuse(
        "there is no ff abandon: the open change is dropped with ff restore --all, an editing \
         session or a held rewrite with ff done --abandon, and a commit that has closed comes \
         apart with ff lift --from <rev>, which drops it once nothing is left. A branch goes \
         with ff branch delete"
            .into(),
        exits,
    )
}

/// jj's split takes one commit apart into two. fufu never needs the verb:
/// the open change closes a slice at a time, and a closed commit's files
/// lift back into the open change to close again.
pub fn split(args: &[OsString]) -> Result<()> {
    let commit = match subject(args) {
        Some(path) => format!("ff commit {path}"),
        None => "ff commit <paths>".into(),
    };
    refuse(
        "there is no ff split: the open change closes in slices — ff commit <paths> lands what \
         lies under those paths and leaves the rest open — and a commit that has closed comes \
         apart with ff lift --from <rev> <paths>, which brings those files back into the open \
         change to close again"
            .into(),
        vec![commit, "ff lift --from <rev> <paths>".into()],
    )
}
