//! Versioned JSON envelope for all `--json` output.

use ff_core::{Error, Result};
use serde::Serialize;

/// The current JSON contract version.
pub const CONTRACT: u32 = 1;

/// Serialize `data` inside the versioned envelope and write one line to stdout.
pub fn emit<T: Serialize>(cmd: &str, data: &T) -> Result<()> {
    write(&mut std::io::stdout(), cmd, data)
}

/// Same, writing to an arbitrary sink (used by the log family, which writes
/// through the pager's writer rather than stdout).
pub fn write<W: std::io::Write, T: Serialize>(out: &mut W, cmd: &str, data: &T) -> Result<()> {
    let envelope = serde_json::json!({
        "ff": CONTRACT,
        "cmd": cmd,
        "data": data,
    });
    let line = serde_json::to_string(&envelope).map_err(Error::repo)?;
    writeln!(out, "{line}").map_err(Error::repo)
}
