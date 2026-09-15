//! Installer behavior for `ff hook` and `ff unhook`: rc-file editing, the
//! settings merge for the clients that take one, and the plugin
//! directories fufu owns for the rest.
//!
//! Every path here is env-redirected (HOME, ZDOTDIR, XDG_CONFIG_HOME,
//! SHELL, and FF_DOCUMENTS_DIR for PowerShell's profile on Windows) so the
//! suite never touches a real config file, and every client binary seam
//! points at nothing, so a client on the developer's PATH never runs.

use std::path::Path;
use std::process::{Command, Output};

use ff_testsupport::Fixture;
use ff_testsupport::fixtures::{ageless, null_device};

fn ff_env(dir: &Path, args: &[&str], envs: &[(&str, &str)]) -> Output {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_ff"));
    cmd.current_dir(dir)
        .args(args)
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("GIT_CONFIG_GLOBAL", null_device())
        .env("GIT_CONFIG_SYSTEM", null_device())
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("FF_CODEX", "/nonexistent")
        .env("FF_OPENCODE", "/nonexistent")
        .env("FF_COPILOT", "/nonexistent");
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

fn text(out: &Output) -> String {
    String::from_utf8(out.stdout.clone()).expect("stdout is utf-8")
}

fn json_at(path: &Path) -> serde_json::Value {
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

fn envelope(out: &Output) -> serde_json::Value {
    serde_json::from_slice(&out.stdout).expect("an envelope")
}

/// Codex's table, in the order it is written; Qwen names the same five.
const FAMILY_EVENTS: [&str; 5] = [
    "PreToolUse",
    "UserPromptSubmit",
    "SessionStart",
    "Stop",
    "SessionEnd",
];

// ---- the shells ------------------------------------------------------------

#[test]
fn bash_install_uninstall_round_trip() {
    let home = tempfile::TempDir::new().unwrap();
    let rc = home.path().join(".bashrc");
    std::fs::write(&rc, "# my prompt setup\nexport FOO=bar\n").unwrap();
    let env = [("HOME", home.path().to_str().unwrap())];

    let out = ff_env(home.path(), &["hook", "bash"], &env);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let contents = std::fs::read_to_string(&rc).unwrap();
    assert!(contents.starts_with("# my prompt setup\nexport FOO=bar\n"));
    assert!(
        contents.contains("alias git='ff git'  # fufu — added by `ff hook`"),
        "marked line appended: {contents:?}"
    );

    // Idempotent.
    let before = contents.clone();
    ff_env(home.path(), &["hook", "bash"], &env);
    assert_eq!(std::fs::read_to_string(&rc).unwrap(), before);

    // Uninstall removes exactly the marked lines.
    let out = ff_env(home.path(), &["unhook", "bash"], &env);
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

    assert!(ff_env(home.path(), &["hook", "zsh"], &env).status.success());
    let zshrc = std::fs::read_to_string(zdot.join(".zshrc")).unwrap();
    assert!(zshrc.contains("alias git='ff git'"));

    assert!(
        ff_env(home.path(), &["hook", "fish"], &env)
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
    let out = ff_env(home.path(), &["hook", "bash"], &env);
    assert!(out.status.success());
    assert!(
        text(&out).contains("hand"),
        "explains why: {:?}",
        text(&out)
    );
    let contents = std::fs::read_to_string(&rc).unwrap();
    assert!(
        contents.starts_with(original),
        "hand-written alias untouched: {contents:?}"
    );
    assert!(
        contents.contains("ff trigger shell"),
        "ambient hook still installed: {contents:?}"
    );

    let out = ff_env(home.path(), &["unhook", "bash"], &env);
    assert!(out.status.success());
    assert_eq!(
        std::fs::read_to_string(&rc).unwrap(),
        original,
        "uninstall removed only the ambient hook it added, leaving the hand-written alias"
    );
}

#[test]
fn install_writes_both_the_alias_and_the_prompt_hook() {
    let home = tempfile::TempDir::new().unwrap();
    let rc = home.path().join(".bashrc");
    std::fs::write(&rc, "# existing stuff\n").unwrap();
    let env = [("HOME", home.path().to_str().unwrap())];

    let out = ff_env(home.path(), &["hook", "bash"], &env);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let contents = std::fs::read_to_string(&rc).unwrap();
    assert!(contents.contains("alias git='ff git'"), "{contents:?}");
    assert!(
        contents.contains("PROMPT_COMMAND") && contents.contains("ff trigger shell"),
        "the prompt hook: {contents:?}"
    );
    // Every line fufu wrote carries the marker, so uninstall can find them.
    for line in contents.lines().filter(|l| l.contains("ff ")) {
        assert!(line.contains("# fufu — added by"), "unmarked: {line:?}");
    }
}

#[test]
fn zsh_install_writes_two_marked_ambient_lines() {
    let home = tempfile::TempDir::new().unwrap();
    let env = [("HOME", home.path().to_str().unwrap())];
    assert!(ff_env(home.path(), &["hook", "zsh"], &env).status.success());
    let contents = std::fs::read_to_string(home.path().join(".zshrc")).unwrap();
    assert!(
        contents.contains("_fufu_ambient() { ff trigger shell }"),
        "{contents:?}"
    );
    assert!(
        contents.contains("precmd_functions+=(_fufu_ambient)"),
        "{contents:?}"
    );
    assert_eq!(
        contents
            .lines()
            .filter(|l| l.contains("_fufu_ambient"))
            .count(),
        2,
        "both halves are marked: {contents:?}"
    );
}

#[test]
fn fish_install_writes_the_event_function() {
    let home = tempfile::TempDir::new().unwrap();
    let env = [("HOME", home.path().to_str().unwrap())];
    assert!(
        ff_env(home.path(), &["hook", "fish"], &env)
            .status
            .success()
    );
    let contents = std::fs::read_to_string(home.path().join(".config/fish/config.fish")).unwrap();
    assert!(
        contents.contains("--on-event fish_prompt; ff trigger shell; end"),
        "{contents:?}"
    );
}

#[test]
fn shell_install_is_idempotent_with_both_lines() {
    let home = tempfile::TempDir::new().unwrap();
    let env = [("HOME", home.path().to_str().unwrap())];
    ff_env(home.path(), &["hook", "bash"], &env);
    let before = std::fs::read_to_string(home.path().join(".bashrc")).unwrap();
    ff_env(home.path(), &["hook", "bash"], &env);
    assert_eq!(
        std::fs::read_to_string(home.path().join(".bashrc")).unwrap(),
        before
    );
}

#[test]
fn a_hand_written_prompt_hook_is_detected_and_left_alone() {
    let home = tempfile::TempDir::new().unwrap();
    let rc = home.path().join(".bashrc");
    // Unmarked, so it belongs to whoever wrote it.
    std::fs::write(&rc, "PROMPT_COMMAND=\"ff trigger shell;$PROMPT_COMMAND\"\n").unwrap();
    let env = [("HOME", home.path().to_str().unwrap())];

    let out = ff_env(home.path(), &["hook", "bash"], &env);
    assert!(out.status.success());
    assert!(text(&out).contains("by hand"), "says why: {:?}", text(&out));
    let contents = std::fs::read_to_string(&rc).unwrap();
    assert!(
        contents.starts_with("PROMPT_COMMAND=\"ff trigger shell;$PROMPT_COMMAND\"\n"),
        "untouched: {contents:?}"
    );
    // The alias was absent, so it was still installed.
    assert!(contents.contains("alias git='ff git'"), "{contents:?}");

    // And uninstall leaves the hand-written line behind.
    assert!(
        ff_env(home.path(), &["unhook", "bash"], &env)
            .status
            .success()
    );
    assert_eq!(
        std::fs::read_to_string(&rc).unwrap(),
        "PROMPT_COMMAND=\"ff trigger shell;$PROMPT_COMMAND\"\n"
    );
}

#[test]
fn unknown_slugs_are_hard_errors() {
    let home = tempfile::TempDir::new().unwrap();
    let env = [("HOME", home.path().to_str().unwrap())];
    for verb in ["hook", "unhook"] {
        let out = ff_env(home.path(), &[verb, "tcsh"], &env);
        // A usage error, and the tool spells those 2.
        assert_eq!(out.status.code(), Some(2), "{verb} tcsh");
        let stderr = String::from_utf8(out.stderr).unwrap();
        assert!(stderr.contains("unknown slug"), "{stderr:?}");
        // The complaint teaches the slugs rather than making you look.
        assert!(
            stderr.contains("claude") && stderr.contains("fish"),
            "{stderr:?}"
        );
    }
}

// ---- powershell ------------------------------------------------------------

const PROFILE: &str = "Microsoft.PowerShell_profile.ps1";

/// The env that pins PowerShell's profile under a temp home on every
/// platform: HOME everywhere, and on Windows the Documents folder too,
/// since the real one comes from the known-folder API rather than from any
/// variable.
fn powershell_env(home: &Path) -> Vec<(&'static str, String)> {
    let mut env = vec![("HOME", home.to_str().unwrap().to_string())];
    if cfg!(windows) {
        env.push((
            "FF_DOCUMENTS_DIR",
            home.join("Documents").to_str().unwrap().to_string(),
        ));
    }
    env
}

/// Where `ff hook powershell` writes under a temp home when neither
/// profile exists: PowerShell 7's.
fn powershell_profile(home: &Path) -> std::path::PathBuf {
    if cfg!(windows) {
        home.join("Documents").join("PowerShell").join(PROFILE)
    } else {
        home.join(".config/powershell").join(PROFILE)
    }
}

fn ff_ps(dir: &Path, args: &[&str], env: &[(&str, String)]) -> Output {
    let borrowed: Vec<(&str, &str)> = env.iter().map(|(k, v)| (*k, v.as_str())).collect();
    ff_env(dir, args, &borrowed)
}

#[test]
fn powershell_install_uninstall_round_trip() {
    let home = tempfile::TempDir::new().unwrap();
    let env = powershell_env(home.path());
    let rc = powershell_profile(home.path());
    std::fs::create_dir_all(rc.parent().unwrap()).unwrap();
    let seed = "# my prompt setup\nSet-Alias ll Get-ChildItem\n";
    std::fs::write(&rc, seed).unwrap();

    let out = ff_ps(home.path(), &["hook", "powershell"], &env);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let said = text(&out);
    assert!(
        said.contains(&format!("wired into {}", rc.display())),
        "names the profile it wrote: {said:?}"
    );
    let contents = std::fs::read_to_string(&rc).unwrap();
    assert!(contents.starts_with(seed), "prefix preserved: {contents:?}");
    assert!(
        contents.contains("function git { ff git @args }  # fufu — added by `ff hook`"),
        "the git function: {contents:?}"
    );
    assert!(
        contents.contains(
            "if (-not (Test-Path Function:_fufu_prompt)) { $function:global:_fufu_prompt = $function:prompt; function global:prompt { ff trigger shell | Out-Null; _fufu_prompt } }  # fufu — added by `ff hook`"
        ),
        "the wrapped prompt: {contents:?}"
    );

    // Idempotent.
    let before = contents.clone();
    let out = ff_ps(home.path(), &["hook", "powershell"], &env);
    assert!(text(&out).contains("already wired"), "{:?}", text(&out));
    assert_eq!(std::fs::read_to_string(&rc).unwrap(), before);

    // Uninstall removes exactly the marked lines.
    let out = ff_ps(home.path(), &["unhook", "powershell"], &env);
    assert!(out.status.success());
    assert_eq!(std::fs::read_to_string(&rc).unwrap(), seed);
}

/// Windows PowerShell 5.1 and PowerShell 7 read different files. The 7
/// file is the one wired, unless the 5.1 file is the only profile on disk.
#[cfg(windows)]
#[test]
fn powershell_prefers_an_existing_windows_powershell_profile() {
    let home = tempfile::TempDir::new().unwrap();
    let env = powershell_env(home.path());
    let five = home
        .path()
        .join("Documents")
        .join("WindowsPowerShell")
        .join(PROFILE);
    std::fs::create_dir_all(five.parent().unwrap()).unwrap();
    std::fs::write(&five, "# 5.1\n").unwrap();

    let out = ff_ps(home.path(), &["hook", "powershell"], &env);
    assert!(out.status.success());
    assert!(
        text(&out).contains(&format!("wired into {}", five.display())),
        "{:?}",
        text(&out)
    );
    assert!(
        std::fs::read_to_string(&five)
            .unwrap()
            .contains("function git")
    );
    assert!(
        !powershell_profile(home.path()).exists(),
        "the 7 file is not created when 5.1's is the one"
    );
}

#[test]
fn a_hand_written_git_function_is_left_alone() {
    let home = tempfile::TempDir::new().unwrap();
    let env = powershell_env(home.path());
    let rc = powershell_profile(home.path());
    std::fs::create_dir_all(rc.parent().unwrap()).unwrap();
    let original = "function git { ff git @args }  # mine\n";
    std::fs::write(&rc, original).unwrap();

    let out = ff_ps(home.path(), &["hook", "powershell"], &env);
    assert!(out.status.success());
    assert!(
        text(&out).contains("hand"),
        "explains why: {:?}",
        text(&out)
    );
    let contents = std::fs::read_to_string(&rc).unwrap();
    assert!(contents.starts_with(original), "{contents:?}");
    assert!(
        contents.contains("ff trigger shell"),
        "the prompt hook still lands: {contents:?}"
    );

    let out = ff_ps(home.path(), &["unhook", "powershell"], &env);
    assert!(out.status.success());
    assert_eq!(std::fs::read_to_string(&rc).unwrap(), original);
}

#[cfg(not(windows))]
#[test]
fn powershell_honors_xdg_config_home() {
    let home = tempfile::TempDir::new().unwrap();
    let xdg = home.path().join("xdg");
    let mut env = powershell_env(home.path());
    env.push(("XDG_CONFIG_HOME", xdg.to_str().unwrap().to_string()));

    assert!(
        ff_ps(home.path(), &["hook", "powershell"], &env)
            .status
            .success()
    );
    let contents = std::fs::read_to_string(xdg.join("powershell").join(PROFILE)).unwrap();
    assert!(
        contents.contains("function git { ff git @args }"),
        "{contents:?}"
    );
    assert!(!powershell_profile(home.path()).exists());
}

/// A CRLF profile, the kind a Windows editor writes, keeps its line
/// endings through the append and through the removal.
#[test]
fn a_crlf_profile_keeps_its_line_endings() {
    let home = tempfile::TempDir::new().unwrap();
    let env = powershell_env(home.path());
    let rc = powershell_profile(home.path());
    std::fs::create_dir_all(rc.parent().unwrap()).unwrap();
    let seed = "# mine\r\nSet-Alias ll Get-ChildItem\r\n";
    std::fs::write(&rc, seed).unwrap();

    assert!(
        ff_ps(home.path(), &["hook", "powershell"], &env)
            .status
            .success()
    );
    let contents = std::fs::read_to_string(&rc).unwrap();
    assert!(contents.starts_with(seed), "{contents:?}");
    assert!(
        contents.ends_with("_fufu_prompt } }  # fufu — added by `ff hook`\r\n"),
        "the appended lines are CRLF too: {contents:?}"
    );
    assert!(!contents.replace("\r\n", "").contains('\n'), "{contents:?}");

    assert!(
        ff_ps(home.path(), &["unhook", "powershell"], &env)
            .status
            .success()
    );
    assert_eq!(std::fs::read_to_string(&rc).unwrap(), seed);
}

/// The first of `names` on PATH, resolved the way the OS resolves it (with
/// PATHEXT on Windows) and without spawning anything.
fn on_path(names: &[&str]) -> Option<std::path::PathBuf> {
    let path = std::env::var_os("PATH")?;
    let exts: Vec<String> = if cfg!(windows) {
        std::env::var("PATHEXT")
            .unwrap_or_else(|_| ".EXE".into())
            .split(';')
            .map(str::to_string)
            .collect()
    } else {
        vec![String::new()]
    };
    for dir in std::env::split_paths(&path) {
        for name in names {
            for ext in &exts {
                let candidate = dir.join(format!("{name}{ext}"));
                if candidate.is_file() {
                    return Some(candidate);
                }
            }
        }
    }
    None
}

/// The lines fufu writes, run by the real shell: dot-sourced twice, the
/// profile parses, `git` is a function, and `prompt` is wrapped exactly
/// once — the guard is what keeps a second dot-source from wrapping the
/// wrapper. Skips, and says so, where no PowerShell is installed.
#[test]
fn the_profile_parses_and_wraps_once_under_pwsh() {
    let Some(pwsh) = on_path(&["pwsh", "powershell"]) else {
        eprintln!("skipping: neither pwsh nor powershell is on PATH");
        return;
    };
    let home = tempfile::TempDir::new().unwrap();
    let env = powershell_env(home.path());
    let out = ff_ps(home.path(), &["hook", "powershell"], &env);
    assert!(out.status.success());
    let rc = powershell_profile(home.path());
    assert!(rc.is_file(), "{:?}", text(&out));

    // The binary's directory goes first on PATH so `ff trigger shell`
    // would resolve if the prompt ever ran.
    let bin_dir = Path::new(env!("CARGO_BIN_EXE_ff")).parent().unwrap();
    let mut paths = vec![bin_dir.to_path_buf()];
    paths.extend(std::env::split_paths(
        &std::env::var_os("PATH").unwrap_or_default(),
    ));
    let script = format!(
        ". '{rc}'; . '{rc}'; (Get-Command git).CommandType; $function:prompt; $function:_fufu_prompt",
        rc = rc.display()
    );
    let out = Command::new(&pwsh)
        .args(["-NoProfile", "-NonInteractive", "-Command", &script])
        .env("PATH", std::env::join_paths(paths).unwrap())
        .current_dir(home.path())
        .output()
        .expect("spawn pwsh");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        out.status.success(),
        "the profile parses and runs twice:\n{stdout}\n{stderr}"
    );
    assert!(stdout.contains("Function"), "git is a function: {stdout:?}");
    assert_eq!(
        stdout.matches("ff trigger shell").count(),
        1,
        "prompt names the trigger once and _fufu_prompt not at all: {stdout:?}"
    );
    assert!(
        stdout.contains("_fufu_prompt"),
        "prompt calls the saved one: {stdout:?}"
    );
}

// ---- the report ------------------------------------------------------------

/// `ff hook -l` and `ff doctor` read one `statuses()` vector, so they
/// cannot disagree about what is wired.
#[test]
fn the_report_and_doctor_agree() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    let home = tempfile::TempDir::new().unwrap();
    let env = [
        ("HOME", home.path().to_str().unwrap()),
        ("XDG_CACHE_HOME", home.path().to_str().unwrap()),
    ];

    // Nothing wired: the report says so, and doctor warns that nothing at
    // all feeds capture.
    let listing = text(&ff_env(home.path(), &["hook", "-l"], &env));
    assert!(
        listing.contains("claude"),
        "every slug has a row: {listing:?}"
    );
    assert!(listing.contains("not wired"), "{listing:?}");
    let doctor = text(&ff_env(&fx.path(), &["doctor"], &env));
    assert!(doctor.contains("WARN  triggers"), "{doctor:?}");

    // Wire one, and both change together.
    assert!(
        ff_env(home.path(), &["hook", "bash"], &env)
            .status
            .success()
    );
    let listing = text(&ff_env(home.path(), &["hook", "-l"], &env));
    assert!(
        listing.contains("alias wired") && listing.contains("ambient wired"),
        "{listing:?}"
    );
    let doctor = text(&ff_env(&fx.path(), &["doctor"], &env));
    assert!(doctor.contains("ok    alias"), "{doctor:?}");
    assert!(!doctor.contains("triggers"), "{doctor:?}");
}

/// Naming nothing where nothing may prompt reports and touches nothing.
/// The report is the useful half; acting without being asked is not.
#[test]
fn bare_hook_acts_on_nothing_when_it_cannot_ask() {
    let home = tempfile::TempDir::new().unwrap();
    std::fs::create_dir_all(home.path().join(".claude")).unwrap();
    let env = [
        ("HOME", home.path().to_str().unwrap()),
        ("FF_NONINTERACTIVE", "1"),
    ];
    let out = ff_env(home.path(), &["hook"], &env);
    assert!(out.status.success());
    let listing = text(&out);
    assert!(listing.contains("claude"), "reports: {listing:?}");
    assert!(
        listing.contains("name what you want: ff hook"),
        "teaches the explicit form: {listing:?}"
    );
    assert!(
        !home.path().join(".claude/skills/fufu").exists(),
        "nothing was wired"
    );
}

#[test]
fn the_report_is_a_json_envelope() {
    let home = tempfile::TempDir::new().unwrap();
    let env = [("HOME", home.path().to_str().unwrap())];
    let out = ff_env(home.path(), &["--json", "hook", "-l"], &env);
    assert!(out.status.success());
    let value: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(value["cmd"], "hook");
    let rows = value["data"]["integrations"].as_array().unwrap();
    assert_eq!(rows.len(), 10, "one row per slug: {rows:?}");
    assert_eq!(rows[0]["slug"], "claude");
    assert_eq!(rows[0]["wiring"]["state"], "not-wired");
}

// ---- the settings clients --------------------------------------------------

/// One file per vendor, each in the shape that vendor documents: a plugin
/// hooks file for Codex, nested settings entries for Qwen, and Cursor's
/// flat file with its schema version.
#[test]
fn each_client_is_wired_in_its_own_schema() {
    let home = tempfile::TempDir::new().unwrap();
    let env = [("HOME", home.path().to_str().unwrap())];

    assert!(
        ff_env(home.path(), &["hook", "codex"], &env)
            .status
            .success()
    );
    let v = json_at(
        &home
            .path()
            .join(".agents")
            .join("plugins")
            .join("fufu")
            .join("hooks")
            .join("hooks.json"),
    );
    assert_eq!(v["hooks"]["PreToolUse"][0]["matcher"], "Bash|apply_patch");
    let command = v["hooks"]["PreToolUse"][0]["hooks"][0]["command"]
        .as_str()
        .unwrap();
    assert!(command.ends_with("\" trigger codex"), "{command:?}");
    assert_eq!(v["hooks"]["PreToolUse"][0]["hooks"][0]["type"], "command");
    assert!(
        !home.path().join(".codex").join("hooks.json").exists(),
        "nothing is written to the old settings file"
    );

    assert!(
        ff_env(home.path(), &["hook", "qwen"], &env)
            .status
            .success()
    );
    let v = json_at(&home.path().join(".qwen").join("settings.json"));
    assert_eq!(
        v["hooks"]["PreToolUse"][0]["matcher"],
        "run_shell_command|write_file|replace|edit"
    );
    assert_eq!(
        v["hooks"]["PreToolUse"][0]["hooks"][0]["command"],
        "ff trigger qwen"
    );
    assert!(v["hooks"]["SessionStart"][0].get("matcher").is_none());

    // Cursor's file is flatter, and carries a schema version.
    assert!(
        ff_env(home.path(), &["hook", "cursor"], &env)
            .status
            .success()
    );
    let v = json_at(&home.path().join(".cursor/hooks.json"));
    assert_eq!(v["version"], 1);
    assert_eq!(v["hooks"]["preToolUse"][0]["matcher"], "Shell|Write|Delete");
    assert_eq!(v["hooks"]["preToolUse"][0]["command"], "ff trigger cursor");
    assert!(v["hooks"]["preToolUse"][0].get("hooks").is_none());
}

/// Codex skips a hook it has not been asked to trust, so an install that
/// did not say so would look like capture silently never happening.
#[test]
fn the_codex_trust_step_is_reported() {
    let home = tempfile::TempDir::new().unwrap();
    let env = [("HOME", home.path().to_str().unwrap())];
    let out = ff_env(home.path(), &["hook", "codex"], &env);
    assert!(out.status.success());
    assert!(text(&out).contains("/hooks"), "{:?}", text(&out));

    // And it stays in the report, where the wiring looks identical whether
    // or not it has been trusted.
    let listing = text(&ff_env(home.path(), &["hook", "-l"], &env));
    assert!(listing.contains("/hooks"), "{listing:?}");
}

#[test]
fn install_preserves_foreign_content() {
    let home = tempfile::TempDir::new().unwrap();
    let env = [("HOME", home.path().to_str().unwrap())];
    let settings = home.path().join(".qwen").join("settings.json");
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
        ff_env(home.path(), &["hook", "qwen"], &env)
            .status
            .success()
    );
    let v = json_at(&settings);
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
        v["hooks"]["PreToolUse"][1]["hooks"][0]["command"], "ff trigger qwen",
        "our entry appended after foreign ones"
    );
    // A foreign `Stop` entry and ours share the event.
    assert_eq!(v["hooks"]["Stop"].as_array().unwrap().len(), 2, "{v}");

    // Uninstall removes only ours.
    assert!(
        ff_env(home.path(), &["unhook", "qwen"], &env)
            .status
            .success()
    );
    let v = json_at(&settings);
    assert_eq!(v["hooks"]["PreToolUse"].as_array().unwrap().len(), 1);
    assert_eq!(
        v["hooks"]["PreToolUse"][0]["hooks"][0]["command"],
        "my-linter"
    );
    assert_eq!(v["hooks"]["Stop"].as_array().unwrap().len(), 1);
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
fn install_refuses_malformed_files_untouched() {
    let home = tempfile::TempDir::new().unwrap();
    let env = [("HOME", home.path().to_str().unwrap())];
    let settings = home.path().join(".qwen").join("settings.json");
    std::fs::create_dir_all(settings.parent().unwrap()).unwrap();

    for bad in [
        "{ not json",
        "[1, 2, 3]",
        r#"{ "hooks": "not an object" }"#,
        r#"{ "hooks": { "PreToolUse": "not an array" } }"#,
    ] {
        std::fs::write(&settings, bad).unwrap();
        let out = ff_env(home.path(), &["--json", "hook", "qwen"], &env);
        assert_eq!(out.status.code(), Some(1), "must refuse: {bad}");
        assert_eq!(envelope(&out)["error"]["id"], "hook/malformed", "{bad}");
        assert_eq!(
            std::fs::read_to_string(&settings).unwrap(),
            bad,
            "file untouched on refusal"
        );
    }

    // The report still renders, in both forms: a file that will not parse
    // is a complaint in the row, never a crash of the listing.
    std::fs::write(&settings, "{ not json").unwrap();
    let listing = ff_env(home.path(), &["hook", "-l"], &env);
    assert!(listing.status.success());
    assert!(
        text(&listing).contains("not valid JSON"),
        "{:?}",
        text(&listing)
    );
    let out = ff_env(home.path(), &["--json", "hook", "-l"], &env);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let value: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let qwen = value["data"]["integrations"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["slug"] == "qwen")
        .expect("a qwen row");
    assert_eq!(qwen["wiring"]["state"], "unavailable");
    assert!(
        qwen["wiring"]["complaint"]
            .as_str()
            .is_some_and(|c| c.contains("not valid JSON")),
        "{qwen}"
    );
}

