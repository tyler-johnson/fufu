//! The child: one tool call, one child `ff`, one envelope back.
//!
//! The child is an ordinary invocation — `<exe> [-C <cwd>] [--session <s>]
//! <args…> --json` — with stdin closed and `FF_NONINTERACTIVE` set, so
//! nothing it does can differ from what a script running the same line
//! would get. The words arrive already spelled by `tools::Typed::call`;
//! this module runs them, reads the child's stdout, and when that is one
//! envelope, hands it over as structured content beside the text.
//!
//! `isError` is the envelope's kind and not the exit code: an `error`
//! envelope is an error, and a `data` envelope is not, whatever the code
//! beside it. fufu's own `pull` prints a `data` envelope and exits 3 when a
//! rewrite held, and `doctor` prints one and exits 1 as its verdict; both
//! are outcomes the agent reads from the report, not failures. The code
//! itself rides every result a child produced as `_meta.exit`, for a
//! client or harness that wants the number.

use std::path::Path;

use ff_core::Error;
use rmcp::model::{CallToolResult, ContentBlock, JsonObject, MetaObject};

/// The marker the server sets on every child, and the one thing that says
/// an `ff` was started by a tool call rather than typed at a shell. What
/// reads it is `provenance::route`, which stamps the operation's route,
/// the signal `ff op log 'route(tool)'` filters on — and nothing else.
/// The child inherits it through anything it spawns in turn, so an
/// extension's writes under a produced tool say `tool` too.
///
/// Not `FF_NONINTERACTIVE`: that says there is nobody to prompt, which is
/// also true of a setup script, a hook, and CI, and none of those arrived
/// on a tool.
pub const TOOL_CALL: &str = "FF_TOOL_CALL";

/// Whether this process was started by a tool call.
pub fn is_tool_call() -> bool {
    std::env::var_os(TOOL_CALL).is_some_and(|value| !value.is_empty())
}

