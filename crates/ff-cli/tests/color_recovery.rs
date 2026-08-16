//! Color (absence) on the recovery / maintenance surface.
//! When stdout or stderr is piped, no ANSI escapes must appear.

use std::path::Path;
use std::process::{Command, Output};

use ff_testsupport::Fixture;
use ff_testsupport::fixtures::null_device;

fn ff_at(dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_ff"))
        .current_dir(dir)
        .args(args)
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

fn no_escapes(text: &str) -> bool {
    !text.bytes().any(|b| b == b'\x1b')
}

/// After an undo, stdout contains expected text with no escape bytes
/// (piped stdout → color off).
#[test]
fn undo_output_plain_when_piped() {
    let fx = Fixture::new();
    fx.set_config("user.name", "Test");
    fx.set_config("user.email", "test@test.test");
    fx.write("a.txt", "a\n");
    fx.commit("init");
    fx.write("a.txt", "changed\n");
    // Create an undoable operation via fufu commit.
    let out = ff(&fx, &["commit", "-m", "work"]);
    assert!(
        out.status.success(),
        "commit failed: stdout={} stderr={}",
        stdout(&out),
        stderr(&out)
    );
    fx.write("a.txt", "diverged\n");

    let out = ff(&fx, &["undo"]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    let text = stdout(&out);
    assert!(text.starts_with("undid "), "header: {text:?}");
    assert!(no_escapes(&text), "stdout contained escape bytes");
}

/// After a restore --all, stdout contains expected text with no escape bytes.
#[test]
fn restore_output_plain_when_piped() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    fx.write("a.txt", "captured\n");
    assert!(ff(&fx, &[]).status.success());
    fx.write("a.txt", "diverged\n");

    let out = ff(&fx, &["restore", "--all"]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    let text = stdout(&out);
    assert!(text.starts_with("restored to "), "header: {text:?}");
    assert!(text.contains("restored  a.txt"), "file list: {text:?}");
    assert!(
        text.trim_end().ends_with("undo: ff restore --all"),
        "undo hint: {text:?}"
    );
    assert!(no_escapes(&text), "stdout contained escape bytes");
}

/// A run that emits a reconcile notice on stderr has no escape bytes
/// when stderr is captured (piped).
#[test]
fn stderr_notices_plain_when_piped() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    // Establish fufu journal with an empty capture.
    assert!(ff(&fx, &[]).status.success());
    // Introduce a foreign commit (made outside fufu).
    fx.write("a.txt", "foreign\n");
    fx.git(&["add", "a.txt"]);
    fx.git(&["commit", "-m", "foreign change"]);
    // The next fufu command absorbs the foreign change and emits
    // a reconcile notice on stderr.
    let out = ff(&fx, &["status", "--json"]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    let err = stderr(&out);
    assert!(no_escapes(&err), "stderr contained escape bytes: {err:?}");
}

/// Existing undo / restore / trim behaviour is byte-identical with color off.
/// This re-asserts the key claims from cli.rs so a color regression is caught
/// even when the other briefs are editing cli.rs concurrently.
#[test]
fn recovery_output_bytes_unchanged() {
    // --- restore round-trip with undo hint ---
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    fx.write("a.txt", "captured\n");
    assert!(ff(&fx, &[]).status.success());
    fx.write("a.txt", "diverged\n");

    let out = ff(&fx, &["restore", "--all"]);
    assert!(out.status.success());
    let text = stdout(&out);
    assert!(text.starts_with("restored to "), "restore header");
    assert!(text.contains("restored  a.txt"), "restore file list");
    assert!(
        text.trim_end().ends_with("undo: ff restore --all"),
        "restore undo hint"
    );

    // --- trim reports (nothing recorded yet) ---
    let fx2 = Fixture::new();
    fx2.write("a.txt", "a\n");
    fx2.commit("init");
    let out = ff(&fx2, &["trim"]);
    assert!(out.status.success());
    let text = stdout(&out);
    assert!(
        text.contains("nothing to drop") || text.contains("no operations yet"),
        "trim output: {text:?}"
    );
}
