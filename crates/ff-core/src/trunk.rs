//! Trunk resolution — which branch is "main" for sync, status, and bare start.

use crate::error::{Error, Result};

/// Where trunk lives.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TrunkKind {
    /// A local branch: `refs/heads/<name>`.
    Local,
    /// Remote-tracking only: `refs/remotes/<remote>/<name>`.
    Remote { remote: String },
}

#[derive(Debug, Clone)]
pub struct Trunk {
    /// Short name as the user would say it: `main`.
    pub name: String,
    /// Full ref name that resolves today.
    pub full_ref: String,
    pub kind: TrunkKind,
    /// Which rule in the ladder produced this.
    pub source: TrunkSource,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TrunkSource {
    Config,
    OriginHead,
    LoneMainOrMaster,
    LoneBranch,
}

/// Resolve the trunk branch using the configured ladder.
///
/// Ladder (first match wins):
/// 1. `fufu.trunk` config, when set.
/// 2. `origin/HEAD`, when the remote declares it.
/// 3. A lone local `main` or `master`.
/// 4. A lone local branch, whatever it is named.
///
/// Ambiguity at any step is an error, never a guess.
pub fn trunk(repo: &gix::Repository) -> Result<Trunk> {
    // 1. fufu.trunk config
    if let Some(val) = repo.config_snapshot().string("fufu.trunk") {
        let val = val.to_string();
        if !val.is_empty() {
            return resolve_config_trunk(repo, &val);
        }
    }

    // 2. origin/HEAD symbolic ref
    if let Some(target) = read_origin_head(repo)?
        && crate::refs::ref_target(repo, &target)?.is_some()
    {
        let (name, remote) = parse_remote_ref(&target);
        return Ok(Trunk {
            name,
            full_ref: target,
            kind: TrunkKind::Remote { remote },
            source: TrunkSource::OriginHead,
        });
    }

    // 3. Lone local main or master
    let names = crate::switch::branch_names(repo)?;
    let has_main = names.iter().any(|n| n == "main");
    let has_master = names.iter().any(|n| n == "master");
    match (has_main, has_master) {
        (true, false) => {
            return Ok(Trunk {
                name: "main".to_string(),
                full_ref: "refs/heads/main".to_string(),
                kind: TrunkKind::Local,
                source: TrunkSource::LoneMainOrMaster,
            });
        }
        (false, true) => {
            return Ok(Trunk {
                name: "master".to_string(),
                full_ref: "refs/heads/master".to_string(),
                kind: TrunkKind::Local,
                source: TrunkSource::LoneMainOrMaster,
            });
        }
        (true, true) => {
            return Err(Error::msg(
                "cannot tell which branch is trunk (candidates: main, master); \
                 set one with ff config trunk <branch>",
            ));
        }
        (false, false) => {}
    }

    // 4. Lone local branch
    if names.len() == 1 {
        let name = names.into_iter().next().unwrap();
        return Ok(Trunk {
            full_ref: format!("refs/heads/{name}"),
            kind: TrunkKind::Local,
            source: TrunkSource::LoneBranch,
            name,
        });
    }

    Err(Error::msg(
        "cannot tell which branch is trunk; set one with ff config trunk <branch>",
    ))
}

fn resolve_config_trunk(repo: &gix::Repository, value: &str) -> Result<Trunk> {
    // Local-first: an explicit local branch is the more specific match,
    // mirroring git's own preference for refs/heads over refs/remotes.

    // 1. Try refs/heads/{value}
    let local_ref = format!("refs/heads/{value}");
    if crate::refs::ref_target(repo, &local_ref)?.is_some() {
        return Ok(Trunk {
            name: value.to_string(),
            full_ref: local_ref,
            kind: TrunkKind::Local,
            source: TrunkSource::Config,
        });
    }

    // 2. If the value contains a slash, try refs/remotes/{remote}/{rest}
    if let Some((remote, branch)) = value.split_once('/') {
        let full_ref = format!("refs/remotes/{remote}/{branch}");
        if crate::refs::ref_target(repo, &full_ref)?.is_some() {
            return Ok(Trunk {
                name: branch.to_string(),
                full_ref,
                kind: TrunkKind::Remote {
                    remote: remote.to_string(),
                },
                source: TrunkSource::Config,
            });
        }
    }

    Err(Error::msg(format!(
        "fufu.trunk names {value}, which is not a branch here"
    )))
}

fn read_origin_head(repo: &gix::Repository) -> Result<Option<String>> {
    let Some(ref_) = repo
        .try_find_reference("refs/remotes/origin/HEAD")
        .map_err(Error::repo)?
    else {
        return Ok(None);
    };
    // try_name() returns Some for symbolic refs, None for object refs.
    if let Some(target_name) = ref_.target().try_name() {
        return Ok(Some(target_name.as_bstr().to_string()));
    }
    Ok(None)
}

/// Parse `refs/remotes/<remote>/<name>` into `(name, remote)`.
fn parse_remote_ref(full_ref: &str) -> (String, String) {
    if let Some(suffix) = full_ref.strip_prefix("refs/remotes/")
        && let Some((remote, name)) = suffix.split_once('/')
    {
        return (name.to_string(), remote.to_string());
    }
    (full_ref.to_string(), String::new())
}
