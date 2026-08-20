//! Versioned JSON envelope for all `--json` output. Every emission — success
//! or failure — is shaped here, so the two forms cannot drift apart.

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
    write_line(
        out,
        &serde_json::json!({ "ff": CONTRACT, "cmd": cmd, "data": data }),
    )
}

/// The error form: `error` replaces `data`, never both.
pub fn emit_error(cmd: &str, err: &Error) -> Result<()> {
    write_line(
        &mut std::io::stdout(),
        &serde_json::json!({
            "ff": CONTRACT,
            "cmd": cmd,
            "error": {
                "id": err.id(),
                "message": err.to_string(),
                // The same block the human rendering prints, so a machine
                // reading the envelope is told what a terminal would be.
                "exits": crate::explain::exits_for(err),
            },
        }),
    )
}

/// One already-shaped envelope, one line, one trailing newline.
fn write_line<W: std::io::Write>(out: &mut W, envelope: &serde_json::Value) -> Result<()> {
    let line = serde_json::to_string(envelope).map_err(Error::repo)?;
    writeln!(out, "{line}").map_err(Error::repo)
}

/// False when `FF_NONINTERACTIVE` is set to a non-empty value, or when stdin
/// is not a terminal. Nothing may prompt or open an editor when this is false.
/// `var_os` rather than `var`: a non-UTF-8 value is still a value, and the
/// rest of the tool reads its environment the same way.
pub fn interactive() -> bool {
    let forced_off = std::env::var_os("FF_NONINTERACTIVE").is_some_and(|v| !v.is_empty());
    !forced_off && std::io::IsTerminal::is_terminal(&std::io::stdin())
}
