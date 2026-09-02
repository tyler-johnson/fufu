//! Installer behavior for `ff hook` and `ff unhook`: rc-file editing, the
//! settings merge for the three clients that take one, and the Claude
//! plugin directory that replaces it.
//!
//! Every path here is env-redirected (HOME, ZDOTDIR, XDG_CONFIG_HOME,
//! SHELL, and FF_DOCUMENTS_DIR for PowerShell's profile on Windows) so the
//! suite never touches a real config file.

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

fn text(out: &Output) -> String {
    String::from_utf8(out.stdout.clone()).expect("stdout is utf-8")
}

fn json_at(path: &Path) -> serde_json::Value {
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

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
    assert_eq!(rows.len(), 8, "one row per slug: {rows:?}");
    assert_eq!(rows[0]["slug"], "claude");
    assert_eq!(rows[0]["wiring"]["state"], "not-wired");
}

// ---- the settings clients --------------------------------------------------

/// One config file per vendor, each in the shape that vendor documents.
#[test]
fn each_client_is_wired_in_its_own_schema() {
    let home = tempfile::TempDir::new().unwrap();
    let env = [("HOME", home.path().to_str().unwrap())];

    assert!(
        ff_env(home.path(), &["hook", "codex"], &env)
            .status
            .success()
    );
    let v = json_at(&home.path().join(".codex/hooks.json"));
    assert_eq!(v["hooks"]["PreToolUse"][0]["matcher"], "Bash|apply_patch");
    assert_eq!(
        v["hooks"]["PreToolUse"][0]["hooks"][0]["command"],
        "ff trigger codex"
    );
    assert_eq!(v["hooks"]["PreToolUse"][0]["hooks"][0]["type"], "command");

    assert!(
        ff_env(home.path(), &["hook", "gemini"], &env)
            .status
            .success()
    );
    let v = json_at(&home.path().join(".gemini/settings.json"));
    assert_eq!(
        v["hooks"]["BeforeTool"][0]["matcher"],
        "run_shell_command|write_file|replace"
    );
    assert_eq!(
        v["hooks"]["BeforeTool"][0]["hooks"][0]["command"],
        "ff trigger gemini"
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
    let settings = home.path().join(".codex/hooks.json");
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
        ff_env(home.path(), &["hook", "codex"], &env)
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
        v["hooks"]["PreToolUse"][1]["hooks"][0]["command"], "ff trigger codex",
        "our entry appended after foreign ones"
    );

    // Uninstall removes only ours.
    assert!(
        ff_env(home.path(), &["unhook", "codex"], &env)
            .status
            .success()
    );
    let v = json_at(&settings);
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
fn install_refuses_malformed_files_untouched() {
    let home = tempfile::TempDir::new().unwrap();
    let env = [("HOME", home.path().to_str().unwrap())];
    let settings = home.path().join(".codex/hooks.json");
    std::fs::create_dir_all(settings.parent().unwrap()).unwrap();

    for bad in [
        "{ not json",
        "[1, 2, 3]",
        r#"{ "hooks": "not an object" }"#,
        r#"{ "hooks": { "PreToolUse": "not an array" } }"#,
    ] {
        std::fs::write(&settings, bad).unwrap();
        let out = ff_env(home.path(), &["hook", "codex"], &env);
        assert_eq!(out.status.code(), Some(1), "must refuse: {bad}");
        assert_eq!(
            std::fs::read_to_string(&settings).unwrap(),
            bad,
            "file untouched on refusal"
        );
    }
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
    assert!(command.ends_with("trigger claude"), "{command:?}");
    assert!(
        command.len() > "ff trigger claude".len(),
        "absolute path baked in: {command:?}"
    );

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

/// Codex's two mechanisms are independent: entries merged into a settings
/// file it owns, and a skill directory fufu owns outright.
#[test]
fn the_codex_skill_is_its_own_directory() {
    let home = tempfile::TempDir::new().unwrap();
    let env = [("HOME", home.path().to_str().unwrap())];
    let hooks = home.path().join(".codex/hooks.json");
    let skill = home.path().join(".codex/skills/fufu/SKILL.md");

    assert!(
        ff_env(home.path(), &["hook", "codex"], &env)
            .status
            .success()
    );
    assert!(skill.exists(), "the skill lands beside the wiring");
    assert!(
        std::fs::read_to_string(&skill)
            .unwrap()
            .starts_with("---\nname: fufu\n")
    );

    // Removing the wiring removes the skill, because unhook takes back
    // exactly what hook added — both halves of it.
    assert!(
        ff_env(home.path(), &["unhook", "codex"], &env)
            .status
            .success()
    );
    assert!(!home.path().join(".codex/skills/fufu").exists());
    let v = json_at(&hooks);
    assert!(v.get("hooks").is_none(), "the entries went too: {v}");
}

/// The print route. Cursor and Gemini read no skills directory, and a
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
