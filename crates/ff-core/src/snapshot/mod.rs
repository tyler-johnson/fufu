//! Floor 1 — continuous capture, reduced to the two things capture actually
//! is: naming the chain a working tree belongs to, and turning that working
//! tree into a git tree object.
//!
//! The commit that carries the tree is [`crate::ops`]'s business now. A
//! snapshot is what an operation carries, so there is no second writer here,
//! no second id space, and no second chain — only the tree assembly
//! (`tree.rs`), the configuration that bounds it (`config.rs`), and the
//! provenance a capture is labeled with.

pub mod chain;
pub mod config;
pub(crate) mod tree;

use crate::error::{Error, Result};

/// Where a capture came from; formatted as the commit subject
/// (`source` or `source: detail`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Provenance {
    /// `manual`, `pre`, `claude[<sess8>]`, …
    pub source: String,
    /// The command, message, or tool detail, if any.
    pub detail: Option<String>,
    /// The session this capture belongs to, if any.
    pub session: Option<String>,
}

impl Provenance {
    pub fn new(source: impl Into<String>, detail: Option<String>) -> Self {
        Provenance {
            source: source.into(),
            detail,
            session: None,
        }
    }

    /// Attach a session name; the session lives in the trailer, never the subject.
    pub fn with_session(self, session: Option<String>) -> Self {
        Provenance { session, ..self }
    }

    /// The raw subject line (before whitespace collapsing / capping).
    pub fn subject(&self) -> String {
        match &self.detail {
            Some(detail) if !detail.is_empty() => format!("{}: {}", self.source, detail),
            _ => self.source.clone(),
        }
    }
}

/// Injection points for tests; production uses the defaults.
#[derive(Debug, Clone, Default)]
pub struct TakeOptions {
    /// Commit/reflog timestamp; `None` = the wall clock.
    pub now: Option<i64>,
    /// Oversize cutoff in bytes; `None` = `fufu.maxFileSize` (default 50 MiB).
    pub max_file_size: Option<u64>,
}

/// Count file-level changes between two trees (subtree rows excluded).
pub(crate) fn count_file_changes(
    repo: &gix::Repository,
    lhs: gix::ObjectId,
    rhs: gix::ObjectId,
) -> Result<usize> {
    if lhs == rhs {
        return Ok(0);
    }
    let lhs_obj = repo.find_object(lhs).map_err(Error::repo)?.detach();
    let rhs_obj = repo.find_object(rhs).map_err(Error::repo)?.detach();
    let mut recorder = gix::diff::tree::Recorder::default();
    gix::diff::tree(
        gix::objs::TreeRefIter::from_bytes(&lhs_obj.data),
        gix::objs::TreeRefIter::from_bytes(&rhs_obj.data),
        gix::diff::tree::State::default(),
        &repo.objects,
        &mut recorder,
    )
    .map_err(Error::repo)?;
    use gix::diff::tree::recorder::Change as Rec;
    Ok(recorder
        .records
        .iter()
        .filter(|change| {
            let mode = match change {
                Rec::Addition { entry_mode, .. }
                | Rec::Deletion { entry_mode, .. }
                | Rec::Modification { entry_mode, .. } => entry_mode,
            };
            !mode.is_tree()
        })
        .count())
}
