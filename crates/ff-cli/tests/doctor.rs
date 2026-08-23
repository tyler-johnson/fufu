//! Integration suite for `ff doctor` — drives the real `ff` binary against
//! hermetic fixture repositories with isolated HOME directories.

use std::path::Path;
use std::process::{Command, Output};

use ff_testsupport::fixtures::{Fixture, null_device};

// ── Runner ────────────────────────────────────────────────────────────────

/// Run `ff` with doctor-specific isolation (HOME + XDG_CACHE_HOME set).
fn doctor_env(dir: &Path, args: &[&str], home: &Path) -> Output {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_ff"));
    cmd.current_dir(dir)
        .args(args)
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("HOME", home)
        .env("XDG_CACHE_HOME", home.join(".cache"))
        .env("GIT_CONFIG_GLOBAL", null_device())
        .env("GIT_CONFIG_SYSTEM", null_device())
        .env("GIT_CONFIG_NOSYSTEM", "1");
    #[cfg(windows)]
    for key in ["SYSTEMROOT", "WINDIR", "TEMP", "TMP", "PATHEXT", "COMSPEC"] {
        if let Some(value) = std::env::var_os(key) {
            cmd.env(key, value);
        }
    }
    cmd.output().expect("spawn ff")
}

/// Convenience: run `ff doctor` (with optional extra args) in a fixture repo.
fn doctor(fx: &Fixture, extra_args: &[&str]) -> Output {
    let mut args = vec!["doctor"];
    args.extend(extra_args.iter().copied());
    doctor_env(&fx.path(), &args, &fx.root().join("home"))
}