// ---- the claude plugin -----------------------------------------------------

#[test]
fn the_claude_plugin_round_trips() {
    let home = tempfile::TempDir::new().unwrap();
    let env = [("HOME", home.path().to_str().unwrap())];
    let plugin = home.path().join(".claude/skills/fufu");

    let out = ff_env(home.path(), &["hook", "claude"], &env);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let manifest = json_at(&plugin.join(".claude-plugin/plugin.json"));
    assert_eq!(manifest["name"], "fufu");
    let hooks = json_at(&plugin.join("hooks/hooks.json"));
    assert_eq!(
        hooks["hooks"]["PreToolUse"][0]["matcher"],
        "Bash|Edit|Write|NotebookEdit"
    );
    // The binary's absolute path is baked in, so the plugin does not depend
    // on `ff` being on whatever PATH the client happens to have.
    let command = hooks["hooks"]["PreToolUse"][0]["hooks"][0]["command"]
        .as_str()
        .unwrap();
    assert!(command.ends_with("\" trigger claude"), "{command:?}");
    assert!(
        command.len() > "\"ff\" trigger claude".len(),
        "absolute path baked in: {command:?}"
    );
    // Always quoted: Git Bash on Windows runs the string, and an unquoted
    // `C:\Users\…` collapses to `C:Users…` there.
    assert!(command.starts_with('"'), "{command:?}");

    // The shipped skill rides inside the plugin, under the layout a
    // plugin's own skills take.
    let skill = plugin.join("skills/fufu/SKILL.md");
    let text_on_disk = std::fs::read_to_string(&skill).expect("the skill lands with the plugin");
    assert!(
        text_on_disk.starts_with("---\nname: fufu\n"),
        "frontmatter first"
    );

    // Idempotent, and reported as already wired.
    assert!(
        ff_env(home.path(), &["hook", "claude"], &env)
            .status
            .success()
    );
    let listing = text(&ff_env(home.path(), &["hook", "-l"], &env));
    assert!(listing.contains("wired (plugin)"), "{listing:?}");
    assert!(
        listing.contains("skill"),
        "the report says the skill is there: {listing:?}"
    );

    assert!(
        ff_env(home.path(), &["unhook", "claude"], &env)
            .status
            .success()
    );
    assert!(!plugin.exists(), "the directory fufu owns goes whole");
    assert!(!skill.exists(), "…and the skill inside it with it");
}

