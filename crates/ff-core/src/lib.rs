pub mod absorb;
mod accounted;
pub mod branch;
pub mod branchmeta;
mod changestat;
mod close;
pub mod collide;
pub mod describe;
pub mod done;
pub mod edit;
pub mod error;
mod evolog;
pub mod futures;
mod head;
pub mod held;
pub mod history;
mod hooks;
pub mod index;
mod jsonfile;
pub mod linked;
mod log;
pub mod map;
pub mod model;
pub mod ops;
pub mod patch;
pub mod petname;
pub mod preflight;
pub mod publish;
mod published;
mod refs;
pub mod remote;
pub mod resolve;
pub mod restack;
mod restore;
mod revert;
pub mod revset;
pub mod rewrite;
pub mod sha;
pub mod snapid;
pub mod snapshot;
mod start;
pub mod stash;
mod status;
mod switch;
pub mod sync;
mod trim;
mod trunk;
mod undo;
mod upstream;
pub mod watch;
mod worktree;

use std::path::Path;

/// Re-exported so downstream crates name the exact gix this core was built with.
pub use gix;

pub use accounted::accounted_for;
pub use changestat::{DiffOptions, change_diff, change_stat, tree_diff, tree_diff_stat};
pub use close::{CloseOptions, close};
pub use collide::{Collision, Pairing, Side, collide};
pub use error::{Error, Result};
pub use evolog::{EvologOptions, evolog, open_change, ref_ids, segment_anchors};
pub use head::{head_state, operation};
pub use history::{Step, history};
pub use linked::add::add_worktree;
pub use linked::remove::remove_worktree;
pub use linked::survey::survey;
pub use log::{Log, LogOptions, log};
pub use map::{Map, MapNode, MapOptions, MapRef, MapRow};
pub use model::*;
pub use ops::{CaptureOutcome, OpId, capture, capture_with};
pub use published::{ever_published, published_tip};
pub use restack::Aim;
/// The `--at` grammar, exported because `ff op log --at` asks the same
/// question of the same clock and must not grow a second parser for it.
pub use restore::parse_time as restore_time;
pub use restore::{RestoreOptions, RestoreSource, path_exists, restore};
pub use revert::{OpVerbOptions, revert};
pub use snapshot::{Provenance, TakeOptions};
pub use start::{StartOptions, start};
pub use status::status;
pub use switch::{SwitchOptions, resolve_branch, switch};
pub use trim::{TrimOptions, trim};
pub use trunk::{Trunk, TrunkKind, TrunkSource, trunk};
pub use undo::{Landing, RewindOptions, redo, rewind, undo};
pub use upstream::upstream;
pub use watch::{Chains, Event, Filter, Motion, Rewrite, Watched, classify, classify_in};

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
