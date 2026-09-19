//! `--fields`: the projection of a `--json` payload to dotted paths. Runs the
//! real `ff` binary against hermetic fixtures; the projection's own rule is
//! unit-tested beside it in `src/fields.rs`.

use std::path::Path;
use std::process::{Command, Output};

use ff_testsupport::Fixture;
use ff_testsupport::fixtures::null_device;
use serde_json::Value;

fn ff_at(dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_ff"))
        .current_dir(dir)
        .args(args)
        // Hermetic like the fixtures: production discover() reads these.
        .env("GIT_CONFIG_GLOBAL", null_device())
        .env("GIT_CONFIG_SYSTEM", null_device())
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env_remove("GIT_AUTHOR_NAME")
        .env_remove("GIT_AUTHOR_EMAIL")
        .env_remove("GIT_AUTHOR_DATE")
        .env_remove("GIT_COMMITTER_NAME")
        .env_remove("GIT_COMMITTER_EMAIL")
        .env_remove("GIT_COMMITTER_DATE")
        .env_remove("EMAIL")
        .output()
        .expect("spawn ff")
}

fn ff(fx: &Fixture, args: &[&str]) -> Output {
    ff_at(&fx.path(), args)
}

fn stdout(out: &Output) -> String {
    String::from_utf8(out.stdout.clone()).expect("utf-8 stdout")
}

fn stderr(out: &Output) -> String {
    String::from_utf8(out.stderr.clone()).expect("utf-8 stderr")
}

fn json(out: &Output) -> Value {
    serde_json::from_str(&stdout(out)).expect("valid json")
}

/// The keys of an object, in the order the payload carries them.
fn keys(value: &Value) -> Vec<&str> {
    value
        .as_object()
        .expect("an object")
        .keys()
        .map(String::as_str)
        .collect()
}

/// Two commits and an open edit, so log has rows and an open block.
fn fixture() -> Fixture {
    let fx = Fixture::new();
    fx.set_config("user.name", "Fields Tester");
    fx.set_config("user.email", "fields@test.test");
    fx.write("a.txt", "a\n");
    fx.commit("first");
    fx.write("b.txt", "b\n");
    fx.commit("second");
    fx.write("a.txt", "a2\n");
    fx
}

/// A success envelope with `ff` and `cmd` untouched, returning `data`.
fn payload(out: &Output, cmd: &str) -> Value {
    assert_eq!(out.status.code(), Some(0), "{}", stderr(out));
    let envelope = json(out);
    assert_eq!(envelope["ff"], 1);
    assert_eq!(envelope["cmd"], cmd);
    assert_eq!(keys(&envelope), ["ff", "cmd", "data"]);
    envelope["data"].clone()
}

/// The error envelope for a refusal: exit 2, the id, and nothing else on stdout.
fn refusal(out: &Output, cmd: &str, id: &str) -> Value {
    assert_eq!(out.status.code(), Some(2), "{}", stderr(out));
    assert_eq!(stdout(out).lines().count(), 1, "one line: {}", stdout(out));
    let envelope = json(out);
    assert_eq!(envelope["cmd"], cmd);
    assert_eq!(keys(&envelope), ["ff", "cmd", "error"]);
    assert_eq!(envelope["error"]["id"], id);
    envelope["error"].clone()
}

#[test]
fn log_rows_keep_only_the_named_keys() {
    let fx = fixture();
    let out = ff(
        &fx,
        &["log", "--json", "--fields", "commits.subject,commits.body"],
    );
    let data = payload(&out, "log");
    assert_eq!(keys(&data), ["commits"]);
    let rows = data["commits"].as_array().expect("rows");
    assert_eq!(rows.len(), 2);
    for row in rows {
        assert_eq!(keys(row), ["subject", "body"]);
    }
    assert_eq!(rows[0]["subject"], "second");
    assert_eq!(rows[1]["subject"], "first");
}

#[test]
fn a_nested_path_keeps_its_nesting() {
    let fx = fixture();
    let out = ff(
        &fx,
        &["log", "--json", "--fields", "open.subject,open.pending"],
    );
    let data = payload(&out, "log");
    assert_eq!(keys(&data), ["open"]);
    assert_eq!(keys(&data["open"]), ["subject", "pending"]);
}

#[test]
fn an_array_path_maps_over_rows() {
    let fx = fixture();
    let out = ff(&fx, &["log", "--json", "--fields", "commits.id"]);
    let data = payload(&out, "log");
    for row in data["commits"].as_array().expect("rows") {
        assert_eq!(keys(row), ["id"]);
        assert!(row["id"].is_string());
    }
    // The `write` route, through the pager's writer rather than stdout.
    let out = ff(&fx, &["op", "log", "--json", "--fields", "ops.id"]);
    let data = payload(&out, "op log");
    let ops = data["ops"].as_array().expect("ops");
    assert!(!ops.is_empty());
    for op in ops {
        assert_eq!(keys(op), ["id"]);
    }
}

