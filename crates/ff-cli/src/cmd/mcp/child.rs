//! The relay: one tool call, one child `ff`, one envelope back.
//!
//! The child is an ordinary invocation — `<exe> [-C <cwd>] [--session <s>]
//! <args…> --json` — with stdin closed and `FF_NONINTERACTIVE` set, so
//! nothing it does can differ from what a script running the same line
//! would get. The server reads the child's stdout, and when that is one
//! envelope, hands it over as structured content beside the text.
//!
//! `isError` is the envelope's kind and not the exit code: an `error`
//! envelope is an error, and a `data` envelope is not, whatever the code
//! beside it. fufu's own `sync` prints a `data` envelope and exits 3 when a
//! rewrite held, and `doctor` prints one and exits 1 as its verdict; both
//! are outcomes the agent reads from the report, not failures. The code
//! itself rides every result a child produced as `_meta.exit`, for a
//! client or harness that wants the number.

use std::path::Path;

use ff_core::Error;
use rmcp::ErrorData;
use rmcp::model::{CallToolResult, ContentBlock, JsonObject, MetaObject};

/// The verbs the tool does not offer, and the reason. Each owns its
/// stream (`git` passes real git's through, `watch` is a stream of
/// envelopes, `mcp` is this server), talks a person through something
/// (`update`), or wires the machine (`hook`, `unhook`). `extension` is
/// there for a stronger reason than the rest: the registry it writes is
/// the allowlist for everything fufu says about an extension, so an agent
/// that could write it through the tool would be deciding for itself what
/// fufu vouches for. None of them makes sense inside a tool call, and the
/// registry entry for the refusal names a shell as where to run them.
pub const EXCLUDED: [&str; 7] = [
    "git",
    "update",
    "watch",
    "hook",
    "unhook",
    "mcp",
    "extension",
];

/// The marker the relay sets on every child, and the one thing that says
/// an `ff` was started by this tool rather than typed at a shell. What
/// reads it is `cmd::config`, which refuses a write to a policy key
/// through the tool the write would be turning off.
///
/// Not `FF_NONINTERACTIVE`: that says there is nobody to prompt, which is
/// also true of a setup script, a hook, and CI, and none of those should
/// lose the ability to set a tier. Not a list of argument shapes read
/// here either — the relay holds an unparsed word list where global flags
/// may precede the verb, so classifying `config` here would be a second,
/// weaker parser beside clap's, and anything it misspells is a way
/// through. The child has clap's answer, and it inherits this variable
/// through anything it spawns in turn.
///
/// An agent that can set environment variables cannot forge its way past
/// this, because the forgery it would need is the opposite one: the relay
/// sets the marker on the child itself, and the tool's schema carries
/// only `args` and `cwd`, so no call can clear it. Setting it by hand
/// somewhere else only refuses that shell its own policy writes.
pub const TOOL_CALL: &str = "FF_TOOL_CALL";

/// Whether this process was started by the relay.
pub fn is_tool_call() -> bool {
    std::env::var_os(TOOL_CALL).is_some_and(|value| !value.is_empty())
}

/// Which of the two routes a call arrived on.
///
/// The one `ff` tool carries a single set of annotations over everything it
/// relays, and "nothing here is destructive" is honest only while `ff undo`
/// takes all of it back, so the args array serves an extension only when
/// its manifest says `undoable: true`. A tool a declared extension produced
/// states its own `readOnlyHint` and `destructiveHint` and is honest on its
/// own, so it needs neither the blanket promise nor `undoable: true`. The
/// two routes stand together: an undoable extension gets both, and a
/// non-undoable one gets its produced tools.
///
/// The one thing [`refuse_in`] reads this for is that difference. Every
/// other question it asks is asked of both.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Route {
    /// The one `ff` tool's args array, under fufu's own annotations.
    Relay,
    /// A tool a declared extension produced, under its own.
    Produced,
}

/// One call, as the schema shapes it.
pub struct Call {
    pub args: Vec<String>,
    pub cwd: Option<String>,
    /// Which tool the client called. Not a word of the command line — the
    /// child is the same ordinary invocation either way.
    pub route: Route,
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
    Ok(Call {
        args,
        cwd,
        route: Route::Relay,
    })
}

