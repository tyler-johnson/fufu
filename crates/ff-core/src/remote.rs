//! The remotes this repository knows, and where each one points.
//!
//! fufu's own verbs name a remote — `ff push --to` checks the name
//! against the list, and `ff pull` refuses to guess when the list has more
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
            let fetch_url = repo.find_remote(&*name).ok().and_then(|remote| {
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

/// Delete every tracking ref of `remote` the fetch refspecs cover that the
/// remote no longer holds — gix's fetch has no `--prune`, so this is that
/// step, run after `receive` on the ref map the handshake produced.
///
/// `present` is the set of local destinations the ref map named, one per
/// remote ref the refspecs matched. For each fetch refspec with a
/// destination (a negative spec has none), the destination pattern is
/// split at its `*` and the refs under the prefix are walked: a symbolic
/// one (`<remote>/HEAD`) is left alone, a name that does not end in the
/// suffix is not this spec's, and the rest are deleted unless `present`
/// names them. Best-effort per ref, so one `ref/contended` does not stop
/// the others; the names deleted come back.
///
/// Outside any operation, like the fetch's own ref writes: `refs/remotes/`
/// is outside `TRACKED_PREFIXES`, so reconcile has nothing to say about it.
pub fn prune_tracking(
    repo: &gix::Repository,
    remote: &str,
    present: &std::collections::HashSet<gix::bstr::BString>,
    now: i64,
) -> Result<Vec<String>> {
    use gix::bstr::ByteSlice;

    let handle = repo
        .find_remote(remote)
        .map_err(crate::error::Error::repo)?;
    let mut gone: Vec<(String, gix::ObjectId)> = Vec::new();
    for spec in handle.refspecs(gix::remote::Direction::Fetch) {
        let spec = spec.to_ref();
        let Some(destination) = spec.destination() else {
            continue;
        };
        let destination = destination.to_str_lossy().into_owned();
        // `prefixed` is a path walk, so a destination with no `*` is walked
        // by its own name and matched exactly — `origin/main` must not take
        // `origin/main2` with it.
        let (prefix, suffix) = match destination.split_once('*') {
            Some((prefix, suffix)) => (prefix, Some(suffix)),
            None => (destination.as_str(), None),
        };
        let covers = |name: &str| match suffix {
            Some(suffix) => name.len() >= prefix.len() + suffix.len() && name.ends_with(suffix),
            None => name == destination,
        };
        let platform = repo.references().map_err(crate::error::Error::repo)?;
        let Ok(iter) = platform.prefixed(prefix) else {
            continue;
        };
        for reference in iter.flatten() {
            let name = reference.name().as_bstr().to_str_lossy().into_owned();
            if !covers(&name) {
                continue;
            }
            let target = reference.target();
            if target.try_name().is_some() {
                continue;
            }
            let Some(tip) = target.try_id() else {
                continue;
            };
            if present.contains(name.as_bytes().as_bstr()) {
                continue;
            }
            gone.push((name, tip.to_owned()));
        }
    }
    gone.sort();
    gone.dedup();
    let mut deleted = Vec::new();
    for (name, tip) in gone {
        if crate::refs::delete_ref(repo, &name, tip, now).is_ok() {
            deleted.push(name);
        }
    }
    Ok(deleted)
}
