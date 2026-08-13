//! CLI conventions: JSON shapes, exit codes, human output basics.
//! Runs the real `ff` binary against hermetic fixtures.

use std::path::Path;
use std::process::{Command, Output};

use ff_testsupport::Fixture;

fn ff_at(dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_ff"))
        .current_dir(dir)
        .args(args)
        // Hermetic like the fixtures: production discover() reads these.
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .expect("spawn ff")
}

fn ff(fx: &Fixture, args: &[&str]) -> Output {
    ff_at(&fx.path(), args)
}

fn stdout(out: &Output) -> String {
    String::from_utf8(out.stdout.clone()).expect("utf-8 stdout")
}

#[test]
fn status_json_shape() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("one");
    fx.write("a.txt", "changed\n");
    fx.write("new.txt", "untracked\n");
    let out = ff(&fx, &["status", "--json"]);
    assert!(out.status.success());
    let text = stdout(&out);
    assert!(
        text.ends_with('\n') && !text[..text.len() - 1].contains('\n'),
        "one line + one newline"
    );
    let v: serde_json::Value = serde_json::from_str(&text).expect("valid json");
    assert_eq!(v["head"]["state"], "branch");
    assert_eq!(v["head"]["name"], "main");
    assert_eq!(v["head"]["ref"], "refs/heads/main");
    assert_eq!(v["operation"], serde_json::Value::Null);
    assert_eq!(v["upstream"], serde_json::Value::Null);
    assert_eq!(v["unstaged"][0]["path"], "a.txt");
    assert_eq!(v["unstaged"][0]["kind"], "modified");
    assert_eq!(v["untracked"][0], "new.txt");
    assert_eq!(v["staged"].as_array().unwrap().len(), 0);
    assert_eq!(v["conflicts"].as_array().unwrap().len(), 0);
}

#[test]
fn status_json_is_stable_bytes() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("one");
    let a = ff(&fx, &["status", "--json"]);
    let b = ff(&fx, &["status", "--json"]);
    assert_eq!(a.stdout, b.stdout, "identical bytes run to run");
}

#[test]
fn status_human_header() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("one");
    let out = ff(&fx, &["status"]);
    assert!(out.status.success());
    let text = stdout(&out);
    assert!(text.starts_with("on main\n"), "header: {text:?}");
    assert!(text.contains("clean"), "clean tree: {text:?}");
}

#[test]
fn status_human_unborn() {
    let fx = Fixture::new();
    let out = ff(&fx, &["status"]);
    assert!(out.status.success());
    assert!(stdout(&out).starts_with("on main (no commits yet)"));
}

#[test]
fn log_json_envelope_and_limit() {
    let fx = Fixture::new();
    for i in 0..4 {
        fx.write("f.txt", &format!("{i}\n"));
        fx.commit(&format!("c{i}"));
    }
    let out = ff(&fx, &["log", "--json", "-n", "2"]);
    assert!(out.status.success());
    let v: serde_json::Value = serde_json::from_str(&stdout(&out)).unwrap();
    let commits = v["commits"].as_array().unwrap();
    assert_eq!(commits.len(), 2);
    assert_eq!(commits[0]["subject"], "c3");
    for key in [
        "id",
        "short_id",
        "subject",
        "author_name",
        "author_email",
        "time",
    ] {
        assert!(!commits[0][key].is_null(), "missing key {key}");
    }
}

#[test]
fn log_defaults_to_25() {
    let fx = Fixture::new();
    for i in 0..30 {
        fx.write("f.txt", &format!("{i}\n"));
        fx.commit(&format!("c{i}"));
    }
    let out = ff(&fx, &["log", "--json"]);
    let v: serde_json::Value = serde_json::from_str(&stdout(&out)).unwrap();
    assert_eq!(v["commits"].as_array().unwrap().len(), 25);
    let out = ff(&fx, &["log", "--json", "-n", "0"]);
    let v: serde_json::Value = serde_json::from_str(&stdout(&out)).unwrap();
    assert_eq!(
        v["commits"].as_array().unwrap().len(),
        30,
        "-n 0 is unlimited"
    );
}

#[test]
fn log_unborn_is_empty_success() {
    let fx = Fixture::new();
    let out = ff(&fx, &["log", "--json"]);
    assert!(out.status.success(), "unborn log exits 0");
    assert_eq!(stdout(&out), "{\"commits\":[]}\n");
    let human = ff(&fx, &["log"]);
    assert!(human.status.success());
    assert_eq!(stdout(&human), "", "unborn human log prints nothing");
}

#[test]
fn bare_repo_status_errors_log_works() {
    let fx = Fixture::new_bare();
    let status = ff(&fx, &["status"]);
    assert_eq!(status.status.code(), Some(1));
    let stderr = String::from_utf8(status.stderr).unwrap();
    assert!(stderr.starts_with("ff: "), "error convention: {stderr:?}");
    let log = ff(&fx, &["log", "--json"]);
    assert!(log.status.success(), "bare log works");
}

#[test]
fn outside_repo_is_runtime_error() {
    let dir = tempfile::TempDir::new().unwrap();
    let out = ff_at(dir.path(), &["status"]);
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(stderr.starts_with("ff: "));
}

#[test]
fn usage_errors_exit_2() {
    let fx = Fixture::new();
    let unknown_flag = ff(&fx, &["status", "--nope"]);
    assert_eq!(unknown_flag.status.code(), Some(2));
    let no_command = ff(&fx, &[]);
    assert_eq!(no_command.status.code(), Some(2));
    let bad_count = ff(&fx, &["log", "-n", "many"]);
    assert_eq!(bad_count.status.code(), Some(2));
}

#[test]
fn merge_conflict_renders_sections() {
    let fx = Fixture::new();
    fx.write("conflict.txt", "base\n");
    fx.commit("base");
    fx.git(&["checkout", "-q", "-b", "other"]);
    fx.write("conflict.txt", "theirs\n");
    fx.commit("theirs");
    fx.git(&["checkout", "-q", "main"]);
    fx.write("conflict.txt", "ours\n");
    fx.commit("ours");
    let merge = fx.try_git(&["merge", "other"]);
    assert!(!merge.status.success());

    let out = ff(&fx, &["status"]);
    let text = stdout(&out);
    assert!(text.starts_with("on main · merging\n"), "header: {text:?}");
    assert!(
        text.contains("conflicts:\n  conflict.txt\n"),
        "body: {text:?}"
    );
}
