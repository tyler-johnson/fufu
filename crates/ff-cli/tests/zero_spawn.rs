//! The zero-spawn proof: run the real `ff` binary with PATH pointing at a
//! booby-trap directory whose only executable is a fake `git` that logs its
//! argv and fails. If ff ever shells out, the trap log appears and the fake
//! git's failure would surface — so a clean run with no log is proof that no
//! spawn happened anywhere in the binary.

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
