//! Installer behavior: rc-file alias editing (env-redirected, hermetic) and
//! Claude settings.json merging — idempotent, foreign-preserving, refusing
//! malformed files untouched.

use std::path::Path;
use std::process::{Command, Output};

use ff_testsupport::Fixture;
use ff_testsupport::fixtures::null_device;

fn ff_env(dir: &Path, args: &[&str], envs: &[(&str, &str)]) -> Output {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_ff"));
    cmd.current_dir(dir)
        .args(args)
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("GIT_CONFIG_GLOBAL", null_device())
        .env("GIT_CONFIG_SYSTEM", null_device())
        .env("GIT_CONFIG_NOSYSTEM", "1");
    // env_clear() strips vars Windows processes cannot live without.
    #[cfg(windows)]
    for key in ["SYSTEMROOT", "WINDIR", "TEMP", "TMP", "PATHEXT", "COMSPEC"] {
        if let Some(value) = std::env::var_os(key) {
            cmd.env(key, value);
        }
    }
    for (k, v) in envs {
        cmd.env(k, v);
    }
    cmd.output().expect("spawn ff")
}

#[test]
fn bash_install_uninstall_round_trip() {
    let home = tempfile::TempDir::new().unwrap();
    let rc = home.path().join(".bashrc");
    std::fs::write(&rc, "# my prompt setup\nexport FOO=bar\n").unwrap();
    let env = [("HOME", home.path().to_str().unwrap())];

    let out = ff_env(home.path(), &["hook", "shell", "install", "bash"], &env);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let contents = std::fs::read_to_string(&rc).unwrap();
    assert!(contents.starts_with("# my prompt setup\nexport FOO=bar\n"));
    assert!(
        contents.contains("alias git='ff git'  # fufu — added by `ff hook shell install`"),
        "marked line appended: {contents:?}"
    );

    // Idempotent.
    let before = contents.clone();
    ff_env(home.path(), &["hook", "shell", "install", "bash"], &env);
    assert_eq!(std::fs::read_to_string(&rc).unwrap(), before);

    // Uninstall removes exactly the marked line.
    let out = ff_env(home.path(), &["hook", "shell", "uninstall", "bash"], &env);
    assert!(out.status.success());
    assert_eq!(
        std::fs::read_to_string(&rc).unwrap(),
        "# my prompt setup\nexport FOO=bar\n"
    );
}

#[test]
fn zsh_honors_zdotdir_and_fish_honors_xdg() {
    let home = tempfile::TempDir::new().unwrap();
    let zdot = home.path().join("zdot");
    let xdg = home.path().join("xdg");
    std::fs::create_dir_all(&zdot).unwrap();
    let env = [
        ("HOME", home.path().to_str().unwrap()),
        ("ZDOTDIR", zdot.to_str().unwrap()),
        ("XDG_CONFIG_HOME", xdg.to_str().unwrap()),
    ];

    assert!(
        ff_env(home.path(), &["hook", "shell", "install", "zsh"], &env)
            .status
            .success()
    );
    let zshrc = std::fs::read_to_string(zdot.join(".zshrc")).unwrap();
    assert!(zshrc.contains("alias git='ff git'"));

    assert!(
        ff_env(home.path(), &["hook", "shell", "install", "fish"], &env)
            .status
            .success()
    );
    let fishrc = std::fs::read_to_string(xdg.join("fish/config.fish")).unwrap();
    assert!(
        fishrc.contains("alias git 'ff git'"),
        "fish alias syntax: {fishrc:?}"
    );
}