/// Run bare `ff` (the snapshot command) in a fixture repo.
/// Creates an untracked file first so `ff` actually takes a snapshot
/// (bare `ff` skips when the working tree is clean with no prior snapshots).
fn ff_init(fx: &Fixture) {
    fx.write(".ff_init_marker", "1");
    let out = doctor_env(&fx.path(), &[], &fx.root().join("home"));
    assert!(
        out.status.success(),
        "bare ff should succeed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// Extract stdout as a UTF-8 string.
fn stdout(out: &Output) -> String {
    String::from_utf8(out.stdout.clone()).expect("stdout is utf-8")
}

// ── Tests ─────────────────────────────────────────────────────────────────

/// 1. Fresh repo, nothing captured: the log warn + triggers warn, and the
///    checks that only mean something once a log exists (identity, reflogs,
///    gc config) skipped entirely.
#[test]
fn fresh_repo_with_no_log_warns() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");

    let out = doctor(&fx, &[]);
    assert_eq!(out.status.code(), Some(1), "exit 1 when warns exist");

    let out_text = stdout(&out);
    assert!(
        out_text.contains("WARN  log"),
        "log warn present:\n{out_text}"
    );
    assert!(
        out_text.contains("no refs/fufu/wt/main/ops — the engine has never run here"),
        "log detail:\n{out_text}"
    );
    assert!(
        out_text.contains("WARN  triggers"),
        "triggers warn present:\n{out_text}"
    );
    assert!(
        out_text.contains("neither the alias nor the claude hooks are wired"),
        "triggers detail:\n{out_text}"
    );

    // Checks that need a log must not appear
    assert!(
        !out_text.contains("identity"),
        "identity should be skipped:\n{out_text}"
    );
    assert!(
        !out_text.contains("reflogs"),
        "reflogs should be skipped:\n{out_text}"
    );
    assert!(
        !out_text.contains("gc config"),
        "gc config should be skipped:\n{out_text}"
    );
}

/// 2. After running `ff` (snapshot) + wiring the bash alias, everything is green.
#[test]
fn all_green_after_snapshot_and_wiring() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");

    // Initialize captures + the operation log + the gc guard
    ff_init(&fx);

    // Wire bash alias
    let out = doctor_env(
        &fx.path(),
        &["hook", "shell", "install", "bash"],
        &fx.root().join("home"),
    );
    assert!(
        out.status.success(),
        "alias install succeeded: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    // Now doctor should be clean
    let out = doctor(&fx, &[]);
    assert_eq!(out.status.code(), Some(0), "exit 0 when all green");

    let out_text = stdout(&out);
    // One log, so one row — and the per-branch refs are reported as what
    // they are, pointers into it.
    assert!(out_text.contains("ok    log"), "log ok:\n{out_text}");
    assert!(
        out_text.contains("refs/fufu/wt/main/ops"),
        "the log names its ref:\n{out_text}"
    );
    assert!(
        out_text.contains("1 branch pointer(s) into the log: main"),
        "pointer row:\n{out_text}"
    );
    assert!(
        out_text.contains("ok    identity"),
        "identity ok:\n{out_text}"
    );
    assert!(
        out_text.contains("the log tip is a fufu operation"),
        "identity detail:\n{out_text}"
    );
    assert!(
        out_text.contains("ok    reflogs"),
        "reflogs ok:\n{out_text}"
    );
    assert!(
        out_text.contains("ok    gc config"),
        "gc config ok:\n{out_text}"
    );
    assert!(
        out_text.contains("reflog expiry disabled for refs/fufu/*"),
        "gc config detail:\n{out_text}"
    );
    assert!(
        out_text.contains("ok    objects"),
        "objects ok:\n{out_text}"
    );
    // Loose vs packed is why a chain walk is fast or slow, so doctor says it.
    let objects = out_text
        .lines()
        .find(|line| line.contains("objects"))
        .expect("objects row");
    let loose: usize = objects
        .split_whitespace()
        .nth(2)
        .and_then(|n| n.parse().ok())
        .expect("loose count is a number");
    assert!(loose > 0, "a snapshotted repo has loose objects: {objects}");
    assert!(objects.contains("pack"), "pack count named: {objects}");
    assert!(
        out_text.contains("info  last op"),
        "last op info:\n{out_text}"
    );
    assert!(
        out_text.contains("info  settings"),
        "settings info:\n{out_text}"
    );
    assert!(
        out_text.contains("all defaults"),
        "settings defaults:\n{out_text}"
    );
    assert!(out_text.contains("info  trim"), "trim info:\n{out_text}");
    assert!(
        out_text.contains("info  claude hooks"),
        "claude hooks info:\n{out_text}"
    );
    assert!(
        out_text.contains("not wired (optional"),
        "claude hooks detail:\n{out_text}"
    );
    assert!(out_text.contains("ok    alias"), "alias ok:\n{out_text}");
    assert!(
        out_text.contains("git='ff git' installed in"),
        "alias detail:\n{out_text}"
    );
    assert!(
        out_text.contains("info  update"),
        "update info:\n{out_text}"
    );
    assert!(
        out_text.contains("source build — updates via cargo install"),
        "update detail:\n{out_text}"
    );
    // No triggers row when wired
    assert!(
        !out_text.contains("triggers"),
        "no triggers row when wired:\n{out_text}"
    );
    assert!(
        out_text.contains("no findings — the net is under you"),
        "clean summary:\n{out_text}"
    );
    assert!(
        out_text.contains("ok    id index"),
        "id index ok:\n{out_text}"
    );
    let id_index = out_text
        .lines()
        .find(|line| line.contains("id index"))
        .expect("id index row");
    let mut detail_parts = id_index
        .split("id index")
        .nth(1)
        .expect("id index detail")
        .trim()
        .splitn(2, ' ');
    let n: usize = detail_parts
        .next()
        .and_then(|s| s.parse().ok())
        .expect("id count is a number");
    assert!(n >= 1, "id index has at least one id: {id_index}");
    assert_eq!(
        detail_parts.next(),
        Some("ids, in sync"),
        "id index detail: {id_index}"
    );
}

/// 3. Missing gc config keys warn; --fix repairs them; subsequent doctor is clean.
#[test]
fn gc_missing_warns_and_fix_repairs() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");

    // Initialize snapshots
    ff_init(&fx);

    // Wire bash alias (so gc is the only finding)
    let out = doctor_env(
        &fx.path(),
        &["hook", "shell", "install", "bash"],
        &fx.root().join("home"),
    );
    assert!(out.status.success(), "alias install succeeded");

    // Delete both gc keys — use try_git since unset returns nonzero if key absent
    let _ = fx.try_git(&["config", "--unset", "gc.refs/fufu/*.reflogExpire"]);
    let _ = fx.try_git(&[
        "config",
        "--unset",
        "gc.refs/fufu/*.reflogExpireUnreachable",
    ]);

    // Should warn
    let out = doctor(&fx, &[]);
    assert_eq!(out.status.code(), Some(1), "exit 1 with gc warning");
    let out_text = stdout(&out);
    assert!(
        out_text.contains("WARN  gc config"),
        "gc config warn:\n{out_text}"
    );
    assert!(
        out_text.contains("1 finding(s) — `ff doctor --fix` repairs 1 of them"),
        "summary with fix hint:\n{out_text}"
    );

    // Fix
    let out = doctor(&fx, &["--fix"]);
    assert_eq!(out.status.code(), Some(0), "exit 0 after fix");
    let out_text = stdout(&out);
    assert!(
        out_text.contains("reflog expiry disabled for refs/fufu/* (fixed)"),
        "fixed detail:\n{out_text}"
    );
    assert!(
        out_text.contains("no findings"),
        "clean summary after fix:\n{out_text}"
    );

    // Verify both keys are now `never`
    let v1 = fx.git(&["config", "--local", "--get", "gc.refs/fufu/*.reflogExpire"]);
    assert_eq!(v1.trim(), "never", "reflogExpire is never");

    let v2 = fx.git(&[
        "config",
        "--local",
        "--get",
        "gc.refs/fufu/*.reflogExpireUnreachable",
    ]);
    assert_eq!(v2.trim(), "never", "reflogExpireUnreachable is never");
}

