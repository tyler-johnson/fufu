pub mod error;
mod head;
mod log;
pub mod model;
mod status;
mod upstream;

use std::path::Path;

/// Re-exported so downstream crates name the exact gix this core was built with.
pub use gix;

pub use error::{Error, Result};
pub use head::{head_state, operation};
pub use log::{LogOptions, log};
pub use model::*;
pub use status::status;
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
