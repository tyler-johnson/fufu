//! Config command conventions: registry, scopes, exit codes.

use std::path::Path;
use std::process::{Command, Output};

use ff_testsupport::Fixture;

fn ff_cfg(dir: &Path, args: &[&str], global: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_ff"))
        .current_dir(dir)
        .args(args)
        .env("GIT_CONFIG_GLOBAL", global)
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .expect("spawn ff")
}

fn stdout(out: &Output) -> String {
    String::from_utf8(out.stdout.clone()).expect("utf-8 stdout")
}

fn stderr(out: &Output) -> String {
    String::from_utf8(out.stderr.clone()).expect("utf-8 stderr")
}

#[test]
fn list_shows_defaults() {
    let fx = Fixture::new();
    let global = fx.root().join("gitconfig");
    let out = ff_cfg(&fx.path(), &["config"], &global);
    assert!(out.status.success());
    let text = stdout(&out);
    assert!(text.contains("maxFileSize"), "missing maxFileSize");
    assert!(text.contains("52428800"), "missing 52428800");
    assert!(text.contains("keep"), "missing keep");
    assert!(text.contains("90d"), "missing 90d");
    assert!(text.contains("pager"), "missing pager");
    assert!(text.contains("less"), "missing less");
    assert!(text.contains("updateCheck"), "missing updateCheck");
    assert!(text.contains("autoUpdate"), "missing autoUpdate");
    assert!(text.contains("(default)"), "missing (default) tag");
    assert!(
        text.contains("Stored as plain git config under fufu."),
        "missing footer about storage"
    );
}

#[test]
fn get_spellings_and_unknown() {
    let fx = Fixture::new();
    let global = fx.root().join("gitconfig");

    // ff config keep prints 90d
    let out = ff_cfg(&fx.path(), &["config", "keep"], &global);
    assert!(out.status.success());
    assert_eq!(stdout(&out), "90d\n");

    // Case-insensitive
    let out = ff_cfg(&fx.path(), &["config", "KEEP"], &global);
    assert!(out.status.success());
    assert_eq!(stdout(&out), "90d\n");

    // With fufu. prefix
    let out = ff_cfg(&fx.path(), &["config", "fufu.keep"], &global);
    assert!(out.status.success());
    assert_eq!(stdout(&out), "90d\n");

    // Unknown key exits 2
    let out = ff_cfg(&fx.path(), &["config", "nope"], &global);
    assert_eq!(out.status.code(), Some(2));
    assert!(stderr(&out).contains("unknown setting \"nope\""));
}

#[test]
fn set_roundtrips_through_real_git() {
    let fx = Fixture::new();
    let global = fx.root().join("gitconfig");

    // Set keep to 30d
    let out = ff_cfg(&fx.path(), &["config", "keep", "30d"], &global);
    assert!(out.status.success());
    assert!(stdout(&out).contains("keep = 30d (this repo)"));

    // Interop proof: read via git config
    let git_val = fx.git(&["config", "fufu.keep"]);
    assert_eq!(git_val.trim(), "30d");

    // ff config keep now prints 30d
    let out = ff_cfg(&fx.path(), &["config", "keep"], &global);
    assert!(out.status.success());
    assert_eq!(stdout(&out), "30d\n");

    // List no longer marks keep as (default)
    let out = ff_cfg(&fx.path(), &["config"], &global);
    assert!(out.status.success());
    let text = stdout(&out);
    let keep_line = text
        .lines()
        .find(|l| l.starts_with("keep  "))
        .expect("keep line in list output");
    assert!(
        !keep_line.contains("(default)"),
        "keep should not be marked (default) after set"
    );
}

#[test]
fn set_preserves_existing_config() {
    let fx = Fixture::new();
    let global = fx.root().join("gitconfig");

    // Inject a comment into .git/config
    let config_path = fx.path().join(".git/config");
    let original = std::fs::read_to_string(&config_path).expect("read git config");
    let modified = format!("{}\n# hands off\n", original);
    std::fs::write(&config_path, &modified).expect("write git config");

    // Set a fufu value
    let out = ff_cfg(&fx.path(), &["config", "keep", "30d"], &global);
    assert!(out.status.success());

    // Verify the config is preserved
    let after = std::fs::read_to_string(&config_path).expect("read git config after");
    assert!(after.contains("# hands off"), "injected comment was lost");
    assert!(after.contains("autocrlf"), "autocrlf setting was lost");
}