/// 4. Invalid fufu.keep value warns on settings row; trim preview is skipped.
#[test]
fn invalid_keep_warns_settings() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");

    fx.set_config("fufu.keep", "bogus");

    // Initialize snapshots (still works despite bogus keep)
    ff_init(&fx);

    let out = doctor(&fx, &[]);
    assert_eq!(out.status.code(), Some(1), "exit 1 with invalid keep");
    let out_text = stdout(&out);
    assert!(
        out_text.contains("WARN  settings"),
        "settings warn:\n{out_text}"
    );
    assert!(
        out_text.contains("fufu.keep is \"bogus\" — invalid (`ff config keep` explains)"),
        "settings detail:\n{out_text}"
    );
    // Trim preview skipped when keep is invalid
    assert!(
        !out_text.contains("trim preview"),
        "trim preview should be skipped with invalid keep:\n{out_text}"
    );
}

/// 5. Moving a chain tip to a non-snapshot commit warns on identity.
#[test]
fn a_moved_log_tip_warns_identity() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");

    // Initialize the log
    ff_init(&fx);

    // Point the log at HEAD — a user commit, not a fufu operation. The guard
    // is `is_op_commit` and not "does it bear the fufu identity", because a
    // record commit bears the identity too.
    fx.git(&["update-ref", "refs/fufu/wt/main/ops", "HEAD"]);

    let out = doctor(&fx, &[]);
    assert_eq!(out.status.code(), Some(1), "exit 1 with identity warning");
    let out_text = stdout(&out);
    assert!(
        out_text.contains("WARN  identity"),
        "identity warn:\n{out_text}"
    );
    assert!(
        out_text.contains(
            "the log tip is not a fufu operation — the ref was moved by something other than fufu"
        ),
        "identity detail:\n{out_text}"
    );
}

