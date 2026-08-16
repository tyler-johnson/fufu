//! Which chain a working tree belongs to, and what it is based on.
//!
//! Two functions is all that survives the one-log cutover. The chain ref, the
//! trash ref, the snapshot identity and the "is this a snapshot" guard all
//! belonged to a second log that no longer exists: the namespace is
//! [`crate::ops::BRANCH_PREFIX`], the identity is [`crate::ops::FUFU_NAME`],
//! and the guard is [`crate::ops::is_op_commit`] — which is stricter, because
//! it also refuses the record commit hanging off an operation.

use crate::error::{Error, Result};
use crate::model::HeadState;

/// The chain name used while HEAD is detached. Part of [`chain_name`]'s own
/// vocabulary: a detached head has no branch, and every op still needs one
/// chain to belong to.
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

/// The base commit of the current head state, if any.
pub fn base_commit(head: &HeadState) -> Result<Option<gix::ObjectId>> {
    Ok(match head {
        HeadState::Unborn { .. } => None,
        HeadState::Branch { commit, .. } | HeadState::Detached { commit } => {
            Some(gix::ObjectId::from_hex(commit.as_bytes()).map_err(Error::repo)?)
        }
    })
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
        assert_eq!(chain_name(&branch), "feat/x/y");
        let unborn = HeadState::Unborn {
            r#ref: "refs/heads/main".into(),
        };
        assert_eq!(chain_name(&unborn), "main");
        let detached = HeadState::Detached {
            commit: "0".repeat(40),
        };
        assert_eq!(chain_name(&detached), DETACHED);
    }
}
