//! Coverage for a declared extension's own skills: asked for through
//! `ff-<name> --ff-skill <skill>`, installed one directory each beside
//! fufu's for Claude and Codex, printed by `ff hook --skill <skill>`, and
//! left alone for Cursor and Gemini, which read no skills directory at all.
//!
//! Unix only, for the reason `tests/ext.rs` is: the stub `ff-<name>` is a
//! shell script, and the handshake runs it.
//!
//! PATH is pinned to the test's own bin directory rather than prepended to
//! the real one, and the user roots are pinned under the test's home — the
//! landmine `tests/doctor_extensions.rs` and `tests/extension.rs` document:
//! this machine can carry a real `ff-tower` on PATH and a real declared
//! registry, and either would decide a test's outcome instead of the
//! fixture.

#![cfg(unix)]

use std::os::unix::fs::PermissionsExt;
use std::path::Path;
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

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
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

/// A manifest of the smallest shape, naming the given skills.
fn manifest(name: &str, skills: &[&str]) -> Value {
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
fn declare(home: &Path, bin: &Path, name: &str, skills: &[&str]) {
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

/// A registry with nothing in it, which is what `ff extension remove` of
/// the last extension leaves.
fn undeclare_all(home: &Path) {
    std::fs::write(
        userdirs::registry(home),
        serde_json::json!({ "ff": 1, "extensions": [] }).to_string(),
    )
    .expect("write registry");
}

/// How the stub answers `--ff-skill`.
#[derive(Clone, Copy)]
enum Stub {
    /// One envelope on one line from `<bin>/<name>.skills/<skill>.json`,
    /// and an error envelope when there is no such file.
    Answers,
    /// Exits 1 with a word on stderr.
    Exits,
    /// A banner on stdout before the envelope.
    Banners,
}

/// An executable `ff-<name>` on the bin directory that answers the skill
/// handshake from the JSON files the test wrote beside it.
fn ext_bin(bin: &Path, name: &str, stub: Stub) {
    let store = bin.join(format!("{name}.skills"));
    std::fs::create_dir_all(&store).expect("create skill store");
    let answer = match stub {
        Stub::Answers => "",
        Stub::Exits => "echo 'not today' >&2; exit 1\n",
        Stub::Banners => "echo 'tower 0.1.0'\n",
    };
    let script = format!(
        r#"#!/bin/sh
# PATH is the bin directory alone for the length of the ask, and `cat` is
# not there.
PATH=/bin:/usr/bin; export PATH
if [ "$1" != "--ff-skill" ] || [ $# -ne 2 ]; then
  echo "unexpected: $*" >&2
  exit 2
fi
{answer}file="{store}/$2.json"
if [ -f "$file" ]; then
  printf '{{"ff":1,"cmd":"{name} --ff-skill %s","data":' "$2"
  cat "$file"
  printf '}}\n'
else
  printf '{{"ff":1,"cmd":"{name} --ff-skill %s","error":{{"id":"{name}/skill/unknown","message":"no such skill","exits":[]}}}}\n' "$2"
fi
"#,
        store = store.display(),
    );
    let path = bin.join(format!("ff-{name}"));
    std::fs::write(&path, script).expect("write stub");
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).expect("chmod");
}

/// What the stub answers for one skill: the `files` list, compacted onto
/// one line so the envelope stays one line.
fn skill_answer(bin: &Path, name: &str, skill: &str, files: &[(&str, &str)]) {
    let files: Vec<Value> = files
        .iter()
        .map(|(path, content)| serde_json::json!({"path": path, "content": content}))
        .collect();
    std::fs::write(
        bin.join(format!("{name}.skills"))
            .join(format!("{skill}.json")),
        serde_json::json!({ "files": files }).to_string(),
    )
    .expect("write skill answer");
}

/// A `SKILL.md` with the front matter both clients want.
fn skill_md(name: &str) -> String {
    format!("---\nname: {name}\ndescription: the {name} manual\n---\n# {name}\n")
}

/// tower with two skills: `tower`, one file, and `tower-plan`, with a script
/// beside its `SKILL.md`.
fn tower(home: &Path, bin: &Path) {
    ext_bin(bin, "tower", Stub::Answers);
    skill_answer(bin, "tower", "tower", &[("SKILL.md", &skill_md("tower"))]);
    skill_answer(
        bin,
        "tower",
        "tower-plan",
        &[
            ("SKILL.md", &skill_md("tower-plan")),
            ("scripts/run.sh", "#!/bin/sh\nff tower\n"),
        ],
    );
    declare(home, bin, "tower", &["tower", "tower-plan"]);
}

fn claude_skills(home: &Path) -> std::path::PathBuf {
    home.join(".claude/skills/fufu/skills")
}

fn codex_skills(home: &Path) -> std::path::PathBuf {
    home.join(".codex/skills")
}

// ---- Claude and Codex install a declared extension's skills ---------------

#[test]
fn claude_writes_each_skill_in_its_own_directory_beside_fufus() {
    let (home, bin) = machine();
    tower(home.path(), bin.path());

    let out = ff(home.path(), Some(bin.path()), &["hook", "claude"]);
    assert!(out.status.success(), "{}", stdout(&out));
    let said = stdout(&out);
    assert!(
        said.contains("tower skills written to") && said.contains(": tower, tower-plan"),
        "{said}"
    );

    let root = claude_skills(home.path());
    assert_eq!(text_at(&root.join("tower/SKILL.md")), skill_md("tower"));
    assert_eq!(
        text_at(&root.join("tower-plan/SKILL.md")),
        skill_md("tower-plan")
    );
    assert_eq!(
        text_at(&root.join("tower-plan/scripts/run.sh")),
        "#!/bin/sh\nff tower\n"
    );
    assert!(
        root.join("fufu/SKILL.md").is_file(),
        "fufu's own is beside them"
    );
}

#[test]
fn codex_writes_each_skill_in_its_own_directory() {
    let (home, bin) = machine();
    tower(home.path(), bin.path());

    let out = ff(home.path(), Some(bin.path()), &["hook", "codex"]);
    assert!(out.status.success(), "{}", stdout(&out));
    let said = stdout(&out);
    assert!(
        said.contains("tower skills written to") && said.contains(": tower, tower-plan"),
        "{said}"
    );

    let root = codex_skills(home.path());
    assert_eq!(text_at(&root.join("tower/SKILL.md")), skill_md("tower"));
    assert_eq!(
        text_at(&root.join("tower-plan/SKILL.md")),
        skill_md("tower-plan")
    );
    assert_eq!(
        text_at(&root.join("tower-plan/scripts/run.sh")),
        "#!/bin/sh\nff tower\n"
    );
    assert!(root.join("fufu/SKILL.md").is_file());
}

// ---- `ff hook --skill <skill>` ----------------------------------------------

#[test]
fn skill_name_prints_that_skills_manual() {
    let (home, bin) = machine();
    tower(home.path(), bin.path());

    let out = ff(
        home.path(),
        Some(bin.path()),
        &["hook", "--skill", "tower-plan"],
    );
    assert!(out.status.success(), "{}", stderr(&out));
    assert_eq!(stdout(&out), skill_md("tower-plan"));

    let out = ff(
        home.path(),
        Some(bin.path()),
        &["hook", "--skill", "tower-plan", "--json"],
    );
    assert!(out.status.success(), "{}", stderr(&out));
    let envelope: Value = serde_json::from_str(stdout(&out).trim()).expect("one envelope");
    assert_eq!(envelope["data"]["skill"], skill_md("tower-plan"));

    // A print is a print: nothing was written.
    assert!(!home.path().join(".claude").exists());
    assert!(!home.path().join(".codex").exists());
}

#[test]
fn skill_name_nothing_declares_is_refused() {
    let (home, bin) = machine();
    tower(home.path(), bin.path());

    for name in ["nope", "tower-loop"] {
        let out = ff(
            home.path(),
            Some(bin.path()),
            &["hook", "--skill", name, "--json"],
        );
        assert!(!out.status.success(), "{name}: {}", stdout(&out));
        let envelope: Value = serde_json::from_str(stdout(&out).trim()).expect("one envelope");
        assert_eq!(envelope["error"]["id"], "extension/not-declared", "{name}");
        assert!(
            envelope["error"]["message"]
                .as_str()
                .unwrap()
                .contains(&format!("declares a skill named `{name}`")),
            "{envelope}"
        );
    }
}

#[test]
fn skill_name_the_binary_will_not_answer_is_skill_failed() {
    let (home, bin) = machine();
    ext_bin(bin.path(), "tower", Stub::Exits);
    declare(home.path(), bin.path(), "tower", &["tower"]);

    let out = ff(
        home.path(),
        Some(bin.path()),
        &["hook", "--skill", "tower", "--json"],
    );
    assert!(!out.status.success());
    let envelope: Value = serde_json::from_str(stdout(&out).trim()).expect("one envelope");
    assert_eq!(envelope["error"]["id"], "extension/skill-failed");
    assert!(
        envelope["error"]["message"]
            .as_str()
            .unwrap()
            .contains("not today"),
        "{envelope}"
    );
}

#[test]
fn skill_name_the_binary_answers_badly_is_bad_skill() {
    let (home, bin) = machine();
    ext_bin(bin.path(), "tower", Stub::Answers);
    skill_answer(
        bin.path(),
        "tower",
        "tower",
        &[("SKILL.md", "x"), ("../evil.sh", "rm -rf /")],
    );
    declare(home.path(), bin.path(), "tower", &["tower"]);

    let out = ff(
        home.path(),
        Some(bin.path()),
        &["hook", "--skill", "tower", "--json"],
    );
    assert!(!out.status.success());
    let envelope: Value = serde_json::from_str(stdout(&out).trim()).expect("one envelope");
    assert_eq!(envelope["error"]["id"], "extension/bad-skill");
}

// ---- Cursor and Gemini are unaffected ---------------------------------------

#[test]
fn cursor_and_gemini_write_no_skills_directory_for_a_declared_extension() {
    let (home, bin) = machine();
    tower(home.path(), bin.path());

    for slug in ["cursor", "gemini"] {
        let out = ff(home.path(), Some(bin.path()), &["hook", slug]);
        assert!(out.status.success(), "{slug}: {}", stdout(&out));
    }
    assert!(!home.path().join(".cursor/skills").exists());
    assert!(!home.path().join(".gemini/skills").exists());
}

// ---- reinstall refreshes, unhook removes -----------------------------------

/// A reinstall refreshes exactly what the binary produces now: new content
/// lands, a file a skill dropped does not linger, and a skill the manifest
/// no longer names goes with its directory.
#[test]
fn rerunning_hook_refreshes_and_drops_what_is_no_longer_named() {
    let (home, bin) = machine();
    tower(home.path(), bin.path());
    assert!(
        ff(home.path(), Some(bin.path()), &["hook", "claude"])
            .status
            .success()
    );
    let root = claude_skills(home.path());
    assert!(root.join("tower/SKILL.md").exists());
    assert!(root.join("tower-plan/scripts/run.sh").exists());

    skill_answer(
        bin.path(),
        "tower",
        "tower-plan",
        &[("SKILL.md", "version two")],
    );
    declare(home.path(), bin.path(), "tower", &["tower-plan"]);
    assert!(
        ff(home.path(), Some(bin.path()), &["hook", "claude"])
            .status
            .success()
    );
    assert_eq!(text_at(&root.join("tower-plan/SKILL.md")), "version two");
    assert!(
        !root.join("tower-plan/scripts").exists(),
        "a file the skill no longer carries must not linger"
    );
    assert!(
        !root.join("tower").exists(),
        "a skill no longer named must not linger"
    );
}

/// The plugin's `skills/` is wholly fufu's, so a rerun sweeps what an
/// extension removed from the registry left behind — the Claude half of
/// the gap the Codex test below keeps.
#[test]
fn claude_drops_a_removed_extensions_skills_on_rerun() {
    let (home, bin) = machine();
    tower(home.path(), bin.path());
    assert!(
        ff(home.path(), Some(bin.path()), &["hook", "claude"])
            .status
            .success()
    );
    let root = claude_skills(home.path());
    assert!(root.join("tower").exists());

    undeclare_all(home.path());
    assert!(
        ff(home.path(), Some(bin.path()), &["hook", "claude"])
            .status
            .success()
    );
    let mut left: Vec<String> = std::fs::read_dir(&root)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    left.sort();
    assert_eq!(left, ["fufu"]);
}

#[test]
fn unhook_claude_removes_the_extensions_skills_with_the_plugin() {
    let (home, bin) = machine();
    tower(home.path(), bin.path());
    assert!(
        ff(home.path(), Some(bin.path()), &["hook", "claude"])
            .status
            .success()
    );

    let out = ff(home.path(), Some(bin.path()), &["unhook", "claude"]);
    assert!(out.status.success(), "{}", stdout(&out));
    assert!(!home.path().join(".claude/skills/fufu").exists());
}

/// Removal goes by the names on record and runs no handshake: the stub is
/// gone from the bin directory by the time `unhook` runs, and the
/// directories still go.
#[test]
fn unhook_codex_removes_a_still_declared_extensions_skills() {
    let (home, bin) = machine();
    tower(home.path(), bin.path());
    assert!(
        ff(home.path(), Some(bin.path()), &["hook", "codex"])
            .status
            .success()
    );
    let root = codex_skills(home.path());
    assert!(root.join("tower").exists());
    assert!(root.join("tower-plan").exists());

    std::fs::remove_file(bin.path().join("ff-tower")).unwrap();
    let out = ff(home.path(), Some(bin.path()), &["unhook", "codex"]);
    assert!(out.status.success(), "{}", stdout(&out));
    assert!(!root.join("tower").exists());
    assert!(!root.join("tower-plan").exists());
    assert!(!root.join("fufu").exists());
}

/// An extension taken back with `ff extension remove` before `ff unhook
/// codex` runs leaves its directories behind: the removal loop walks the
/// registry, and there is nothing left in it naming that extension's
/// skills. A documented limitation, not a bug — Codex's `skills/` is not a
/// directory fufu owns outright the way Claude's plugin is, so nothing here
/// prunes it by scanning.
#[test]
fn unhook_codex_leaves_an_undeclared_extensions_skills_behind() {
    let (home, bin) = machine();
    tower(home.path(), bin.path());
    assert!(
        ff(home.path(), Some(bin.path()), &["hook", "codex"])
            .status
            .success()
    );
    let root = codex_skills(home.path());
    assert!(root.join("tower-plan").exists());

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
    assert!(
        root.join("tower-plan").exists(),
        "orphaned rather than removed — a known gap"
    );
    assert!(!root.join("fufu").exists());
}

// ---- a skill that does not come back never breaks the install -------------

/// The manifest names a skill the binary has no answer for: the install
/// succeeds, the other skill lands, and the line says which was left out.
#[test]
fn a_skill_the_binary_does_not_produce_is_left_out_and_said() {
    let (home, bin) = machine();
    ext_bin(bin.path(), "tower", Stub::Answers);
    skill_answer(
        bin.path(),
        "tower",
        "tower",
        &[("SKILL.md", &skill_md("tower"))],
    );
    declare(home.path(), bin.path(), "tower", &["tower", "tower-loop"]);

    for (slug, root) in [
        ("claude", claude_skills(home.path())),
        ("codex", codex_skills(home.path())),
    ] {
        let out = ff(home.path(), Some(bin.path()), &["hook", slug]);
        assert!(out.status.success(), "{slug}: {}", stdout(&out));
        let said = stdout(&out);
        assert!(said.contains("tower skill written to"), "{slug}: {said}");
        assert!(
            said.contains("tower-loop left out: ") && said.contains("tower/skill/unknown"),
            "{slug}: {said}"
        );
        assert!(root.join("tower/SKILL.md").exists(), "{slug}");
        assert!(!root.join("tower-loop").exists(), "{slug}");
        assert!(root.join("fufu/SKILL.md").exists(), "{slug}");
    }
}

/// A binary that prints a banner before its envelope fails every skill's
/// handshake, and the install still lands everything else.
#[test]
fn a_binary_that_answers_badly_leaves_every_skill_out() {
    let (home, bin) = machine();
    ext_bin(bin.path(), "tower", Stub::Banners);
    skill_answer(
        bin.path(),
        "tower",
        "tower",
        &[("SKILL.md", &skill_md("tower"))],
    );
    declare(home.path(), bin.path(), "tower", &["tower"]);

    let out = ff(home.path(), Some(bin.path()), &["hook", "claude"]);
    assert!(out.status.success(), "{}", stdout(&out));
    let said = stdout(&out);
    assert!(!said.contains("tower skill written"), "{said}");
    assert!(said.contains("tower left out: "), "{said}");
    assert!(!claude_skills(home.path()).join("tower").exists());
    assert!(claude_skills(home.path()).join("fufu/SKILL.md").exists());
}

#[test]
fn a_binary_gone_from_path_leaves_every_skill_out() {
    let (home, bin) = machine();
    declare(home.path(), bin.path(), "tower", &["tower", "tower-plan"]);

    for (slug, root) in [
        ("claude", claude_skills(home.path())),
        ("codex", codex_skills(home.path())),
    ] {
        let out = ff(home.path(), Some(bin.path()), &["hook", slug]);
        assert!(out.status.success(), "{slug}: {}", stdout(&out));
        let said = stdout(&out);
        assert!(
            said.contains("tower left out: ff-tower is not on PATH"),
            "{slug}: {said}"
        );
        assert!(
            said.contains("tower-plan left out: ff-tower is not on PATH"),
            "{slug}: {said}"
        );
        assert!(!root.join("tower").exists(), "{slug}");
        assert!(!root.join("tower-plan").exists(), "{slug}");
        assert!(root.join("fufu/SKILL.md").exists(), "{slug}");
    }
}
