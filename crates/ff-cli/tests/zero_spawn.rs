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
//!
//! `ff clone` is the one verb this trap cannot honestly speak for, so it is
//! stated here instead of asserted. fufu does the clone's protocol, pack and
//! checkout itself — nothing shells out to `git clone` — but it still reaches
//! git's *configuration and authentication* surface: gix opens with
//! `git_binary: true` and runs `git config -l` once per process, so that
//! `url.<base>.insteadOf`, `http.proxy` and `credential.helper` from the
//! installation config are honored rather than ignored; a credential helper
//! runs when a remote asks for auth; and `ssh` runs for an ssh URL. Those are
//! inherited whole rather than reimplemented, the same trade `net.rs` states
//! for fetch and push. `ff init` makes none of those calls, and is asserted
//! below like everything else.

// unix-only: the booby-trap is a `#!/bin/sh` script on PATH, which Windows
// cannot execute — the trap would never spring, proving nothing.
#![cfg(unix)]

use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::{Command, Output};
use std::time::Duration;

use ff_testsupport::Fixture;

struct Trap {
    _dir: tempfile::TempDir,
    bin: std::path::PathBuf,
    log: std::path::PathBuf,
}

/// A HOME no test may escape. `ff doctor --fix` and the wiring verbs write
/// into the user's own config files, so a suite that let the real HOME
/// through would rewrite the config of whoever is running it.
fn scratch_home() -> &'static std::path::Path {
    static HOME: std::sync::OnceLock<tempfile::TempDir> = std::sync::OnceLock::new();
    HOME.get_or_init(|| tempfile::TempDir::new().expect("scratch HOME"))
        .path()
}

fn build_trap() -> Trap {
    build_trap_with(1)
}

/// The trap with a fake git that *succeeds*. Needed where the spawn is
/// sanctioned and the interesting question is what happens after it returns
/// Ok — a failed push short-circuits the rest of the verb, which would let a
/// second spawn hide behind the failure.
fn build_trap_ok() -> Trap {
    build_trap_with(0)
}

fn build_trap_with(exit: i32) -> Trap {
    let dir = tempfile::TempDir::new().unwrap();
    let bin = dir.path().join("bin");
    std::fs::create_dir(&bin).unwrap();
    let log = dir.path().join("trap.log");
    let script = format!(
        "#!/bin/sh\necho \"git $*\" >> {}\nexit {exit}\n",
        log.display()
    );
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
        .env("HOME", scratch_home())
        // Real CI sets CI, and the auto-trim lane skips when it is set —
        // remove so the zero-spawn auto-trim case actually exercises the lane.
        .env_remove("CI")
        .output()
        .expect("spawn ff")
}

