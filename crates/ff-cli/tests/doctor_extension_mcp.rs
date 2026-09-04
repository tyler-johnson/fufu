//! Coverage for what `ff doctor` reports about a declared extension's own
//! MCP server: folded into the same row `tests/doctor_extensions.rs` covers
//! for the rest of a declared extension, since a server sits on both the
//! client axis `wiring.rs`'s `mcp` row already reports and the extension
//! axis that row is named for.
//!
//! Unix only, for the reason `tests/doctor_extensions.rs` is: the handshake
//! runs a real binary, and a shell script is the smallest one to write.
//!
//! PATH is pinned to the test's own bin directory and nothing else — the
//! landmine `tests/extension.rs` documents: this machine can carry a real
//! `ff-tower` and a real registry, and either would decide a test's outcome
//! instead of the fixture.

#![cfg(unix)]

use std::fs::Permissions;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use ff_testsupport::fixtures::null_device;
use ff_testsupport::userdirs;
use serde_json::Value;
use tempfile::TempDir;

fn ff(home: &Path, bin: &Path, args: &[&str]) -> Output {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_ff"));
    cmd.current_dir(home).args(args);
    userdirs::pin(&mut cmd, home)
        .env_remove("FF_SESSION")
        .env("GIT_CONFIG_GLOBAL", null_device())
        .env("GIT_CONFIG_SYSTEM", null_device())
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("PATH", bin.display().to_string())
        .output()
        .expect("spawn ff")
}

fn stdout(out: &Output) -> String {
    String::from_utf8(out.stdout.clone()).expect("utf-8 stdout")
}

/// The report's `data` object: `{findings, fixable, checks}`.
fn data(out: &Output) -> Value {
    let envelope: Value = serde_json::from_str(stdout(out).trim())
        .unwrap_or_else(|err| panic!("stdout is not one envelope ({err}): {:?}", stdout(out)));
    envelope["data"].clone()
}

/// The `checks` array of a `ff doctor --json` run.
fn checks(out: &Output) -> Vec<Value> {
    data(out)["checks"]
        .as_array()
        .expect("checks array")
        .clone()
}

/// The one row named `name`, if doctor printed one.
fn row<'a>(checks: &'a [Value], name: &str) -> Option<&'a Value> {
    checks.iter().find(|c| c["name"] == name)
}

/// A manifest naming a server of its own: `ff-<name>`, with the given
/// arguments, under whatever version the test needs.
fn manifest_with_mcp(name: &str, version: &str, args: &[&str]) -> String {
    serde_json::json!({
        "name": name,
        "version": version,
        "contract": 1,
        "verbs": [{"name": "board", "read_only": true}],
        "undoable": true,
        "mcp": {"command": format!("ff-{name}"), "args": args},
    })
    .to_string()
}

/// A manifest naming no server at all — the ordinary case.
fn manifest(name: &str, version: &str) -> String {
    serde_json::json!({
        "name": name,
        "version": version,
        "contract": 1,
        "verbs": [{"name": "board", "read_only": true}],
        "undoable": true,
    })
    .to_string()
}

/// An `ff-<name>` that answers the handshake with `data`, on one line, and
/// otherwise just echoes its arguments the way a real extension's `mcp`
/// server subcommand would be invoked but never is in these tests.
fn ext_bin(dir: &Path, name: &str, data: &str) -> PathBuf {
    let compact = serde_json::to_string(
        &serde_json::from_str::<Value>(data).expect("the manifest is valid json"),
    )
    .expect("compact");
    let path = dir.join(format!("ff-{name}"));
    std::fs::write(
        &path,
        format!(
            "#!/bin/sh\nif [ \"$1\" = \"--ff-manifest\" ]; then\n  echo '{{\"ff\":1,\"cmd\":\"{name} --ff-manifest\",\"data\":{compact}}}'\n  exit 0\nfi\necho \"$@\"\n"
        ),
    )
    .expect("write script");
    std::fs::set_permissions(&path, Permissions::from_mode(0o755)).expect("chmod script");
    path
}

/// A machine: a home with nothing declared, and a directory on PATH to put
/// extensions in.
fn machine() -> (TempDir, TempDir) {
    (
        TempDir::new().expect("create home"),
        TempDir::new().expect("create bin dir"),
    )
}