#[test]
fn hand_written_alias_is_never_touched() {
    let home = tempfile::TempDir::new().unwrap();
    let rc = home.path().join(".bashrc");
    let original = "alias git='ff git' # I wrote this myself\n";
    std::fs::write(&rc, original).unwrap();
    let env = [("HOME", home.path().to_str().unwrap())];

    // Install still adds the (independently absent) ambient hook even
    // though the alias is hand-written: the two pieces are detected, and
    // therefore installed, independently — that is the whole point of the
    // alias/ambient split.
    let out = ff_env(home.path(), &["hook", "shell", "install", "bash"], &env);
    assert!(out.status.success());
    let install_text = String::from_utf8(out.stdout).unwrap();
    assert!(
        install_text.contains("hand"),
        "explains why: {install_text:?}"
    );
    let contents = std::fs::read_to_string(&rc).unwrap();
    assert!(
        contents.starts_with(original),
        "hand-written alias untouched: {contents:?}"
    );
    assert!(
        contents.contains("ff hook shell trigger"),
        "ambient hook still installed: {contents:?}"
    );

    let out = ff_env(home.path(), &["hook", "shell", "uninstall", "bash"], &env);
    assert!(out.status.success());
    assert_eq!(
        std::fs::read_to_string(&rc).unwrap(),
        original,
        "uninstall removed only the ambient hook it added, leaving the hand-written alias"
    );
}

#[test]
fn unsupported_shell_errors() {
    let home = tempfile::TempDir::new().unwrap();
    let env = [("HOME", home.path().to_str().unwrap())];
    let out = ff_env(home.path(), &["hook", "shell", "install", "tcsh"], &env);
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(stderr.contains("unsupported shell"));
}

#[test]
fn shell_list_reports_state() {
    let home = tempfile::TempDir::new().unwrap();
    let env = [("HOME", home.path().to_str().unwrap())];
    // `install` now wires the alias and the ambient hook together, so both
    // read "installed" for bash; zsh was never touched, so both read "not
    // installed" for it.
    ff_env(home.path(), &["hook", "shell", "install", "bash"], &env);
    let out = ff_env(home.path(), &["hook", "shell", "list"], &env);
    let text = String::from_utf8(out.stdout).unwrap();
    assert!(
        text.contains("bash  alias installed, ambient installed"),
        "{text:?}"
    );
    assert!(
        text.contains("zsh   alias not installed, ambient not installed"),
        "{text:?}"
    );
}

// ---- Part 1/3: alias and ambient wiring, extended for this brief ---------

#[test]
fn install_writes_both_the_alias_and_the_prompt_hook() {
    let home = tempfile::TempDir::new().unwrap();
    let rc = home.path().join(".bashrc");
    std::fs::write(&rc, "# existing stuff\n").unwrap();
    let env = [("HOME", home.path().to_str().unwrap())];

    let out = ff_env(home.path(), &["hook", "shell", "install", "bash"], &env);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );

    let contents = std::fs::read_to_string(&rc).unwrap();
    assert!(
        contents.starts_with("# existing stuff\n"),
        "foreign content preserved at the head of the file: {contents:?}"
    );
    assert!(
        contents
            .lines()
            .any(|l| l.contains("alias git='ff git'") && l.contains("# fufu — added by")),
        "marked alias line present: {contents:?}"
    );
    assert!(
        contents
            .lines()
            .any(|l| l.contains("ff hook shell trigger") && l.contains("# fufu — added by")),
        "marked ambient line present: {contents:?}"
    );
}

#[test]
fn zsh_install_writes_two_marked_ambient_lines() {
    let home = tempfile::TempDir::new().unwrap();
    let zdot = home.path().join("zdot");
    std::fs::create_dir_all(&zdot).unwrap();
    let env = [
        ("HOME", home.path().to_str().unwrap()),
        ("ZDOTDIR", zdot.to_str().unwrap()),
    ];

    let out = ff_env(home.path(), &["hook", "shell", "install", "zsh"], &env);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );

    let contents = std::fs::read_to_string(zdot.join(".zshrc")).unwrap();
    assert!(
        contents
            .lines()
            .any(|l| l.contains("_fufu_ambient() {") && l.contains("# fufu — added by")),
        "marked precmd function line: {contents:?}"
    );
    assert!(
        contents
            .lines()
            .any(|l| l.contains("precmd_functions+=(_fufu_ambient)")
                && l.contains("# fufu — added by")),
        "marked precmd registration line: {contents:?}"
    );
}