/// Like [`ff_trapped`], but with the real PATH still behind the trap, so
/// `git` resolves to the booby trap while helper binaries under other names
/// — `git-upload-pack`, which every local transport spawns — still resolve.
/// For verbs where the question is *which* git invocation happens rather than
/// whether any does.
fn ff_trapped_keeping_path(trap: &Trap, dir: &Path, args: &[&str]) -> Output {
    let real = std::env::var("PATH").unwrap_or_default();
    let path = format!("{}:{real}", trap.bin.display());
    Command::new(env!("CARGO_BIN_EXE_ff"))
        .current_dir(dir)
        .args(args)
        .env("PATH", path)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("HOME", scratch_home())
        .env_remove("CI")
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
        &["op", "log"][..],
        // The set language reaches the same rows by another route, and it
        // must not reach git on the way.
        &["op", "log", "kind(op)"][..],
        &["evolog"][..],
        // The patch surfaces open every changed blob. `prepare_diff()` hands
        // back an external command when `diff.external` is configured, and
        // driving it is exactly where a spawn would sneak in — so every
        // spelling that reaches it is trapped here.
        &["diff"][..],
        &["diff", "--json"][..],
        &["show"][..],
        &["show", "HEAD"][..],
        &["op", "show", "-p"][..],
        &["evolog", "-p"][..],
        // A rename boundary in the path walk runs a tree diff — the patch
        // layer's blob platform again, the one place a `diff.external`
        // spawn could reappear.
        &["log", "a.txt"][..],
        // The moves rather than the operations, and the same rule: both
        // walks are native.
        &["history"][..],
        // The prompt hook, which takes a snapshot at every shell prompt.
        // It reaches the full capture path — nothing short-circuits ahead
        // of it — and stays zero-spawn because capture is native. A `git`
        // subprocess on every prompt would be a permanent, invisible tax.
        &["trigger", "shell"][..],
        // Bare ff is the map: both its capture and its read are native, so
        // both spellings stay in the trapped set.
        &[][..],
        &["--json"][..],
        // The map's branch-scope paths.
        &["-n", "2"][..],
        &["--all"][..],
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
    assert_eq!(v["data"]["head"]["name"], "main");
    assert_eq!(v["data"]["changes"][0]["path"], "a.txt");
    assert_eq!(v["data"]["changes"][0]["kind"], "modified");
    assert_eq!(v["data"]["changes"][1]["path"], "new.txt");
    assert_eq!(v["data"]["changes"][1]["kind"], "added");

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

/// With fufu.translate on, a translated `ff git status` never execs git:
/// capture + translation are fully native. (Verbatim passthrough forms exec
/// by design — not this test.)
#[test]
fn translated_git_status_never_spawns() {
    let fx = Fixture::new();
    fx.set_config("fufu.translate", "true");
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
        &["worktree"][..],
        &["worktree", "list"][..],
        &["op", "log"][..],
        &["undo"][..],
        &["redo"][..],
        &["new", "-m", "next change"][..],
        &["start", "-m", "next change"][..],
        // The plain delete stays spawn-free even though it now reads the
        // branch's remote axis.
        &["branch", "delete", "other"][..],
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

/// `ff remote` reads the remotes through the ordinary `discover` handle and
/// so spawns nothing — even with a remote configured, which is the state
/// that would tempt a `git remote -v` detour. `find_remote`'s other call
/// site runs on the wire's handle, whose `git_binary: true` is the one that
/// spawns `git config -l`; this test pins that `ff remote` is not it.
#[test]
fn remote_never_spawns() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("one");
    fx.set_config("remote.origin.url", "file:///nonexistent");
    fx.set_config("remote.origin.fetch", "+refs/heads/*:refs/remotes/origin/*");

    let trap = build_trap();
    for args in [&["remote"][..], &["remote", "--json"][..]] {
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
        "ff remote spawned git: {}",
        std::fs::read_to_string(&trap.log).unwrap_or_default()
    );
}

/// `ff init` spawns nothing — not to create the repository, not to write the
/// gc guard, not to lay the log's floor. `gix::init` needs no installation
/// config, which is the whole reason this verb can be in the trapped set when
/// its sibling `ff clone` cannot.
#[test]
fn init_never_spawns() {
    let dir = tempfile::TempDir::new().unwrap();
    let fresh = dir.path().join("fresh");
    std::fs::create_dir(&fresh).unwrap();

    let trap = build_trap();
    // Creating, then adopting the same place: both shapes, both trapped.
    for args in [&["init"][..], &["init"][..], &["init", "--json"][..]] {
        let out = ff_trapped(&trap, &fresh, args);
        assert!(
            out.status.success(),
            "ff {:?} failed under trap PATH: {}",
            args,
            String::from_utf8_lossy(&out.stderr)
        );
    }
    // It really built a repository, or the assertion below proves nothing.
    assert!(fresh.join(".git").is_dir(), "ff init made a repository");
    assert!(
        !trap.log.exists(),
        "ff init spawned a subprocess: {}",
        std::fs::read_to_string(&trap.log).unwrap_or_default()
    );
}

/// `ff version` reads the binary and a cache file and nothing else. It is the
/// one verb that never opens a repository at all, so it runs here outside one
/// — if it ever reached for `git` to answer "which version am I", the trap
/// would be the only thing that noticed.
#[test]
fn version_never_spawns() {
    let dir = tempfile::TempDir::new().unwrap();
    let trap = build_trap();

    for args in [&["version"][..], &["version", "--json"][..], &["-v"][..]] {
        let out = ff_trapped(&trap, dir.path(), args);
        assert!(
            out.status.success(),
            "ff {:?} failed under trap PATH: {}",
            args,
            String::from_utf8_lossy(&out.stderr)
        );
    }
    assert!(
        !trap.log.exists(),
        "ff version spawned a subprocess: {}",
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

/// The auto lane deliberately skips the `git gc --auto` nudge that manual
/// `ff trim` does, so the commands that carry the lane stay spawn-free.
#[test]
fn auto_trim_never_spawns() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");

    // Take two snapshots so there is content to trim.
    fx.write("a.txt", "dirty1\n");
    let out = ff_trapped(&build_trap(), &fx.path(), &[]);
    assert!(out.status.success(), "snapshot one");

    fx.write("a.txt", "dirty2\n");
    let out = ff_trapped(&build_trap(), &fx.path(), &[]);
    assert!(out.status.success(), "snapshot two");

    // Make snapshots old enough to drop.
    fx.set_config("fufu.keep", "0s");
    std::thread::sleep(Duration::from_millis(1200));

    // Overwrite the stamp as due.
    let dir = fx.path().join(".git/fufu");
    std::fs::create_dir_all(&dir).ok();
    std::fs::write(
        dir.join("autotrim-main.json"),
        r#"{"trimmed_at":0,"interval_secs":0}"#,
    )
    .expect("write due stamp");

    // Build a fresh trap so the log is clean for this specific check.
    let trap = build_trap();

    // Run bare ff — the auto-trim lane should fire inline, without spawning.
    let out = ff_trapped(&trap, &fx.path(), &[]);
    assert!(
        out.status.success(),
        "bare ff succeeded: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    // The trim really ran — without this the test proves nothing.
    assert!(
        fx.try_git(&["rev-parse", "--verify", "refs/fufu/wt/main/trash/@ops"])
            .status
            .success(),
        "trash ref exists — trim actually ran"
    );

    // No spawn happened.
    assert!(
        !trap.log.exists(),
        "auto-trim spawned a subprocess: {}",
        std::fs::read_to_string(&trap.log).unwrap_or_default()
    );
}

/// The other side of that contract: manual `ff trim` nudges gc on any real
/// run, not only one that dropped something. Native writes never trigger
/// auto-gc, so a repo younger than its retention window would otherwise never
/// pack — and an unpacked store makes every chain walk pay for it.
#[test]
fn manual_trim_nudges_gc_even_when_nothing_dropped() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    fx.write("a.txt", "dirty\n");
    assert!(
        ff_trapped(&build_trap(), &fx.path(), &[]).status.success(),
        "snapshot taken"
    );

    // Retention leaves everything in place: this run drops nothing.
    let trap = build_trap();
    let out = ff_trapped(&trap, &fx.path(), &["trim"]);
    assert!(
        out.status.success(),
        "ff trim succeeded: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        String::from_utf8_lossy(&out.stdout).contains("nothing to drop"),
        "nothing was dropped: {}",
        String::from_utf8_lossy(&out.stdout)
    );
    let logged = std::fs::read_to_string(&trap.log).unwrap_or_default();
    assert!(
        logged.contains("gc --auto"),
        "trim nudged git's gc: {logged:?}"
    );

    // --dry-run stays inert.
    let trap = build_trap();
    assert!(
        ff_trapped(&trap, &fx.path(), &["trim", "--dry-run"])
            .status
            .success()
    );
    assert!(
        !trap.log.exists(),
        "dry run spawned a subprocess: {}",
        std::fs::read_to_string(&trap.log).unwrap_or_default()
    );
}

/// Sync's local half is two replays, and both are native: with the network
/// switched off, sync reaches no process at all — no fetch, no push, nothing
/// hiding behind them. The assertion that keeps it that way as sync grows.
#[test]
fn sync_without_the_network_never_spawns() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("base");
    fx.git(&["branch", "feature"]);
    fx.set_config("user.name", "Zero Spawn");
    fx.set_config("user.email", "zero@spawn.test");
    fx.write("a.txt", "two\n");
    fx.commit("main one");
    fx.write("a.txt", "three\n");
    fx.commit("main two");
    fx.git(&["switch", "-q", "feature"]);
    // A disjoint file, so the replays merge cleanly and sync can land: a
    // conflict here would hold and fail the run, not spawn.
    fx.write("f.txt", "feature change\n");
    fx.commit("feature one");

    // No remote configured at all: a fetch would have nowhere to aim.
    let trap = build_trap();
    let out = ff_trapped(&trap, &fx.path(), &["sync", "--no-fetch"]);
    assert!(
        out.status.success(),
        "sync without the network failed under trap PATH: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        !trap.log.exists(),
        "sync's local half spawned a subprocess: {}",
        std::fs::read_to_string(&trap.log).unwrap_or_default()
    );
}

/// Sync's fetch used to be a sanctioned spawn — a named `git fetch origin`,
/// and this test proved it was the *only* process fufu started. The rung is
/// climbed, so the contract inverts: no porcelain runs, and a remote fufu
/// cannot reach fails as fufu's own coded error rather than as git's exit
/// code read back through stderr.
///
/// Nothing at all is spawned here, config read included: the trapped
/// environment pins `GIT_CONFIG_SYSTEM` and `GIT_CONFIG_NOSYSTEM`, so there
/// is no installation config left for `git_binary` to go asking about. The
/// case where it does ask is `a_fetch_speaks_the_protocol_itself`, which is
/// also the case where a fetch has somewhere real to aim.
///
/// Publish's push remains the sanctioned spawn, and is now a different verb
/// entirely — gix sends no packs, so there is nothing to climb to.
#[test]
fn syncs_fetch_fails_natively_and_spawns_nothing() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("base");
    fx.git(&["branch", "feature"]);
    fx.set_config("user.name", "Zero Spawn");
    fx.set_config("user.email", "zero@spawn.test");
    fx.write("a.txt", "two\n");
    fx.commit("main one");
    fx.write("a.txt", "three\n");
    fx.commit("main two");
    fx.git(&["switch", "-q", "feature"]);
    // A disjoint file, so the replays merge cleanly and sync can land: a
    // conflict here would hold and fail the run, not spawn.
    fx.write("f.txt", "feature change\n");
    fx.commit("feature one");
    // A configured remote, so the sanctioned fetch has somewhere to aim.
    fx.set_config("remote.origin.url", "/nonexistent/remote.git");
    fx.set_config("remote.origin.fetch", "+refs/heads/*:refs/remotes/origin/*");

    let trap = build_trap();
    let out = ff_trapped(&trap, &fx.path(), &["sync"]);
    assert!(
        !out.status.success(),
        "a remote that is not there is not a sync that succeeded: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("fetching from origin failed"),
        "the failure is fufu's own, not git's read back: {stderr}"
    );
    assert!(
        !trap.log.exists(),
        "the fetch spawned git: {}",
        std::fs::read_to_string(&trap.log).unwrap_or_default()
    );
}