/// The escape hatch back to settings entries buys capture and nothing
/// else: the skill rides the plugin, so a machine on `--settings` has no
/// skill, and the briefing must not name one.
#[test]
fn the_settings_hatch_wires_capture_and_no_skill() {
    let home = tempfile::TempDir::new().unwrap();
    let env = [("HOME", home.path().to_str().unwrap())];
    let skill = home.path().join(".claude/skills/fufu/skills/fufu/SKILL.md");

    assert!(
        ff_env(home.path(), &["hook", "claude"], &env)
            .status
            .success()
    );
    assert!(skill.exists());

    assert!(
        ff_env(home.path(), &["hook", "claude", "--settings"], &env)
            .status
            .success()
    );
    assert!(!skill.exists(), "the plugin went, and the skill with it");
}

// ---- the codex plugin ------------------------------------------------------

/// The plugin fufu owns under `~/.agents/plugins/fufu/`: the legacy
/// manifest with a hash-suffixed version, the five events with the tool
/// matcher on `PreToolUse`, the skill inside, and one entry merged into
/// the personal marketplace beside it. Idempotent, stale when an extra
/// event is missing, refreshed by `-u`, and removed whole.
#[test]
fn the_codex_plugin_round_trips() {
    let home = tempfile::TempDir::new().unwrap();
    let env = [("HOME", home.path().to_str().unwrap())];
    let plugin = home.path().join(".agents").join("plugins").join("fufu");
    let marketplace = home
        .path()
        .join(".agents")
        .join("plugins")
        .join("marketplace.json");

    let out = ff_env(home.path(), &["hook", "codex"], &env);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let said = text(&out);
    assert!(said.contains("plugin written to"), "{said:?}");
    assert!(said.contains("marketplace entry written to"), "{said:?}");
    assert!(
        said.contains("Codex is not on PATH — run: codex plugin add fufu@fufu"),
        "{said:?}"
    );
    assert!(said.contains("Codex trusts a hook by its hash"), "{said:?}");

    // The legacy manifest, and no root one: Codex loads a plugin's hooks
    // from the legacy manifest alone, and a root `$schema` manifest beside
    // it would win and drop them.
    let manifest = json_at(&plugin.join(".codex-plugin").join("plugin.json"));
    assert!(!plugin.join("plugin.json").exists(), "no root manifest");
    assert!(manifest.get("$schema").is_none(), "{manifest}");
    assert_eq!(manifest["name"], "fufu");
    assert_eq!(manifest.as_object().unwrap().len(), 4, "{manifest}");
    let version = manifest["version"].as_str().unwrap();
    let (_, suffix) = version.split_once("+ff.").expect("a hash suffix");
    assert_eq!(suffix.len(), 8, "{version}");
    assert!(suffix.chars().all(|c| c.is_ascii_hexdigit()), "{version}");

    let hooks_path = plugin.join("hooks").join("hooks.json");
    let hooks = json_at(&hooks_path);
    assert_eq!(
        hooks["hooks"]
            .as_object()
            .unwrap()
            .keys()
            .collect::<Vec<_>>(),
        FAMILY_EVENTS.to_vec(),
        "the five events, in order: {hooks}"
    );
    for event in FAMILY_EVENTS {
        let entry = &hooks["hooks"][event][0];
        let command = entry["hooks"][0]["command"].as_str().unwrap();
        assert!(
            command.ends_with("\" trigger codex"),
            "{event}: {command:?}"
        );
        assert!(command.starts_with('"'), "quoted: {command:?}");
    }
    assert_eq!(
        hooks["hooks"]["PreToolUse"][0]["matcher"],
        "Bash|apply_patch"
    );
    assert!(hooks["hooks"]["Stop"][0].get("matcher").is_none());
    assert!(!plugin.join("mcp.json").exists(), "no server rides along");
    let skill = plugin.join("skills").join("fufu").join("SKILL.md");
    let on_disk = std::fs::read_to_string(&skill).expect("the skill lands with the plugin");
    assert!(
        on_disk.starts_with("---\nname: fufu\n"),
        "frontmatter first"
    );

    let market = json_at(&marketplace);
    assert_eq!(market["name"], "fufu");
    let plugins = market["plugins"].as_array().unwrap();
    assert_eq!(plugins.len(), 1);
    assert_eq!(plugins[0]["name"], "fufu");
    assert_eq!(plugins[0]["source"]["source"], "local");
    assert_eq!(plugins[0]["source"]["path"], "./.agents/plugins/fufu");
    assert_eq!(plugins[0]["policy"]["installation"], "INSTALLED_BY_DEFAULT");
    assert_eq!(plugins[0]["policy"]["authentication"], "ON_INSTALL");

    // Idempotent, and reported as already wired.
    let again = text(&ff_env(home.path(), &["hook", "codex"], &env));
    assert!(again.contains("already wired in"), "{again:?}");
    assert!(!again.contains("marketplace entry written"), "{again:?}");
    let value = envelope(&ff_env(home.path(), &["--json", "hook", "codex"], &env));
    assert_eq!(value["data"]["changed"], serde_json::json!([]));
    let listing = text(&ff_env(home.path(), &["hook", "-l"], &env));
    let row = listing.lines().find(|l| l.starts_with("codex")).unwrap();
    assert!(row.contains("wired (plugin)"), "{row:?}");
    assert!(row.contains(", skill"), "{row:?}");
    assert!(
        listing.contains("Codex trusts a hook by its hash"),
        "the trust step is on the row: {listing:?}"
    );

    // -u over a current plugin moves nothing.
    let said = text(&ff_env(home.path(), &["hook", "-u"], &env));
    assert!(said.contains("already wired in"), "{said:?}");

    // An extra event missing reads as stale, and -u restores the bytes.
    let current = std::fs::read_to_string(&hooks_path).unwrap();
    let mut fewer: serde_json::Value = serde_json::from_str(&current).unwrap();
    fewer["hooks"].as_object_mut().unwrap().remove("SessionEnd");
    std::fs::write(&hooks_path, serde_json::to_string_pretty(&fewer).unwrap()).unwrap();
    let value = envelope(&ff_env(home.path(), &["--json", "hook", "-l"], &env));
    let row = value["data"]["integrations"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["slug"] == "codex")
        .unwrap();
    assert_eq!(row["wiring"]["state"], "wired", "{row}");
    assert_eq!(row["stale"], true, "{row}");
    let said = text(&ff_env(home.path(), &["hook", "-u"], &env));
    assert!(said.contains("plugin written to"), "{said:?}");
    assert_eq!(std::fs::read_to_string(&hooks_path).unwrap(), current);

    let said = text(&ff_env(home.path(), &["unhook", "codex"], &env));
    assert!(said.contains("removed the fufu entry from"), "{said:?}");
    assert!(said.contains("codex plugin remove fufu@fufu"), "{said:?}");
    assert!(!plugin.exists(), "the directory fufu owns goes whole");
    assert!(!skill.exists(), "…and the skill inside it with it");
    let market = json_at(&marketplace);
    assert_eq!(market["name"], "fufu", "the file keeps its name");
    assert_eq!(market["plugins"], serde_json::json!([]));
    let said = text(&ff_env(home.path(), &["unhook", "codex"], &env));
    assert!(said.contains("no fufu plugin installed"), "{said:?}");
}