/// 6. Wiring infos when nothing wired; triggers warn disappears after alias install.
#[test]
fn wiring_infos_and_triggers_warn() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");

    // Initialize snapshots
    ff_init(&fx);

    // Nothing wired
    let out = doctor(&fx, &[]);
    let out_text = stdout(&out);
    assert!(
        out_text.contains("info  claude hooks"),
        "claude hooks info:\n{out_text}"
    );
    assert!(
        out_text.contains("not wired (optional — `ff hook agent install`)"),
        "claude hooks detail:\n{out_text}"
    );
    assert!(out_text.contains("info  alias"), "alias info:\n{out_text}");
    assert!(
        out_text.contains("no `ff git` alias found in shell rc files (heuristic)"),
        "alias detail:\n{out_text}"
    );
    assert!(
        out_text.contains("WARN  triggers"),
        "triggers warn:\n{out_text}"
    );

    // Install bash alias
    let out = doctor_env(
        &fx.path(),
        &["hook", "shell", "install", "bash"],
        &fx.root().join("home"),
    );
    assert!(out.status.success(), "alias install succeeded");

    // Triggers should be gone
    let out = doctor(&fx, &[]);
    let out_text = stdout(&out);
    assert!(
        out_text.contains("ok    alias"),
        "alias now ok:\n{out_text}"
    );
    assert!(
        !out_text.contains("triggers"),
        "no triggers row after wiring:\n{out_text}"
    );
}

/// 7. Partial hook wiring (only PreToolUse, missing UserPromptSubmit) warns on
///    claude hooks but no triggers row (partial wiring is still wiring).
#[test]
fn partial_hook_wiring_warns() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");

    // Initialize snapshots
    ff_init(&fx);

    // Wire bash alias (so we only test the partial hook side)
    let out = doctor_env(
        &fx.path(),
        &["hook", "shell", "install", "bash"],
        &fx.root().join("home"),
    );
    assert!(out.status.success(), "alias install succeeded");

    // Write settings.json with only PreToolUse wired
    let home = fx.root().join("home");
    let claude_dir = home.join(".claude");
    std::fs::create_dir_all(&claude_dir).expect("create .claude dir");
    std::fs::write(
        claude_dir.join("settings.json"),
        r#"{"hooks":{"PreToolUse":[{"matcher":"Bash|Edit|Write|NotebookEdit","hooks":[{"type":"command","command":"ff hook agent trigger claude"}]}]}}"#,
    )
    .expect("write settings.json");

    let out = doctor(&fx, &[]);
    assert_eq!(out.status.code(), Some(1), "exit 1 with partial hook warn");
    let out_text = stdout(&out);
    assert!(
        out_text.contains("WARN  claude hooks"),
        "claude hooks warn:\n{out_text}"
    );
    assert!(
        out_text.contains("PreToolUse wired but UserPromptSubmit missing — capture is partial (`ff hook agent install` repairs)"),
        "partial hook detail:\n{out_text}"
    );
    // Partial wiring is still wiring — no triggers row
    assert!(
        !out_text.contains("triggers"),
        "no triggers row with partial wiring:\n{out_text}"
    );
}

/// 8. JSON output shape: parses, findings matches warn count, every check has level/name/detail fields.
#[test]
fn json_shape_and_exit() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");

    // Don't run ff — chains + triggers will warn
    let out = doctor(&fx, &["--json"]);
    assert_eq!(out.status.code(), Some(1), "exit 1 with warns in JSON mode");

    let out_text = stdout(&out);
    let trimmed = out_text.trim();

    // Parse as JSON
    let body: serde_json::Value = serde_json::from_str(trimmed).expect("stdout is valid JSON");
    let d = &body["data"];

    let findings = d["findings"].as_u64().expect("findings is a number");
    let fixable = d["fixable"].as_u64().expect("fixable is a number");
    let checks = d["checks"].as_array().expect("checks is an array");

    // findings >= 2 (chains + triggers at minimum)
    assert!(
        findings >= 2,
        "at least 2 findings (chains + triggers), got {findings}"
    );

    // fixable is a number (already asserted above via as_u64)
    let _ = fixable;

    // findings == number of warn entries
    let warn_count = checks.iter().filter(|c| c["level"] == "warn").count();
    assert_eq!(findings as usize, warn_count, "findings matches warn count");

    // Every check has non-empty level, name, detail
    for (i, check) in checks.iter().enumerate() {
        assert!(
            check["level"].is_string() && !check["level"].as_str().unwrap().is_empty(),
            "check[{i}] has non-empty level"
        );
        assert!(
            check["name"].is_string() && !check["name"].as_str().unwrap().is_empty(),
            "check[{i}] has non-empty name"
        );
        assert!(
            check["detail"].is_string() && !check["detail"].as_str().unwrap().is_empty(),
            "check[{i}] has non-empty detail"
        );
    }
}

