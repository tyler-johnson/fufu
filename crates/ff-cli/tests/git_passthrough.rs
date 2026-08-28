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

/// Every tier runs git verbatim: `ff git status` reaches the fake git with
/// the argv it was given, and a read earns no tip under any of them.
#[test]
fn a_read_reaches_git_verbatim_under_every_tier() {
    for tier in ["observe", "coach", "strict"] {
        let fx = Fixture::new();
        fx.set_config("fufu.gitPolicy", tier);
        fx.write("a.txt", "a\n");
        fx.commit("init");
        fx.write("a.txt", "dirty\n");

        let fake = fake_git();
        let out = ff_with_path(&fake.bin, &fx.path(), &["git", "status"], &[]);
        assert!(out.status.success(), "ff git status failed under {tier}");
        let log = std::fs::read_to_string(&fake.log).unwrap();
        assert!(
            log.contains("argv: [status]"),
            "{tier} must reach git verbatim: {log:?}"
        );
        let stderr = String::from_utf8(out.stderr).unwrap();
        assert!(!stderr.contains("tip"), "{tier} coached a read: {stderr:?}");
        let subject = fx.git(&["log", "-1", "--format=%s", "refs/fufu/snap/main"]);
        assert_eq!(subject.trim(), "pre: git status");
    }
}

/// Coach is the default: the first write of a word earns a line naming the
/// fufu verb, the second says nothing, and git ran both times.
#[test]
fn coach_names_the_verb_once_per_word() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    fx.write("a.txt", "dirty\n");

    let fake = fake_git();
    let out = ff_with_path(&fake.bin, &fx.path(), &["git", "commit", "-m", "x"], &[]);
    assert!(out.status.success());
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(
        stderr.contains("ff: tip: that's ff commit"),
        "coach names the verb: {stderr:?}"
    );

    let again = ff_with_path(&fake.bin, &fx.path(), &["git", "commit", "-m", "y"], &[]);
    let stderr = String::from_utf8(again.stderr).unwrap();
    assert!(!stderr.contains("tip"), "coach says it once: {stderr:?}");

    let log = std::fs::read_to_string(&fake.log).unwrap();
    assert_eq!(
        log.matches("argv: [commit]").count(),
        2,
        "git ran both times: {log:?}"
    );
}

/// Strict refuses the words fufu has verbs for — and only those. A write
/// fufu has no answer for passes through under strict like any other.
#[test]
fn strict_refuses_only_what_it_can_answer() {
    let fx = Fixture::new();
    fx.set_config("fufu.gitPolicy", "strict");
    fx.write("a.txt", "a\n");
    fx.commit("init");
    fx.write("a.txt", "dirty\n");

    let fake = fake_git();
    let out = ff_with_path(&fake.bin, &fx.path(), &["git", "commit", "-m", "x"], &[]);
    assert_eq!(out.status.code(), Some(2), "strict refuses with exit 2");
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(
        stderr.contains("ff commit"),
        "the refusal names the verb: {stderr:?}"
    );
    assert!(
        !fake.log.exists(),
        "a refusal must not run git: {}",
        std::fs::read_to_string(&fake.log).unwrap_or_default()
    );

    // No fufu verb to name, so nothing to refuse.
    let out = ff_with_path(&fake.bin, &fx.path(), &["git", "apply", "p.diff"], &[]);
    assert!(out.status.success(), "git apply is not fufu's to refuse");
    let log = std::fs::read_to_string(&fake.log).unwrap();
    assert!(log.contains("argv: [apply] [p.diff]"), "{log:?}");
}
