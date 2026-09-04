//! Coverage for a declared extension's own skill files: installed beside
//! fufu's for Claude and Codex, printed by `ff hook --skill <name>`, and
//! left alone for Cursor and Gemini, which read no skills directory at all.
//!
//! Unix only, for the reason `tests/ext.rs` is: one test resolves a real
//! binary on PATH.
//!
//! PATH is pinned to the test's own bin directory rather than prepended to
//! the real one, and the user roots are pinned under the test's home — the
//! landmine `tests/doctor_extensions.rs` and `tests/extension.rs` document:
//! this machine can carry a real `ff-tower` on PATH and a real declared
//! registry, and either would decide a test's outcome instead of the
//! fixture.

#![cfg(unix)]

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use ff_testsupport::fixtures::null_device;
use ff_testsupport::userdirs;
use serde_json::Value;
use tempfile::TempDir;

fn ff(home: &Path, bin: Option<&Path>, args: &[&str]) -> Output {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_ff"));
    cmd.current_dir(home).args(args);
    userdirs::pin(&mut cmd, home)
        .env_remove("FF_SESSION")
        .env("GIT_CONFIG_GLOBAL", null_device())
        .env("GIT_CONFIG_SYSTEM", null_device())
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env(
            "PATH",
            bin.map(|bin| bin.display().to_string()).unwrap_or_default(),
        )
        .output()
        .expect("spawn ff")
}

fn stdout(out: &Output) -> String {
    String::from_utf8(out.stdout.clone()).expect("utf-8 stdout")
}

fn text_at(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|err| panic!("read {}: {err}", path.display()))
}

/// A machine: a home with nothing declared, and a directory that is the
/// whole of PATH.
fn machine() -> (TempDir, TempDir) {
    (
        TempDir::new().expect("create home"),
        TempDir::new().expect("create bin dir"),
    )
}

/// A manifest of the smallest shape, naming the given skill files.
fn manifest(name: &str, skills: &[String]) -> Value {
    serde_json::json!({
        "name": name,
        "version": "0.1.0",
        "contract": 1,
        "verbs": [{"name": "go", "read_only": true}],
        "undoable": true,
        "skills": skills,
    })
}

/// Write the registry this machine reads, declaring one extension. Bypasses
/// `ff extension add`'s handshake — these tests are about what a hook
/// install does with a manifest already on record, not about declaring one,
/// the same shortcut `tests/hook.rs`'s `declared_extensions` module takes.
fn declare(home: &Path, bin: &Path, name: &str, skills: &[String]) {
    let record = serde_json::json!({
        "path": bin.join(format!("ff-{name}")),
        "declared_at": 1_788_462_398_i64,
        "manifest": manifest(name, skills),
    });
    let file = userdirs::registry(home);
    std::fs::create_dir_all(file.parent().expect("parent")).expect("create config dir");
    std::fs::write(
        &file,
        serde_json::json!({ "ff": 1, "extensions": [record] }).to_string(),
    )
    .expect("write registry");
}

/// A skill source file under `dir`, created with `content`. Answers its own
/// path, for splicing into a manifest's `skills` field.
fn skill_source(dir: &Path, filename: &str, content: &str) -> PathBuf {
    std::fs::create_dir_all(dir).expect("create source dir");
    let path = dir.join(filename);
    std::fs::write(&path, content).expect("write source");
    path
}

/// An executable `ff-<name>` that does nothing — enough for `resolve` to
/// find it, which is all a relative skill path needs from the binary.
fn ext_bin(bin: &Path, name: &str) {
    let path = bin.join(format!("ff-{name}"));
    std::fs::write(&path, "#!/bin/sh\nexit 0\n").expect("write stub");
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).expect("chmod");
}

// ---- Claude and Codex install a declared extension's own skill ------------

#[test]
fn claude_writes_a_declared_extensions_skill_beside_fufus() {
    let (home, bin) = machine();
    let src = TempDir::new().unwrap();
    let file = skill_source(src.path(), "tower.md", "the tower manual");
    declare(
        home.path(),
        bin.path(),
        "tower",
        &[file.display().to_string()],
    );

    let out = ff(home.path(), Some(bin.path()), &["hook", "claude"]);
    assert!(out.status.success(), "{}", stdout(&out));
    assert!(
        stdout(&out).contains("tower skill written to"),
        "{}",
        stdout(&out)
    );

    let installed = home
        .path()
        .join(".claude/skills/fufu/skills/tower/tower.md");
    assert_eq!(text_at(&installed), "the tower manual");
}

#[test]
fn codex_writes_a_declared_extensions_skill_in_its_own_directory() {
    let (home, bin) = machine();
    let src = TempDir::new().unwrap();
    let file = skill_source(src.path(), "tower.md", "the tower manual");
    declare(
        home.path(),
        bin.path(),
        "tower",
        &[file.display().to_string()],
    );

    let out = ff(home.path(), Some(bin.path()), &["hook", "codex"]);
    assert!(out.status.success(), "{}", stdout(&out));
    assert!(
        stdout(&out).contains("tower skill written to"),
        "{}",
        stdout(&out)
    );

    let installed = home.path().join(".codex/skills/tower/tower.md");
    assert_eq!(text_at(&installed), "the tower manual");
}