/// 9. Running doctor outside a git repository: repo info + wiring checks still run.
#[test]
fn outside_repository() {
    let dir = tempfile::TempDir::new().expect("create temp dir");
    let home = dir.path().join("home");
    std::fs::create_dir_all(&home).expect("create home");

    let out = doctor_env(dir.path(), &["doctor"], &home);
    assert_eq!(
        out.status.code(),
        Some(1),
        "exit 1 — triggers warn even outside repo"
    );

    let out_text = stdout(&out);
    assert!(
        out_text.contains("info  repository"),
        "repository info:\n{out_text}"
    );
    assert!(
        out_text.contains("not inside a git repository — repo checks skipped"),
        "repo detail:\n{out_text}"
    );
    // Wiring checks still run
    assert!(
        out_text.contains("claude hooks"),
        "claude hooks present:\n{out_text}"
    );
    assert!(out_text.contains("alias"), "alias present:\n{out_text}");
    assert!(out_text.contains("update"), "update present:\n{out_text}");
    // chains should NOT appear (repo check)
    assert!(
        !out_text.contains("chains"),
        "chains should be absent outside repo:\n{out_text}"
    );
}

/// 10. Auto-trim row reports the lane is on with the default cadence.
#[test]
fn auto_trim_row_reports_on() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");

    ff_init(&fx);

    let out = doctor(&fx, &[]);
    let out_text = stdout(&out);
    assert!(
        out_text.contains("auto-trim"),
        "auto-trim row present:\n{out_text}"
    );
    assert!(
        out_text.contains("1d"),
        "default cadence named:\n{out_text}"
    );
}

/// 11. Auto-trim row reports off when `fufu.autoTrim` is false.
#[test]
fn auto_trim_row_reports_off() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");

    fx.set_config("fufu.autoTrim", "false");

    ff_init(&fx);

    let out = doctor(&fx, &[]);
    let out_text = stdout(&out);
    assert!(
        out_text.contains("auto-trim"),
        "auto-trim row present:\n{out_text}"
    );
    assert!(
        out_text.contains("off (the autoTrim setting) — trim runs only by hand"),
        "off detail:\n{out_text}"
    );
}

/// 12. No shell wiring: the ambient row is info, not a finding.
#[test]
fn ambient_row_reports_not_installed() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");

    let out = doctor(&fx, &[]);
    let out_text = stdout(&out);
    assert!(
        out_text.contains("info  ambient"),
        "ambient info row:\n{out_text}"
    );
    assert!(
        out_text.contains("no prompt hook found"),
        "not-installed detail:\n{out_text}"
    );
    // Deliberately no exit-code assertion: this fixture has its own
    // warnings, and this test is about the row alone.
}

/// 13. `fufu.ambient` false: the row says the channel is off.
#[test]
fn ambient_row_reports_off_when_the_setting_is_false() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");

    fx.set_config("fufu.ambient", "false");

    let out = doctor(&fx, &[]);
    let out_text = stdout(&out);
    assert!(
        out_text.contains("off (the ambient setting)"),
        "off detail:\n{out_text}"
    );
    assert!(
        out_text.contains("the prompt channel is silent"),
        "off detail:\n{out_text}"
    );
}

