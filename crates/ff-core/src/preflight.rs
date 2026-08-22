//! What a branch answers to, read before either verb touches the network.
//!
//! `ff sync` takes the incoming half and `ff publish` the outgoing one, and
//! they are separate verbs — but the facts they need first are the same:
//! the branch underfoot, the remote it answers to, the shared copy of
//! itself, and whether a rewrite is already held on it. Reading them lives
//! here so that neither verb owns the other's preconditions.
//!
//! Every guard that can refuse runs before this returns. Sync pays for a
//! fetch and publish leaves the machine; paying for a round trip only to
//! refuse afterwards is the rudest possible order.

use crate::model::HeadState;
use crate::{Error, Result};

/// Which verb is asking. Only the refusals differ — the facts do not — and
/// a refusal that names the wrong verb is worse than a vague one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verb {
    Sync,
    Publish,
}

impl Verb {
    fn name(self) -> &'static str {
        match self {
            Verb::Sync => "sync",
            Verb::Publish => "publish",
        }
    }

    fn gerund(self) -> &'static str {
        match self {
            Verb::Sync => "syncing",
            Verb::Publish => "publishing",
        }
    }
}

/// What either verb must know before it reaches the network: the branch
/// underfoot, the remote it answers to, and where the tracking ref stands.
/// Sync reads it twice, once on each side of its fetch; publish reads it
/// once, and the tip it finds is exactly "what I last saw".
pub struct Preflight {
    pub branch: String,
    pub branch_tip: gix::ObjectId,
    /// The remote to fetch from and push to. `None` when the repository has
    /// no remote at all, which makes sync entirely a base-axis affair and
    /// leaves publish with nowhere to send anything.
    pub remote: Option<String>,
    /// The branch's shared copy, when it has an upstream.
    pub tracking: Option<Tracking>,
    /// A rewrite is already held on this branch, so its exit is blocked
    /// before either verb has done anything at all.
    pub held: bool,
}

pub struct Tracking {
    /// The full ref: `refs/remotes/origin/feature`.
    pub full: String,
    /// What a person calls it: `origin/feature`.
    pub name: String,
    /// The branch's name on the remote side, from `branch.<n>.merge`:
    /// `feature`. A lease and a refspec are written in terms of this and not
    /// of the local name, which is free to differ.
    pub remote_branch: String,
    /// Its tip as it stands, before any fetch. `None` when it is absent.
    pub tip: Option<gix::ObjectId>,
}

