//! Color painting on mutation verbs: when stdout is piped (non-TTY) color
//! is off so paint_* is the identity function and every byte is unchanged.

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

/// After a commit the stdout contains "closed " and "undo: ff undo" with
/// no ANSI escape byte anywhere — color is off when piped.
#[test]
fn commit_output_plain_when_piped() {
    let fx = Fixture::new();
    fx.set_config("user.name", "Test User");
    fx.set_config("user.email", "test@user.test");
    fx.write("a.txt", "a\n");
    fx.commit("init");
    fx.write("a.txt", "changed\n");

    let out = ff(&fx, &["commit", "-m", "update"]);
    assert!(out.status.success(), "commit succeeded");
    let text = stdout(&out);
    assert!(text.starts_with("closed "), "starts with closed: {text:?}");
    assert!(
        text.contains("undo: ff undo"),
        "undo hint present: {text:?}"
    );
    assert!(
        !text.as_bytes().contains(&b'\x1b'),
        "no ANSI escape when piped: {text:?}"
    );
}

/// Switching branches produces plain text with no ANSI escape when piped.
#[test]
fn switch_output_plain_when_piped() {
    let fx = Fixture::new();
    fx.set_config("user.name", "Test User");
    fx.set_config("user.email", "test@user.test");
    fx.write("a.txt", "a\n");
    fx.commit("init");
    fx.git(&["branch", "other"]);

    let out = ff(&fx, &["switch", "other"]);
    assert!(out.status.success(), "switch succeeded");
    let text = stdout(&out);
    assert!(
        text.contains("switched to other"),
        "switch confirmation present: {text:?}"
    );
    assert!(
        !text.as_bytes().contains(&b'\x1b'),
        "no ANSI escape when piped: {text:?}"
    );
}

/// Existing assertions on mutation verb output still pass unmodified —
/// paint_* with color off is the identity function so nothing observable
/// changes. This replicates the checks from cli.rs tests to pin that
/// property in this file.
#[test]
fn verb_output_bytes_unchanged() {
    // commit_closes_described_empty_change: stdout starts_with "closed "
    {
        let fx = Fixture::new();
        fx.set_config("user.name", "Test User");
        fx.set_config("user.email", "test@user.test");
        fx.write("a.txt", "a\n");
        fx.commit("init");
        let out = ff(&fx, &["describe", "-m", "planned work"]);
        assert!(out.status.success());
        let out = ff(&fx, &["commit"]);
        assert!(out.status.success());
        assert!(
            stdout(&out).starts_with("closed "),
            "closed prefix preserved"
        );
    }

    // commit_totally_empty_refuses: stdout contains "nothing to close on main"
    {
        let fx = Fixture::new();
        fx.set_config("user.name", "Test User");
        fx.set_config("user.email", "test@user.test");
        fx.write("a.txt", "a\n");
        fx.commit("init");
        let out = ff(&fx, &["commit"]);
        assert!(out.status.success());
        assert!(
            stdout(&out).contains("nothing to close on main"),
            "refusal message preserved"
        );
    }
}