/// A relative entry in `skills` is resolved against the directory the
/// extension's binary lives in, exactly as the manifest documents the
/// field — the one case that needs a real, resolvable `ff-<name>` on PATH.
#[test]
fn a_relative_skill_path_resolves_against_the_binarys_directory() {
    let (home, bin) = machine();
    ext_bin(bin.path(), "tower");
    skill_source(bin.path(), "tower.md", "relative manual");
    declare(home.path(), bin.path(), "tower", &["tower.md".to_string()]);

    let out = ff(home.path(), Some(bin.path()), &["hook", "codex"]);
    assert!(out.status.success(), "{}", stdout(&out));

    let installed = home.path().join(".codex/skills/tower/tower.md");
    assert_eq!(text_at(&installed), "relative manual");
}

// ---- `ff hook --skill <name>` ----------------------------------------------

#[test]
fn skill_name_prints_a_declared_extensions_skill() {
    let (home, bin) = machine();
    let src = TempDir::new().unwrap();
    let file = skill_source(src.path(), "tower.md", "the tower manual");
    declare(
        home.path(),
        bin.path(),
        "tower",
        &[file.display().to_string()],
    );

    let out = ff(home.path(), Some(bin.path()), &["hook", "--skill", "tower"]);
    assert!(out.status.success(), "{}", stdout(&out));
    assert_eq!(stdout(&out), "the tower manual");

    // A print is a print: nothing was written.
    assert!(!home.path().join(".claude").exists());
    assert!(!home.path().join(".codex").exists());
}

/// More than one file in the manifest prints as more than one, in the
/// manifest's own order.
#[test]
fn skill_name_concatenates_more_than_one_file_in_order() {
    let (home, bin) = machine();
    let src = TempDir::new().unwrap();
    let a = skill_source(src.path(), "a.md", "first");
    let b = skill_source(src.path(), "b.md", "second");
    declare(
        home.path(),
        bin.path(),
        "tower",
        &[a.display().to_string(), b.display().to_string()],
    );

    let out = ff(home.path(), Some(bin.path()), &["hook", "--skill", "tower"]);
    assert!(out.status.success(), "{}", stdout(&out));
    let printed = stdout(&out);
    assert!(
        printed.find("first").unwrap() < printed.find("second").unwrap(),
        "{printed:?}"
    );
}

#[test]
fn skill_name_for_an_undeclared_extension_is_refused() {
    let (home, bin) = machine();
    let out = ff(home.path(), Some(bin.path()), &["hook", "--skill", "nope"]);
    assert!(!out.status.success(), "{}", stdout(&out));
    assert!(
        String::from_utf8_lossy(&out.stderr)
            .contains("nothing on this machine is declared under `nope`"),
        "{:?}",
        out
    );
}

/// A declared extension naming no readable skill prints nothing rather than
/// erroring — the same doctrine a hook install applies to it.
#[test]
fn skill_name_for_a_declared_extension_with_no_readable_file_prints_nothing() {
    let (home, bin) = machine();
    declare(
        home.path(),
        bin.path(),
        "tower",
        &["/nowhere/tower.md".to_string()],
    );
    let out = ff(home.path(), Some(bin.path()), &["hook", "--skill", "tower"]);
    assert!(out.status.success(), "{}", stdout(&out));
    assert_eq!(stdout(&out), "");
}

// ---- Cursor and Gemini are unaffected ---------------------------------------

#[test]
fn cursor_and_gemini_write_no_skills_directory_for_a_declared_extension() {
    let (home, bin) = machine();
    let src = TempDir::new().unwrap();
    let file = skill_source(src.path(), "tower.md", "the tower manual");
    declare(
        home.path(),
        bin.path(),
        "tower",
        &[file.display().to_string()],
    );

    for slug in ["cursor", "gemini"] {
        let out = ff(home.path(), Some(bin.path()), &["hook", slug]);
        assert!(out.status.success(), "{slug}: {}", stdout(&out));
    }
    assert!(!home.path().join(".cursor/skills").exists());
    assert!(!home.path().join(".gemini/skills").exists());
}

// ---- reinstall refreshes, unhook removes -----------------------------------