/// The preflight facts, with the remote named outright when `to` carries
/// one: an explicit name wins over both the branch's configured upstream
/// and the repository default, so the ambiguity refusal never runs. A `to`
/// that names a remote the branch already answers to is accepted and
/// changes nothing; a branch answering elsewhere is refused, because
/// pointing it at a second remote would open a second shared copy.
pub fn preflight_to(repo: &gix::Repository, verb: Verb, to: Option<&str>) -> Result<Preflight> {
    if repo.workdir().is_none() {
        return Err(Error::coded(
            "repo/bare",
            format!("bare repository: nothing to {}", verb.name()),
            vec![],
        ));
    }
    if let Some(op) = crate::head::operation(repo) {
        return Err(Error::coded(
            "repo/mid-operation",
            format!(
                "a {op:?} is in progress: finish it with git (git rebase --abort / git merge --abort); fufu owns merges in a later phase"
            ),
            vec![],
        ));
    }
    let branch = match crate::head::head_state(repo)? {
        HeadState::Detached { .. } => {
            return Err(Error::coded(
                "repo/detached",
                format!(
                    "detached HEAD: {} acts on the branch you are standing on",
                    verb.name()
                ),
                vec!["ff switch <branch>".into()],
            ));
        }
        HeadState::Unborn { .. } => {
            return Err(Error::coded(
                "target/unresolvable",
                format!(
                    "nothing is committed yet: there is nothing to {}",
                    verb.name()
                ),
                vec!["ff commit -m <msg>".into()],
            ));
        }
        HeadState::Branch { name, .. } => name,
    };
    if crate::held::resolving(repo, &branch)?.is_some() {
        return Err(Error::coded(
            "held/resolving",
            match verb {
                Verb::Sync => format!(
                    "a resolution is open on {branch}: its conflicts are in your working tree, and syncing over them would move the ground they were computed against"
                ),
                Verb::Publish => format!(
                    "a resolution is open on {branch}: the exit stays blocked until the rewrite under it lands"
                ),
            },
            vec![
                "ff done".into(),
                "ff resolve --abandon".into(),
                "ff status".into(),
            ],
        ));
    }
    if crate::branchmeta::read(repo, &branch)?.session.is_some() {
        return Err(Error::coded(
            "session/open",
            format!(
                "{branch} is an editing session: finish it before {}",
                verb.gerund()
            ),
            vec!["ff done".into(), "ff done --abandon".into()],
        ));
    }

    let branch_tip = branch_tip(repo, &branch)?;
    let held = crate::held::of(repo, &branch)?.is_some();

    // The remote this branch answers to: an explicit `to` first — it names
    // the remote outright, so neither the branch's own `remote` nor the
    // repository default gets a say, and the ambiguity refusal never runs —
    // then the branch's own `remote`, then the repository default, and only
    // then the refusal.
    let remote = if let Some(name) = to {
        if !repo
            .remote_names()
            .iter()
            .any(|remote| remote.to_string() == name)
        {
            return Err(Error::coded(
                "publish/unknown-remote",
                format!("no remote named {name}: fufu will not invent one to publish to"),
                vec!["ff remote".into(), "ff git remote add <name> <url>".into()],
            ));
        }
        let existing = repo
            .branch_remote_name(branch.as_str(), gix::remote::Direction::Fetch)
            .as_ref()
            .and_then(|name| name.as_symbol())
            .map(|name| name.to_string());
        if let Some(existing) = existing.filter(|existing| existing != name) {
            return Err(Error::coded(
                "publish/retarget",
                format!(
                    "{branch} already answers to {existing}: publishing to {name} as well would open a second shared copy"
                ),
                vec![
                    "ff publish".into(),
                    "ff git branch --set-upstream-to <remote>/<branch>".into(),
                ],
            ));
        }
        Some(name.to_string())
    } else {
        let named = repo.branch_remote_name(branch.as_str(), gix::remote::Direction::Fetch);
        let remote = named
            .as_ref()
            .and_then(|name| name.as_symbol())
            .map(|name| name.to_string())
            .or_else(|| {
                repo.remote_default_name(gix::remote::Direction::Fetch)
                    .map(|name| name.to_string())
            });
        match remote {
            Some(name) => Some(name),
            None if repo.remote_names().is_empty() => None,
            None => {
                return Err(Error::coded(
                    "sync/ambiguous-remote",
                    format!(
                        "{} remotes are configured and none is named origin: fufu will not guess which one {branch} answers to",
                        repo.remote_names().len()
                    ),
                    vec![
                        "ff publish --to <remote>".into(),
                        "ff remote".into(),
                        "ff git branch --set-upstream-to <remote>/<branch>".into(),
                    ],
                ));
            }
        }
    };

    let tracking = match crate::futures::remote_for(repo, &branch)? {
        None => None,
        Some(sync_ref) => {
            let tip = (!sync_ref.tip.is_empty())
                .then(|| gix::ObjectId::from_hex(sync_ref.tip.as_bytes()))
                .transpose()
                .map_err(Error::repo)?;
            let full: gix::refs::FullName = format!("refs/heads/{branch}")
                .as_str()
                .try_into()
                .map_err(Error::repo)?;
            let remote_branch = repo
                .branch_remote_ref_name(full.as_ref(), gix::remote::Direction::Fetch)
                .and_then(|name| name.ok())
                .map(|name| name.as_ref().shorten().to_string())
                .unwrap_or_else(|| branch.clone());
            Some(Tracking {
                full: sync_ref.r#ref,
                name: sync_ref.name,
                remote_branch,
                tip,
            })
        }
    };

    Ok(Preflight {
        branch,
        branch_tip,
        remote,
        tracking,
        held,
    })
}

/// The preflight facts with no remote named: `preflight_to` with `to` unset.
pub fn preflight(repo: &gix::Repository, verb: Verb) -> Result<Preflight> {
    preflight_to(repo, verb, None)
}

/// The branch's tip by short name. Every caller has already heard from
/// HEAD that the branch exists, so a miss is fufu's bug, not the user's.
pub(crate) fn branch_tip(repo: &gix::Repository, branch: &str) -> Result<gix::ObjectId> {
    crate::refs::ref_target(repo, &format!("refs/heads/{branch}"))?.ok_or_else(|| {
        // Uncoded on purpose: a curated id is a promise that a person can
        // reach this and be told what to do about it, and nobody can.
        Error::msg(format!(
            "HEAD is on {branch}, but the branch ref is missing: internal inconsistency"
        ))
    })
}
