//! Integration coverage for the extensions floor of `ff doctor`: every
//! `ff-<name>` found on PATH, whether it is declared, and for a declared
//! one whether the binary's manifest still matches what was recorded.
//!
//! Unix only, for the reason `tests/ext.rs` is: the handshake runs a real
//! binary, and a shell script is the smallest one to write.
//!
//! PATH is pinned to the test's own bin directory and nothing else, the
//! landmine `tests/extension.rs` documents: these tests turn on which names
//! are found and which are not, and a machine with a real `ff-tower`
//! installed would answer a walk they expect to come back empty or exact.

#![cfg(unix)]

use std::fs::Permissions;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use ff_testsupport::fixtures::null_device;
use serde_json::Value;
use tempfile::TempDir;

/// The whole environment a declaration and a doctor run read: PATH for the
/// walk, HOME for the config root the registry sits under. See
/// `tests/extension.rs` for why PATH is pinned rather than prepended.
fn ff(home: &Path, bin: Option<&Path>, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_ff"))
        .current_dir(home)
        .args(args)
        .env("HOME", home)
        .env_remove("XDG_CONFIG_HOME")
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

/// The `checks` array of a `ff doctor --json` run.
fn checks(out: &Output) -> Vec<Value> {
    let envelope: Value = serde_json::from_str(stdout(out).trim())
        .unwrap_or_else(|err| panic!("stdout is not one envelope ({err}): {:?}", stdout(out)));
    envelope["data"]["checks"]
        .as_array()
        .expect("checks array")
        .clone()
}

/// The one row named `name`, if doctor printed one.
fn row<'a>(checks: &'a [Value], name: &str) -> Option<&'a Value> {
    checks.iter().find(|c| c["name"] == name)
}

/// An extension's own row is named for it — `row` with a friendlier name at
/// that call site.
fn row_named<'a>(checks: &'a [Value], name: &str) -> Option<&'a Value> {
    row(checks, name)
}

/// A manifest of the smallest shape, under whatever name and version the
/// test needs.
fn manifest(name: &str, version: &str) -> String {
    format!(
        r#"{{"name":"{name}","version":"{version}","contract":1,
            "verbs":[{{"name":"board","read_only":true}}],"undoable":true}}"#
    )
}

/// An `ff-<name>` that answers the handshake with `data`, on one line.
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

/// Nothing declared and nothing on PATH: the extensions floor is silent —
/// an empty net is not news.
#[test]
fn nothing_declared_and_nothing_on_path_is_silent() {
    let (home, bin) = machine();
    let out = ff(home.path(), Some(bin.path()), &["doctor", "--json"]);
    let checks = checks(&out);

    assert!(
        row(&checks, "extensions").is_none(),
        "no aggregate row: {checks:?}"
    );
    assert!(
        !checks.iter().any(|c| c["detail"]
            .as_str()
            .is_some_and(|d| d.contains("ff-") || d.contains("PATH"))),
        "nothing names an extension: {checks:?}"
    );
}

/// An undeclared `ff-<name>` on PATH is git's idiom working as designed:
/// it is named, and it is never a finding.
#[test]
fn an_undeclared_binary_on_path_raises_no_finding() {
    let (home, bin) = machine();
    ext_bin(bin.path(), "bay", &manifest("bay", "0.1.0"));

    let out = ff(home.path(), Some(bin.path()), &["doctor", "--json"]);
    let checks = checks(&out);

    let row = row(&checks, "extensions").expect("an info row naming it");
    assert_eq!(row["level"], "info", "{row}");
    assert!(row["detail"].as_str().unwrap().contains("ff-bay"), "{row}");
    assert!(
        row["detail"]
            .as_str()
            .unwrap()
            .contains("ff extension add <name> declares one"),
        "{row}"
    );
    // And nothing named "bay" itself: an undeclared extension gets the one
    // aggregate row and no row of its own.
    assert!(row_named(&checks, "bay").is_none(), "{checks:?}");
}