/// One call, as [`super::tools::Typed::call`] spelled it: the words after
/// `ff`, and the directory to run them in.
pub struct Call {
    pub args: Vec<String>,
    pub cwd: Option<String>,
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
/// there is one, whether the call failed, and the child's exit code when a
/// child ran and exited. `None` is a spawn that failed or a child that
/// died by signal.
fn result(
    text: String,
    structured: Option<serde_json::Value>,
    failed: bool,
    exit: Option<i32>,
) -> CallToolResult {
    let content = vec![ContentBlock::text(text)];
    let mut result = if failed {
        CallToolResult::error(content)
    } else {
        CallToolResult::success(content)
    };
    result.structured_content = structured;
    result.meta = exit.map(|code| {
        let mut meta = JsonObject::new();
        meta.insert("exit".into(), serde_json::Value::from(code));
        MetaObject(meta)
    });
    result
}

/// The verb the envelope names: the first word of the line, which every
/// typed tool's prefix puts there.
fn verb(args: &[String]) -> &str {
    args.first().map_or("", String::as_str)
}

/// The envelope a failure of the server's own carries, in the shape `main`
/// prints for one.
fn refused(args: &[String], err: &Error) -> CallToolResult {
    let envelope = crate::machine::error_envelope(verb(args), err);
    result(envelope.to_string(), Some(envelope), true, None)
}

/// Run one call to completion and shape what came back.
pub async fn run(exe: &Path, session: Option<&str>, call: Call) -> CallToolResult {
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
        // The one mark of a tool-borne call, read by `provenance::route`.
        .env(TOOL_CALL, "1")
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
/// text and as structured content, and is an error exactly when it is an
/// `error` envelope: a `data` envelope at 3 is a held outcome with a
/// report, and one at 1 is `doctor`'s verdict, neither a failure. Anything
/// else — a help page, a crash before the envelope — is text, failed on
/// the status alone, and a crash that printed nothing is synthesized into
/// an envelope so the agent still gets an id. The exit code rides every
/// one of these as `_meta.exit`.
fn shape(args: &[String], output: &std::process::Output) -> CallToolResult {
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let failed = !output.status.success();
    let exit = output.status.code();

    if let Some(envelope) = crate::machine::one_envelope(&stdout) {
        let failed = envelope.get("error").is_some();
        return result(stdout.trim_end().to_string(), Some(envelope), failed, exit);
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
        return result(envelope.to_string(), Some(envelope), true, exit);
    }
    let mut text = stdout.trim_end().to_string();
    if !stderr.trim().is_empty() {
        if !text.is_empty() {
            text.push('\n');
        }
        text.push_str(stderr.trim_end());
    }
    result(text, None, failed, exit)
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
    fn json_rides_every_line_but_help() {
        assert!(wants_json(&args(&["status"])));
        assert!(wants_json(&args(&["op", "log", "-n", "3"])));
        assert!(!wants_json(&args(&["help", "log"])));
        assert!(!wants_json(&args(&["log", "--help"])));
        assert!(!wants_json(&args(&["log", "-h"])));
        assert!(!wants_json(&args(&["--json", "status"])), "never twice");
    }

    #[test]
    fn the_tail_keeps_the_end() {
        let long: String = "x".repeat(3_000);
        assert_eq!(tail(&long).chars().count(), 2_000);
        assert_eq!(tail("  short  "), "short");
    }

    /// What a child left behind: its stdout, its stderr, and the code it
    /// exited with, as `wait` reports the last.
    #[cfg(unix)]
    fn exited(stdout: &str, stderr: &str, code: i32) -> std::process::Output {
        use std::os::unix::process::ExitStatusExt;
        std::process::Output {
            status: std::process::ExitStatus::from_raw(code << 8),
            stdout: stdout.as_bytes().to_vec(),
            stderr: stderr.as_bytes().to_vec(),
        }
    }

    #[cfg(unix)]
    fn exit_of(result: &CallToolResult) -> Option<i64> {
        result
            .meta
            .as_ref()
            .and_then(|meta| meta.0.get("exit"))
            .and_then(serde_json::Value::as_i64)
    }

    /// `isError` is the envelope's kind: a `data` envelope is never an
    /// error, whatever the code beside it, and the code rides `_meta.exit`.
    #[cfg(unix)]
    #[test]
    fn a_data_envelope_is_not_an_error_whatever_the_code() {
        let held = exited(r#"{"ff":1,"cmd":"pull","data":{"held":["topic"]}}"#, "", 3);
        let shaped = shape(&args(&["pull"]), &held);
        assert_ne!(shaped.is_error, Some(true), "a held pull is an outcome");
        assert_eq!(exit_of(&shaped), Some(3));
        assert_eq!(
            shaped
                .structured_content
                .as_ref()
                .and_then(|v| v.get("data")),
            Some(&serde_json::json!({"held": ["topic"]}))
        );

        let verdict = exited(r#"{"ff":1,"cmd":"doctor","data":{"findings":1}}"#, "", 1);
        let shaped = shape(&args(&["doctor"]), &verdict);
        assert_ne!(shaped.is_error, Some(true), "doctor's verdict is data");
        assert_eq!(exit_of(&shaped), Some(1));

        let fine = exited(r#"{"ff":1,"cmd":"status","data":{}}"#, "", 0);
        let shaped = shape(&args(&["status"]), &fine);
        assert_ne!(shaped.is_error, Some(true));
        assert_eq!(exit_of(&shaped), Some(0));
    }

    /// And an `error` envelope is one, carrying its code the same way.
    #[cfg(unix)]
    #[test]
    fn an_error_envelope_is_an_error_and_carries_its_code() {
        let usage = exited(
            r#"{"ff":1,"cmd":"show","error":{"id":"usage/revset-unknown-revision","message":"no","exits":[]}}"#,
            "",
            2,
        );
        let shaped = shape(&args(&["show", "nope"]), &usage);
        assert_eq!(shaped.is_error, Some(true));
        assert_eq!(exit_of(&shaped), Some(2));
    }

    /// A child that died saying nothing on stdout is synthesized into an
    /// `internal` error so the agent still gets an id, and the code rides
    /// with it.
    #[cfg(unix)]
    #[test]
    fn a_silent_crash_is_an_internal_error_with_its_code() {
        let crashed = exited("", "thread 'main' panicked", 1);
        let shaped = shape(&args(&["status"]), &crashed);
        assert_eq!(shaped.is_error, Some(true));
        assert_eq!(exit_of(&shaped), Some(1));
        let envelope = shaped.structured_content.expect("a synthesized envelope");
        assert_eq!(envelope["error"]["id"], "internal");
        assert_eq!(envelope["error"]["message"], "thread 'main' panicked");

        // A help page is text on the status alone, and still carries it.
        let help = exited("usage: ff status", "", 0);
        let shaped = shape(&args(&["help", "status"]), &help);
        assert_ne!(shaped.is_error, Some(true));
        assert!(shaped.structured_content.is_none());
        assert_eq!(exit_of(&shaped), Some(0));

        // A failure of the server's own ran no child, so it carries no code.
        let refusal = refused(&args(&["status"]), &Error::msg("no"));
        assert_eq!(refusal.is_error, Some(true));
        assert!(refusal.meta.is_none());
    }
}
