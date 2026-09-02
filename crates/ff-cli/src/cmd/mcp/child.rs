//! The relay: one tool call, one child `ff`, one envelope back.
//!
//! The child is an ordinary invocation — `<exe> [-C <cwd>] [--session <s>]
//! <args…> --json` — with stdin closed and `FF_NONINTERACTIVE` set, so
//! nothing it does can differ from what a script running the same line
//! would get. The server reads the child's stdout, and when that is one
//! envelope, hands it over as structured content beside the text.

use std::path::Path;

use ff_core::Error;
use rmcp::ErrorData;
use rmcp::model::{CallToolResult, ContentBlock, JsonObject};

/// The verbs the tool does not offer, and the reason, in the order
/// `ff --help` lists them. Each owns its stream (`git` passes real git's
/// through, `watch` is a stream of envelopes, `mcp` is this server), talks
/// a person through something (`update`), or wires the machine (`hook`,
/// `unhook`). None of them makes sense inside a tool call, and the
/// registry entry for the refusal names a shell as where to run them.
pub const EXCLUDED: [&str; 6] = ["git", "update", "watch", "hook", "unhook", "mcp"];

/// One call, as the schema shapes it.
pub struct Call {
    pub args: Vec<String>,
    pub cwd: Option<String>,
}

/// The schema, enforced. `Err` is a JSON-RPC error, which is the right
/// answer for input the tool's own schema already forbids — a client that
/// sends it has a bug, and the agent is not the one to tell.
pub fn parse(arguments: Option<JsonObject>) -> Result<Call, ErrorData> {
    let mut arguments = arguments.unwrap_or_default();
    let args = match arguments.remove("args") {
        Some(serde_json::Value::Array(items)) => items
            .into_iter()
            .map(|item| match item {
                serde_json::Value::String(word) => Ok(word),
                other => Err(ErrorData::invalid_params(
                    format!("args items must be strings; got {other}"),
                    None,
                )),
            })
            .collect::<Result<Vec<_>, _>>()?,
        Some(other) => {
            return Err(ErrorData::invalid_params(
                format!("args must be an array of strings; got {other}"),
                None,
            ));
        }
        None => {
            return Err(ErrorData::invalid_params(
                "args is required: the command line after `ff`, one word per item",
                None,
            ));
        }
    };
    let cwd = match arguments.remove("cwd") {
        None | Some(serde_json::Value::Null) => None,
        Some(serde_json::Value::String(dir)) => Some(dir),
        Some(other) => {
            return Err(ErrorData::invalid_params(
                format!("cwd must be a string; got {other}"),
                None,
            ));
        }
    };
    Ok(Call { args, cwd })
}

/// The verb the envelope names: the first word, or the map when there is
/// none, on the same rule `main` uses for a command line with no verb.
fn verb(args: &[String]) -> &str {
    args.first().map_or("map", String::as_str)
}

/// The refusal for a call the tool does not relay: nothing to run, or one
/// of the six verbs that belong in a shell. Raised through `Error::coded`
/// so the explain registry carries it like every other id.
fn refuse(args: &[String]) -> Option<Error> {
    let first = args.first()?;
    if !EXCLUDED.contains(&first.as_str()) {
        return None;
    }
    Some(Error::coded(
        "usage/mcp-verb-unavailable",
        format!(
            "ff {first} is not offered through the MCP tool: it owns a stream or wires the \
             machine, so run it in a shell"
        ),
        vec![format!("ff {first}"), "ff help".into()],
    ))
}

/// Whether `--json` should ride the child. clap's help does not take it,
/// and a line that already carries it must not carry it twice.
fn wants_json(args: &[String]) -> bool {
    !(args.first().is_some_and(|a| a == "help")
        || args
            .iter()
            .any(|a| a == "-h" || a == "--help" || a == "--json"))
}

/// One result: the text the agent reads, the envelope beside it when
/// there is one, and whether the call failed.
fn result(text: String, structured: Option<serde_json::Value>, failed: bool) -> CallToolResult {
    let content = vec![ContentBlock::text(text)];
    let mut result = if failed {
        CallToolResult::error(content)
    } else {
        CallToolResult::success(content)
    };
    result.structured_content = structured;
    result
}

/// The envelope a refusal carries, in the shape `main` prints for one.
fn refused(args: &[String], err: &Error) -> CallToolResult {
    let envelope = crate::machine::error_envelope(verb(args), err);
    result(envelope.to_string(), Some(envelope), true)
}

