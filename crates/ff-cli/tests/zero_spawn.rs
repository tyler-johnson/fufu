//! The zero-spawn proof: run the real `ff` binary with PATH pointing at a
//! booby-trap directory whose only executable is a fake `git` that logs its
//! argv and fails. If ff ever shells out, the trap log appears and the fake
//! git's failure would surface — so a clean run with no log is proof that no
//! spawn happened anywhere in the binary.
//!
//! One sanctioned self-spawn exists outside this proof: official release
//! builds may spawn `<current_exe> update --check` (absolute path, never
//! PATH) from the passive update lane. Test binaries are never official
//! (FF_OFFICIAL_BUILD unset), so that lane is structurally dead here and
//! every assertion below still proves full zero-spawn for everything else.

// unix-only: the booby-trap is a `#!/bin/sh` script on PATH, which Windows
// cannot execute — the trap would never spring, proving nothing.
#![cfg(unix)]

use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::{Command, Output};

use ff_testsupport::Fixture;

struct Trap {
    _dir: tempfile::TempDir,
    bin: std::path::PathBuf,
    log: std::path::PathBuf,
}

fn build_trap() -> Trap {
    let dir = tempfile::TempDir::new().unwrap();
    let bin = dir.path().join("bin");
    std::fs::create_dir(&bin).unwrap();
    let log = dir.path().join("trap.log");
    let script = format!("#!/bin/sh\necho \"git $*\" >> {}\nexit 1\n", log.display());
    let git = bin.join("git");
    std::fs::write(&git, script).unwrap();
    std::fs::set_permissions(&git, std::fs::Permissions::from_mode(0o755)).unwrap();
    Trap {
        _dir: dir,
        bin,
        log,
    }
}

fn ff_trapped(trap: &Trap, dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_ff"))
        .current_dir(dir)
        .args(args)
        // Only the child gets the booby-trapped PATH — parallel-safe.
        .env("PATH", &trap.bin)
        // The pager is TTY-gated; aim it at the trap so a broken gate (a
        // pager spawn on piped stdout) springs it.
        .env("FF_PAGER", trap.bin.join("git"))
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .expect("spawn ff")
}

#[test]
fn status_and_log_never_spawn() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("one");
    fx.write("a.txt", "changed\n");
    fx.write("new.txt", "untracked\n");
    let index_before = fx.index_bytes();

    let trap = build_trap();
    for args in [
        &["status", "--json"][..],
        &["status"][..],
        &["log", "--json"][..],
        &["log", "-n", "5"][..],
        &["log", "--ops"][..],
        &["evolog"][..],
        // Bare ff captures natively — the write side is zero-spawn too.
        &[][..],
        &["--json"][..],
    ] {
        let out = ff_trapped(&trap, &fx.path(), args);
        assert!(
            out.status.success(),
            "ff {:?} failed under trap PATH: {}",
            args,
            String::from_utf8_lossy(&out.stderr)
        );
    }

    // JSON output still correct under the trap PATH.
    let out = ff_trapped(&trap, &fx.path(), &["status", "--json"]);
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).expect("valid status json");
    assert_eq!(v["head"]["name"], "main");
    assert_eq!(v["unstaged"][0]["path"], "a.txt");
    assert_eq!(v["untracked"][0], "new.txt");

    assert!(
        !trap.log.exists(),
        "ff spawned a subprocess: {}",
        std::fs::read_to_string(&trap.log).unwrap_or_default()
    );
    assert_eq!(
        index_before,
        fx.index_bytes(),
        ".git/index must stay byte-identical"
    );
}

/// A translated `ff git status` never execs git: capture + translation are
/// fully native. (Verbatim passthrough forms exec by design — not this test.)
#[test]
fn translated_git_status_never_spawns() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("one");
    fx.write("a.txt", "dirty\n");
    let index_before = fx.index_bytes();

    let trap = build_trap();
    let out = ff_trapped(&trap, &fx.path(), &["git", "status"]);
    assert!(
        out.status.success(),
        "ff git status failed under trap PATH: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(
        stderr.contains("ff: tip: that's ff status"),
        "first translation hints once: {stderr:?}"
    );

    let again = ff_trapped(&trap, &fx.path(), &["git", "status"]);
    let stderr = String::from_utf8(again.stderr).unwrap();
    assert!(!stderr.contains("tip"), "hint prints only once: {stderr:?}");

    assert!(
        !trap.log.exists(),
        "translated ff git spawned: {}",
        std::fs::read_to_string(&trap.log).unwrap_or_default()
    );
    assert_eq!(index_before, fx.index_bytes());

    // The capture happened, natively.
    let subject = fx.git(&["log", "-1", "--format=%s", "refs/fufu/snap/main"]);
    assert_eq!(subject.trim(), "pre: git status");
}