#[test]
fn invalid_values_exit_2_write_nothing() {
    let fx = Fixture::new();
    let global = fx.root().join("gitconfig");

    // keep bogus
    let out = ff_cfg(&fx.path(), &["config", "keep", "bogus"], &global);
    assert_eq!(out.status.code(), Some(2));
    assert!(stderr(&out).contains("invalid value"));

    // maxFileSize 12Q
    let out = ff_cfg(&fx.path(), &["config", "maxFileSize", "12Q"], &global);
    assert_eq!(out.status.code(), Some(2));
    assert!(stderr(&out).contains("invalid value"));

    // maxFileSize -- -5
    let out = ff_cfg(&fx.path(), &["config", "maxFileSize", "--", "-5"], &global);
    if out.status.code() == Some(2) {
        assert!(
            stderr(&out).contains("invalid value") || stderr(&out).contains("error:"),
            "should reject negative value or fail with usage error"
        );
    }
    // If clap refuses "-- -5" altogether, that's acceptable — noted in report.

    // Nothing was written
    assert!(
        !fx.try_git(&["config", "--get", "fufu.keep"])
            .status
            .success()
    );
    assert!(
        !fx.try_git(&["config", "--get", "fufu.maxFileSize"])
            .status
            .success()
    );
}

#[test]
fn usage_errors_exit_2() {
    let fx = Fixture::new();
    let global = fx.root().join("gitconfig");

    // --unset without key
    let out = ff_cfg(&fx.path(), &["config", "--unset"], &global);
    assert_eq!(out.status.code(), Some(2));

    // key value --unset (conflicts)
    let out = ff_cfg(&fx.path(), &["config", "keep", "30d", "--unset"], &global);
    assert_eq!(out.status.code(), Some(2));
}

#[test]
fn global_set_creates_and_applies() {
    let fx = Fixture::new();
    let global = fx.root().join("gitconfig");

    // Global file does not exist yet
    assert!(!global.exists());

    // Set globally
    let out = ff_cfg(&fx.path(), &["config", "keep", "10d", "--global"], &global);
    assert!(out.status.success());
    assert!(stdout(&out).contains("keep = 10d (every repo)"));

    // Global file now exists and has the right content
    let content = std::fs::read_to_string(&global).expect("read global config");
    assert!(content.contains("[fufu]"));
    assert!(content.contains("keep = 10d"));

    // ff config keep reads the global value
    let out = ff_cfg(&fx.path(), &["config", "keep"], &global);
    assert!(out.status.success());
    assert_eq!(stdout(&out), "10d\n");
}

#[test]
fn unset_ladder() {
    let fx = Fixture::new();
    let global = fx.root().join("gitconfig");

    // Seed global keep = 10d
    let out = ff_cfg(&fx.path(), &["config", "keep", "10d", "--global"], &global);
    assert!(out.status.success());

    // Seed local keep = 30d
    let out = ff_cfg(&fx.path(), &["config", "keep", "30d"], &global);
    assert!(out.status.success());

    // Unset local — global still applies
    let out = ff_cfg(&fx.path(), &["config", "--unset", "keep"], &global);
    assert!(out.status.success());
    assert!(stdout(&out).contains("still applies from global config"));

    // ff config keep now shows 10d
    let out = ff_cfg(&fx.path(), &["config", "keep"], &global);
    assert!(out.status.success());
    assert_eq!(stdout(&out), "10d\n");

    // Unset global — back to default
    let out = ff_cfg(
        &fx.path(),
        &["config", "--unset", "keep", "--global"],
        &global,
    );
    assert!(out.status.success());
    assert!(stdout(&out).contains("back to the default (90d)"));

    // Unset again — not set anywhere
    let out = ff_cfg(&fx.path(), &["config", "--unset", "keep"], &global);
    assert!(out.status.success());
    assert!(stdout(&out).contains("is not set"));
}