/// Run one call to completion and shape what came back.
pub async fn run(exe: &Path, session: Option<&str>, call: Call) -> CallToolResult {
    if call.args.is_empty() {
        let err = Error::coded(
            "usage/mcp-verb-unavailable",
            "args is empty: name a verb, such as [\"status\"]",
            vec!["ff status".into(), "ff help".into()],
        );
        return refused(&call.args, &err);
    }
    if let Some(err) = refuse(&call.args) {
        return refused(&call.args, &err);
    }

    let mut cmd = tokio::process::Command::new(exe);
    if let Some(cwd) = &call.cwd {
        cmd.arg("-C").arg(cwd);
    }
    if let Some(session) = session {
        cmd.arg("--session").arg(session);
    }
    cmd.args(&call.args);
    if wants_json(&call.args) {
        cmd.arg("--json");
    }
    cmd.stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        // Belt and braces: stdin is already not a terminal, and this says
        // so to a verb that checks the variable first.
        .env("FF_NONINTERACTIVE", "1")
        // A client cancelling the call drops the future, and the child
        // must not outlive it.
        .kill_on_drop(true);

    let output = match cmd.output().await {
        Ok(output) => output,
        Err(err) => {
            let failure = Error::msg(format!("could not run {}: {err}", exe.display()));
            return refused(&call.args, &failure);
        }
    };
    shape(&call.args, &output)
}

/// What the client sees. One envelope on stdout is handed over whole, as
/// text and as structured content; anything else — a help page, a crash
/// before the envelope — is text, and a crash that printed nothing is
/// synthesized into an envelope so the agent still gets an id.
fn shape(args: &[String], output: &std::process::Output) -> CallToolResult {
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let failed = !output.status.success();

    if let Some(envelope) = one_envelope(&stdout) {
        return result(stdout.trim_end().to_string(), Some(envelope), failed);
    }
    if stdout.trim().is_empty() && !output.status.success() {
        let tail = tail(&stderr);
        let envelope = serde_json::json!({
            "ff": crate::machine::CONTRACT,
            "cmd": verb(args),
            "error": {
                "id": "internal",
                "message": if tail.is_empty() {
                    format!("ff exited with {} and said nothing", output.status)
                } else {
                    tail
                },
                "exits": [],
            },
        });
        return result(envelope.to_string(), Some(envelope), failed);
    }
    let mut text = stdout.trim_end().to_string();
    if !stderr.trim().is_empty() {
        if !text.is_empty() {
            text.push('\n');
        }
        text.push_str(stderr.trim_end());
    }
    result(text, None, failed)
}

/// Exactly one JSON object carrying an `ff` key, and nothing else.
fn one_envelope(stdout: &str) -> Option<serde_json::Value> {
    let line = stdout.trim();
    if line.is_empty() || line.contains('\n') {
        return None;
    }
    let value: serde_json::Value = serde_json::from_str(line).ok()?;
    value.get("ff")?;
    Some(value)
}

/// The last of stderr, for a crash message: enough to read, not enough to
/// flood the agent's context.
fn tail(stderr: &str) -> String {
    const KEEP: usize = 2_000;
    let trimmed = stderr.trim();
    let count = trimmed.chars().count();
    if count <= KEEP {
        return trimmed.to_string();
    }
    let skip = count - KEEP;
    trimmed.chars().skip(skip).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(words: &[&str]) -> Vec<String> {
        words.iter().map(|w| w.to_string()).collect()
    }

    #[test]
    fn the_schema_is_enforced_before_anything_runs() {
        assert!(parse(None).is_err(), "args is required");
        let bad = serde_json::json!({ "args": "status" });
        let serde_json::Value::Object(bad) = bad else {
            unreachable!()
        };
        assert!(parse(Some(bad)).is_err(), "args must be an array");
        let bad = serde_json::json!({ "args": ["status", 1] });
        let serde_json::Value::Object(bad) = bad else {
            unreachable!()
        };
        assert!(parse(Some(bad)).is_err(), "items must be strings");
        let good = serde_json::json!({ "args": ["status"], "cwd": "/tmp" });
        let serde_json::Value::Object(good) = good else {
            unreachable!()
        };
        let call = parse(Some(good)).unwrap();
        assert_eq!(call.args, args(&["status"]));
        assert_eq!(call.cwd.as_deref(), Some("/tmp"));
    }

    #[test]
    fn the_six_verbs_are_refused_by_id() {
        for verb in EXCLUDED {
            let err = refuse(&args(&[verb, "status"])).expect(verb);
            assert_eq!(err.id(), "usage/mcp-verb-unavailable");
        }
        assert!(refuse(&args(&["status"])).is_none());
        assert!(refuse(&args(&["op", "log"])).is_none());
    }

    #[test]
    fn json_rides_every_line_but_help() {
        assert!(wants_json(&args(&["status"])));
        assert!(wants_json(&args(&["op", "log", "-n", "3"])));
        assert!(!wants_json(&args(&["help", "log"])));
        assert!(!wants_json(&args(&["log", "--help"])));
        assert!(!wants_json(&args(&["log", "-h"])));
        assert!(!wants_json(&args(&["--json", "status"])), "never twice");
    }

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

    #[test]
    fn the_tail_keeps_the_end() {
        let long: String = "x".repeat(3_000);
        assert_eq!(tail(&long).chars().count(), 2_000);
        assert_eq!(tail("  short  "), "short");
    }
}
