//! PATH extension dispatch: `ff <name>` finds `ff-<name>` the way git finds
//! its own, and only after a builtin verb has already declined the word.

#![cfg(unix)]

use std::fs::Permissions;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use ff_testsupport::Fixture;
use ff_testsupport::fixtures::null_device;
use tempfile::TempDir;

fn ff_at(dir: &Path, bin: Option<&Path>, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_ff"))
        .current_dir(dir)
        .args(args)
        // Hermetic like the fixtures: production discover() reads these.
        .env("GIT_CONFIG_GLOBAL", null_device())
        .env("GIT_CONFIG_SYSTEM", null_device())
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env_remove("FF_SESSION")
        // Prepend the extension's directory to the PATH the test owns.
        .env(
            "PATH",
            match bin {
                Some(bin) => format!("{}:{}", bin.display(), process_path()),
                None => process_path(),
            },
        )
        .output()
        .expect("spawn ff")
}

fn process_path() -> String {
    std::env::var_os("PATH")
        .map(|path| path.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn ff_ext(fx: &Fixture, bin: &Path, args: &[&str]) -> Output {
    ff_at(&fx.path(), Some(bin), args)
}

/// An `ff-<name>` shell script in a fresh directory the caller keeps alive.
fn ext_bin(name: &str, body: &str) -> (TempDir, PathBuf) {
    let dir = TempDir::new().expect("create tempdir");
    let path = dir.path().join(format!("ff-{name}"));
    std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).expect("write script");
    std::fs::set_permissions(&path, Permissions::from_mode(0o755)).expect("chmod script");
    (dir, path)
}

fn stdout(out: &Output) -> String {
    String::from_utf8(out.stdout.clone()).expect("utf-8 stdout")
}

fn stderr(out: &Output) -> String {
    String::from_utf8(out.stderr.clone()).expect("utf-8 stderr")
}

#[test]
fn an_extension_on_path_receives_the_rest_of_the_argv() {
    let fx = Fixture::new();
    let (bin_dir, _) = ext_bin("tower", "echo \"$@\"");
    let out = ff_ext(&fx, bin_dir.path(), &["tower", "next", "--peek"]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    assert_eq!(stdout(&out), "next --peek\n");
}

#[test]
fn the_extensions_exit_code_is_the_command_s() {
    let fx = Fixture::new();
    let (bin_dir, _) = ext_bin("tower", "exit 42");
    let out = ff_ext(&fx, bin_dir.path(), &["tower", "next"]);
    assert!(!out.status.success());
    assert_eq!(out.status.code(), Some(42));
}

#[test]
fn the_session_reaches_the_child_in_the_environment() {
    let fx = Fixture::new();
    let (bin_dir, _) = ext_bin("tower", "echo \"$FF_SESSION\"");
    let out = ff_ext(
        &fx,
        bin_dir.path(),
        &["--session", "flight-9", "tower", "go"],
    );
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    assert_eq!(stdout(&out), "flight-9\n");
}

#[test]
fn a_builtin_wins_over_an_extension() {
    let fx = Fixture::new();
    let (bin_dir, _) = ext_bin("status", "echo HIJACKED");
    let out = ff_ext(&fx, bin_dir.path(), &["status"]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    assert!(!stdout(&out).contains("HIJACKED"));
}

#[test]
fn an_unknown_word_still_says_unrecognized_subcommand() {
    let fx = Fixture::new();
    let (bin_dir, _) = ext_bin("tower", "echo tower");
    let out = ff_ext(&fx, bin_dir.path(), &["bogus"]);
    assert!(!out.status.success());
    assert!(
        stderr(&out).contains("unrecognized subcommand"),
        "stderr: {}",
        stderr(&out)
    );
}