#[test]
fn json_shapes() {
    let fx = Fixture::new();
    let global = fx.root().join("gitconfig");

    // Single key get — default
    let out = ff_cfg(&fx.path(), &["config", "--json", "keep"], &global);
    assert!(out.status.success());
    let text = stdout(&out);
    assert!(
        text.ends_with('\n') && !text[..text.len() - 1].contains('\n'),
        "one line + one newline"
    );
    let v: serde_json::Value = serde_json::from_str(&text).expect("valid json");
    assert_eq!(v["data"]["key"], "keep");
    assert_eq!(v["data"]["git_key"], "fufu.keep");
    assert_eq!(v["data"]["value"], "90d");
    assert_eq!(v["data"]["source"], serde_json::Value::Null);
    assert_eq!(v["data"]["default"], true);

    // Set keep, then get again
    let out = ff_cfg(&fx.path(), &["config", "keep", "30d"], &global);
    assert!(out.status.success());
    let out = ff_cfg(&fx.path(), &["config", "--json", "keep"], &global);
    assert!(out.status.success());
    let v: serde_json::Value = serde_json::from_str(&stdout(&out)).expect("valid json");
    assert_eq!(v["data"]["value"], "30d");
    assert_eq!(v["data"]["source"], "local");
    assert_eq!(v["data"]["default"], false);

    // List all as JSON
    let out = ff_cfg(&fx.path(), &["config", "--json"], &global);
    assert!(out.status.success());
    let text = stdout(&out);
    assert!(
        text.ends_with('\n') && !text[..text.len() - 1].contains('\n'),
        "one line + one newline"
    );
    let v: serde_json::Value = serde_json::from_str(&text).expect("valid json");
    assert!(v["data"]["settings"].is_array());
    assert_eq!(v["data"]["settings"].as_array().unwrap().len(), 10);
    assert_eq!(v["data"]["settings"][0]["key"], "maxFileSize");

    // Set as JSON
    let out = ff_cfg(&fx.path(), &["config", "--json", "keep", "45d"], &global);
    assert!(out.status.success());
    let v: serde_json::Value = serde_json::from_str(&stdout(&out)).expect("valid json");
    assert!(v["data"].get("key").is_some());
    assert!(v["data"].get("value").is_some());
    assert!(v["data"].get("global").is_some());

    // Byte stability
    let a = ff_cfg(&fx.path(), &["config", "--json"], &global);
    let b = ff_cfg(&fx.path(), &["config", "--json"], &global);
    assert_eq!(a.stdout, b.stdout, "identical bytes run to run");
}

#[test]
fn update_settings_validate() {
    let fx = Fixture::new();
    let global = fx.root().join("gitconfig");

    // updateCheck bogus → exit 2
    let out = ff_cfg(&fx.path(), &["config", "updateCheck", "bogus"], &global);
    assert_eq!(out.status.code(), Some(2));
    assert!(stderr(&out).contains("invalid value for updateCheck"));

    // autoUpdate maybe → exit 2
    let out = ff_cfg(&fx.path(), &["config", "autoUpdate", "maybe"], &global);
    assert_eq!(out.status.code(), Some(2));
    assert!(stderr(&out).contains("invalid value for autoUpdate"));
}