/// The marketplace is a file fufu does not own: a foreign plugin, the
/// file's own name, and its key order all survive hook and unhook, with
/// fufu's entry appended then removed — and the selector Codex is told
/// takes the file's name.
#[test]
fn a_foreign_marketplace_survives() {
    let home = tempfile::TempDir::new().unwrap();
    let env = [("HOME", home.path().to_str().unwrap())];
    let marketplace = home
        .path()
        .join(".agents")
        .join("plugins")
        .join("marketplace.json");
    std::fs::create_dir_all(marketplace.parent().unwrap()).unwrap();
    let seed = serde_json::json!({
        "plugins": [{
            "name": "theirs",
            "source": { "source": "local", "path": "./.agents/plugins/theirs" },
            "policy": { "installation": "AVAILABLE" }
        }],
        "name": "mine",
        "metadata": { "description": "my plugins" }
    });
    std::fs::write(&marketplace, serde_json::to_string_pretty(&seed).unwrap()).unwrap();

    let out = ff_env(home.path(), &["hook", "codex"], &env);
    assert!(out.status.success());
    assert!(
        text(&out).contains("codex plugin add fufu@mine"),
        "{:?}",
        text(&out)
    );
    let v = json_at(&marketplace);
    assert_eq!(v["name"], "mine");
    assert_eq!(v["metadata"]["description"], "my plugins");
    let keys: Vec<&String> = v.as_object().unwrap().keys().collect();
    assert_eq!(keys, vec!["plugins", "name", "metadata"], "{v}");
    let plugins = v["plugins"].as_array().unwrap();
    assert_eq!(plugins.len(), 2);
    assert_eq!(
        plugins[0], seed["plugins"][0],
        "the foreign entry, value for value"
    );
    assert_eq!(plugins[1]["name"], "fufu");

    assert!(
        ff_env(home.path(), &["unhook", "codex"], &env)
            .status
            .success()
    );
    let v = json_at(&marketplace);
    assert_eq!(v["name"], "mine");
    let keys: Vec<&String> = v.as_object().unwrap().keys().collect();
    assert_eq!(keys, vec!["plugins", "name", "metadata"], "{v}");
    assert_eq!(v["plugins"], seed["plugins"]);
}

/// A marketplace that will not parse is refused, the plugin already
/// written — since the plugin captures and the file is not fufu's to guess
/// at.
#[test]
fn a_malformed_marketplace_is_refused_untouched() {
    let home = tempfile::TempDir::new().unwrap();
    let env = [("HOME", home.path().to_str().unwrap())];
    let marketplace = home
        .path()
        .join(".agents")
        .join("plugins")
        .join("marketplace.json");
    std::fs::create_dir_all(marketplace.parent().unwrap()).unwrap();
    std::fs::write(&marketplace, "{ not json").unwrap();
    let out = ff_env(home.path(), &["--json", "hook", "codex"], &env);
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(envelope(&out)["error"]["id"], "hook/malformed");
    assert_eq!(std::fs::read_to_string(&marketplace).unwrap(), "{ not json");
}

/// Add-then-remove: once the plugin has verified, what an older fufu wrote
/// into Codex's settings file goes — fufu's entries and nothing beside
/// them — with the skill directory and the `config.toml` block beside it;
/// a foreign entry and a skill of the user's own under `~/.codex/skills`
/// stay.
#[test]
fn the_migration_strips_the_old_codex_wiring() {
    let home = tempfile::TempDir::new().unwrap();
    let env = [("HOME", home.path().to_str().unwrap())];
    let codex = home.path().join(".codex").join("hooks.json");
    std::fs::create_dir_all(codex.parent().unwrap()).unwrap();
    let mut events = serde_json::Map::new();
    for event in ["PreToolUse", "UserPromptSubmit"] {
        events.insert(
            event.into(),
            serde_json::json!([
                { "hooks": [{ "type": "command", "command": "my-linter" }] },
                { "hooks": [{ "type": "command", "command": "ff trigger codex" }] }
            ]),
        );
    }
    std::fs::write(
        &codex,
        serde_json::to_string_pretty(&serde_json::json!({ "hooks": events })).unwrap(),
    )
    .unwrap();
    let old_skill = home
        .path()
        .join(".codex")
        .join("skills")
        .join("fufu")
        .join("SKILL.md");
    std::fs::create_dir_all(old_skill.parent().unwrap()).unwrap();
    std::fs::write(&old_skill, "---\nname: fufu\ndescription: old\n---\n").unwrap();
    let theirs = home
        .path()
        .join(".codex")
        .join("skills")
        .join("theirs")
        .join("SKILL.md");
    std::fs::create_dir_all(theirs.parent().unwrap()).unwrap();
    std::fs::write(&theirs, "---\nname: theirs\n---\n").unwrap();
    let config = home.path().join(".codex").join("config.toml");
    let mine = "model = \"o3\"\n";
    std::fs::write(
        &config,
        format!(
            "{mine}# >>> fufu (ff hook codex) >>>\n[mcp_servers.fufu]\ncommand = \"/old/place/ff\"\nargs = [\"mcp\"]\n# <<< fufu <<<\n"
        ),
    )
    .unwrap();

    // Before the plugin, the settings entries are not wiring the plugin
    // adapter reports — they capture, and `ff hook codex` is the move.
    let listing = text(&ff_env(home.path(), &["hook", "-l"], &env));
    let row = listing.lines().find(|l| l.starts_with("codex")).unwrap();
    assert!(row.ends_with("not wired"), "{row:?}");

    let said = text(&ff_env(home.path(), &["hook", "codex"], &env));
    assert!(said.contains("moved off ~/.codex/hooks.json"), "{said:?}");
    assert!(said.contains("removed ~/.codex/skills/fufu"), "{said:?}");
    assert!(said.contains("MCP server removed from"), "{said:?}");

    let v = json_at(&codex);
    for event in ["PreToolUse", "UserPromptSubmit"] {
        let entries = v["hooks"][event].as_array().unwrap();
        assert_eq!(entries.len(), 1, "{event}: only the foreign one stays: {v}");
        assert_eq!(entries[0]["hooks"][0]["command"], "my-linter");
    }
    assert!(!old_skill.exists(), "the old skill directory goes");
    assert!(theirs.exists(), "a skill of the user's own stays");
    assert_eq!(std::fs::read_to_string(&config).unwrap(), mine);

    // The second run has nothing left to strip and says so by silence.
    let again = text(&ff_env(home.path(), &["hook", "codex"], &env));
    assert!(!again.contains("moved off"), "{again:?}");
    assert!(!again.contains("removed ~/.codex"), "{again:?}");
    assert!(!again.contains("MCP server"), "{again:?}");
}

// ---- the opencode plugin ---------------------------------------------------

