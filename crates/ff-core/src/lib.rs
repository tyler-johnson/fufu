pub mod branch;
pub mod branchmeta;
mod close;
pub mod describe;
pub mod error;
mod evolog;
mod head;
mod hooks;
pub mod index;
pub mod journal;
mod log;
pub mod model;
pub mod petname;
mod refs;
mod restore;
pub mod snapid;
pub mod snapshot;
mod start;
pub mod stash;
mod status;
mod switch;
mod trim;
mod trunk;
mod undo;
mod upstream;
mod worktree;

use std::path::Path;

/// Re-exported so downstream crates name the exact gix this core was built with.
pub use gix;

pub use close::{CloseOptions, close};
pub use error::{Error, Result};
pub use evolog::{EvologOptions, chain_ids, evolog, open_change, segment_anchors};
pub use head::{head_state, operation};
pub use log::{LogOptions, log};
pub use model::*;
pub use restore::{RestoreOptions, RestoreTarget, parse_target, restore};
pub use snapshot::{Provenance, TakeOptions, take, take_with};
pub use start::{StartOptions, start};
pub use status::status;
pub use switch::{SwitchOptions, resolve_branch, switch};
pub use trim::{TrimOptions, trim};
pub use trunk::{Trunk, TrunkKind, TrunkSource, trunk};
pub use undo::{UndoOptions, undo};
pub use upstream::upstream;

/// Verifying the index trailer SHA-1 on every read costs ~2ms on a 5k-file
/// repo (gix hashes in software); a read-only tool skips it — structural
/// decode errors still catch a torn index.
fn read_tuned(options: gix::open::Options) -> gix::sec::trust::Mapping<gix::open::Options> {
    let options = options.config_overrides(["index.skipHash=true"]);
    gix::sec::trust::Mapping {
        full: options.clone(),
        reduced: options,
    }
}

/// Discover the repository containing `dir`, honoring the normal git
/// environment and configuration. This is the production entry point.
pub fn discover(dir: impl AsRef<Path>) -> Result<gix::Repository> {
    let repo = gix::ThreadSafeRepository::discover_opts(
        dir.as_ref(),
        Default::default(),
        read_tuned(gix::open::Options::default()),
    )?;
    Ok(cached(repo.to_thread_local()))
}

/// Chain walks meet every snapshot twice — once as a row, once as the next
/// row's parent check — so a decoded-object cache halves the reads for any
/// command that follows a chain. The cap is a ceiling, not an allocation.
fn cached(mut repo: gix::Repository) -> gix::Repository {
    repo.object_cache_size_if_unset(4 * 1024 * 1024);
    repo
}

/// Discover with an isolated configuration: no environment overrides, no
/// global or system config. Used by tests to stay hermetic on any machine.
pub fn discover_isolated(dir: impl AsRef<Path>) -> Result<gix::Repository> {
    let repo = gix::ThreadSafeRepository::discover_opts(
        dir.as_ref(),
        Default::default(),
        read_tuned(gix::open::Options::isolated()),
    )?;
    Ok(cached(repo.to_thread_local()))
}