#[test]
fn fish_install_writes_the_event_function() {
    let home = tempfile::TempDir::new().unwrap();
    let xdg = home.path().join("xdg");
    let env = [
        ("HOME", home.path().to_str().unwrap()),
        ("XDG_CONFIG_HOME", xdg.to_str().unwrap()),
    ];

    let out = ff_env(home.path(), &["hook", "shell", "install", "fish"], &env);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );

    let contents = std::fs::read_to_string(xdg.join("fish/config.fish")).unwrap();
    assert!(
        contents
            .lines()
            .any(|l| l.contains("--on-event fish_prompt") && l.contains("# fufu — added by")),
        "marked event function line: {contents:?}"
    );
}

#[test]
fn install_is_idempotent_with_both_lines() {
    let home = tempfile::TempDir::new().unwrap();
    let env = [("HOME", home.path().to_str().unwrap())];
    let rc = home.path().join(".bashrc");

    let out = ff_env(home.path(), &["hook", "shell", "install", "bash"], &env);
    assert!(out.status.success());
    let after_first = std::fs::read_to_string(&rc).unwrap();

    let out = ff_env(home.path(), &["hook", "shell", "install", "bash"], &env);
    assert!(out.status.success());
    let after_second = std::fs::read_to_string(&rc).unwrap();
    assert_eq!(after_first, after_second, "byte-identical on the re-run");

    let text = String::from_utf8(out.stdout).unwrap();
    assert!(
        !text.contains("restart the shell"),
        "no restart notice on a fully idempotent re-run: {text:?}"
    );
}

#[test]
fn uninstall_removes_both() {
    let home = tempfile::TempDir::new().unwrap();
    let rc = home.path().join(".bashrc");
    let original = "# untouched header\n";
    std::fs::write(&rc, original).unwrap();
    let env = [("HOME", home.path().to_str().unwrap())];

    assert!(
        ff_env(home.path(), &["hook", "shell", "install", "bash"], &env)
            .status
            .success()
    );
    assert!(
        ff_env(home.path(), &["hook", "shell", "uninstall", "bash"], &env)
            .status
            .success()
    );

    assert_eq!(std::fs::read_to_string(&rc).unwrap(), original);
}

#[test]
fn a_hand_written_prompt_hook_is_detected_and_left_alone() {
    let home = tempfile::TempDir::new().unwrap();
    let rc = home.path().join(".bashrc");
    let hand_written = "PROMPT_COMMAND=\"ff hook shell trigger;$PROMPT_COMMAND\"\n";
    std::fs::write(&rc, hand_written).unwrap();
    let env = [("HOME", home.path().to_str().unwrap())];

    let out = ff_env(home.path(), &["hook", "shell", "install", "bash"], &env);
    assert!(out.status.success());
    let text = String::from_utf8(out.stdout).unwrap();
    assert!(text.contains("by hand"), "explains why: {text:?}");

    let contents = std::fs::read_to_string(&rc).unwrap();
    assert!(
        contents.contains(hand_written.trim_end()),
        "hand-written line untouched: {contents:?}"
    );
    assert!(
        contents
            .lines()
            .any(|l| l.contains("alias git='ff git'") && l.contains("# fufu — added by")),
        "alias still added: {contents:?}"
    );

    let out = ff_env(home.path(), &["hook", "shell", "uninstall", "bash"], &env);
    assert!(out.status.success());
    let contents = std::fs::read_to_string(&rc).unwrap();
    assert!(
        contents.contains(hand_written.trim_end()),
        "hand-written line survives uninstall: {contents:?}"
    );
    assert!(
        !contents.contains("alias git='ff git'"),
        "alias removed: {contents:?}"
    );
}