/// The plugin module OpenCode loads from its config directory: one file
/// fufu owns whole, the binary's path baked in as a string literal, the
/// three hooks, and the skill beside it; idempotent, stale when a byte
/// differs under fufu's header, refreshed by `-u`, refused when the file
/// is someone else's, and removed whole with nothing else in either
/// directory touched.
#[test]
fn the_opencode_plugin_round_trips() {
    let home = tempfile::TempDir::new().unwrap();
    let xdg = home.path().join("xdg");
    let env = [
        ("HOME", home.path().to_str().unwrap()),
        ("XDG_CONFIG_HOME", xdg.to_str().unwrap()),
    ];
    let config = xdg.join("opencode");
    let plugin = config.join("plugins").join("fufu.js");
    let skill = config.join("skills").join("fufu").join("SKILL.md");

    let out = ff_env(home.path(), &["hook", "opencode"], &env);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let said = text(&out);
    assert!(said.contains("plugin written to"), "{said:?}");
    assert!(said.contains("skill written to"), "{said:?}");
    assert!(said.contains("the briefing is standing"), "{said:?}");
    assert!(said.contains("restart OpenCode to load it"), "{said:?}");

    let body = std::fs::read_to_string(&plugin).expect("the plugin file");
    assert!(
        body.starts_with("// Written by `ff hook opencode`."),
        "{body}"
    );
    let exe = serde_json::Value::String(env!("CARGO_BIN_EXE_ff").to_string()).to_string();
    assert!(
        body.contains(&format!("const FF = {exe};")),
        "the binary's path as a JS string literal: {body}"
    );
    for needle in [
        "trigger opencode",
        "OPENCODE_SESSION_ID",
        "\"shell.env\"",
        "\"experimental.chat.system.transform\"",
        "\"tool.execute.before\"",
        "export const FufuPlugin",
    ] {
        assert!(body.contains(needle), "{needle} in {body}");
    }
    assert!(!body.contains("__FF__"), "{body}");
    assert!(
        std::fs::read_to_string(&skill)
            .expect("the skill lands beside it")
            .starts_with("---\nname: fufu\n")
    );

    // Idempotent, and reported as already wired.
    let again = text(&ff_env(home.path(), &["hook", "opencode"], &env));
    assert!(again.contains("already wired in"), "{again:?}");
    assert!(!again.contains("restart OpenCode"), "{again:?}");
    let value = envelope(&ff_env(home.path(), &["--json", "hook", "opencode"], &env));
    assert_eq!(value["data"]["changed"], serde_json::json!([]));
    let listing = text(&ff_env(home.path(), &["hook", "-l"], &env));
    let row = listing.lines().find(|l| l.starts_with("opencode")).unwrap();
    assert!(row.contains("wired (plugin)"), "{row:?}");
    assert!(row.contains(", skill"), "{row:?}");
    assert!(
        listing.contains("the briefing is standing"),
        "the standing briefing is on the row: {listing:?}"
    );

    // -u over a current plugin moves nothing.
    let said = text(&ff_env(home.path(), &["hook", "-u"], &env));
    assert!(said.contains("already wired in"), "{said:?}");

    // A byte changed under fufu's header is stale, and -u restores it.
    std::fs::write(&plugin, format!("{body}\n// one more byte\n")).unwrap();
    let rows =
        envelope(&ff_env(home.path(), &["--json", "hook", "-l"], &env))["data"]["integrations"]
            .clone();
    assert_eq!(rows[3]["slug"], "opencode");
    assert_eq!(rows[3]["wiring"]["state"], "wired", "{rows}");
    assert_eq!(rows[3]["stale"], true, "{rows}");
    let said = text(&ff_env(home.path(), &["hook", "-u"], &env));
    assert!(said.contains("plugin written to"), "{said:?}");
    assert_eq!(std::fs::read_to_string(&plugin).unwrap(), body);

    // A fufu.js without the header is someone else's: reported, the
    // install refused, and unhook leaves it.
    let foreign = "export const Mine = async () => ({});\n";
    std::fs::write(&plugin, foreign).unwrap();
    let listing = text(&ff_env(home.path(), &["hook", "-l"], &env));
    let row = listing.lines().find(|l| l.starts_with("opencode")).unwrap();
    assert!(row.contains("hand-written"), "{row:?}");
    let out = ff_env(home.path(), &["--json", "hook", "opencode"], &env);
    assert_eq!(out.status.code(), Some(1));
    let value = envelope(&out);
    assert_eq!(value["error"]["id"], "hook/failed");
    assert!(
        value["error"]["message"]
            .as_str()
            .unwrap()
            .contains("not fufu's file"),
        "{value}"
    );
    assert_eq!(std::fs::read_to_string(&plugin).unwrap(), foreign);
    let said = text(&ff_env(home.path(), &["unhook", "opencode"], &env));
    assert!(said.contains("is not fufu's — left alone"), "{said:?}");
    assert_eq!(
        std::fs::read_to_string(&plugin).unwrap(),
        foreign,
        "unhook leaves what fufu did not write"
    );
    // The -u pass skips it: not wired, nothing to rewrite.
    let said = text(&ff_env(home.path(), &["hook", "-u"], &env));
    assert!(!said.contains("opencode"), "{said:?}");
    assert_eq!(std::fs::read_to_string(&plugin).unwrap(), foreign);
    assert!(
        !skill.exists(),
        "the skill is fufu's whatever sits under the plugin's name, and unhook took it"
    );

    // Back to fufu's — the plugin file restored, the skill rewritten by
    // the install — with a neighbor in each directory: unhook takes
    // exactly the two paths.
    std::fs::write(&plugin, &body).unwrap();
    let said = text(&ff_env(home.path(), &["hook", "opencode"], &env));
    assert!(said.contains("plugin written to"), "{said:?}");
    assert!(skill.is_file());
    let other = config.join("plugins").join("other.js");
    std::fs::write(&other, "export const Other = async () => ({});\n").unwrap();
    let mine = config.join("skills").join("mine").join("SKILL.md");
    std::fs::create_dir_all(mine.parent().unwrap()).unwrap();
    std::fs::write(&mine, "---\nname: mine\n---\n").unwrap();
    let said = text(&ff_env(home.path(), &["unhook", "opencode"], &env));
    assert!(
        said.contains(&format!("removed {}", plugin.display())),
        "{said:?}"
    );
    assert!(
        said.contains(&format!(
            "removed {}",
            config.join("skills").join("fufu").display()
        )),
        "{said:?}"
    );
    assert!(!plugin.exists());
    assert!(!config.join("skills").join("fufu").exists());
    assert!(other.is_file(), "a neighbor plugin stays");
    assert!(mine.is_file(), "a neighbor skill stays");
    let said = text(&ff_env(home.path(), &["unhook", "opencode"], &env));
    assert!(said.contains("no fufu plugin installed"), "{said:?}");
}

/// OpenCode is present when its config directory is, or when the binary
/// is — `FF_OPENCODE` names it, the seam the harness closes with a path
/// that is not a file.
#[test]
fn opencode_is_detected_by_its_directory_or_its_binary() {
    let home = tempfile::TempDir::new().unwrap();
    let xdg = home.path().join("xdg");
    let env = [
        ("HOME", home.path().to_str().unwrap()),
        ("XDG_CONFIG_HOME", xdg.to_str().unwrap()),
    ];
    let listing = text(&ff_env(home.path(), &["hook", "-l"], &env));
    let row = listing.lines().find(|l| l.starts_with("opencode")).unwrap();
    assert!(row.contains("not on this machine"), "{row:?}");

    let binary = home.path().join("bin").join("opencode");
    std::fs::create_dir_all(binary.parent().unwrap()).unwrap();
    std::fs::write(&binary, "#!/bin/sh\n").unwrap();
    let rows = envelope(&ff_env(
        home.path(),
        &["--json", "hook", "-l"],
        &[
            ("HOME", home.path().to_str().unwrap()),
            ("XDG_CONFIG_HOME", xdg.to_str().unwrap()),
            ("FF_OPENCODE", binary.to_str().unwrap()),
        ],
    ))["data"]["integrations"]
        .clone();
    assert_eq!(rows[3]["slug"], "opencode");
    assert_eq!(rows[3]["presence"]["state"], "present", "{rows}");
    assert_eq!(
        rows[3]["presence"]["evidence"],
        binary.to_str().unwrap(),
        "{rows}"
    );

    std::fs::create_dir_all(xdg.join("opencode")).unwrap();
    let rows =
        envelope(&ff_env(home.path(), &["--json", "hook", "-l"], &env))["data"]["integrations"]
            .clone();
    assert_eq!(rows[3]["presence"]["state"], "present", "{rows}");
    assert_eq!(
        rows[3]["presence"]["evidence"],
        xdg.join("opencode").to_str().unwrap(),
        "the directory is the evidence when it is there: {rows}"
    );
}

/// Doctor reads the OpenCode plugin the way it reads the others: an ok
/// row naming the file, a fixable warning when a byte drifted under
/// fufu's header, and an info row for a file that is not fufu's.
#[test]
fn doctor_reads_the_opencode_plugin() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    let home = tempfile::TempDir::new().unwrap();
    let xdg = home.path().join("xdg");
    let env = [
        ("HOME", home.path().to_str().unwrap()),
        ("XDG_CONFIG_HOME", xdg.to_str().unwrap()),
        ("XDG_CACHE_HOME", home.path().to_str().unwrap()),
    ];
    let plugin = xdg.join("opencode").join("plugins").join("fufu.js");
    std::fs::create_dir_all(xdg.join("opencode")).unwrap();
    let row = || -> serde_json::Value {
        let out = ff_env(&fx.path(), &["doctor", "--json"], &env);
        envelope(&out)["data"]["checks"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["name"] == "opencode")
            .cloned()
            .unwrap_or_else(|| panic!("an opencode row"))
    };
    let found = row();
    assert_eq!(found["level"], "info", "{found}");
    assert!(
        found["detail"]
            .as_str()
            .unwrap()
            .contains("ff hook opencode"),
        "{found}"
    );
    assert!(
        ff_env(home.path(), &["hook", "opencode"], &env)
            .status
            .success()
    );
    let found = row();
    assert_eq!(found["level"], "ok", "{found}");
    assert!(
        found["detail"]
            .as_str()
            .unwrap()
            .starts_with(&format!("plugin wired in {}", plugin.display())),
        "{found}"
    );

    let body = std::fs::read_to_string(&plugin).unwrap();
    std::fs::write(&plugin, format!("{body}// drift\n")).unwrap();
    let found = row();
    assert_eq!(found["level"], "warn", "{found}");
    assert!(
        found["detail"]
            .as_str()
            .unwrap()
            .contains("`ff hook opencode` repairs"),
        "{found}"
    );

    std::fs::write(&plugin, "export const Mine = async () => ({});\n").unwrap();
    let found = row();
    assert_eq!(found["level"], "info", "{found}");
    assert_eq!(
        found["detail"],
        format!("{} is not fufu's — left alone", plugin.display()),
        "{found}"
    );
}

// ---- the copilot plugin ----------------------------------------------------

/// Copilot's row from the JSON listing.
fn copilot_status(home: &Path, env: &[(&str, &str)]) -> serde_json::Value {
    let out = ff_env(home, &["--json", "hook", "-l"], env);
    assert!(out.status.success());
    envelope(&out)["data"]["integrations"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["slug"] == "copilot")
        .unwrap()
        .clone()
}

const COPILOT_EVENTS: [&str; 5] = [
    "preToolUse",
    "userPromptSubmitted",
    "sessionStart",
    "agentStop",
    "sessionEnd",
];

