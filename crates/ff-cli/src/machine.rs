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
    write_line(&mut std::io::stdout(), &error_envelope(cmd, err))
}

/// The error envelope as a value, for the one caller that hands it to a
/// client rather than printing it: `ff mcp`'s refusal of a verb it does not
/// relay carries exactly what `emit_error` would have printed.
pub fn error_envelope(cmd: &str, err: &Error) -> serde_json::Value {
    serde_json::json!({
        "ff": CONTRACT,
        "cmd": cmd,
        "error": {
            "id": err.id(),
            "message": err.to_string(),
            // The same block the human rendering prints, so a machine
            // reading the envelope is told what a terminal would be.
            "exits": crate::explain::exits_for(err),
        },
    })
}

/// Exactly one JSON object carrying an `ff` key, and nothing else.
///
/// The rule for reading an envelope back, beside the rules for writing one:
/// the MCP relay applies it to a child `ff`'s stdout, and the `--ff-manifest`
/// handshake to an extension's. Anything else — a banner, a progress line, a
/// pretty-printed envelope — is not one envelope on one line, and both
/// callers say so rather than guessing at what was meant.
pub fn one_envelope(stdout: &str) -> Option<serde_json::Value> {
    let line = stdout.trim();
    if line.is_empty() || line.contains('\n') {
        return None;
    }
    let value: serde_json::Value = serde_json::from_str(line).ok()?;
    value.get("ff")?;
    Some(value)
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

#[cfg(test)]
mod tests {
    use super::*;

    /// The rule both readers of an envelope share.
    #[test]
    fn one_envelope_means_exactly_one() {
        assert!(one_envelope("{\"ff\":1,\"cmd\":\"status\",\"data\":{}}\n").is_some());
        assert!(
            one_envelope("{\"cmd\":\"status\"}\n").is_none(),
            "no ff key"
        );
        assert!(
            one_envelope("{\"ff\":1}\n{\"ff\":1}\n").is_none(),
            "two lines"
        );
        assert!(one_envelope("Usage: ff log\n").is_none());
        assert!(one_envelope("").is_none());
    }
}