/// 14. Tripwire 2's guard: the ambient row must be ok or info in every
///     fixture shape, never warn — an uninstalled optional channel must not
///     turn `ff doctor` red.
#[test]
fn ambient_row_never_warns() {
    // 1. No shell wiring at all.
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    let out_text = stdout(&doctor(&fx, &[]));
    assert!(
        !out_text.contains("WARN  ambient"),
        "no-wiring fixture:\n{out_text}"
    );

    // 2. The setting is false.
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    fx.set_config("fufu.ambient", "false");
    let out_text = stdout(&doctor(&fx, &[]));
    assert!(
        !out_text.contains("WARN  ambient"),
        "setting-false fixture:\n{out_text}"
    );

    // 3. HOME's .bashrc carries the marked `ff hook shell trigger` line —
    //    the real installer writes it.
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    let out = doctor_env(
        &fx.path(),
        &["hook", "shell", "install", "bash"],
        &fx.root().join("home"),
    );
    assert!(
        out.status.success(),
        "hook install succeeded: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let out_text = stdout(&doctor(&fx, &[]));
    assert!(
        !out_text.contains("WARN  ambient"),
        "installed fixture:\n{out_text}"
    );
}

/// Two remotes, neither named `origin`, and a branch whose section names none
/// of them: doctor calls the remote floor a finding and names the way out.
#[test]
fn ambiguous_remotes_are_a_finding() {
    let fx = Fixture::new();
    fx.write("root.txt", "root\n");
    fx.commit("root");
    ff_init(&fx);

    // Two remotes, neither `origin` — the shape that leaves `for_branch`
    // with nothing to name for the branch underfoot.
    fx.set_config("remote.one.url", "/nonexistent/one.git");
    fx.set_config("remote.one.fetch", "+refs/heads/*:refs/remotes/one/*");
    fx.set_config("remote.two.url", "/nonexistent/two.git");
    fx.set_config("remote.two.fetch", "+refs/heads/*:refs/remotes/two/*");

    let out = doctor(&fx, &[]);
    let text = stdout(&out);
    assert_eq!(out.status.code(), Some(1), "exit 1 with a finding:\n{text}");
    assert!(
        text.contains("WARN  remotes"),
        "the remotes finding:\n{text}"
    );
    assert!(
        text.contains("no nameable remote for"),
        "the refusal is named:\n{text}"
    );
}

/// A plain `ff branch delete` of a published branch keeps its section and
/// tracking ref on purpose, so doctor reports it as `info`, not a warning,
/// and the run stays green.
#[test]
fn a_plain_delete_of_a_published_branch_is_not_a_finding() {
    let fx = Fixture::new_cloned();
    // The gc guard a first snapshot would write — the first ops here are
    // publish/delete (no snapshot), so no close has written it yet.
    fx.set_config("gc.refs/fufu/*.reflogExpire", "never");
    fx.set_config("gc.refs/fufu/*.reflogExpireUnreachable", "never");

    // Publish `main`, then a second branch `shared`, then delete `shared` —
    // the state a real publish-then-delete leaves.
    fx.write("root.txt", "root\n");
    fx.commit("root");
    let pub_main = doctor_env(&fx.path(), &["publish"], &fx.root().join("home"));
    assert!(
        pub_main.status.success(),
        "first publish of main succeeds: {}",
        String::from_utf8_lossy(&pub_main.stderr)
    );

    fx.git(&["switch", "-q", "-c", "shared"]);
    fx.write("shared.txt", "shared\n");
    fx.commit("shared");
    let pub_shared = doctor_env(&fx.path(), &["publish"], &fx.root().join("home"));
    assert!(
        pub_shared.status.success(),
        "publish of shared succeeds: {}",
        String::from_utf8_lossy(&pub_shared.stderr)
    );

    // Delete `shared` from under `main` — `ff branch delete` refuses the
    // current branch, so stand on `main`.
    fx.git(&["switch", "-q", "main"]);
    let del = doctor_env(
        &fx.path(),
        &["branch", "delete", "shared"],
        &fx.root().join("home"),
    );
    assert!(
        del.status.success(),
        "branch delete of the published shared succeeds: {}",
        String::from_utf8_lossy(&del.stderr)
    );

    // Initialize captures + the operation log + the gc guard, and wire the
    // bash alias so the unrelated rows stay green.
    ff_init(&fx);
    let alias = doctor_env(
        &fx.path(),
        &["hook", "shell", "install", "bash"],
        &fx.root().join("home"),
    );
    assert!(
        alias.status.success(),
        "alias install succeeded: {}",
        String::from_utf8_lossy(&alias.stderr)
    );

    let out = doctor(&fx, &[]);
    let text = stdout(&out);
    assert!(
        text.contains("info  upstreams"),
        "the deliberate residue is info:\n{text}"
    );
    assert!(
        !text.contains("WARN  upstreams"),
        "a plain published-delete is not a finding:\n{text}"
    );
    assert_eq!(
        out.status.code(),
        Some(0),
        "exit 0 when nothing is a finding:\n{text}"
    );
}

/// A `[branch "<n>"]` section naming a branch that is not here *and* whose
/// shared copy is also gone is repairable: `--fix` removes exactly that
/// section.
#[test]
fn a_section_pointing_at_nothing_is_fixable() {
    let fx = Fixture::new_cloned();
    // The gc guard a first snapshot would write — the first ops here are
    // publish/delete (no snapshot), so no close has written it yet.
    fx.set_config("gc.refs/fufu/*.reflogExpire", "never");
    fx.set_config("gc.refs/fufu/*.reflogExpireUnreachable", "never");

    fx.write("root.txt", "root\n");
    fx.commit("root");
    let pub_main = doctor_env(&fx.path(), &["publish"], &fx.root().join("home"));
    assert!(
        pub_main.status.success(),
        "first publish of main succeeds: {}",
        String::from_utf8_lossy(&pub_main.stderr)
    );

    fx.git(&["switch", "-q", "-c", "shared"]);
    fx.write("shared.txt", "shared\n");
    fx.commit("shared");
    let pub_shared = doctor_env(&fx.path(), &["publish"], &fx.root().join("home"));
    assert!(
        pub_shared.status.success(),
        "publish of shared succeeds: {}",
        String::from_utf8_lossy(&pub_shared.stderr)
    );

    fx.git(&["switch", "-q", "main"]);
    let del = doctor_env(
        &fx.path(),
        &["branch", "delete", "shared"],
        &fx.root().join("home"),
    );
    assert!(
        del.status.success(),
        "branch delete of the published shared succeeds: {}",
        String::from_utf8_lossy(&del.stderr)
    );

    // Now the shared copy is gone too: the section points at nothing.
    fx.git(&["update-ref", "-d", "refs/remotes/origin/shared"]);

    ff_init(&fx);
    let alias = doctor_env(
        &fx.path(),
        &["hook", "shell", "install", "bash"],
        &fx.root().join("home"),
    );
    assert!(
        alias.status.success(),
        "alias install succeeded: {}",
        String::from_utf8_lossy(&alias.stderr)
    );

    let out = doctor(&fx, &[]);
    let text = stdout(&out);
    assert_eq!(out.status.code(), Some(1), "exit 1 with a finding:\n{text}");
    assert!(
        text.contains("WARN  upstreams"),
        "the dead section is a finding:\n{text}"
    );

    let fixed = doctor(&fx, &["--fix"]);
    let fixed_text = stdout(&fixed);
    assert!(
        fixed.status.success(),
        "--fix repairs and stays green: {fixed_text}"
    );
    let remaining = fx.try_git(&["config", "--get", "branch.shared.remote"]);
    assert!(
        !remaining.status.success(),
        "the section is gone after --fix:\n{fixed_text}"
    );
}