/// The Agent Plugins 1.0 plugin under `~/.agents/plugins/copilot/fufu/`,
/// its marketplace beside it, and the registration merged into Copilot's
/// settings: written beside Codex's plugin without touching it, foreign
/// settings kept value for value, idempotent without rewriting the
/// settings file, and removed whole.
#[test]
fn the_copilot_plugin_round_trips_beside_codex_and_foreign_settings() {
    let home = tempfile::TempDir::new().unwrap();
    let home = home.path();
    let env = [("HOME", home.to_str().unwrap())];
    let root = home.join(".agents").join("plugins").join("copilot");
    let dir = root.join("fufu");
    let settings = home.join(".copilot").join("settings.json");
    std::fs::create_dir_all(settings.parent().unwrap()).unwrap();
    let foreign = serde_json::json!({
        "theme": "dark", "enabledPlugins": {"other@mine": true},
        "extraKnownMarketplaces": {"mine": {"source": {"source": "directory", "path": "/mine"}}}
    });
    std::fs::write(&settings, foreign.to_string()).unwrap();
    assert!(ff_env(home, &["hook", "codex"], &env).status.success());
    let codex_market = home
        .join(".agents")
        .join("plugins")
        .join("marketplace.json");
    let codex_manifest = home
        .join(".agents")
        .join("plugins")
        .join("fufu")
        .join(".codex-plugin")
        .join("plugin.json");
    let codex_before = (
        std::fs::read(&codex_market).unwrap(),
        std::fs::read(&codex_manifest).unwrap(),
    );

    let out = ff_env(home, &["--json", "hook", "copilot"], &env);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        envelope(&out)["data"]["changed"],
        serde_json::json!(["copilot"])
    );
    let status = copilot_status(home, &env);
    assert_eq!(status["wiring"]["state"], "wired", "{status}");
    assert_eq!(status["wiring"]["mechanism"], "plugin");
    assert_eq!(status["skill"]["state"], "wired");
    assert!(status.get("note").is_none(), "no trust step: {status}");
    let manifest = json_at(&dir.join("plugin.json"));
    assert_eq!(
        manifest["$schema"],
        "https://agent-plugins.org/schemas/1.0.0/plugin.schema.json"
    );
    assert_eq!(manifest["name"], "fufu");
    let version = manifest["version"].as_str().unwrap();
    let (_, hash) = version.split_once("+ff.").unwrap();
    assert_eq!(hash.len(), 8);
    assert!(hash.chars().all(|c| c.is_ascii_hexdigit()));
    assert!(!dir.join(".codex-plugin").exists());
    assert!(
        std::fs::read_to_string(dir.join("skills").join("fufu").join("SKILL.md"))
            .unwrap()
            .starts_with("---\nname: fufu\n")
    );
    let hooks = json_at(
        &dir.join("com.github.copilot")
            .join("hooks")
            .join("hooks.json"),
    );
    assert_eq!(hooks["version"], 1);
    assert_eq!(hooks["hooks"].as_object().unwrap().len(), 5);
    for event in COPILOT_EVENTS {
        let entries = hooks["hooks"][event].as_array().unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0]["type"], "command");
        let bash = entries[0]["bash"].as_str().unwrap();
        assert!(bash.ends_with("\" trigger copilot"), "{bash:?}");
        assert!(bash.starts_with('"'), "quoted: {bash:?}");
        assert_eq!(entries[0]["env"]["FF_HOOK_EVENT"], event);
        assert_eq!(entries[0]["timeoutSec"], 30);
        assert!(entries[0].get("hooks").is_none());
    }
    let market = json_at(&root.join("marketplace.json"));
    assert_eq!(market["name"], "fufu-ff");
    assert_eq!(market["owner"]["name"], "fufu");
    assert_eq!(market["plugins"][0]["name"], "fufu");
    assert_eq!(market["plugins"][0]["source"], "./fufu");
    let registration = json_at(&settings);
    assert_eq!(registration["theme"], "dark");
    assert_eq!(registration["enabledPlugins"]["other@mine"], true);
    assert_eq!(registration["enabledPlugins"]["fufu@fufu-ff"], true);
    assert_eq!(
        registration["extraKnownMarketplaces"]["fufu-ff"]["source"]["source"],
        "directory"
    );
    assert_eq!(
        registration["extraKnownMarketplaces"]["fufu-ff"]["source"]["path"],
        root.display().to_string()
    );
    assert_eq!(
        registration["extraKnownMarketplaces"]["mine"]["source"]["path"],
        "/mine"
    );

    // Idempotent, and the settings file is not rewritten for nothing.
    let before = std::fs::metadata(&settings).unwrap().modified().unwrap();
    let out = ff_env(home, &["--json", "hook", "copilot"], &env);
    assert!(out.status.success());
    assert_eq!(envelope(&out)["data"]["changed"], serde_json::json!([]));
    assert_eq!(
        std::fs::metadata(&settings).unwrap().modified().unwrap(),
        before
    );
    let said = text(&ff_env(home, &["hook", "copilot"], &env));
    assert!(
        said.contains("already wired in") && said.contains("registered live as fufu@fufu-ff"),
        "{said:?}"
    );

    assert!(ff_env(home, &["unhook", "copilot"], &env).status.success());
    assert!(!dir.exists());
    assert!(!root.join("marketplace.json").exists());
    assert_eq!(json_at(&settings), foreign);
    assert_eq!(std::fs::read(&codex_market).unwrap(), codex_before.0);
    assert_eq!(std::fs::read(&codex_manifest).unwrap(), codex_before.1);
    assert_eq!(copilot_status(home, &env)["wiring"]["state"], "not-wired");
    let out = ff_env(home, &["--json", "unhook", "copilot"], &env);
    assert!(out.status.success());
    assert_eq!(envelope(&out)["data"]["changed"], serde_json::json!([]));
}

/// The marketplace root is shared with tower. A file tower created keeps
/// its name and its entry; fufu's entry joins the list, the selector
/// follows the file's name, and unhook takes fufu's entry and its
/// `enabledPlugins` key while the file and the marketplace registration
/// stay for tower.
#[test]
fn copilot_shares_a_marketplace_tower_created() {
    let home = tempfile::TempDir::new().unwrap();
    let home = home.path();
    let env = [("HOME", home.to_str().unwrap())];
    let root = home.join(".agents").join("plugins").join("copilot");
    let market = root.join("marketplace.json");
    std::fs::create_dir_all(&root).unwrap();
    let seed = serde_json::json!({
        "name": "tower-atc", "owner": { "name": "tower" },
        "plugins": [{ "name": "tower", "source": "./tower" }]
    });
    std::fs::write(&market, serde_json::to_string_pretty(&seed).unwrap()).unwrap();
    let settings = home.join(".copilot").join("settings.json");
    std::fs::create_dir_all(settings.parent().unwrap()).unwrap();
    std::fs::write(
        &settings,
        serde_json::json!({
            "enabledPlugins": { "tower@tower-atc": true },
            "extraKnownMarketplaces": { "tower-atc": { "source": { "source": "directory", "path": root } } }
        })
        .to_string(),
    )
    .unwrap();

    let said = text(&ff_env(home, &["hook", "copilot"], &env));
    assert!(
        said.contains("registered live as fufu@tower-atc"),
        "{said:?}"
    );
    let v = json_at(&market);
    assert_eq!(v["name"], "tower-atc");
    assert_eq!(v["owner"]["name"], "tower");
    let plugins = v["plugins"].as_array().unwrap();
    assert_eq!(plugins.len(), 2);
    assert_eq!(plugins[0], seed["plugins"][0]);
    assert_eq!(plugins[1]["source"], "./fufu");
    let registration = json_at(&settings);
    assert_eq!(registration["enabledPlugins"]["tower@tower-atc"], true);
    assert_eq!(registration["enabledPlugins"]["fufu@tower-atc"], true);
    assert_eq!(copilot_status(home, &env)["wiring"]["state"], "wired");

    // tower rewriting the file whole reads as partial, and -u repairs.
    std::fs::write(&market, serde_json::to_string_pretty(&seed).unwrap()).unwrap();
    let status = copilot_status(home, &env);
    assert_eq!(status["wiring"]["state"], "partial", "{status}");
    assert_eq!(status["wiring"]["missing"], "marketplace entry", "{status}");
    assert!(ff_env(home, &["hook", "-u"], &env).status.success());
    assert_eq!(copilot_status(home, &env)["wiring"]["state"], "wired");

    let said = text(&ff_env(home, &["unhook", "copilot"], &env));
    assert!(said.contains("registration fufu@tower-atc"), "{said:?}");
    let v = json_at(&market);
    assert_eq!(v["plugins"], seed["plugins"], "tower's entry stays");
    assert_eq!(v["name"], "tower-atc");
    let registration = json_at(&settings);
    assert_eq!(registration["enabledPlugins"]["tower@tower-atc"], true);
    assert!(
        registration["enabledPlugins"]
            .get("fufu@tower-atc")
            .is_none(),
        "{registration}"
    );
    assert!(
        registration["extraKnownMarketplaces"]["tower-atc"].is_object(),
        "the marketplace registration is tower's too: {registration}"
    );
    assert!(!root.join("fufu").exists());
}

/// Copilot's settings file is refused whole when it is not the object
/// Copilot reads, before anything is written or removed.
#[test]
fn copilot_refuses_malformed_settings_before_writing_or_removing_files() {
    for bad in [
        "{ broken",
        "[]",
        r#"{"enabledPlugins":false}"#,
        r#"{"extraKnownMarketplaces":[]}"#,
    ] {
        let home = tempfile::TempDir::new().unwrap();
        let home = home.path();
        let env = [("HOME", home.to_str().unwrap())];
        let settings = home.join(".copilot").join("settings.json");
        std::fs::create_dir_all(settings.parent().unwrap()).unwrap();
        std::fs::write(&settings, bad).unwrap();
        let out = ff_env(home, &["--json", "hook", "copilot"], &env);
        assert_eq!(out.status.code(), Some(1), "{bad}");
        assert_eq!(envelope(&out)["error"]["id"], "hook/malformed", "{bad}");
        assert_eq!(std::fs::read_to_string(&settings).unwrap(), bad);
        assert!(
            !home
                .join(".agents")
                .join("plugins")
                .join("copilot")
                .exists()
        );

        std::fs::write(&settings, "{}").unwrap();
        assert!(ff_env(home, &["hook", "copilot"], &env).status.success());
        std::fs::write(&settings, bad).unwrap();
        assert_eq!(
            copilot_status(home, &env)["wiring"]["state"],
            "unavailable",
            "{bad}"
        );
        let out = ff_env(home, &["--json", "unhook", "copilot"], &env);
        assert_eq!(envelope(&out)["error"]["id"], "hook/malformed", "{bad}");
        assert_eq!(std::fs::read_to_string(&settings).unwrap(), bad);
        assert!(
            home.join(".agents")
                .join("plugins")
                .join("copilot")
                .join("fufu")
                .join("plugin.json")
                .exists()
        );
    }
}

/// Every piece install writes is one `-u` puts back: a missing extra
/// event is stale, and a wrong event environment, a missing manifest, a
/// missing marketplace, a registration set false, or a drifted skill is a
/// finding doctor names and the refresh repairs.
#[test]
fn copilot_repairs_missing_events_manifest_registration_and_skill() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    let home = tempfile::TempDir::new().unwrap();
    let home = home.path();
    let env = [
        ("HOME", home.to_str().unwrap()),
        ("XDG_CACHE_HOME", home.to_str().unwrap()),
    ];
    let root = home.join(".agents").join("plugins").join("copilot");
    let dir = root.join("fufu");
    assert!(ff_env(home, &["hook", "copilot"], &env).status.success());
    let path = dir
        .join("com.github.copilot")
        .join("hooks")
        .join("hooks.json");
    let hooks = json_at(&path);
    // Every level doctor gives a row named for the slug: the wiring row,
    // and the skill row when the skill has drifted, which is named for
    // the slug whose installer repairs it.
    let doctor = || -> Vec<String> {
        envelope(&ff_env(&fx.path(), &["doctor", "--json"], &env))["data"]["checks"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|r| r["name"] == "copilot")
            .map(|r| r["level"].as_str().unwrap().to_string())
            .collect()
    };
    assert_eq!(doctor(), vec!["ok"]);
    let mut old = hooks.clone();
    old["hooks"].as_object_mut().unwrap().remove("agentStop");
    std::fs::write(&path, old.to_string()).unwrap();
    let status = copilot_status(home, &env);
    assert_eq!(status["wiring"]["state"], "wired");
    assert_eq!(status["stale"], true);
    assert_eq!(doctor(), vec!["warn"]);
    assert!(ff_env(home, &["hook", "-u"], &env).status.success());
    assert_eq!(json_at(&path), hooks);

    for damage in ["event", "manifest", "marketplace", "registration", "skill"] {
        match damage {
            "event" => {
                let mut wrong = hooks.clone();
                wrong["hooks"]["preToolUse"][0]["env"]["FF_HOOK_EVENT"] = "agentStop".into();
                std::fs::write(&path, wrong.to_string()).unwrap();
            }
            "manifest" => std::fs::remove_file(dir.join("plugin.json")).unwrap(),
            "marketplace" => std::fs::remove_file(root.join("marketplace.json")).unwrap(),
            "registration" => std::fs::write(
                home.join(".copilot").join("settings.json"),
                r#"{"enabledPlugins":{"fufu@fufu-ff":false}}"#,
            )
            .unwrap(),
            "skill" => {
                std::fs::write(dir.join("skills").join("fufu").join("SKILL.md"), "old").unwrap()
            }
            _ => unreachable!(),
        }
        let status = copilot_status(home, &env);
        if damage != "skill" {
            assert_eq!(status["wiring"]["state"], "partial", "{damage}: {status}");
        } else {
            assert_eq!(status["skill"]["state"], "partial", "{damage}: {status}");
        }
        assert!(doctor().iter().any(|l| l == "warn"), "{damage}");
        assert!(ff_env(home, &["hook", "-u"], &env).status.success());
        assert_eq!(doctor(), vec!["ok"], "{damage}");
    }
}

