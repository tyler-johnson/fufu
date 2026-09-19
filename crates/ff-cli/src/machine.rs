//! Versioned JSON envelope for all `--json` output. Every emission — success
//! or failure — is shaped here, so the two forms cannot drift apart.

use std::sync::OnceLock;

use ff_core::{Error, Result};
use serde::Serialize;

use crate::fields::Fields;

/// The current JSON contract version.
pub const CONTRACT: u32 = 1;

/// The `--fields` projection, installed once by `main` after the flags are
/// settled and only when `--json` took.
///
/// A static rather than a parameter: it is output shaping of an
/// already-produced value, read in exactly one function ([`write`]), and the
/// alternative is a parameter on the sixty-odd `emit` and `write` call
/// sites. Unlike `--at-op`, nothing a verb computes depends on it, so
/// nothing but the envelope needs to know it exists.
static PROJECTION: OnceLock<Fields> = OnceLock::new();

/// Install the `--fields` projection every `data` envelope is cut down by.
/// A second install is ignored: the first one is the command line's.
pub fn project_with(fields: Fields) {
    let _ = PROJECTION.set(fields);
}

/// Serialize `data` inside the versioned envelope and write one line to stdout.
pub fn emit<T: Serialize>(cmd: &str, data: &T) -> Result<()> {
    write(&mut std::io::stdout(), cmd, data)
}

/// Same, writing to an arbitrary sink (used by the log family, which writes
/// through the pager's writer rather than stdout).
pub fn write<W: std::io::Write, T: Serialize>(out: &mut W, cmd: &str, data: &T) -> Result<()> {
    let mut data = serde_json::to_value(data).map_err(Error::repo)?;
    if let Some(fields) = PROJECTION.get() {
        data = fields.project(data)?;
    }
    write_line(
        out,
        &serde_json::json!({ "ff": CONTRACT, "cmd": cmd, "data": data }),
    )
}

/// The error form: `error` replaces `data`, never both.
pub fn emit_error(cmd: &str, err: &Error) -> Result<()> {
    write_line(&mut std::io::stdout(), &error_envelope(cmd, err))
}

/// The error envelope as a value.
fn error_envelope(cmd: &str, err: &Error) -> serde_json::Value {
    serde_json::json!({
        "ff": CONTRACT,
        "cmd": cmd,
        "error": error_object(err),
    })
}

/// The error as a value on its own, the object the envelope carries under
/// `error`: for a report that files one failure among several outcomes,
/// the way `ff push` files a refused branch beside the ones that landed.
pub fn error_object(err: &Error) -> serde_json::Value {
    serde_json::json!({
        "id": err.id(),
        "message": err.to_string(),
        // The same block the human rendering prints, so a machine
        // reading the envelope is told what a terminal would be.
        "exits": crate::explain::exits_for(err),
    })
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

/// `[Y/n]` on stdin. No new dependency, and no selector: this is one
/// question with a default, and a TUI for it would be a TUI to maintain.
/// Callers gate on [`interactive`] first — nothing may prompt when it is false.
pub fn confirm(question: &str) -> Result<bool> {
    use std::io::Write;
    print!("\n{question} [Y/n] ");
    std::io::stdout().flush().map_err(Error::repo)?;
    let mut answer = String::new();
    if std::io::stdin().read_line(&mut answer).is_err() {
        return Ok(false);
    }
    let answer = answer.trim().to_ascii_lowercase();
    Ok(answer.is_empty() || answer == "y" || answer == "yes")
}
