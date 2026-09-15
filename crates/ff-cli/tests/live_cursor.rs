//! Cursor's installed plugin loader against fufu's user-local plugin. No
//! account or model is needed; the authenticated session evidence lives
//! beside the loader shim in `fixtures/cursor/`.
//!
//! Needs `cursor-agent` on `PATH`. Skips with a line when it is absent,
//! and fails instead under `FF_LIVE=1`, which is how CI runs it.

mod support;

use std::path::Path;
use std::process::Command;
use std::time::Duration;

use serde_json::Value;
use support::{client_env, hook, live_client, repo, run_within, scratch_home, unhook};

fn load(client: &Path, home: &Path, cwd: &Path) -> Value {
    let client = std::fs::canonicalize(client).expect("resolve the Cursor installer's symlink");
    let root = client.parent().unwrap();
    let bundle = root.join("index.js");
    assert!(
        bundle.is_file(),
        "Cursor's installed bundle is missing: {}",
        bundle.display()
    );
    let mut command = Command::new(root.join(if cfg!(windows) { "node.exe" } else { "node" }));
    client_env(&mut command, home, cwd);
    command
        .arg("-e")
        .arg(include_str!("fixtures/cursor/loader.cjs"))
        // `node -e` puts the first argument at argv[1]; match the file
        // invocation's argv[2].
        .arg("cursor-loader")
        .arg(bundle)
        .arg(home)
        .arg(cwd);
    let out = run_within(&mut command, Duration::from_secs(30));
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stdout)
    );
    serde_json::from_slice(&out.stdout).expect("Cursor loader JSON")
}

#[test]
fn cursor_discovers_the_plugin_hooks_and_skill_and_forgets_them_on_unhook() {
    let Some(cursor) = live_client("cursor-agent") else {
        return;
    };
    let fx = repo();
    let home = scratch_home(&fx, ".cursor");
    assert_eq!(
        load(&cursor, &home, &fx.path())["plugins"],
        serde_json::json!([])
    );
    hook(&home, &fx.path(), "cursor");
    let loaded = load(&cursor, &home, &fx.path());
    assert_eq!(loaded["failures"], serde_json::json!([]), "{loaded}");
    assert_eq!(loaded["sourceUnavailable"], false, "{loaded}");
    let plugins = loaded["plugins"].as_array().unwrap();
    assert_eq!(plugins.len(), 1, "{loaded}");
    let plugin = &plugins[0];
    assert_eq!(plugin["identifier"]["source"], "user-local");
    assert_eq!(plugin["identifier"]["sourceInfo"]["name"], "fufu");
    assert_eq!(
        plugin["installPath"],
        home.join(".cursor")
            .join("plugins")
            .join("local")
            .join("fufu")
            .to_str()
            .unwrap()
    );
    let hooks = &plugin["hooks"]["config"];
    assert_eq!(hooks["version"], 1, "{plugin}");
    assert_eq!(hooks["hooks"].as_object().unwrap().len(), 3);
    for name in ["sessionStart", "preToolUse", "sessionEnd"] {
        let commands = hooks["hooks"][name].as_array().unwrap();
        assert_eq!(commands.len(), 1, "{plugin}");
        assert!(
            commands[0]["command"]
                .as_str()
                .unwrap()
                .ends_with("trigger cursor")
        );
    }
    assert_eq!(
        hooks["hooks"]["preToolUse"][0]["matcher"],
        "Shell|Write|Delete"
    );
    let skills = plugin["skills"].as_array().unwrap();
    assert_eq!(skills.len(), 1, "{plugin}");
    assert!(skills[0].to_string().contains("fufu"), "{plugin}");
    assert!(skills[0].to_string().contains("SKILL.md"), "{plugin}");

    unhook(&home, &fx.path(), "cursor");
    assert_eq!(
        load(&cursor, &home, &fx.path())["plugins"],
        serde_json::json!([])
    );
}