#[test]
fn copilot_is_detected_by_directory_or_binary() {
    let home = tempfile::TempDir::new().unwrap();
    let home = home.path();
    let env = [("HOME", home.to_str().unwrap())];
    assert_eq!(copilot_status(home, &env)["presence"]["state"], "absent");
    let binary = home.join("copilot");
    std::fs::write(&binary, "fake").unwrap();
    let row = copilot_status(
        home,
        &[
            ("HOME", home.to_str().unwrap()),
            ("FF_COPILOT", binary.to_str().unwrap()),
        ],
    );
    assert_eq!(row["presence"]["state"], "present");
    assert_eq!(row["presence"]["evidence"], binary.to_str().unwrap());
    std::fs::create_dir(home.join(".copilot")).unwrap();
    assert_eq!(copilot_status(home, &env)["presence"]["state"], "present");
}

// ---- gemini and qwen -------------------------------------------------------

/// Gemini CLI's adapter went: `ff hook gemini` and `ff unhook gemini` are
/// the unknown-slug refusal, `--all` never touches `~/.gemini`, and the
/// listing has no row for it. The stored trigger spelling keeps capturing
/// — `hook.rs` proves that half.
#[test]
fn gemini_is_an_unknown_slug_and_its_file_is_left() {
    let home = tempfile::TempDir::new().unwrap();
    let env = [("HOME", home.path().to_str().unwrap())];
    let gemini = home.path().join(".gemini").join("settings.json");
    std::fs::create_dir_all(gemini.parent().unwrap()).unwrap();
    let seed = r#"{"hooks":{"SessionStart":[{"hooks":[{"type":"command","command":"ff trigger gemini"}]}]}}"#;
    std::fs::write(&gemini, seed).unwrap();

    for verb in ["hook", "unhook"] {
        let out = ff_env(home.path(), &["--json", verb, "gemini"], &env);
        assert_eq!(out.status.code(), Some(2), "{verb} gemini");
        let value = envelope(&out);
        assert_eq!(value["error"]["id"], "usage/unknown-slug");
        let message = value["error"]["message"].as_str().unwrap();
        assert!(message.contains("unknown slug"), "{message}");
        assert!(message.contains("qwen"), "names the known: {message}");
    }
    std::fs::create_dir_all(home.path().join(".codex")).unwrap();
    assert!(
        ff_env(home.path(), &["hook", "--all"], &env)
            .status
            .success()
    );
    assert_eq!(std::fs::read_to_string(&gemini).unwrap(), seed);
    let listing = text(&ff_env(home.path(), &["hook", "-l"], &env));
    assert!(!listing.contains("gemini"), "{listing:?}");
}

/// Qwen Code takes the family's five events in its own settings file, the
/// tool matcher on `PreToolUse`, no skills directory.
#[test]
fn qwen_is_wired_in_its_settings_file() {
    let home = tempfile::TempDir::new().unwrap();
    let env = [("HOME", home.path().to_str().unwrap())];
    assert!(
        ff_env(home.path(), &["hook", "qwen"], &env)
            .status
            .success()
    );
    let settings = home.path().join(".qwen").join("settings.json");
    let qwen = json_at(&settings);
    assert_eq!(
        qwen["hooks"]
            .as_object()
            .unwrap()
            .keys()
            .collect::<Vec<_>>(),
        FAMILY_EVENTS.to_vec(),
        "Qwen names the family's five: {qwen}"
    );
    for event in FAMILY_EVENTS {
        let entry = &qwen["hooks"][event][0];
        assert_eq!(entry["hooks"][0]["command"], "ff trigger qwen", "{event}");
        assert_eq!(entry["hooks"][0]["type"], "command");
    }
    assert_eq!(
        qwen["hooks"]["PreToolUse"][0]["matcher"],
        "run_shell_command|write_file|replace|edit"
    );
    assert!(qwen["hooks"]["Stop"][0].get("matcher").is_none());
    assert!(!home.path().join(".qwen").join("skills").exists());
    let listing = text(&ff_env(home.path(), &["hook", "-l"], &env));
    let row = listing.lines().find(|l| l.starts_with("qwen")).unwrap();
    assert!(row.contains("wired (settings)"), "{row:?}");
    assert!(!row.contains(", skill"), "{row:?}");

    assert!(
        ff_env(home.path(), &["unhook", "qwen"], &env)
            .status
            .success()
    );
    let v = json_at(&settings);
    assert!(v.get("hooks").is_none(), "the entries went: {v}");
}

/// The print route. Cursor and Qwen read no skills directory, and a
/// client fufu has never heard of reads nothing fufu knows about — so the
/// manual has to be reachable without an install. What it prints is the
/// same bytes an install writes, which is what makes redirecting it into a
/// foreign client honest.
#[test]
fn the_skill_prints_and_writes_nothing() {
    let home = tempfile::TempDir::new().unwrap();
    let env = [("HOME", home.path().to_str().unwrap())];

    // Outside a repository: `hook` takes no capture lane, so this is the
    // one place that claim is asserted rather than assumed.
    let out = ff_env(home.path(), &["hook", "--skill"], &env);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let printed = text(&out);
    assert!(
        printed.starts_with("---\nname: fufu\n"),
        "frontmatter survives the redirect: {:?}",
        &printed[..printed.len().min(40)]
    );

    // A print is a print: nothing on this machine changed.
    assert!(!home.path().join(".claude").exists());
    assert!(!home.path().join(".codex").exists());

    // …and it is byte-for-byte what an install writes.
    assert!(
        ff_env(home.path(), &["hook", "claude"], &env)
            .status
            .success()
    );
    let installed =
        std::fs::read_to_string(home.path().join(".claude/skills/fufu/skills/fufu/SKILL.md"))
            .unwrap();
    assert_eq!(printed, installed, "printed and installed are one text");
}

/// Anything fufu tells a person, a script reads as data.
#[test]
fn the_printed_skill_has_a_json_form() {
    let home = tempfile::TempDir::new().unwrap();
    let env = [("HOME", home.path().to_str().unwrap())];

    let out = ff_env(home.path(), &["--json", "hook", "--skill"], &env);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let v: serde_json::Value = serde_json::from_str(text(&out).trim()).unwrap();
    assert_eq!(v["ff"], 1);
    assert_eq!(v["cmd"], "hook");
    let skill = v["data"]["skill"].as_str().expect("the skill is a string");
    assert!(skill.starts_with("---\nname: fufu\n"), "{skill:?}");
}

/// Every one of these is a question with no answer — print, or act? clap
/// refusing them beats picking one.
#[test]
fn printing_the_skill_conflicts_with_acting() {
    let home = tempfile::TempDir::new().unwrap();
    let env = [("HOME", home.path().to_str().unwrap())];

    for args in [
        &["hook", "--skill", "--all"][..],
        &["hook", "--skill", "-l"][..],
        &["hook", "claude", "--skill"][..],
    ] {
        let out = ff_env(home.path(), args, &env);
        assert_eq!(out.status.code(), Some(2), "{args:?} is a usage error");
    }
    assert!(!home.path().join(".claude").exists());
}

/// A skill an older fufu wrote still reads, so it is a repair rather than
/// a hole: doctor names it, `--fix` rewrites it, and capture never enters
/// the question.
#[test]
fn a_drifted_skill_is_a_finding_doctor_fixes() {
    let home = tempfile::TempDir::new().unwrap();
    let env = [("HOME", home.path().to_str().unwrap())];
    let skill = home.path().join(".claude/skills/fufu/skills/fufu/SKILL.md");

    assert!(
        ff_env(home.path(), &["hook", "claude"], &env)
            .status
            .success()
    );
    let shipped = std::fs::read_to_string(&skill).unwrap();
    std::fs::write(&skill, "---\nname: fufu\ndescription: an older fufu\n---\n").unwrap();

    let fx = Fixture::new();
    let report = text(&ff_env(&fx.path(), &["doctor"], &env));
    assert!(report.contains("older fufu wrote the skill"), "{report:?}");
    assert!(report.contains("--fix"), "offers the repair: {report:?}");

    let report = text(&ff_env(&fx.path(), &["doctor", "--fix"], &env));
    assert!(report.contains("rewired"), "{report:?}");
    assert_eq!(
        std::fs::read_to_string(&skill).unwrap(),
        shipped,
        "the fix rewrote it to what this binary ships"
    );
}

/// The migration is add-then-remove: the plugin lands and verifies before
/// the settings entries go, so there is never a moment with no wiring.
/// The opposite order would leave a window with no capture at all.
#[test]
fn the_settings_to_plugin_migration_never_leaves_nothing_wired() {
    let home = tempfile::TempDir::new().unwrap();
    let env = [("HOME", home.path().to_str().unwrap())];
    let settings = home.path().join(".claude/settings.json");
    let plugin = home.path().join(".claude/skills/fufu");

    // The state every existing install is in: the current stored spelling,
    // in settings entries, beside foreign content.
    std::fs::create_dir_all(settings.parent().unwrap()).unwrap();
    std::fs::write(
        &settings,
        serde_json::to_string_pretty(&serde_json::json!({
            "model": "opus",
            "hooks": {
                "PreToolUse": [{
                    "matcher": "Bash|Edit|Write|NotebookEdit",
                    "hooks": [{ "type": "command", "command": "ff hook agent trigger claude" }]
                }],
                "UserPromptSubmit": [{
                    "hooks": [{ "type": "command", "command": "ff hook agent trigger claude" }]
                }]
            }
        }))
        .unwrap(),
    )
    .unwrap();

    // Before: wired through settings, and reported as the older mechanism.
    let listing = text(&ff_env(home.path(), &["hook", "-l"], &env));
    assert!(listing.contains("wired (settings)"), "{listing:?}");

    assert!(
        ff_env(home.path(), &["hook", "claude"], &env)
            .status
            .success()
    );

    // After: the plugin is there AND the settings entries are gone. Both
    // halves matter — the first is the new wiring, the second is why there
    // is not now double capture forever.
    assert!(plugin.join("hooks/hooks.json").exists());
    let v = json_at(&settings);
    assert_eq!(v["model"], "opus", "foreign content survived the migration");
    assert!(v.get("hooks").is_none(), "settings entries stripped: {v}");
    let listing = text(&ff_env(home.path(), &["hook", "-l"], &env));
    assert!(listing.contains("wired (plugin)"), "{listing:?}");
}

/// The escape hatch: `--settings` wires the entries and removes the plugin,
/// which is the migration run backwards.
#[test]
fn the_settings_escape_hatch_goes_back() {
    let home = tempfile::TempDir::new().unwrap();
    let env = [("HOME", home.path().to_str().unwrap())];
    let plugin = home.path().join(".claude/skills/fufu");

    assert!(
        ff_env(home.path(), &["hook", "claude"], &env)
            .status
            .success()
    );
    assert!(plugin.exists());

    assert!(
        ff_env(home.path(), &["hook", "claude", "--settings"], &env)
            .status
            .success()
    );
    assert!(!plugin.exists(), "the plugin went");
    let v = json_at(&home.path().join(".claude/settings.json"));
    assert_eq!(
        v["hooks"]["PreToolUse"][0]["hooks"][0]["command"],
        "ff trigger claude"
    );
}