/// A manifest naming no server says nothing about one at all — the row
/// reads exactly as it does for a plain declared extension.
#[test]
fn a_manifest_naming_no_server_is_silent_about_one() {
    let (home, bin) = machine();
    ext_bin(bin.path(), "tower", &manifest("tower", "0.4.1"));
    assert!(
        ff(home.path(), bin.path(), &["extension", "add", "tower"])
            .status
            .success()
    );

    let out = ff(home.path(), bin.path(), &["doctor", "--json"]);
    let checks = checks(&out);
    let row = row(&checks, "tower").expect("a row for tower");
    assert_eq!(row["level"], "ok", "{row}");
    assert!(
        !row["detail"].as_str().unwrap().contains("MCP"),
        "silent about a server nothing declares: {row}"
    );
}

/// A client whose hook is wired before the extension is declared has no
/// entry for it yet — the same shape a missing fufu server takes — and
/// `ff hook <slug>` is offered, and genuinely repairs it under `--fix`.
#[test]
fn a_missing_registration_for_a_wired_client_is_a_fixable_warning() {
    let (home, bin) = machine();
    assert!(
        ff(home.path(), bin.path(), &["hook", "claude"])
            .status
            .success()
    );
    ext_bin(
        bin.path(),
        "tower",
        &manifest_with_mcp("tower", "0.4.1", &["serve", "--mcp"]),
    );
    assert!(
        ff(home.path(), bin.path(), &["extension", "add", "tower"])
            .status
            .success()
    );

    let out = ff(home.path(), bin.path(), &["doctor", "--json"]);
    let before_fix = data(&out);
    assert_eq!(before_fix["fixable"], 1, "{before_fix}");
    let tower_row =
        row(before_fix["checks"].as_array().unwrap(), "tower").expect("a row for tower");
    assert_eq!(tower_row["level"], "warn", "{tower_row}");
    let detail = tower_row["detail"].as_str().unwrap();
    assert!(
        detail.contains("not registered with claude, whose hook is wired"),
        "{detail}"
    );
    assert!(detail.contains("ff hook claude"), "{detail}");
    assert!(
        detail.contains("repairs)"),
        "names itself fixable: {detail}"
    );

    // `--fix` makes good on the promise: rerunning `ff hook claude` writes
    // the entry the row named.
    assert!(
        ff(home.path(), bin.path(), &["doctor", "--fix", "--json"])
            .status
            .success()
    );
    let mcp_path = home.path().join(".claude/skills/fufu/.mcp.json");
    let v: Value = serde_json::from_str(&std::fs::read_to_string(&mcp_path).unwrap()).unwrap();
    assert_eq!(v["mcpServers"]["tower"]["command"], "ff-tower");
    assert_eq!(
        v["mcpServers"]["tower"]["args"],
        serde_json::json!(["serve", "--mcp"])
    );

    let out = ff(home.path(), bin.path(), &["doctor", "--json"]);
    let after_fix = checks(&out);
    let tower_row = row(&after_fix, "tower").expect("a row for tower");
    assert_eq!(tower_row["level"], "ok", "now registered: {tower_row}");
}