/// A declared extension whose binary still answers what was recorded is
/// `ok`, named for the extension.
#[test]
fn a_declared_extension_that_matches_is_ok() {
    let (home, bin) = machine();
    ext_bin(bin.path(), "tower", &manifest("tower", "0.4.1"));
    let added = ff(
        home.path(),
        Some(bin.path()),
        &["extension", "add", "tower"],
    );
    assert!(added.status.success(), "{:?}", stdout(&added));

    let out = ff(home.path(), Some(bin.path()), &["doctor", "--json"]);
    let checks = checks(&out);

    let row = row_named(&checks, "tower").expect("a row for tower");
    assert_eq!(row["level"], "ok", "{row}");
    assert!(row["detail"].as_str().unwrap().contains("0.4.1"), "{row}");
}

/// A declared extension whose binary now answers a different version than
/// what was recorded is a finding, naming both versions.
#[test]
fn a_declared_extension_whose_version_drifted_is_a_finding() {
    let (home, bin) = machine();
    ext_bin(bin.path(), "tower", &manifest("tower", "0.4.1"));
    let added = ff(
        home.path(),
        Some(bin.path()),
        &["extension", "add", "tower"],
    );
    assert!(added.status.success(), "{:?}", stdout(&added));

    // The binary is upgraded without a re-declaration — the registry still
    // holds 0.4.1.
    ext_bin(bin.path(), "tower", &manifest("tower", "0.5.0"));

    let out = ff(home.path(), Some(bin.path()), &["doctor", "--json"]);
    let checks = checks(&out);

    let row = row_named(&checks, "tower").expect("a row for tower");
    assert_eq!(row["level"], "warn", "{row}");
    let detail = row["detail"].as_str().unwrap();
    assert!(detail.contains("0.4.1"), "{detail}");
    assert!(detail.contains("0.5.0"), "{detail}");
    assert!(detail.contains("ff extension add tower"), "{detail}");
}

/// A declared extension whose binary has left PATH is a finding: dispatch
/// is the PATH walk every time, so a record outliving its binary is a
/// promise fufu can no longer keep.
#[test]
fn a_declared_extension_whose_binary_is_gone_is_a_finding() {
    let (home, bin) = machine();
    ext_bin(bin.path(), "tower", &manifest("tower", "0.4.1"));
    let added = ff(
        home.path(),
        Some(bin.path()),
        &["extension", "add", "tower"],
    );
    assert!(added.status.success(), "{:?}", stdout(&added));
    std::fs::remove_file(bin.path().join("ff-tower")).expect("uninstall it");

    let out = ff(home.path(), Some(bin.path()), &["doctor", "--json"]);
    let checks = checks(&out);

    let row = row_named(&checks, "tower").expect("a row for tower");
    assert_eq!(row["level"], "warn", "{row}");
    assert!(
        row["detail"]
            .as_str()
            .unwrap()
            .contains("no ff-tower on PATH any more"),
        "{row}"
    );
    assert!(
        row["detail"]
            .as_str()
            .unwrap()
            .contains("ff extension remove tower"),
        "{row}"
    );
}

/// A registry a person hand-edited into something unreadable is a finding
/// too, and the whole file is refused rather than serving part of it.
#[test]
fn a_corrupt_registry_is_a_finding() {
    let (home, bin) = machine();
    let dir = home.path().join(".config").join("fufu");
    std::fs::create_dir_all(&dir).expect("create the fufu dir");
    std::fs::write(dir.join("extensions.json"), "{ this was hand-edited").expect("write it");

    let out = ff(home.path(), Some(bin.path()), &["doctor", "--json"]);
    let checks = checks(&out);

    let row = row(&checks, "extensions").expect("a row for the registry");
    assert_eq!(row["level"], "warn", "{row}");
    assert!(
        row["detail"]
            .as_str()
            .unwrap()
            .contains("the registry does not read as one"),
        "{row}"
    );
}