/// `ff unhook claude` takes back whatever install put there, whichever
/// mechanism it used — including a settings file left behind by a fufu old
/// enough to predate the plugin.
#[test]
fn unhook_claude_removes_both_mechanisms() {
    let home = tempfile::TempDir::new().unwrap();
    let env = [("HOME", home.path().to_str().unwrap())];
    let settings = home.path().join(".claude/settings.json");

    assert!(
        ff_env(home.path(), &["hook", "claude", "--settings"], &env)
            .status
            .success()
    );
    // Plant a plugin beside it, as the add-then-remove window would.
    assert!(
        ff_env(home.path(), &["hook", "claude"], &env)
            .status
            .success()
    );
    std::fs::write(
        &settings,
        r#"{"hooks":{"PreToolUse":[{"matcher":"Bash","hooks":[{"type":"command","command":"ff trigger claude"}]}]}}"#,
    )
    .unwrap();

    assert!(
        ff_env(home.path(), &["unhook", "claude"], &env)
            .status
            .success()
    );
    assert!(!home.path().join(".claude/skills/fufu").exists());
    let v = json_at(&settings);
    assert!(v.get("hooks").is_none(), "both mechanisms cleared: {v}");
}

// ---- legacy stored strings -------------------------------------------------

/// A settings file carrying a retired spelling is upgraded in place by
/// install — never duplicated, never orphaned.
#[test]
fn a_legacy_settings_command_is_upgraded_not_duplicated() {
    let home = tempfile::TempDir::new().unwrap();
    let env = [("HOME", home.path().to_str().unwrap())];
    let settings = home.path().join(".claude/settings.json");
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
    let write_legacy =
        || std::fs::write(&settings, serde_json::to_string_pretty(&legacy).unwrap()).unwrap();

    write_legacy();
    assert!(
        ff_env(home.path(), &["hook", "claude", "--settings"], &env)
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
        v["hooks"]["PreToolUse"][0]["hooks"][0]["command"], "ff trigger claude",
        "legacy command upgraded in place"
    );
    assert!(!text.contains("\"ff hook claude\""), "old spelling gone");

    // And uninstall recognizes the old spelling too.
    write_legacy();
    assert!(
        ff_env(home.path(), &["unhook", "claude"], &env)
            .status
            .success()
    );
    let v = json_at(&settings);
    assert!(v.get("hooks").is_none(), "legacy entries removed: {v}");
}

/// The Phase 1 shell marker is still recognized, so a line fufu wrote under
/// a retired spelling stays fufu-managed rather than becoming a line nobody
/// will ever remove.
#[test]
fn a_legacy_shell_marker_is_still_managed() {
    let home = tempfile::TempDir::new().unwrap();
    let rc = home.path().join(".bashrc");
    std::fs::write(
        &rc,
        "export FOO=bar\nalias git='ff git'  # fufu — added by `ff shell install`\n",
    )
    .unwrap();
    let env = [("HOME", home.path().to_str().unwrap())];

    let listing = text(&ff_env(home.path(), &["hook", "-l"], &env));
    assert!(listing.contains("alias wired"), "{listing:?}");

    assert!(
        ff_env(home.path(), &["unhook", "bash"], &env)
            .status
            .success()
    );
    assert_eq!(std::fs::read_to_string(&rc).unwrap(), "export FOO=bar\n");
}

/// The repair for a stored string nothing else will ever rewrite: doctor is
/// the command people run when they are already suspicious.
#[test]
fn doctor_fix_repairs_outdated_wiring() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    let home = tempfile::TempDir::new().unwrap();
    let env = [
        ("HOME", home.path().to_str().unwrap()),
        ("XDG_CACHE_HOME", home.path().to_str().unwrap()),
    ];
    let rc = home.path().join(".bashrc");
    std::fs::write(
        &rc,
        "alias git='ff git'  # fufu — added by `ff hook shell install`\n\
         ff hook shell trigger  # fufu — added by `ff hook shell install`\n",
    )
    .unwrap();

    // It still captures, so it is a finding and never an outage.
    let out = ff_env(&fx.path(), &["doctor"], &env);
    let report = text(&out);
    assert!(report.contains("WARN  bash"), "{report:?}");
    assert!(report.contains("retired spelling"), "{report:?}");
    assert!(report.contains("--fix"), "offers the repair: {report:?}");

    let report = text(&ff_env(&fx.path(), &["doctor", "--fix"], &env));
    assert!(report.contains("rewired"), "{report:?}");
    let contents = std::fs::read_to_string(&rc).unwrap();
    assert!(contents.contains("ff trigger shell"), "{contents:?}");

    // And now doctor is quiet about it.
    let report = text(&ff_env(&fx.path(), &["doctor"], &env));
    assert!(!report.contains("retired spelling"), "{report:?}");
}

// ---- the prompt hook runtime -----------------------------------------------

fn fixture_with_a_verdict() -> Fixture {
    let fx = Fixture::new();
    fx.write("f1.txt", "f1\n");
    fx.commit("add f1");
    fx
}

#[test]
fn the_shell_trigger_outside_a_repo_exits_zero_and_says_nothing() {
    let dir = tempfile::TempDir::new().unwrap();
    let env: [(&str, &str); 0] = [];
    let out = ff_env(dir.path(), &["trigger", "shell"], &env);
    assert!(out.status.success());
    assert!(out.stdout.is_empty(), "{:?}", text(&out));
    assert!(out.stderr.is_empty(), "{:?}", out.stderr);
}

/// The prompt hook is a snapshot, not a channel: it lands an operation and
/// prints nothing on either stream. Silence is the contract — a line above
/// the prompt is noise where the snapshot is the whole point.
#[test]
fn the_shell_trigger_captures_and_says_nothing() {
    let fx = fixture_with_a_verdict();
    fx.write("f1.txt", "moved\n");
    let env: [(&str, &str); 0] = [];
    let out = ff_env(&fx.path(), &["trigger", "shell"], &env);
    assert!(out.status.success());
    assert!(out.stdout.is_empty(), "{:?}", text(&out));
    assert!(out.stderr.is_empty(), "{:?}", out.stderr);

    let log = text(&ff_env(&fx.path(), &["op", "log"], &env));
    assert!(log.contains("shell"), "the prompt hook's operation:\n{log}");
}

/// Leaning on Enter writes nothing: `ff_core::capture` answers `NoOp` when
/// the tree has not moved, so a snapshot at every prompt costs an unchanged
/// log. And nothing writes the retired fingerprint file any more.
#[test]
fn a_second_shell_trigger_on_an_unmoved_tree_adds_no_operation() {
    let fx = fixture_with_a_verdict();
    fx.write("f1.txt", "moved\n");
    let env: [(&str, &str); 0] = [];

    assert!(
        ff_env(&fx.path(), &["trigger", "shell"], &env)
            .status
            .success()
    );
    let first = text(&ff_env(&fx.path(), &["op", "log"], &env));

    assert!(
        ff_env(&fx.path(), &["trigger", "shell"], &env)
            .status
            .success()
    );
    let second = text(&ff_env(&fx.path(), &["op", "log"], &env));

    assert_eq!(
        ageless(&first),
        ageless(&second),
        "an unmoved tree lands no second operation"
    );
    assert!(
        !fx.path().join(".git/fufu/ambient").exists(),
        "the retired fingerprint file is never written"
    );
}

// ---- the MCP server a fufu before v0.15 registered ------------------------

/// A registration pointing at `ff mcp` would fail at every client start
/// once the verb is gone, so `ff hook` strips the one fufu wrote: the
/// v0.14 shapes go, a foreign server beside each stays, a second run says
/// nothing, and a hand-written `fufu` entry survives both hook and unhook.
#[test]
fn hook_strips_the_registration_an_earlier_fufu_wrote() {
    let home = tempfile::TempDir::new().unwrap();
    let env = [("HOME", home.path().to_str().unwrap())];
    let cursor = home.path().join(".cursor/mcp.json");
    let codex = home.path().join(".codex/config.toml");
    for dir in [".cursor", ".codex"] {
        std::fs::create_dir_all(home.path().join(dir)).unwrap();
    }
    std::fs::write(
        &cursor,
        r#"{"mcpServers":{"fufu":{"type":"stdio","command":"/old/place/ff","args":["mcp"]},"other":{"command":"x"}}}"#,
    )
    .unwrap();
    let theirs = "model = \"o3\"\n\n[mcp_servers.other]\ncommand = \"x\"\nargs = [\"y\"]\n";
    std::fs::write(
        &codex,
        format!(
            "{theirs}# >>> fufu (ff hook codex) >>>\n[mcp_servers.fufu]\ncommand = \"/old/place/ff\"\nargs = [\"mcp\"]\n# <<< fufu <<<\n"
        ),
    )
    .unwrap();

    for slug in ["cursor", "codex"] {
        let out = ff_env(home.path(), &["hook", slug], &env);
        assert!(
            out.status.success(),
            "{slug}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(
            text(&out).contains("MCP server removed from"),
            "{slug} names the strip: {}",
            text(&out)
        );
    }
    let v = json_at(&cursor);
    assert!(v["mcpServers"].get("fufu").is_none(), "{v}");
    assert_eq!(v["mcpServers"]["other"]["command"], "x");
    let after = std::fs::read_to_string(&codex).unwrap();
    assert_eq!(after, theirs);

    // A second run is silent about it.
    for slug in ["cursor", "codex"] {
        let out = ff_env(home.path(), &["hook", slug], &env);
        assert!(out.status.success());
        assert!(
            !text(&out).contains("MCP server"),
            "{slug} says nothing the second time: {}",
            text(&out)
        );
    }

    // A hand-written entry under the same name is somebody's own. Codex's
    // unhook removes the plugin and never opens `config.toml`; its hook
    // reads the block and leaves one that is not fufu's.
    let mine_json = r#"{"mcpServers":{"fufu":{"command":"/my/wrapper.sh","args":["serve"]}}}"#;
    std::fs::write(&cursor, mine_json).unwrap();
    let mine_toml = "[mcp_servers.fufu]\ncommand = \"/my/wrapper.sh\"\n";
    std::fs::write(&codex, mine_toml).unwrap();
    for verb in ["hook", "unhook"] {
        for slug in ["cursor", "codex"] {
            let out = ff_env(home.path(), &[verb, slug], &env);
            assert!(out.status.success(), "{verb} {slug}");
            assert!(
                !text(&out).contains("MCP server"),
                "{verb} {slug} neither touches nor names it: {}",
                text(&out)
            );
        }
        assert_eq!(std::fs::read_to_string(&cursor).unwrap(), mine_json);
        assert_eq!(std::fs::read_to_string(&codex).unwrap(), mine_toml);
    }

    // Claude's plugin carries no `.mcp.json` any more.
    assert!(
        ff_env(home.path(), &["hook", "claude"], &env)
            .status
            .success()
    );
    assert!(
        home.path()
            .join(".claude/skills/fufu/.claude-plugin/plugin.json")
            .is_file()
    );
    assert!(!home.path().join(".claude/skills/fufu/.mcp.json").exists());
}
