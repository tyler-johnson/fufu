//! Coverage for a declared extension's own MCP server: registered beside
//! fufu's in all four agent clients, taken back by `ff unhook`, and never
//! written over a registration somebody made themselves.
//!
//! Unix only, and PATH is pinned to the test's own directory rather than
//! prepended to the real one, with `XDG_CONFIG_HOME` removed — the
//! landmine `tests/hook_extension_skills.rs` and `tests/extension.rs`
//! document: this machine can carry a real `ff-tower` on PATH and a real
//! declared registry, and either would decide a test's outcome instead of
//! the fixture.

#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use ff_testsupport::fixtures::null_device;
use serde_json::Value;
use tempfile::TempDir;

fn ff(home: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_ff"))
        .current_dir(home)
        .args(args)
        .env("HOME", home)
        .env_remove("XDG_CONFIG_HOME")
        .env_remove("FF_SESSION")
        .env("GIT_CONFIG_GLOBAL", null_device())
        .env("GIT_CONFIG_SYSTEM", null_device())
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("PATH", "")
        .output()
        .expect("spawn ff")
}

fn stdout(out: &Output) -> String {
    String::from_utf8(out.stdout.clone()).expect("utf-8 stdout")
}

fn json_at(path: &Path) -> Value {
    serde_json::from_str(
        &std::fs::read_to_string(path)
            .unwrap_or_else(|err| panic!("read {}: {err}", path.display())),
    )
    .expect("valid JSON")
}

fn text_at(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|err| panic!("read {}: {err}", path.display()))
}

/// The directory `registry::path` reads on this platform.
fn config_dir(home: &Path) -> PathBuf {
    if cfg!(target_os = "macos") {
        home.join("Library").join("Application Support")
    } else {
        home.join(".config")
    }
}

/// Write the registry this machine reads, declaring one extension with the
/// given `mcp` field. Bypasses `ff extension add`'s handshake, the same
/// shortcut `tests/hook_extension_skills.rs` takes: these tests are about
/// what a hook install does with a manifest already on record.
fn declare(home: &Path, name: &str, mcp: Option<Value>) {
    let mut manifest = serde_json::json!({
        "name": name,
        "version": "0.1.0",
        "contract": 1,
        "verbs": [{"name": "go", "read_only": true}],
        "undoable": true,
    });
    if let Some(mcp) = mcp {
        manifest["mcp"] = mcp;
    }
    let record = serde_json::json!({
        "path": format!("/nowhere/ff-{name}"),
        "declared_at": 1_788_462_398_i64,
        "manifest": manifest,
    });
    let file = config_dir(home).join("fufu").join("extensions.json");
    std::fs::create_dir_all(file.parent().expect("parent")).expect("create config dir");
    std::fs::write(
        &file,
        serde_json::json!({ "ff": 1, "extensions": [record] }).to_string(),
    )
    .expect("write registry");
}

/// The server a manifest names, with an environment to carry through.
fn server() -> Value {
    serde_json::json!({
        "command": "ff-tower",
        "args": ["serve", "--mcp"],
        "env": {"TOWER_BOARD": "ff"},
    })
}

/// The entry every JSON client should carry, allowing for the transport
/// the client either spells or does not.
fn assert_towers_entry(entry: &Value) {
    assert_eq!(entry["command"], "ff-tower", "{entry}");
    assert_eq!(entry["args"], serde_json::json!(["serve", "--mcp"]));
    assert_eq!(entry["env"]["TOWER_BOARD"], "ff", "{entry}");
}

#[test]
fn every_client_registers_a_declared_extensions_server_beside_fufus() {
    let home = TempDir::new().unwrap();
    declare(home.path(), "tower", Some(server()));

    for slug in ["claude", "codex", "cursor", "gemini"] {
        let out = ff(home.path(), &["hook", slug]);
        assert!(out.status.success(), "{slug}: {}", stdout(&out));
        assert!(
            stdout(&out).contains("tower's MCP server registered"),
            "{slug}: {}",
            stdout(&out)
        );
    }

    // The three JSON files, each under the bare name the manifest carries.
    for file in [
        ".claude/skills/fufu/.mcp.json",
        ".cursor/mcp.json",
        ".gemini/settings.json",
    ] {
        let v = json_at(&home.path().join(file));
        assert!(v["mcpServers"]["fufu"].is_object(), "{file}: {v}");
        assert_towers_entry(&v["mcpServers"]["tower"]);
    }
    assert_eq!(
        json_at(&home.path().join(".cursor/mcp.json"))["mcpServers"]["tower"]["type"],
        "stdio",
        "cursor spells the transport"
    );
    assert!(
        json_at(&home.path().join(".gemini/settings.json"))["mcpServers"]["tower"]
            .get("type")
            .is_none(),
        "gemini spells none"
    );

    // Codex: one marked block, a table apiece.
    let toml = text_at(&home.path().join(".codex/config.toml"));
    assert!(toml.contains("[mcp_servers.fufu]"), "{toml}");
    assert!(
        toml.contains(
            "[mcp_servers.tower]\ncommand = \"ff-tower\"\nargs = [\"serve\", \"--mcp\"]\n\
             env = { \"TOWER_BOARD\" = \"ff\" }\n"
        ),
        "{toml}"
    );
    assert_eq!(toml.matches("# >>> fufu").count(), 1, "one block: {toml}");

    // Idempotent.
    let before = toml.clone();
    assert!(ff(home.path(), &["hook", "codex"]).status.success());
    assert_eq!(text_at(&home.path().join(".codex/config.toml")), before);

    // Unhook takes both back together.
    for slug in ["claude", "codex", "cursor", "gemini"] {
        assert!(
            ff(home.path(), &["unhook", slug]).status.success(),
            "unhook {slug}"
        );
    }
    assert!(!home.path().join(".claude/skills/fufu").exists());
    for file in [".cursor/mcp.json", ".gemini/settings.json"] {
        let v = json_at(&home.path().join(file));
        assert!(v.get("mcpServers").is_none(), "{file}: {v}");
    }
    assert_eq!(text_at(&home.path().join(".codex/config.toml")), "");
}

