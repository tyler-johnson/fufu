//! Chain naming and the snapshot identity.
//!
//! One chain per branch under `refs/fufu/snap/<branch>` (branch names may
//! contain slashes), plus `refs/fufu/snap/@detached` for detached HEADs.
//! `refs/fufu/trash/<branch>` holds the pre-trim tip — trim's one-deep undo.

use crate::error::{Error, Result};
use crate::model::HeadState;

/// The fixed identity every snapshot commit bears as both author and
/// committer. It terminates the timeline walk and guards restore targets.
pub const FUFU_NAME: &str = "fufu";
pub const FUFU_EMAIL: &str = "fufu@local";

pub const SNAP_PREFIX: &str = "refs/fufu/snap/";
pub const TRASH_PREFIX: &str = "refs/fufu/trash/";
/// The chain name used while HEAD is detached.
pub const DETACHED: &str = "@detached";

/// The short chain name for a head state: branch name, or `@detached`.
pub fn chain_name(head: &HeadState) -> String {
    match head {
        HeadState::Branch { name, .. } => name.clone(),
        HeadState::Unborn { r#ref } => r#ref
            .strip_prefix("refs/heads/")
            .unwrap_or(r#ref)
            .to_string(),
        HeadState::Detached { .. } => DETACHED.to_string(),
    }
}

/// The full chain ref for a head state, e.g. `refs/fufu/snap/main`.
pub fn chain_ref(head: &HeadState) -> String {
    format!("{SNAP_PREFIX}{}", chain_name(head))
}

/// The trash ref paired with a chain, e.g. `refs/fufu/trash/main`.
pub fn trash_ref(chain_name: &str) -> String {
    format!("{TRASH_PREFIX}{chain_name}")
}

/// The base commit of the current head state, if any.
pub fn base_commit(head: &HeadState) -> Result<Option<gix::ObjectId>> {
    Ok(match head {
        HeadState::Unborn { .. } => None,
        HeadState::Branch { commit, .. } | HeadState::Detached { commit } => {
            Some(gix::ObjectId::from_hex(commit.as_bytes()).map_err(Error::repo)?)
        }
    })
}

/// The current tip of a chain ref, `None` when the ref does not exist.
pub fn tip(repo: &gix::Repository, ref_name: &str) -> Result<Option<gix::ObjectId>> {
    Ok(repo
        .try_find_reference(ref_name)
        .map_err(Error::repo)?
        .and_then(|r| r.target().try_id().map(|id| id.to_owned())))
}

/// Whether a commit bears the fufu snapshot identity — author AND committer.
pub fn is_snapshot_commit(commit: &gix::objs::CommitRef<'_>) -> bool {
    commit.author.name == FUFU_NAME
        && commit.author.email == FUFU_EMAIL
        && commit.committer.name == FUFU_NAME
        && commit.committer.email == FUFU_EMAIL
}

/// Decode a commit and report whether it is a fufu snapshot.
pub fn id_is_snapshot(repo: &gix::Repository, id: gix::ObjectId) -> Result<bool> {
    let obj = repo.find_object(id).map_err(Error::repo)?;
    if obj.kind != gix::objs::Kind::Commit {
        return Ok(false);
    }
    let commit = gix::objs::CommitRef::from_bytes(&obj.data).map_err(Error::repo)?;
    Ok(is_snapshot_commit(&commit))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chain_names() {
        let branch = HeadState::Branch {
            name: "feat/x/y".into(),
            r#ref: "refs/heads/feat/x/y".into(),
            commit: "0".repeat(40),
        };
        assert_eq!(chain_ref(&branch), "refs/fufu/snap/feat/x/y");
        let unborn = HeadState::Unborn {
            r#ref: "refs/heads/main".into(),
        };
        assert_eq!(chain_ref(&unborn), "refs/fufu/snap/main");
        let detached = HeadState::Detached {
            commit: "0".repeat(40),
        };
        assert_eq!(chain_ref(&detached), "refs/fufu/snap/@detached");
        assert_eq!(trash_ref("main"), "refs/fufu/trash/main");
    }
}