/// An entry that still runs the extension's own binary but with arguments
/// the manifest has since moved past is stale rather than hand-written —
/// and `ff hook <slug>` will not touch it, so `--fix` must not promise it
/// will: this is the recorded cost `ff-93` names, and doctor is the one
/// place it has to say so plainly.
#[test]
fn a_stale_registration_is_a_warning_and_fix_leaves_it_alone() {
    let (home, bin) = machine();
    ext_bin(
        bin.path(),
        "tower",
        &manifest_with_mcp("tower", "0.4.1", &["serve"]),
    );
    assert!(
        ff(home.path(), bin.path(), &["extension", "add", "tower"])
            .status
            .success()
    );
    assert!(
        ff(home.path(), bin.path(), &["hook", "claude"])
            .status
            .success()
    );

    // The manifest's `mcp.args` moves on, same version and contract, and
    // the extension is re-declared — but nobody re-runs `ff hook claude`.
    ext_bin(
        bin.path(),
        "tower",
        &manifest_with_mcp("tower", "0.4.1", &["serve", "--mcp"]),
    );
    assert!(
        ff(home.path(), bin.path(), &["extension", "add", "tower"])
            .status
            .success()
    );

    let out = ff(home.path(), bin.path(), &["doctor", "--json"]);
    let report = data(&out);
    assert_eq!(report["fixable"], 0, "never offered as fixable: {report}");
    let row = row(report["checks"].as_array().unwrap(), "tower").expect("a row for tower");
    assert_eq!(row["level"], "warn", "{row}");
    let detail = row["detail"].as_str().unwrap();
    assert!(detail.contains("older argument list"), "{detail}");
    assert!(detail.contains("will not overwrite"), "{detail}");
    assert!(!detail.contains("repairs)"), "{detail}");

    // `--fix` makes good on *not* promising a rewrite: the stale entry is
    // exactly as it was. Exit stays 1 — the finding is real and unfixed.
    let fixed = ff(home.path(), bin.path(), &["doctor", "--fix", "--json"]);
    assert_eq!(fixed.status.code(), Some(1), "{}", stdout(&fixed));
    let mcp_path = home.path().join(".claude/skills/fufu/.mcp.json");
    let v: Value = serde_json::from_str(&std::fs::read_to_string(&mcp_path).unwrap()).unwrap();
    assert_eq!(
        v["mcpServers"]["tower"]["args"],
        serde_json::json!(["serve"]),
        "still the old argument list: {v}"
    );
}

/// A registration somebody wrote themselves is reported and never a
/// finding — the same rule a hand-written fufu entry gets.
#[test]
fn a_hand_written_entry_is_reported_and_never_a_finding() {
    let (home, bin) = machine();
    ext_bin(
        bin.path(),
        "tower",
        &manifest_with_mcp("tower", "0.4.1", &["serve"]),
    );
    assert!(
        ff(home.path(), bin.path(), &["extension", "add", "tower"])
            .status
            .success()
    );
    let mcp_dir = home.path().join(".claude/skills/fufu");
    std::fs::create_dir_all(&mcp_dir).unwrap();
    std::fs::write(
        mcp_dir.join(".mcp.json"),
        r#"{"mcpServers":{"tower":{"command":"/my/wrapper.sh","args":["serve"]}}}"#,
    )
    .unwrap();
    assert!(
        ff(home.path(), bin.path(), &["hook", "claude"])
            .status
            .success()
    );

    let out = ff(home.path(), bin.path(), &["doctor", "--json"]);
    let checks = checks(&out);
    let row = row(&checks, "tower").expect("a row for tower");
    assert_ne!(row["level"], "warn", "{row}");
    assert!(
        row["detail"]
            .as_str()
            .unwrap()
            .contains("a hand-written entry stands for it in claude"),
        "{row}"
    );
}

/// A registration for a name nothing declares any more — the trace `ff
/// extension remove` leaves behind — is news, aggregated the way an
/// undeclared binary on PATH is, and never named as a row of its own.
#[test]
fn an_orphaned_registration_is_reported_and_never_a_finding() {
    let (home, bin) = machine();
    ext_bin(
        bin.path(),
        "tower",
        &manifest_with_mcp("tower", "0.4.1", &["serve"]),
    );
    assert!(
        ff(home.path(), bin.path(), &["extension", "add", "tower"])
            .status
            .success()
    );
    assert!(
        ff(home.path(), bin.path(), &["hook", "claude"])
            .status
            .success()
    );
    assert!(
        ff(home.path(), bin.path(), &["extension", "remove", "tower"])
            .status
            .success()
    );

    let out = ff(home.path(), bin.path(), &["doctor", "--json"]);
    let checks = checks(&out);
    assert!(row(&checks, "tower").is_none(), "not declared: {checks:?}");
    let row = checks
        .iter()
        .find(|c| {
            c["name"] == "extensions"
                && c["detail"]
                    .as_str()
                    .is_some_and(|d| d.contains("declared by nothing here"))
        })
        .expect("an orphan row");
    assert_eq!(row["level"], "info", "{row}");
    assert!(
        row["detail"].as_str().unwrap().contains("tower (claude)"),
        "{row}"
    );
}