/// A reinstall refreshes exactly what a fresh manifest names: new content
/// lands, and a file the manifest dropped does not linger from the last
/// run.
#[test]
fn rerunning_hook_refreshes_and_drops_what_is_no_longer_named() {
    let (home, bin) = machine();
    let src = TempDir::new().unwrap();
    let a = skill_source(src.path(), "a.md", "version one");
    let b = skill_source(src.path(), "b.md", "second file");
    declare(
        home.path(),
        bin.path(),
        "tower",
        &[a.display().to_string(), b.display().to_string()],
    );
    assert!(
        ff(home.path(), Some(bin.path()), &["hook", "claude"])
            .status
            .success()
    );
    let dir = home.path().join(".claude/skills/fufu/skills/tower");
    assert!(dir.join("a.md").exists());
    assert!(dir.join("b.md").exists());

    std::fs::write(&a, "version two").unwrap();
    declare(home.path(), bin.path(), "tower", &[a.display().to_string()]);
    assert!(
        ff(home.path(), Some(bin.path()), &["hook", "claude"])
            .status
            .success()
    );
    assert_eq!(text_at(&dir.join("a.md")), "version two");
    assert!(
        !dir.join("b.md").exists(),
        "a file no longer named must not linger"
    );
}

#[test]
fn unhook_claude_removes_the_extensions_skill_with_the_plugin() {
    let (home, bin) = machine();
    let src = TempDir::new().unwrap();
    let file = skill_source(src.path(), "tower.md", "the tower manual");
    declare(
        home.path(),
        bin.path(),
        "tower",
        &[file.display().to_string()],
    );
    assert!(
        ff(home.path(), Some(bin.path()), &["hook", "claude"])
            .status
            .success()
    );

    let out = ff(home.path(), Some(bin.path()), &["unhook", "claude"]);
    assert!(out.status.success(), "{}", stdout(&out));
    assert!(!home.path().join(".claude/skills/fufu").exists());
}

#[test]
fn unhook_codex_removes_a_still_declared_extensions_skill_directory() {
    let (home, bin) = machine();
    let src = TempDir::new().unwrap();
    let file = skill_source(src.path(), "tower.md", "the tower manual");
    declare(
        home.path(),
        bin.path(),
        "tower",
        &[file.display().to_string()],
    );
    assert!(
        ff(home.path(), Some(bin.path()), &["hook", "codex"])
            .status
            .success()
    );
    let dir = home.path().join(".codex/skills/tower");
    assert!(dir.exists());

    let out = ff(home.path(), Some(bin.path()), &["unhook", "codex"]);
    assert!(out.status.success(), "{}", stdout(&out));
    assert!(!dir.exists());
}

/// An extension taken back with `ff extension remove` before `ff unhook
/// codex` runs leaves its directory behind: the removal loop walks the
/// registry, and there is nothing left in it naming that extension. A
/// documented limitation, not a bug — Codex's `skills/` is not a directory
/// fufu owns outright the way Claude's plugin is, so nothing here prunes it
/// by scanning.
#[test]
fn unhook_codex_leaves_an_undeclared_extensions_directory_behind() {
    let (home, bin) = machine();
    let src = TempDir::new().unwrap();
    let file = skill_source(src.path(), "tower.md", "the tower manual");
    declare(
        home.path(),
        bin.path(),
        "tower",
        &[file.display().to_string()],
    );
    assert!(
        ff(home.path(), Some(bin.path()), &["hook", "codex"])
            .status
            .success()
    );
    let dir = home.path().join(".codex/skills/tower");
    assert!(dir.exists());

    assert!(
        ff(
            home.path(),
            Some(bin.path()),
            &["extension", "remove", "tower"]
        )
        .status
        .success()
    );
    assert!(
        ff(home.path(), Some(bin.path()), &["unhook", "codex"])
            .status
            .success()
    );
    assert!(dir.exists(), "orphaned rather than removed — a known gap");
}

// ---- a broken skill file never breaks the install --------------------------

#[test]
fn a_missing_skill_file_does_not_break_the_install() {
    let (home, bin) = machine();
    declare(
        home.path(),
        bin.path(),
        "tower",
        &["/nowhere/tower.md".to_string()],
    );
    let out = ff(home.path(), Some(bin.path()), &["hook", "claude"]);
    assert!(out.status.success(), "{}", stdout(&out));
    assert!(
        !stdout(&out).contains("tower skill written"),
        "nothing landed: {}",
        stdout(&out)
    );
    assert!(
        !home
            .path()
            .join(".claude/skills/fufu/skills/tower")
            .exists()
    );
    // The rest of the plugin is still wired.
    assert!(
        home.path()
            .join(".claude/skills/fufu/skills/fufu/SKILL.md")
            .exists()
    );
}

#[test]
fn an_oversized_skill_file_does_not_break_the_install() {
    let (home, bin) = machine();
    let src = TempDir::new().unwrap();
    let huge = src.path().join("huge.md");
    std::fs::write(&huge, vec![b'x'; 9 * 1024 * 1024]).unwrap();
    declare(
        home.path(),
        bin.path(),
        "tower",
        &[huge.display().to_string()],
    );

    let out = ff(home.path(), Some(bin.path()), &["hook", "codex"]);
    assert!(out.status.success(), "{}", stdout(&out));
    assert!(!home.path().join(".codex/skills/tower").exists());
}
