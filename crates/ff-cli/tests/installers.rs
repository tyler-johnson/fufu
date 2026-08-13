//! Installer behavior: rc-file alias editing (env-redirected, hermetic) and
//! Claude settings.json merging — idempotent, foreign-preserving, refusing
//! malformed files untouched.

use std::path::Path;
use std::process::{Command, Output};

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

    let out = ff_env(home.path(), &["hook", "shell", "install", "bash"], &env);
    assert!(out.status.success());
    assert_eq!(
        std::fs::read_to_string(&rc).unwrap(),
        original,
        "install left it alone"
    );

    let out = ff_env(home.path(), &["hook", "shell", "uninstall", "bash"], &env);
    assert!(out.status.success());
    assert_eq!(
        std::fs::read_to_string(&rc).unwrap(),
        original,
        "uninstall left it alone"
    );
    let text = String::from_utf8(out.stdout).unwrap();
    assert!(text.contains("hand"), "explains why: {text:?}");
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
    ff_env(home.path(), &["hook", "shell", "install", "bash"], &env);
    let out = ff_env(home.path(), &["hook", "shell", "list"], &env);
    let text = String::from_utf8(out.stdout).unwrap();
    assert!(text.contains("bash  installed"), "{text:?}");
    assert!(text.contains("zsh   not installed"), "{text:?}");
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
    assert!(text.contains("bash  installed"), "{text:?}");

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
