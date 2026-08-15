//! Theme integration tests for doctor and config — verifies that palette
//! migration did not change output structure or exit codes.

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

#[test]
fn doctor_output_unchanged_without_color() {
    let fx = Fixture::new();

    let out = ff(&fx, &["doctor"]);
    let text = stdout(&out);

    // Doctor always prints rows with level words; assert structure, not escapes.
    assert!(
        text.lines()
            .any(|l| l.trim().starts_with("ok") || l.trim().starts_with("info")),
        "doctor output should contain ok or info rows: {}",
        text
    );

    // The repository row should be present because the fixture has a repo.
    assert!(
        text.lines().any(|l| l.contains("repository")),
        "doctor output should contain a repository row: {}",
        text
    );

    // Summary line appears at the end.
    assert!(
        text.contains("finding") || text.contains("no findings"),
        "doctor output should contain a summary line: {}",
        text
    );
}

#[test]
fn doctor_exit_codes_unchanged() {
    let fx = Fixture::new();

    // Create a proper fufu snapshot so doctor reports healthy (exit 0).
    fx.write("a.txt", "a\n");
    let snap = ff(&fx, &["-m", "initial"]);
    assert!(snap.status.success(), "snapshot should succeed");

    let out = ff(&fx, &["doctor"]);
    assert!(
        out.status.success(),
        "doctor on a healthy repo should exit 0, got {:?\n}{}",
        out.status.code(),
        stdout(&out)
    );
}

#[test]
fn config_default_marker_still_present() {
    let fx = Fixture::new();

    let out = ff(&fx, &["config"]);
    let text = stdout(&out);

    // Every setting that has not been set shows the (default) marker.
    assert!(
        text.contains("(default)"),
        "config output should mark unset settings with (default): {}",
        text
    );
}