/// Publish's push is the other sanctioned spawn, and the only one it has:
/// the verb decides its whole plan from refs and then makes exactly one call.
#[test]
fn publishs_push_is_its_one_sanctioned_spawn() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("base");
    fx.set_config("user.name", "Zero Spawn");
    fx.set_config("user.email", "zero@spawn.test");
    fx.set_config("remote.origin.url", "/nonexistent/remote.git");
    fx.set_config("remote.origin.fetch", "+refs/heads/*:refs/remotes/origin/*");

    let trap = build_trap();
    let out = ff_trapped(&trap, &fx.path(), &["publish"]);
    assert!(
        !out.status.success(),
        "a push that could not run is not a publish that succeeded: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(trap.log.exists(), "the sanctioned push spawned git");
    let logged = std::fs::read_to_string(&trap.log).unwrap();
    assert!(
        logged.contains("push"),
        "the sanctioned spawn is the named call: {logged:?}"
    );
    let lines: Vec<&str> = logged.lines().filter(|l| !l.trim().is_empty()).collect();
    assert_eq!(lines.len(), 1, "nothing besides the push: {logged:?}");
}

/// And the record publish writes afterwards adds none. A push that fails
/// never reaches the append, so the trap's git has to succeed for this to
/// mean anything — the note and the pointer are both gix writes, and one
/// stray `git` call for either would be a second line in the log.
#[test]
fn recording_the_push_adds_no_second_spawn() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("base");
    fx.set_config("user.name", "Zero Spawn");
    fx.set_config("user.email", "zero@spawn.test");
    fx.set_config("remote.origin.url", "/nonexistent/remote.git");
    fx.set_config("remote.origin.fetch", "+refs/heads/*:refs/remotes/origin/*");

    let trap = build_trap_ok();
    let out = ff_trapped(&trap, &fx.path(), &["publish"]);
    assert!(
        out.status.success(),
        "the fake push succeeded, so the verb must have: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let logged = std::fs::read_to_string(&trap.log).unwrap();
    let lines: Vec<&str> = logged.lines().filter(|l| !l.trim().is_empty()).collect();
    assert_eq!(lines.len(), 1, "nothing besides the push: {logged:?}");
    assert!(lines[0].contains("push"), "{logged:?}");
}