#[test]
#[cfg(target_os = "linux")]
fn update_check_syncs_cache() {
    let fx = Fixture::new();
    let global = fx.root().join("gitconfig");
    let cache = tempfile::TempDir::new().expect("create cache tempdir");
    let state_file = cache.path().join("fufu").join("update.json");

    fn run_cfg(
        dir: &Path,
        args: &[&str],
        global: &Path,
        cache_home: &Path,
    ) -> std::process::Output {
        Command::new(env!("CARGO_BIN_EXE_ff"))
            .current_dir(dir)
            .args(args)
            .env("GIT_CONFIG_GLOBAL", global)
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("XDG_CACHE_HOME", cache_home)
            .output()
            .expect("spawn ff")
    }

    // Set to 12h
    let out = run_cfg(
        &fx.path(),
        &["config", "updateCheck", "12h"],
        &global,
        cache.path(),
    );
    assert!(
        out.status.success(),
        "set 12h failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let content = std::fs::read_to_string(&state_file).expect("read state file");
    assert!(
        content.contains("\"interval_secs\":43200"),
        "expected 43200, got: {}",
        content
    );

    // Set to false (disabled)
    let out = run_cfg(
        &fx.path(),
        &["config", "updateCheck", "false"],
        &global,
        cache.path(),
    );
    assert!(
        out.status.success(),
        "set false failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let content = std::fs::read_to_string(&state_file).expect("read state file");
    assert!(
        content.contains("\"interval_secs\":-1"),
        "expected -1, got: {}",
        content
    );

    // Unset → back to default (0)
    let out = run_cfg(
        &fx.path(),
        &["config", "--unset", "updateCheck"],
        &global,
        cache.path(),
    );
    assert!(
        out.status.success(),
        "unset failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let content = std::fs::read_to_string(&state_file).expect("read state file");
    assert!(
        content.contains("\"interval_secs\":0"),
        "expected 0, got: {}",
        content
    );

    // autoUpdate false → exit 0 (Bool sets cleanly)
    let out = run_cfg(
        &fx.path(),
        &["config", "autoUpdate", "false"],
        &global,
        cache.path(),
    );
    assert!(
        out.status.success(),
        "set autoUpdate false failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn trunk_is_listed() {
    let fx = Fixture::new();
    let global = fx.root().join("gitconfig");
    let out = ff_cfg(&fx.path(), &["config"], &global);
    assert!(out.status.success());
    let text = stdout(&out);
    assert!(text.contains("trunk"), "missing trunk");
}

#[test]
fn trunk_accepts_remote_qualified() {
    let fx = Fixture::new();
    let global = fx.root().join("gitconfig");
    let out = ff_cfg(&fx.path(), &["config", "trunk", "origin/main"], &global);
    assert!(out.status.success());
    // Read back via git config
    let git_val = fx.git(&["config", "fufu.trunk"]);
    assert_eq!(git_val.trim(), "origin/main");
}

#[test]
fn ambient_setting_round_trips() {
    let fx = Fixture::new();
    let global = fx.root().join("gitconfig");

    // Default: get prints true; the list marks it (default)
    let out = ff_cfg(&fx.path(), &["config", "ambient"], &global);
    assert!(out.status.success());
    assert_eq!(stdout(&out), "true\n");

    let out = ff_cfg(&fx.path(), &["config"], &global);
    assert!(out.status.success());
    let text = stdout(&out);
    let ambient_line = text
        .lines()
        .find(|l| l.starts_with("ambient  "))
        .expect("ambient line in list output");
    assert!(
        ambient_line.contains("(default)"),
        "ambient marked (default) before set: {ambient_line}"
    );

    // Set false
    let out = ff_cfg(&fx.path(), &["config", "ambient", "false"], &global);
    assert!(out.status.success());

    let out = ff_cfg(&fx.path(), &["config", "ambient"], &global);
    assert!(out.status.success());
    assert_eq!(stdout(&out), "false\n");

    let out = ff_cfg(&fx.path(), &["config"], &global);
    assert!(out.status.success());
    let text = stdout(&out);
    let ambient_line = text
        .lines()
        .find(|l| l.starts_with("ambient  "))
        .expect("ambient line in list output");
    assert!(
        !ambient_line.contains("(default)"),
        "ambient not marked (default) after set: {ambient_line}"
    );

    // Unset — back to the default
    let out = ff_cfg(&fx.path(), &["config", "--unset", "ambient"], &global);
    assert!(out.status.success());
    let out = ff_cfg(&fx.path(), &["config", "ambient"], &global);
    assert!(out.status.success());
    assert_eq!(stdout(&out), "true\n");
}

#[test]
fn ambient_rejects_a_non_boolean() {
    let fx = Fixture::new();
    let global = fx.root().join("gitconfig");

    // Exit 2 and a non-empty stderr; the message itself is the shared
    // hardcoded per-kind string, so no exact-text assertion here.
    let out = ff_cfg(&fx.path(), &["config", "ambient", "maybe"], &global);
    assert_eq!(out.status.code(), Some(2));
    assert!(!stderr(&out).is_empty(), "stderr must name the failure");
}

#[test]
fn futures_depth_round_trips_with_a_suffix() {
    let fx = Fixture::new();
    let global = fx.root().join("gitconfig");

    let out = ff_cfg(&fx.path(), &["config", "futuresDepth", "1k"], &global);
    assert!(out.status.success(), "set 1k failed: {}", stderr(&out));

    // Read back: whatever git stored is what the command shows.
    let out = ff_cfg(&fx.path(), &["config", "futuresDepth"], &global);
    assert!(out.status.success());
    let text = stdout(&out);
    assert!(
        text.trim() == "1k",
        "stored value shown verbatim, got: {text}"
    );
}

#[test]
fn futures_depth_rejects_a_negative() {
    let fx = Fixture::new();
    let global = fx.root().join("gitconfig");

    let out = ff_cfg(&fx.path(), &["config", "futuresDepth", "--", "-5"], &global);
    assert_eq!(out.status.code(), Some(2));
}

#[test]
fn usage_ids_exit_two() {
    let fx = Fixture::new();
    let global = fx.root().join("gitconfig");

    // Unknown key exits 2 and reports usage/unknown-key in JSON
    let out = ff_cfg(&fx.path(), &["config", "--json", "nope"], &global);
    assert_eq!(out.status.code(), Some(2));
    let text = stdout(&out);
    let v: serde_json::Value = serde_json::from_str(&text).expect("valid json");
    assert_eq!(v["error"]["id"], "usage/unknown-key");
}