/// The verb the envelope names: the first word, or the map when there is
/// none, on the same rule `main` uses for a command line with no verb.
fn verb(args: &[String]) -> &str {
    args.first().map_or("map", String::as_str)
}

/// The refusal for a call this server does not run: nothing to run, one of
/// the seven verbs that belong in a shell, or an extension the route it
/// came in on does not serve. Raised through `Error::coded` so the explain
/// registry carries it like every other id.
fn refuse(call: &Call) -> Option<Error> {
    refuse_in(&call.args, call.route, crate::registry::read())
}

/// [`refuse`] against a registry of the caller's own. The tests' door, and
/// the reason the one cached reader stays a detail of `refuse`.
///
/// The questions are asked in the order `toolpolicy::served` asks them, and
/// for the same reason: a builtin verb always wins, so the registry is read
/// only for a word clap would decline, and `ff status` reaches no file.
///
/// What the server serves is what the registry says it serves. A declared
/// extension is relayed the way a verb is — the child dispatches to
/// `ff-<name>` itself, and the envelope comes back through `shape` — and an
/// undeclared one is refused here, because fufu has read no manifest for it
/// and so knows neither what it answers to nor whether its writes can be
/// taken back. That is the other half of `fufu.toolPolicy`'s rule, which
/// lets an undeclared `ff <name>` through the shell: refused in both
/// places, the verb would have nowhere at all to run.
///
/// The last question is the one [`Route`] answers. A declared extension
/// saying its writes are not undoable is refused on the args array, because
/// the one tool's annotations promise the opposite of everything it relays,
/// and is not refused on a tool it produced, because that tool states its
/// own hints. `toolPolicy` reads the same rule back and keeps letting a
/// non-undoable `ff <name>` through the shell: the registry says an
/// extension promised tools but never which verb each covers, so the shell
/// is the one place such a verb is certainly still runnable.
fn refuse_in(args: &[String], route: Route, registry: &crate::registry::Registry) -> Option<Error> {
    let first = args.first()?;
    if EXCLUDED.contains(&first.as_str()) {
        return Some(Error::coded(
            "usage/mcp-verb-unavailable",
            format!(
                "ff {first} is not offered through the MCP tool: it owns a stream, wires the \
                 machine, or decides what fufu vouches for, so run it in a shell"
            ),
            vec![format!("ff {first}"), "ff help".into()],
        ));
    }
    // A builtin verb, or a word that is no verb at all — `--help`, `-C`, a
    // global's value. Both are the child's to parse, as they always were.
    if crate::toolpolicy::builtin(first) || !crate::ext::valid_name(first) {
        return None;
    }
    let Some(declared) = registry.get(first.as_str()) else {
        return Some(Error::coded(
            "usage/mcp-extension-undeclared",
            format!(
                "ff {first} is an undeclared extension, which this tool does not serve: run ff \
                 {first} in a shell"
            ),
            vec![
                format!("ff {first}"),
                format!("ff extension add {first}"),
                "ff help".into(),
            ],
        ));
    };
    if route == Route::Relay && !declared.manifest.undoable {
        return Some(Error::coded(
            "usage/mcp-extension-not-undoable",
            format!(
                "ff {first} declares undoable: false, and this one tool's annotations say that \
                 nothing it relays is destructive: call a {first}__<tool> this server lists, \
                 which carries annotations of its own, or run the verb in a shell"
            ),
            vec![format!("ff {first}"), "ff doctor".into()],
        ));
    }
    None
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
/// child ran and exited. `None` is a refusal the relay raised itself, a
/// spawn that failed, or a child that died by signal.
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

/// The envelope a refusal carries, in the shape `main` prints for one.
fn refused(args: &[String], err: &Error) -> CallToolResult {
    let envelope = crate::machine::error_envelope(verb(args), err);
    result(envelope.to_string(), Some(envelope), true, None)
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
    if let Some(err) = refuse(&call) {
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
        // The one mark of a tool-borne call, read by `cmd::config`.
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

    /// A machine that has declared nothing, which is the common one.
    fn bare() -> crate::registry::Registry {
        crate::registry::load(None)
    }

    /// A registry with one name on it, written the way `ff extension add`
    /// writes one. The directory is returned so the caller keeps it alive.
    fn declaring(name: &str, undoable: bool) -> (tempfile::TempDir, crate::registry::Registry) {
        let dir = tempfile::TempDir::new().expect("create tempdir");
        let file = dir.path().join("extensions.json");
        let body = serde_json::json!({
            "ff": crate::machine::CONTRACT,
            "extensions": [{
                "path": format!("/usr/local/bin/ff-{name}"),
                "declared_at": 1788462398,
                "manifest": {
                    "name": name,
                    "version": "0.4.1",
                    "contract": crate::machine::CONTRACT,
                    "verbs": [{"name": "brief", "read_only": true}],
                    "undoable": undoable,
                },
            }],
        });
        std::fs::write(&file, body.to_string()).expect("write registry");
        let registry = crate::registry::load(Some(&file));
        assert!(registry.get(name).is_some(), "the fixture declares {name}");
        (dir, registry)
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
    fn the_shell_only_verbs_are_refused_by_id() {
        for verb in EXCLUDED {
            let err = refuse_in(&args(&[verb, "status"]), Route::Relay, &bare()).expect(verb);
            assert_eq!(err.id(), "usage/mcp-verb-unavailable");
        }
        assert!(refuse_in(&args(&["status"]), Route::Relay, &bare()).is_none());
        assert!(refuse_in(&args(&["op", "log"]), Route::Relay, &bare()).is_none());
        // A word that is no verb and names no extension either: a flag, or
        // the value of a global. The child parses those, as it always did.
        assert!(refuse_in(&args(&["--help"]), Route::Relay, &bare()).is_none());
        assert!(refuse_in(&args(&["-C", "sub", "status"]), Route::Relay, &bare()).is_none());
    }

    /// The tool serves the extensions a person declared and refuses the
    /// rest by id, which is the whole of what the registry decides here.
    #[test]
    fn an_undeclared_extension_is_refused_and_a_declared_one_is_relayed() {
        let err = refuse_in(&args(&["tower", "next"]), Route::Relay, &bare()).expect("undeclared");
        assert_eq!(err.id(), "usage/mcp-extension-undeclared");
        assert!(
            err.exits()
                .iter()
                .any(|exit| exit == "ff extension add tower"),
            "the exit names the declaration: {:?}",
            err.exits()
        );

        let (_dir, declared) = declaring("tower", true);
        assert!(refuse_in(&args(&["tower", "next"]), Route::Relay, &declared).is_none());
        // And declaring one name says nothing about another.
        assert_eq!(
            refuse_in(&args(&["bay", "warm"]), Route::Relay, &declared)
                .map(|err| err.id().to_string()),
            Some("usage/mcp-extension-undeclared".into())
        );
    }

    /// The one tool's annotations say nothing it relays is destructive,
    /// which is honest only of an extension whose writes `ff undo` takes
    /// back — and a tool the extension produced says its own, so the same
    /// verb on that route is not refused. The refusal is the args array's
    /// alone, which is the whole of what [`Route`] decides.
    #[test]
    fn the_undoable_gate_is_the_args_arrays_and_not_a_produced_tools() {
        let (_dir, declared) = declaring("tower", false);
        let err =
            refuse_in(&args(&["tower", "next"]), Route::Relay, &declared).expect("not undoable");
        assert_eq!(err.id(), "usage/mcp-extension-not-undoable");
        assert!(
            err.exits().iter().any(|exit| exit == "ff tower"),
            "a shell is still named: {:?}",
            err.exits()
        );
        assert!(
            err.to_string().contains("tower__<tool>"),
            "and so is the route that does serve it: {err}"
        );

        assert!(
            refuse_in(&args(&["tower", "next"]), Route::Produced, &declared).is_none(),
            "a produced tool carries its own hints"
        );
        // Every other question is still asked of both routes.
        assert_eq!(
            refuse_in(&args(&["bay", "warm"]), Route::Produced, &declared)
                .map(|err| err.id().to_string()),
            Some("usage/mcp-extension-undeclared".into())
        );
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
        let held = exited(r#"{"ff":1,"cmd":"sync","data":{"held":["topic"]}}"#, "", 3);
        let shaped = shape(&args(&["sync"]), &held);
        assert_ne!(shaped.is_error, Some(true), "a held sync is an outcome");
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

        // The relay's own refusal ran no child, so it carries no code.
        let refusal = refused(&args(&["git", "status"]), &Error::msg("no"));
        assert_eq!(refusal.is_error, Some(true));
        assert!(refusal.meta.is_none());
    }
}
