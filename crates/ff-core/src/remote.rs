//! The remotes this repository knows, and where each one points.
//!
//! fufu's own verbs name a remote — `ff publish --to` checks the name
//! against the list, and `ff sync` refuses to guess when the list has more
//! than one entry — so the list they check against belongs inside fufu
//! rather than borrowed from `git remote -v`. A refusal that says "no remote
//! named `upstream`" owes the reader the answer to "which names are there?",
//! and the config fufu already reads is where that answer lives.
//!
//! The one rule this module lives by: read through the ordinary
//! [`discover`](crate::discover) handle. The wire's handle — the one opened
//! with `permissions.config.git_binary = true` so a fetch can use git's
//! credential and proxy config — costs a `git config -l` spawn per process,
//! and `ff-cli`'s zero-spawn proof exists to keep that out of the readers.
//! A listing must never be the reason a reader reached outside the process.

use serde::Serialize;

use crate::error::Result;

/// One configured remote.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RemoteInfo {
    pub name: String,
    /// The fetch URL, or `None` for a remote configured without one.
    pub fetch_url: Option<String>,
}

/// Every configured remote, in the order the config table keeps them.
///
/// The listing's job is "what are they called", so a name whose remote
/// section cannot be opened still gets a row, with no URL — the name is the
/// answer, and the URL is the detail.
pub fn list(repo: &gix::Repository) -> Result<Vec<RemoteInfo>> {
    Ok(repo
        .remote_names()
        .into_iter()
        .map(|name| {
            let fetch_url = repo.find_remote(name.as_ref()).ok().and_then(|remote| {
                remote
                    .url(gix::remote::Direction::Fetch)
                    .map(|url| url.to_bstring().to_string())
            });
            RemoteInfo {
                name: name.to_string(),
                fetch_url,
            }
        })
        .collect())
}

/// The answer to "which remote does this branch answer to?", decided but
/// not enforced.
///
/// One rule, one home: `preflight_to`, `futures_for` and `ff doctor` all
/// need this answer, and building the ladder three times is how the three
/// surfaces drift apart. `Ambiguous` is a value rather than an error
/// because deciding and refusing are different jobs — the refusal belongs
/// to the verb that cannot proceed, and a status line needs the same fact
/// without one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RemoteChoice {
    /// The branch answers to this remote, by its own `branch.<n>.remote` or
    /// by the repository default.
    Named(String),
    /// No remotes at all: a purely local repository, legitimately silent.
    NoneConfigured,
    /// Remotes exist and none of them can be named for this branch.
    Ambiguous { count: usize },
}

/// The naming rule itself: the branch's own `remote` first, then the
/// repository default, and only then whether naming is even possible.
///
/// Every step reads config through the ordinary handle, so this obeys the
/// module's zero-spawn rule at the top of the file — and returns a value,
/// never a `Result`, because the refusal is not this function's job.
pub fn for_branch(repo: &gix::Repository, branch: &str) -> RemoteChoice {
    if let Some(name) = repo
        .branch_remote_name(branch, gix::remote::Direction::Fetch)
        .as_ref()
        .and_then(|name| name.as_symbol())
        .map(|name| name.to_string())
    {
        return RemoteChoice::Named(name);
    }
    if let Some(name) = repo.remote_default_name(gix::remote::Direction::Fetch) {
        return RemoteChoice::Named(name.to_string());
    }
    if repo.remote_names().is_empty() {
        RemoteChoice::NoneConfigured
    } else {
        RemoteChoice::Ambiguous {
            count: repo.remote_names().len(),
        }
    }
}