/// The fetch behind `ff sync` runs the protocol itself: a commit that has
/// never existed in this repository arrives while every `git` invocation
/// fails.
///
/// Two things are deliberately *not* asserted, and both are the honest shape
/// of the claim rather than slack in the test.
///
/// `git config -l` still runs: `net::wire_repo` opens with `git_binary` on,
/// without which `url.insteadOf`, `http.proxy` and `credential.helper` would
/// be ignored — exactly where a fetch behind a corporate proxy stops working.
///
/// And the real `git-upload-pack` is left reachable on PATH, because this
/// fixture's remote is a filesystem path, and a local transport *is* a spawned
/// upload-pack — in git as much as in gix. The in-process case is http(s),
/// where gix's reqwest/rustls transport carries the whole conversation. So
/// what this proves is the thing that changed: no `git fetch` porcelain runs,
/// and the negotiation and pack are fufu's own. `ff clone` has the same shape.
#[test]
fn a_fetch_speaks_the_protocol_itself() {
    let fx = Fixture::new_cloned();
    fx.write("a.txt", "a\n");
    fx.commit("one");
    fx.git(&["push", "-q", "-u", "origin", "main"]);

    // A second clone is where the commit this repository has never seen comes
    // from — a bare remote has no worktree to make one in.
    let other = fx.root().join("other");
    fx.git_in(
        fx.root(),
        &[
            "clone",
            "-q",
            &fx.remote_path().to_string_lossy(),
            &other.to_string_lossy(),
        ],
    );
    fx.git_in(&other, &["config", "user.name", "Other"]);
    fx.git_in(&other, &["config", "user.email", "other@spawn.test"]);
    std::fs::write(other.join("b.txt"), "b\n").unwrap();
    fx.git_in(&other, &["add", "-A"]);
    fx.git_in(&other, &["commit", "-q", "-m", "theirs"]);
    fx.git_in(&other, &["push", "-q", "origin", "main"]);
    let theirs = fx.git_in(&other, &["rev-parse", "HEAD"]).trim().to_string();
    assert!(
        fx.try_git(&["cat-file", "-e", &theirs]).status.code() != Some(0),
        "test fixture: the commit must be one this repository has never seen"
    );

    let trap = build_trap();
    let out = ff_trapped_keeping_path(&trap, &fx.path(), &["sync"]);
    assert!(
        out.status.success(),
        "ff sync failed under trap PATH: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        fx.git(&["rev-parse", "refs/remotes/origin/main"]).trim(),
        theirs,
        "the tracking ref must carry what the remote advertised"
    );

    let log = std::fs::read_to_string(&trap.log).unwrap_or_default();
    assert!(
        !log.contains("fetch"),
        "the fetch was spawned rather than spoken: {log}"
    );
}

/// `--to` adds no spawn of its own: the upstream it records is a gix write,
/// and a second `git` line here would be the regression.
#[test]
fn publishing_to_a_named_remote_is_still_one_spawn() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("base");
    fx.set_config("user.name", "Zero Spawn");
    fx.set_config("user.email", "zero@spawn.test");
    fx.set_config("remote.origin.url", "/nonexistent/remote.git");
    fx.set_config("remote.origin.fetch", "+refs/heads/*:refs/remotes/origin/*");

    let trap = build_trap();
    let out = ff_trapped(&trap, &fx.path(), &["publish", "--to", "origin"]);
    assert!(
        !out.status.success(),
        "a push that could not run is not a publish that succeeded: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(trap.log.exists(), "the sanctioned push spawned git");
    let logged = std::fs::read_to_string(&trap.log).unwrap();
    assert!(
        logged.contains("push"),
        "the sanctioned spawn is the named call: {logged:?}"
    );
    let lines: Vec<&str> = logged.lines().filter(|l| !l.trim().is_empty()).collect();
    assert_eq!(lines.len(), 1, "nothing besides the push: {logged:?}");
}