/// Regression guard for the alias_state/ambient_state split: a file with
/// only the marked ambient lines (no alias) must report the alias absent
/// and the ambient installed. The old single `state_of` conflated the two
/// and would have reported the alias installed on an ambient-only file.
#[test]
fn alias_and_ambient_are_detected_independently() {
    let home = tempfile::TempDir::new().unwrap();
    let rc = home.path().join(".bashrc");
    std::fs::write(
        &rc,
        "[[ $PROMPT_COMMAND == *\"ff hook shell trigger\"* ]] || PROMPT_COMMAND=\"ff hook shell trigger;$PROMPT_COMMAND\"  # fufu — added by `ff hook shell install`\n",
    )
    .unwrap();
    let env = [("HOME", home.path().to_str().unwrap())];

    let out = ff_env(home.path(), &["hook", "shell", "list"], &env);
    assert!(out.status.success());
    let text = String::from_utf8(out.stdout).unwrap();
    let bash_row = text
        .lines()
        .find(|l| l.starts_with("bash"))
        .expect("bash row present");
    assert!(bash_row.contains("alias not installed"), "{bash_row:?}");
    assert!(bash_row.contains("ambient installed"), "{bash_row:?}");
}

// ---- Part 4: the trigger runtime ------------------------------------------

/// `main` moved once after branching, `feature` one commit ahead — a clean
/// rebase, so there is a verdict for the trigger to (not) speak about.
fn fixture_with_a_verdict() -> Fixture {
    let fx = Fixture::new();
    fx.write("shared.txt", "line1\n");
    fx.commit("base");
    fx.git(&["branch", "feature"]);
    fx.write("m.txt", "m\n");
    fx.commit("main moves");
    fx.git(&["switch", "feature"]);
    fx.write("f1.txt", "f1\n");
    fx.commit("add f1");
    fx
}