/// Every Phase 2 verb is native: commit, switch, branch, new, describe,
/// undo, and the ops view all run under the trap PATH without a spawn.
#[test]
fn phase2_verbs_never_spawn() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("one");
    fx.git(&["branch", "other"]);
    fx.set_config("user.name", "Zero Spawn");
    fx.set_config("user.email", "zero@spawn.test");

    let trap = build_trap();
    fx.write("a.txt", "change to close\n");
    for args in [
        &["commit", "-m", "closed natively"][..],
        &["describe", "-m", "pending text"][..],
        &["switch", "other"][..],
        &["switch", "main"][..],
        &["branch"][..],
        &["log", "--ops"][..],
        &["undo"][..],
        &["new", "-m", "next change"][..],
    ] {
        let out = ff_trapped(&trap, &fx.path(), args);
        assert!(
            out.status.success(),
            "ff {:?} failed under trap PATH: {}",
            args,
            String::from_utf8_lossy(&out.stderr)
        );
    }
    assert!(
        !trap.log.exists(),
        "a phase-2 verb spawned git: {}",
        std::fs::read_to_string(&trap.log).unwrap_or_default()
    );
}

/// The config write path is native — gix File + config.lock, no `git config` shellout.
#[test]
fn config_never_spawns() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("one");

    let trap = build_trap();
    for args in [
        &["config"][..],
        &["config", "keep"][..],
        &["config", "keep", "30d"][..],
        &["config", "--unset", "keep"][..],
        &["config", "--json"][..],
    ] {
        let out = ff_trapped(&trap, &fx.path(), args);
        assert!(
            out.status.success(),
            "ff {:?} failed under trap PATH: {}",
            args,
            String::from_utf8_lossy(&out.stderr)
        );
    }
    assert!(
        !trap.log.exists(),
        "config spawned git: {}",
        std::fs::read_to_string(&trap.log).unwrap_or_default()
    );
}

/// Doctor reads everything and spawns nothing: engine checks via gix, wiring
/// checks via plain file reads, and the update line from the cache file (the
/// background check spawn is official-build-gated, structurally dead here).
#[test]
fn doctor_never_spawns() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("one");
    fx.write("a.txt", "changed\n");
    fx.write("new.txt", "untracked\n");

    let trap = build_trap();
    // Bare ff creates a chain so doctor exercises the full engine path.
    let out = ff_trapped(&trap, &fx.path(), &[]);
    assert!(
        out.status.success(),
        "bare ff failed under trap: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let index_bytes = fx.index_bytes();

    for args in [
        &["doctor"][..],
        &["doctor", "--json"][..],
        &["doctor", "--fix"][..],
    ] {
        let out = ff_trapped(&trap, &fx.path(), args);
        assert!(
            out.status.code() == Some(0) || out.status.code() == Some(1),
            "ff {:?} exited with unexpected code: {} (stderr: {})",
            args,
            out.status
                .code()
                .map_or("signal".to_string(), |c| c.to_string()),
            String::from_utf8_lossy(&out.stderr)
        );
    }

    assert!(
        !trap.log.exists(),
        "doctor spawned a subprocess: {}",
        std::fs::read_to_string(&trap.log).unwrap_or_default()
    );
    assert_eq!(
        index_bytes,
        fx.index_bytes(),
        ".git/index must stay byte-identical"
    );
}

/// Hooks are sanctioned spawns: with an executable pre-commit present, the
/// close still succeeds under the trap PATH (the hook itself runs), and the
/// trap proves the hook was the only kind of process fufu started — a hook
/// that calls `git` hits the trap and vetoes the commit.
#[test]
fn hook_exec_is_a_sanctioned_spawn_and_distinguished() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("one");
    fx.set_config("user.name", "Zero Spawn");
    fx.set_config("user.email", "zero@spawn.test");
    let hooks = fx.path().join(".git/hooks");
    std::fs::create_dir_all(&hooks).unwrap();
    let marker = fx.path().join(".git/hook-ran");
    // Only shell builtins: the trap PATH has no coreutils.
    std::fs::write(
        hooks.join("pre-commit"),
        format!("#!/bin/sh\n: > {}\nexit 0\n", marker.display()),
    )
    .unwrap();
    std::fs::set_permissions(
        hooks.join("pre-commit"),
        std::fs::Permissions::from_mode(0o755),
    )
    .unwrap();

    let trap = build_trap();
    fx.write("a.txt", "hooked change\n");
    let out = ff_trapped(&trap, &fx.path(), &["commit", "-m", "with hook"]);
    assert!(
        out.status.success(),
        "commit with hook failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(marker.exists(), "the hook really ran");
    assert!(!trap.log.exists(), "fufu itself never called git");

    // A hook that shells out to git is caught by the trap and, because the
    // fake git fails, would veto a hook that depends on it — spawns inside
    // hooks are the hook author's, visibly.
    std::fs::write(hooks.join("pre-commit"), "#!/bin/sh\ngit status\n").unwrap();
    std::fs::set_permissions(
        hooks.join("pre-commit"),
        std::fs::Permissions::from_mode(0o755),
    )
    .unwrap();
    fx.write("a.txt", "second change\n");
    let out = ff_trapped(&trap, &fx.path(), &["commit", "-m", "hook calls git"]);
    assert!(!out.status.success(), "trap git fails → hook declines");
    assert!(trap.log.exists(), "the hook's git call hit the trap");
}

/// The trap itself works: anything that does spawn git gets caught.
#[test]
fn trap_catches_spawns() {
    let trap = build_trap();
    let out = Command::new("git")
        .arg("--version")
        .env("PATH", &trap.bin)
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert!(trap.log.exists(), "fake git must have logged the call");
    let logged = std::fs::read_to_string(&trap.log).unwrap();
    assert!(logged.contains("--version"));
}
