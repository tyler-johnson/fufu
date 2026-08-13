pub mod branch;
pub mod branchmeta;
mod close;
pub mod describe;
pub mod error;
mod head;
mod hooks;
pub mod index;
pub mod journal;
mod log;
pub mod model;
mod new;
pub mod petname;
mod refs;
mod restore;
pub mod snapshot;
pub mod stash;
mod status;
mod switch;
mod timeline;
mod trim;
mod undo;
mod upstream;
mod worktree;

use std::path::Path;

/// Re-exported so downstream crates name the exact gix this core was built with.
pub use gix;

pub use close::{CloseOptions, close};
pub use error::{Error, Result};
pub use head::{head_state, operation};
pub use log::{LogOptions, log};
pub use model::*;
pub use new::{NewOptions, NewTarget, new, resolve_target};
pub use restore::{RestoreOptions, RestoreTarget, parse_target, restore};
pub use snapshot::{Provenance, TakeOptions, take, take_with};
pub use status::status;
pub use switch::{SwitchOptions, resolve_branch, switch};
pub use timeline::{TimelineOptions, timeline};
pub use trim::{TrimOptions, trim};
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
    Ok(repo.to_thread_local())
}

/// Discover with an isolated configuration: no environment overrides, no
/// global or system config. Used by tests to stay hermetic on any machine.
pub fn discover_isolated(dir: impl AsRef<Path>) -> Result<gix::Repository> {
    let repo = gix::ThreadSafeRepository::discover_opts(
        dir.as_ref(),
        Default::default(),
        read_tuned(gix::open::Options::isolated()),
    )?;
    Ok(repo.to_thread_local())
}