#[test]
fn a_whole_key_outranks_its_children() {
    let fx = fixture();
    let whole = ff(&fx, &["log", "--json"]);
    let out = ff(&fx, &["log", "--json", "--fields", "commits,commits.id"]);
    assert_eq!(
        payload(&out, "log")["commits"],
        payload(&whole, "log")["commits"]
    );
}

#[test]
fn status_and_show_project_too() {
    let fx = fixture();
    let out = ff(&fx, &["status", "--json", "--fields", "head,changes"]);
    let data = payload(&out, "status");
    assert_eq!(keys(&data), ["head", "changes"]);
    assert_eq!(data["head"]["name"], "main");
    assert_eq!(data["changes"][0]["path"], "a.txt");

    let out = ff(
        &fx,
        &["show", "HEAD", "--json", "--fields", "subject,parents"],
    );
    let data = payload(&out, "show");
    assert_eq!(keys(&data), ["subject", "parents"]);
    assert_eq!(data["subject"], "second");
    assert_eq!(data["parents"].as_array().map(Vec::len), Some(1));
}

#[test]
fn a_missing_path_is_refused() {
    let fx = fixture();
    let out = ff(&fx, &["log", "--json", "--fields", "commits.subjet"]);
    let error = refusal(&out, "log", "usage/no-such-field");
    let message = error["message"].as_str().expect("message");
    assert!(
        message.contains("--fields commits.subjet matches nothing: `commits` has no `subjet`"),
        "{message}"
    );
    assert!(
        message.contains("subject"),
        "names the keys there: {message}"
    );
    assert_eq!(
        error["exits"],
        serde_json::json!(["ff explain usage/no-such-field"])
    );

    // The `write` route refuses with the same id rather than folding it
    // into an io error on the way out of the pager.
    let out = ff(&fx, &["op", "log", "--json", "--fields", "ops.typo"]);
    refusal(&out, "op log", "usage/no-such-field");
}

#[test]
fn a_key_a_view_dropped_is_missing() {
    let fx = fixture();
    let out = ff(&fx, &["show", "HEAD", "--json", "--fields", "changes"]);
    assert_eq!(keys(&payload(&out, "show")), ["changes"]);
    let out = ff(
        &fx,
        &[
            "show",
            "HEAD",
            "--no-patch",
            "--json",
            "--fields",
            "changes",
        ],
    );
    let error = refusal(&out, "show", "usage/no-such-field");
    let message = error["message"].as_str().expect("message");
    assert!(
        message.contains("the payload has no `changes`"),
        "{message}"
    );
}

#[test]
fn a_null_prefix_is_refused() {
    let fx = fixture();
    let out = ff(
        &fx,
        &["log", "-r", "HEAD", "--json", "--fields", "open.subject"],
    );
    let error = refusal(&out, "log", "usage/no-such-field");
    assert_eq!(
        error["message"],
        "--fields open.subject matches nothing: `open` is null"
    );
}

#[test]
fn fields_without_json_is_refused() {
    let fx = fixture();
    let out = ff(&fx, &["status", "--fields", "head"]);
    assert_eq!(out.status.code(), Some(2));
    assert_eq!(stdout(&out), "");
    let err = stderr(&out);
    assert!(
        err.contains("--fields projects JSON output and needs --json"),
        "{err}"
    );
    assert!(err.contains("ff help"), "the registry's exit: {err}");
    // Either side of the verb: it is global the way --json is.
    for args in [
        ["--json", "status", "--fields", "head"],
        ["status", "--json", "--fields", "head"],
    ] {
        let out = ff(&fx, &args);
        assert_eq!(keys(&payload(&out, "status")), ["head"]);
    }
}

#[test]
fn a_malformed_list_is_refused() {
    let fx = fixture();
    for list in ["", "a..b"] {
        let out = ff(&fx, &["status", "--json", "--fields", list]);
        // Refused before dispatch, so the envelope names the map.
        let error = refusal(&out, "map", "usage/bad-flags");
        let message = error["message"].as_str().expect("message");
        assert!(message.starts_with("--fields"), "{list:?}: {message}");
    }
}

#[test]
fn an_error_envelope_is_never_projected() {
    let fx = fixture();
    let out = ff(&fx, &["show", "nope", "--json", "--fields", "subject"]);
    let error = refusal(&out, "show", "usage/revset-unknown-revision");
    assert_eq!(keys(&error), ["id", "message", "exits"]);
}

#[test]
fn fields_is_inert_where_json_is() {
    let fx = fixture();
    let plain = ff(&fx, &["git", "status"]);
    let out = ff(&fx, &["--json", "--fields", "x", "git", "status"]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert_eq!(stdout(&out), stdout(&plain));
    assert!(stdout(&out).contains("a.txt"));
}
