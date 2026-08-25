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

/// The other two thirds of the handshake `DESIGN.md` promised: which
/// repository this was invoked against, and which JSON contract the child is
/// about to parse.
#[test]
fn the_repository_and_the_contract_reach_the_child_too() {
    let fx = Fixture::new();
    let (bin_dir, _) = ext_bin("tower", "echo \"$FF_REPO\"; echo \"$FF_CONTRACT\"");
    let out = ff_ext(&fx, bin_dir.path(), &["tower", "go"]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    let text = stdout(&out);
    let mut lines = text.lines();
    let repo = lines.next().expect("FF_REPO line");
    assert!(
        ff_testsupport::paths::is(repo, &fx.path()),
        "FF_REPO was {repo:?}, not the worktree"
    );
    // The integer every `{"ff": N, …}` envelope carries, and the same one:
    // `ff status --json` is the contract the child would be parsing.
    let contract = lines.next().expect("FF_CONTRACT line");
    let envelope: serde_json::Value =
        serde_json::from_str(&stdout(&ff_ext(&fx, bin_dir.path(), &["status", "--json"])))
            .expect("valid json");
    assert_eq!(envelope["ff"].to_string(), contract);
}

/// Absent rather than empty when there is no worktree, so a child tests
/// presence instead of parsing emptiness.
#[test]
fn the_repository_is_unset_outside_one() {
    let away = TempDir::new().expect("create tempdir");
    let (bin_dir, _) = ext_bin("tower", "echo \"${FF_REPO-unset}\"");
    let out = ff_at(away.path(), Some(bin_dir.path()), &["tower", "go"]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    assert_eq!(stdout(&out), "unset\n");
}

/// `-C` reaches the path clap never parsed, and is fufu's flag all the way
/// down: the child moves with it and never sees it.
#[test]
fn the_cwd_flag_reaches_the_extension_and_stays_out_of_its_argv() {
    let fx = Fixture::new();
    let away = TempDir::new().expect("create tempdir");
    let (bin_dir, _) = ext_bin("tower", "echo \"$FF_REPO\"; echo \"[$*]\"");
    let dir = fx.path();
    let dir = dir.to_str().expect("utf-8 path");
    // Every spelling clap would have accepted, since none of them reached it.
    for args in [
        vec!["-C", dir, "tower", "go"],
        vec!["--cwd", dir, "tower", "go"],
        vec![&format!("--cwd={dir}")[..], "tower", "go"],
        vec![&format!("-C{dir}")[..], "tower", "go"],
    ] {
        let out = ff_at(away.path(), Some(bin_dir.path()), &args);
        assert!(out.status.success(), "{args:?}: {}", stderr(&out));
        let text = stdout(&out);
        let mut lines = text.lines();
        assert!(
            ff_testsupport::paths::is(lines.next().expect("FF_REPO line"), &fx.path()),
            "{args:?}: {text}"
        );
        assert_eq!(lines.next(), Some("[go]"), "{args:?}: {text}");
    }
}

/// A `-C` after the extension's own word belongs to the extension: that is
/// where fufu's argv stops, so it is neither acted on nor stripped.
#[test]
fn a_flag_after_the_word_belongs_to_the_extension() {
    let fx = Fixture::new();
    let (bin_dir, _) = ext_bin("tower", "echo \"[$*]\"");
    let out = ff_ext(&fx, bin_dir.path(), &["tower", "-C", "elsewhere"]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    assert_eq!(stdout(&out), "[-C elsewhere]\n");
}

/// `--session <v>` was already dropped from the child's argv; `--session=<v>`
/// leaked, because the scan compared one spelling of a flag that has two.
#[test]
fn both_spellings_of_the_session_flag_stay_out_of_the_argv() {
    let fx = Fixture::new();
    let (bin_dir, _) = ext_bin("tower", "echo \"[$*] $FF_SESSION\"");
    for flag in ["--session=flight-9", "--session"] {
        let mut args = vec![flag];
        if flag == "--session" {
            args.push("flight-9");
        }
        args.extend(["tower", "go"]);
        let out = ff_at(&fx.path(), Some(bin_dir.path()), &args);
        assert!(out.status.success(), "{args:?}: {}", stderr(&out));
        assert_eq!(stdout(&out), "[go] flight-9\n", "{args:?}");
    }
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