/// A manifest that names no server registers none, which is the ordinary
/// case: the file carries fufu's key and nothing else.
#[test]
fn an_extension_naming_no_server_registers_none() {
    let home = TempDir::new().unwrap();
    declare(home.path(), "tower", None);
    assert!(ff(home.path(), &["hook", "cursor"]).status.success());
    let v = json_at(&home.path().join(".cursor/mcp.json"));
    assert_eq!(
        v["mcpServers"].as_object().unwrap().len(),
        1,
        "fufu's own and no other: {v}"
    );
    assert!(v["mcpServers"]["fufu"].is_object(), "{v}");
}

/// A registration written under the extension's own name is reported and
/// left exactly as it was, and fufu's own still lands beside it.
#[test]
fn a_hand_written_extension_registration_is_left_alone() {
    let home = TempDir::new().unwrap();
    declare(home.path(), "tower", Some(server()));

    let cursor = home.path().join(".cursor/mcp.json");
    std::fs::create_dir_all(cursor.parent().unwrap()).unwrap();
    let mine = r#"{"mcpServers":{"tower":{"command":"/my/wrapper.sh","args":["serve"]}}}"#;
    std::fs::write(&cursor, mine).unwrap();

    let out = ff(home.path(), &["hook", "cursor"]);
    assert!(out.status.success(), "{}", stdout(&out));
    assert!(
        stdout(&out).contains("registers tower's MCP server by hand"),
        "{}",
        stdout(&out)
    );
    let v = json_at(&cursor);
    assert_eq!(v["mcpServers"]["tower"]["command"], "/my/wrapper.sh");
    assert!(v["mcpServers"]["fufu"].is_object(), "fufu's own lands: {v}");

    let out = ff(home.path(), &["unhook", "cursor"]);
    assert!(out.status.success());
    assert!(
        stdout(&out).contains("was registered by hand"),
        "{}",
        stdout(&out)
    );
    let v = json_at(&cursor);
    assert_eq!(v["mcpServers"]["tower"]["command"], "/my/wrapper.sh");
    assert!(v["mcpServers"].get("fufu").is_none());

    // The same in Codex's TOML: the hand-written table stays where it is,
    // and fufu's block lands with only fufu's own table in it.
    let codex = home.path().join(".codex/config.toml");
    std::fs::create_dir_all(codex.parent().unwrap()).unwrap();
    let mine = "[mcp_servers.tower]\ncommand = \"/my/wrapper.sh\"\n";
    std::fs::write(&codex, mine).unwrap();
    let out = ff(home.path(), &["hook", "codex"]);
    assert!(out.status.success(), "{}", stdout(&out));
    assert!(
        stdout(&out).contains("registers tower's MCP server by hand"),
        "{}",
        stdout(&out)
    );
    let toml = text_at(&codex);
    assert!(toml.starts_with(mine), "{toml}");
    assert!(toml.contains("[mcp_servers.fufu]"), "{toml}");
    assert_eq!(toml.matches("[mcp_servers.tower]").count(), 1, "{toml}");
    assert!(ff(home.path(), &["unhook", "codex"]).status.success());
    assert_eq!(text_at(&codex), mine);
}

/// An extension taken back with `ff extension remove` is written out of
/// Codex's block by the next install, because the block is fufu's outright
/// and is written whole.
#[test]
fn a_forgotten_extension_leaves_the_codex_block() {
    let home = TempDir::new().unwrap();
    declare(home.path(), "tower", Some(server()));
    assert!(ff(home.path(), &["hook", "codex"]).status.success());
    assert!(text_at(&home.path().join(".codex/config.toml")).contains("[mcp_servers.tower]"));

    let file = config_dir(home.path()).join("fufu").join("extensions.json");
    std::fs::write(
        &file,
        serde_json::json!({"ff": 1, "extensions": []}).to_string(),
    )
    .unwrap();
    assert!(ff(home.path(), &["hook", "codex"]).status.success());
    let toml = text_at(&home.path().join(".codex/config.toml"));
    assert!(!toml.contains("tower"), "{toml}");
    assert!(toml.contains("[mcp_servers.fufu]"), "{toml}");
}