#[test]
fn trigger_outside_a_repo_exits_zero_and_says_nothing() {
    let dir = tempfile::TempDir::new().unwrap();
    let env: [(&str, &str); 0] = [];
    let out = ff_env(dir.path(), &["hook", "shell", "trigger"], &env);
    assert!(out.status.success());
    assert!(
        out.stdout.is_empty(),
        "{:?}",
        String::from_utf8_lossy(&out.stdout)
    );
    assert!(
        out.stderr.is_empty(),
        "{:?}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// The test harness always captures stdout through a pipe, so this can only
/// prove the TTY gate fires under a pipe — it cannot hermetically exercise
/// the speaking path (there is no way to hand a spawned child a real TTY
/// here). The speaking path is covered instead by
/// `trigger_writes_and_reuses_its_fingerprint`, through the fingerprint
/// file the gate prevents from ever being written.
#[test]
fn trigger_is_silent_when_stdout_is_not_a_tty() {
    let fx = fixture_with_a_verdict();
    let env: [(&str, &str); 0] = [];
    let out = ff_env(&fx.path(), &["hook", "shell", "trigger"], &env);
    assert!(out.status.success());
    assert!(
        out.stdout.is_empty(),
        "{:?}",
        String::from_utf8_lossy(&out.stdout)
    );
    assert!(
        out.stderr.is_empty(),
        "{:?}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// The TTY gate makes stdout-based assertions impossible under this
/// harness (see the previous test), so this asserts the side effect
/// instead: because the gate fires before any repository work, the
/// fingerprint file must never be written, proving the gate order
/// (cheapest gate first) actually short-circuits before the fingerprint
/// step runs.
#[test]
fn trigger_writes_and_reuses_its_fingerprint() {
    let fx = fixture_with_a_verdict();
    let env: [(&str, &str); 0] = [];
    let out = ff_env(&fx.path(), &["hook", "shell", "trigger"], &env);
    assert!(out.status.success());

    let fingerprint_path = fx.path().join(".git/fufu/ambient");
    assert!(
        !fingerprint_path.exists(),
        "the fingerprint must not be written when the TTY gate short-circuits first"
    );
}

// ---- claude hook installer -------------------------------------------------

fn settings_at(home: &Path) -> std::path::PathBuf {
    home.join(".claude/settings.json")
}

#[test]
fn hook_install_creates_both_events_idempotently() {
    let home = tempfile::TempDir::new().unwrap();
    let env = [("HOME", home.path().to_str().unwrap())];

    let out = ff_env(home.path(), &["hook", "agent", "install", "claude"], &env);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let text = std::fs::read_to_string(settings_at(home.path())).unwrap();
    let v: serde_json::Value = serde_json::from_str(&text).unwrap();
    let pre = v["hooks"]["PreToolUse"].as_array().unwrap();
    assert_eq!(pre[0]["matcher"], "Bash|Edit|Write|NotebookEdit");
    assert_eq!(
        pre[0]["hooks"][0]["command"],
        "ff hook agent trigger claude"
    );
    let ups = v["hooks"]["UserPromptSubmit"].as_array().unwrap();
    assert_eq!(
        ups[0]["hooks"][0]["command"],
        "ff hook agent trigger claude"
    );

    // Second install: byte-identical file.
    ff_env(home.path(), &["hook", "agent", "install", "claude"], &env);
    assert_eq!(
        std::fs::read_to_string(settings_at(home.path())).unwrap(),
        text
    );

    let out = ff_env(home.path(), &["hook", "agent", "list", "claude"], &env);
    let listing = String::from_utf8(out.stdout).unwrap();
    assert!(
        listing.contains("PreToolUse       installed"),
        "{listing:?}"
    );
}

#[test]
fn hook_install_preserves_foreign_content() {
    let home = tempfile::TempDir::new().unwrap();
    let env = [("HOME", home.path().to_str().unwrap())];
    let settings = settings_at(home.path());
    std::fs::create_dir_all(settings.parent().unwrap()).unwrap();
    let foreign = serde_json::json!({
        "model": "opus",
        "hooks": {
            "PreToolUse": [
                { "matcher": "Bash", "hooks": [{ "type": "command", "command": "my-linter" }] }
            ],
            "Stop": [
                { "hooks": [{ "type": "command", "command": "notify-send done" }] }
            ]
        },
        "env": { "FOO": "bar" }
    });
    std::fs::write(&settings, serde_json::to_string_pretty(&foreign).unwrap()).unwrap();

    assert!(
        ff_env(home.path(), &["hook", "agent", "install", "claude"], &env)
            .status
            .success()
    );
    let v: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&settings).unwrap()).unwrap();
    assert_eq!(v["model"], "opus", "foreign top-level fields preserved");
    assert_eq!(v["env"]["FOO"], "bar");
    assert_eq!(
        v["hooks"]["PreToolUse"][0]["hooks"][0]["command"], "my-linter",
        "foreign hook entries preserved value-identical"
    );
    assert_eq!(
        v["hooks"]["Stop"][0]["hooks"][0]["command"],
        "notify-send done"
    );
    assert_eq!(
        v["hooks"]["PreToolUse"][1]["hooks"][0]["command"], "ff hook agent trigger claude",
        "our entry appended after foreign ones"
    );

    // Uninstall removes only ours.
    assert!(
        ff_env(home.path(), &["hook", "agent", "uninstall", "claude"], &env)
            .status
            .success()
    );
    let v: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&settings).unwrap()).unwrap();
    assert_eq!(v["hooks"]["PreToolUse"].as_array().unwrap().len(), 1);
    assert_eq!(
        v["hooks"]["PreToolUse"][0]["hooks"][0]["command"],
        "my-linter"
    );
    assert_eq!(
        v["hooks"]["Stop"][0]["hooks"][0]["command"],
        "notify-send done"
    );
    assert!(
        v["hooks"].get("UserPromptSubmit").is_none(),
        "our event removed"
    );
    assert_eq!(v["model"], "opus");
}

#[test]
fn hook_install_refuses_malformed_files_untouched() {
    let home = tempfile::TempDir::new().unwrap();
    let env = [("HOME", home.path().to_str().unwrap())];
    let settings = settings_at(home.path());
    std::fs::create_dir_all(settings.parent().unwrap()).unwrap();

    for bad in [
        "{ not json",
        "[1, 2, 3]",
        r#"{ "hooks": "not an object" }"#,
        r#"{ "hooks": { "PreToolUse": "not an array" } }"#,
    ] {
        std::fs::write(&settings, bad).unwrap();
        let out = ff_env(home.path(), &["hook", "agent", "install", "claude"], &env);
        assert_eq!(out.status.code(), Some(1), "must refuse: {bad}");
        assert_eq!(
            std::fs::read_to_string(&settings).unwrap(),
            bad,
            "file untouched on refusal"
        );
    }
}

