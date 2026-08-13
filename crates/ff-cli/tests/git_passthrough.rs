//! `ff git` verbatim passthrough, exercised against a fake `git` on PATH:
//! argv fidelity (hyphen flags included), capture-before-exec ordering,
//! exit-code mirroring, `--help` reaching git, and 127 when git is absent.

// unix-only: the fake `git` is a `#!/bin/sh` script on PATH, which Windows
// cannot execute (CreateProcess wants an .exe), so it can't intercept.
#![cfg(unix)]

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use ff_testsupport::Fixture;

struct FakeGit {
    _dir: tempfile::TempDir,
    bin: PathBuf,
    log: PathBuf,
}

/// A fake `git` that records its argv, snapshots what the chain ref pointed
/// at (via the real git binary) at the moment it ran, and exits with
/// `FAKE_GIT_EXIT` (default 0).
fn fake_git() -> FakeGit {
    let real_git = String::from_utf8(
        Command::new("sh")
            .args(["-c", "command -v git"])
            .output()
            .unwrap()
            .stdout,
    )
    .unwrap()
    .trim()
    .to_string();
    let dir = tempfile::TempDir::new().unwrap();
    let bin = dir.path().join("bin");
    std::fs::create_dir(&bin).unwrap();
    let log = dir.path().join("invocations.log");
    let script = format!(
        "#!/bin/sh\n\
         printf 'argv:' >> {log}\n\
         for a in \"$@\"; do printf ' [%s]' \"$a\" >> {log}; done\n\
         printf '\\n' >> {log}\n\
         echo \"chain: $({real_git} rev-parse --verify --quiet refs/fufu/snap/main || echo none)\" >> {log}\n\
         exit ${{FAKE_GIT_EXIT:-0}}\n",
        log = log.display(),
    );
    let git = bin.join("git");
    std::fs::write(&git, script).unwrap();
    std::fs::set_permissions(&git, std::fs::Permissions::from_mode(0o755)).unwrap();
    FakeGit {
        _dir: dir,
        bin,
        log,
    }
}

fn ff_with_path(path: &Path, dir: &Path, args: &[&str], extra_env: &[(&str, &str)]) -> Output {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_ff"));
    cmd.current_dir(dir)
        .args(args)
        .env("PATH", path)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1");
    for (k, v) in extra_env {
        cmd.env(k, v);
    }
    cmd.output().expect("spawn ff")
}

#[test]
fn verbatim_preserves_argv_and_captures_first() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    fx.write("a.txt", "dirty\n");

    let fake = fake_git();
    let out = ff_with_path(
        &fake.bin,
        &fx.path(),
        &["git", "log", "--oneline", "-n", "3", "--format=%H", "-p"],
        &[],
    );
    assert!(out.status.success());

    let log = std::fs::read_to_string(&fake.log).unwrap();
    assert!(
        log.contains("argv: [log] [--oneline] [-n] [3] [--format=%H] [-p]"),
        "argv must pass through byte-for-byte: {log:?}"
    );
    // The chain ref already existed when the fake git ran: capture-then-exec.
    let tip = fx.git(&["rev-parse", "refs/fufu/snap/main"]);
    assert!(
        log.contains(&format!("chain: {}", tip.trim())),
        "snapshot must land before git executes: {log:?}"
    );
    let subject = fx.git(&["log", "-1", "--format=%s", "refs/fufu/snap/main"]);
    assert_eq!(subject.trim(), "pre: git log --oneline -n 3 --format=%H -p");
}

#[test]
fn exit_code_mirrors_git() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    let fake = fake_git();
    let out = ff_with_path(
        &fake.bin,
        &fx.path(),
        &["git", "push"],
        &[("FAKE_GIT_EXIT", "42")],
    );
    assert_eq!(out.status.code(), Some(42), "git's exit code is ff's");
}

#[test]
fn help_reaches_git_not_clap() {
    let fx = Fixture::new();
    let fake = fake_git();
    let out = ff_with_path(&fake.bin, &fx.path(), &["git", "--help"], &[]);
    assert!(out.status.success());
    let log = std::fs::read_to_string(&fake.log).unwrap();
    assert!(
        log.contains("argv: [--help]"),
        "--help must pass through to git: {log:?}"
    );
}

#[test]
fn absent_git_exits_127() {
    let fx = Fixture::new();
    let empty = tempfile::TempDir::new().unwrap();
    let out = ff_with_path(empty.path(), &fx.path(), &["git", "push"], &[]);
    assert_eq!(out.status.code(), Some(127));
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(stderr.contains("git not found"), "stderr: {stderr:?}");
}

#[test]
fn outside_repo_passthrough_is_quiet_about_capture() {
    let dir = tempfile::TempDir::new().unwrap();
    let fake = fake_git();
    let out = ff_with_path(&fake.bin, dir.path(), &["git", "init", "-q"], &[]);
    assert!(out.status.success());
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(
        !stderr.contains("snapshot skipped"),
        "outside a repo there is nothing to capture and nothing to say: {stderr:?}"
    );
    let log = std::fs::read_to_string(&fake.log).unwrap();
    assert!(log.contains("argv: [init] [-q]"));
}

/// Translated forms with a count map onto `ff log -n`.
#[test]
fn translated_log_with_count() {
    let fx = Fixture::new();
    for i in 0..4 {
        fx.write("f.txt", &format!("{i}\n"));
        fx.commit(&format!("c{i}"));
    }
    let fake = fake_git();
    let out = ff_with_path(&fake.bin, &fx.path(), &["git", "log", "-2"], &[]);
    assert!(out.status.success());
    assert!(
        !fake.log.exists(),
        "translated log must not exec git: {}",
        std::fs::read_to_string(&fake.log).unwrap_or_default()
    );
    let text = String::from_utf8(out.stdout).unwrap();
    // Change-centric view: the @ row plus exactly two ● commit rows.
    assert!(text.starts_with("@  "), "{text:?}");
    let commit_rows = text.lines().filter(|l| l.starts_with('●')).count();
    assert_eq!(commit_rows, 2, "count honored: {text:?}");
}
