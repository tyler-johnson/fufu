//! Copilot's real plugin loader against fufu's installed 1.0 plugin and
//! its settings registration. This layer needs no model or credentials;
//! CI pins the client version so a loader change fails here.
//!
//! Needs `copilot` on `PATH`. Skips with a line when it is absent, and
//! fails instead under `FF_LIVE=1`, which is how CI runs it.

mod support;

use std::path::Path;
use std::process::Command;
use std::time::Duration;

use support::{client_env, hook, live_client, repo, run_within, scratch_home, unhook};

fn list(client: &Path, home: &Path, cwd: &Path) -> String {
    let mut command = Command::new(client);
    client_env(&mut command, home, cwd);
    command.args(["plugin", "list"]);
    let out = run_within(&mut command, Duration::from_secs(30));
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stdout)
    );
    String::from_utf8(out.stdout).unwrap()
}

#[test]
fn copilot_loads_the_registered_plugin_live_and_forgets_it_on_unhook() {
    let Some(copilot) = live_client("copilot") else {
        return;
    };
    let fx = repo();
    let home = scratch_home(&fx, ".copilot");
    assert!(!list(&copilot, &home, &fx.path()).contains("fufu@fufu-ff"));
    hook(&home, &fx.path(), "copilot");
    let listing = list(&copilot, &home, &fx.path());
    assert!(listing.contains("Live Plugins"), "{listing}");
    let row = listing
        .lines()
        .find(|line| line.contains("fufu@fufu-ff"))
        .unwrap_or_else(|| panic!("plugin not loaded: {listing}"));
    assert!(row.contains("(enabled)"), "{row}");
    assert!(
        listing.contains(
            &home
                .join(".agents")
                .join("plugins")
                .join("copilot")
                .display()
                .to_string()
        ),
        "{listing}"
    );

    unhook(&home, &fx.path(), "copilot");
    let listing = list(&copilot, &home, &fx.path());
    assert!(!listing.contains("fufu@fufu-ff"), "{listing}");
}