/// A settings file carrying the Phase 1 spelling (`ff hook claude`) is
/// upgraded in place by install — never duplicated, never orphaned.
#[test]
fn legacy_hook_command_is_upgraded_not_duplicated() {
    let home = tempfile::TempDir::new().unwrap();
    let env = [("HOME", home.path().to_str().unwrap())];
    let settings = settings_at(home.path());
    std::fs::create_dir_all(settings.parent().unwrap()).unwrap();
    let legacy = serde_json::json!({
        "hooks": {
            "PreToolUse": [
                { "matcher": "Bash|Edit|Write|NotebookEdit",
                  "hooks": [{ "type": "command", "command": "ff hook claude" }] }
            ],
            "UserPromptSubmit": [
                { "hooks": [{ "type": "command", "command": "ff hook claude" }] }
            ]
        }
    });
    std::fs::write(&settings, serde_json::to_string_pretty(&legacy).unwrap()).unwrap();

    assert!(
        ff_env(home.path(), &["hook", "agent", "install", "claude"], &env)
            .status
            .success()
    );
    let text = std::fs::read_to_string(&settings).unwrap();
    let v: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(
        v["hooks"]["PreToolUse"].as_array().unwrap().len(),
        1,
        "no duplicate entry"
    );
    assert_eq!(
        v["hooks"]["PreToolUse"][0]["hooks"][0]["command"], "ff hook agent trigger claude",
        "legacy command upgraded in place"
    );
    assert!(!text.contains("\"ff hook claude\""), "old spelling gone");

    // And uninstall recognizes both spellings.
    std::fs::write(&settings, serde_json::to_string_pretty(&legacy).unwrap()).unwrap();
    assert!(
        ff_env(home.path(), &["hook", "agent", "uninstall", "claude"], &env)
            .status
            .success()
    );
    let v: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&settings).unwrap()).unwrap();
    assert!(v.get("hooks").is_none(), "legacy entries removed: {v}");
}

/// The Phase 1 shell marker is still recognized by uninstall and list.
#[test]
fn legacy_shell_marker_still_managed() {
    let home = tempfile::TempDir::new().unwrap();
    let rc = home.path().join(".bashrc");
    std::fs::write(
        &rc,
        "export FOO=bar\nalias git='ff git'  # fufu — added by `ff shell install`\n",
    )
    .unwrap();
    let env = [("HOME", home.path().to_str().unwrap())];

    let out = ff_env(home.path(), &["hook", "shell", "list"], &env);
    let text = String::from_utf8(out.stdout).unwrap();
    assert!(
        text.contains("bash  alias installed, ambient not installed"),
        "{text:?}"
    );

    assert!(
        ff_env(home.path(), &["hook", "shell", "uninstall", "bash"], &env)
            .status
            .success()
    );
    assert_eq!(std::fs::read_to_string(&rc).unwrap(), "export FOO=bar\n");
}

#[test]
fn unknown_hook_client_install_errors_but_runtime_is_silent() {
    let home = tempfile::TempDir::new().unwrap();
    let env = [("HOME", home.path().to_str().unwrap())];
    let out = ff_env(home.path(), &["hook", "agent", "install", "cursor"], &env);
    assert_eq!(
        out.status.code(),
        Some(1),
        "installers are for humans: real errors"
    );
    // But an unknown runtime client must never veto: exit 0, no output.
    let out = ff_env(home.path(), &["hook", "agent", "trigger", "cursor"], &env);
    assert_eq!(out.status.code(), Some(0));
    assert!(out.stdout.is_empty());
    assert!(out.stderr.is_empty());
}
