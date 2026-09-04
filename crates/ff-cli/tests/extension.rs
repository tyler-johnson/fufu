//! `ff extension` — declaring an `ff-<name>` on this machine, listing what
//! is declared, and taking a name back off.
//!
//! Unix only, for the reason `tests/ext.rs` is: the handshake runs a real
//! binary, and a shell script is the smallest one to write.

#![cfg(unix)]

use std::fs::Permissions;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use ff_testsupport::userdirs;
use serde_json::Value;
use tempfile::TempDir;

/// The whole environment a declaration reads: PATH for the walk, and the
/// user roots pinned under `home` by `userdirs::pin`, so the registry the
/// binary reads is the one inside the temporary directory the test owns
/// rather than this machine's.
///
/// PATH is the test's own directory and nothing else, where `tests/ext.rs`
/// prepends to the process's. These tests turn on which names resolve and
/// which do not, and a machine that has a real `ff-tower` installed —
/// fufu's own developers, for one — would answer a walk this suite expects
/// to come back empty. `ff` itself is run by absolute path, and the
/// extensions are shell scripts with an absolute shebang, so nothing here
/// needs a PATH of its own.
fn ff(home: &Path, bin: Option<&Path>, args: &[&str]) -> Output {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_ff"));
    cmd.current_dir(home).args(args);
    userdirs::pin(&mut cmd, home)
        .env_remove("FF_SESSION")
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
    String::from_utf8(out.stderr.clone()).expect("utf-8 stderr")
}

/// The envelope a `--json` run printed.
fn envelope(out: &Output) -> Value {
    serde_json::from_str(&stdout(out)).unwrap_or_else(|err| {
        panic!("stdout is not one envelope ({err}): {:?}", stdout(out));
    })
}

fn error_id(out: &Output) -> String {
    envelope(out)["error"]["id"]
        .as_str()
        .unwrap_or_default()
        .to_string()
}

/// A manifest of the smallest shape, under whatever name and version the
/// test needs.
fn manifest(name: &str, version: &str) -> String {
    format!(
        r#"{{"name":"{name}","version":"{version}","contract":1,
            "verbs":[{{"name":"board","read_only":true}},{{"name":"done","read_only":false}}],
            "undoable":true}}"#
    )
}

/// An `ff-<name>` that answers the handshake with `data`, on one line, and
/// runs anything else as a plain echo.
fn ext_bin(dir: &Path, name: &str, data: &str) -> PathBuf {
    let compact = serde_json::to_string(
        &serde_json::from_str::<Value>(data).expect("the manifest is valid json"),
    )
    .expect("compact");
    answering(
        dir,
        name,
        &format!(
            r#"if [ "$1" = "--ff-manifest" ]; then
  echo '{{"ff":1,"cmd":"{name} --ff-manifest","data":{compact}}}'
  exit 0
fi
echo "$@""#
        ),
    )
}

/// An `ff-<name>` whose body is whatever the test wants it to be.
fn answering(dir: &Path, name: &str, body: &str) -> PathBuf {
    let path = dir.join(format!("ff-{name}"));
    std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).expect("write script");
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

/// The registry file as it stands, or `None` when nothing wrote one.
fn registry(home: &Path) -> Option<Value> {
    let body = std::fs::read_to_string(userdirs::registry(home)).ok()?;
    Some(serde_json::from_str(&body).expect("the registry is json"))
}

#[test]
fn declaring_records_the_manifest_the_handshake_read() {
    let (home, bin) = machine();
    ext_bin(bin.path(), "tower", &manifest("tower", "0.4.1"));

    let out = ff(
        home.path(),
        Some(bin.path()),
        &["extension", "add", "tower"],
    );
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    let said = stdout(&out);
    assert!(said.starts_with("declared tower 0.4.1 from "), "{said}");
    assert!(said.contains("its verbs: board, done"), "{said}");
    assert!(said.contains("ff extension remove tower"), "{said}");

    // What landed on disk is the manifest, under the name and the path the
    // walk resolved.
    let file = registry(home.path()).expect("a registry was written");
    assert_eq!(file["ff"], 1);
    let record = &file["extensions"][0];
    assert_eq!(record["manifest"]["name"], "tower");
    assert_eq!(record["manifest"]["version"], "0.4.1");
    assert_eq!(
        record["path"],
        bin.path().join("ff-tower").to_str().unwrap()
    );
    assert!(record["declared_at"].as_i64().expect("a time") > 0);
}

/// The whole manifest goes out under `--json`, unknown fields included: a
/// caller reading the envelope reads what was recorded.
#[test]
fn the_declaration_envelope_carries_the_manifest() {
    let (home, bin) = machine();
    let full = r#"{"name":"tower","version":"0.4.1","contract":1,
        "verbs":[{"name":"board","read_only":true,"summary":"what is filed"}],
        "undoable":true,"briefing":"Work is filed as flights.",
        "skills":["/usr/local/share/tower/skills/tower.md"],
        "events":[{"kind":"SessionStart"}],
        "mcp":{"command":"ff","args":["tower","serve","--mcp"]},
        "colors":{"badge":"amber"}}"#;
    ext_bin(bin.path(), "tower", full);

    let out = ff(
        home.path(),
        Some(bin.path()),
        &["extension", "add", "tower", "--json"],
    );
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    let envelope = envelope(&out);
    assert_eq!(envelope["cmd"], "extension add");
    let manifest = &envelope["data"]["declared"]["manifest"];
    assert_eq!(manifest["name"], "tower");
    assert_eq!(manifest["verbs"][0]["summary"], "what is filed");
    assert_eq!(manifest["events"][0]["kind"], "SessionStart");
    assert_eq!(manifest["colors"]["badge"], "amber");
    // Nothing was replaced: the name was not on the list.
    assert_eq!(envelope["data"]["replaced"], Value::Null);
}

/// Every optional field the manifest carries is one thing declaring just
/// bought, and the human render says which.
#[test]
fn the_declaration_says_what_it_bought() {
    let (home, bin) = machine();
    ext_bin(
        bin.path(),
        "tower",
        r#"{"name":"tower","version":"0.4.1","contract":1,
            "verbs":[{"name":"board","read_only":true}],"undoable":false,
            "briefing":"Work is filed as flights.",
            "skills":["/usr/local/share/tower/skills/tower.md"],
            "events":[{"kind":"SessionStart"},{"kind":"BeforeTool","matcher":"Edit"}],
            "mcp":{"command":"ff","args":["tower","serve"]}}"#,
    );

    let said = stdout(&ff(
        home.path(),
        Some(bin.path()),
        &["extension", "add", "tower"],
    ));
    assert!(said.contains("ff undo does not reach them"), "{said}");
    assert!(said.contains("its briefing line rides fufu's"), "{said}");
    assert!(said.contains("1 skill file,"), "{said}");
    assert!(
        said.contains("it subscribes to SessionStart, BeforeTool"),
        "{said}"
    );
    assert!(said.contains("a server of its own"), "{said}");
}

/// A binary that will not answer the handshake is refused, and nothing is
/// recorded — a half-declared extension is one fufu would describe and
/// could not serve.
#[test]
fn a_binary_that_fails_the_handshake_is_refused_and_records_nothing() {
    for (body, id) in [
        // Exited nonzero, and exited 0 saying something that is not an
        // envelope.
        ("exit 1", "extension/handshake-failed"),
        ("echo hello", "extension/handshake-failed"),
        // An envelope carrying an error in place of the manifest.
        (
            r#"echo '{"ff":1,"cmd":"tower --ff-manifest","error":{"id":"tower/usage/no","message":"no","exits":[]}}'"#,
            "extension/handshake-failed",
        ),
        // A manifest fufu can parse and will not accept.
        (
            r#"echo '{"ff":1,"cmd":"tower --ff-manifest","data":{"name":"tower"}}'"#,
            "extension/bad-manifest",
        ),
    ] {
        let (home, bin) = machine();
        answering(bin.path(), "tower", body);
        let out = ff(
            home.path(),
            Some(bin.path()),
            &["extension", "add", "tower", "--json"],
        );
        assert!(!out.status.success(), "{body}");
        assert_eq!(error_id(&out), id, "{body}");
        assert!(registry(home.path()).is_none(), "{body}");
    }
}

/// The contract is what the handshake exists for: a manifest naming one
/// fufu does not speak is refused before anything is recorded.
#[test]
fn a_contract_this_fufu_does_not_speak_is_refused() {
    let (home, bin) = machine();
    ext_bin(
        bin.path(),
        "tower",
        &manifest("tower", "0.4.1").replace("\"contract\":1", "\"contract\":99"),
    );

    let out = ff(
        home.path(),
        Some(bin.path()),
        &["extension", "add", "tower", "--json"],
    );
    assert_eq!(error_id(&out), "extension/unsupported-contract");
    assert!(registry(home.path()).is_none());
}

/// A manifest claiming another binary's name would put fufu's routing and
/// the binary's own answers under two different names.
#[test]
fn a_manifest_claiming_another_name_is_refused() {
    let (home, bin) = machine();
    ext_bin(bin.path(), "tower", &manifest("bay", "0.4.1"));

    let out = ff(
        home.path(),
        Some(bin.path()),
        &["extension", "add", "tower", "--json"],
    );
    assert_eq!(error_id(&out), "extension/name-mismatch");
    assert!(registry(home.path()).is_none());
}

#[test]
fn a_name_nothing_on_path_answers_to_is_refused() {
    let (home, bin) = machine();
    let out = ff(
        home.path(),
        Some(bin.path()),
        &["extension", "add", "tower", "--json"],
    );
    assert_eq!(error_id(&out), "extension/not-found");
    assert!(registry(home.path()).is_none());
}

/// Declaring the same name again replaces the record and says what it wrote
/// over. Upgrading a binary is not a reordering, so the place in the order
/// is kept.
#[test]
fn re_declaring_replaces_the_record_and_keeps_its_place() {
    let (home, bin) = machine();
    ext_bin(bin.path(), "tower", &manifest("tower", "0.4.1"));
    ext_bin(bin.path(), "bay", &manifest("bay", "0.1.0"));
    ff(
        home.path(),
        Some(bin.path()),
        &["extension", "add", "tower"],
    );
    ff(home.path(), Some(bin.path()), &["extension", "add", "bay"]);

    ext_bin(bin.path(), "tower", &manifest("tower", "0.5.0"));
    let out = ff(
        home.path(),
        Some(bin.path()),
        &["extension", "add", "tower"],
    );
    let said = stdout(&out);
    assert!(said.starts_with("re-declared tower 0.5.0 from "), "{said}");
    assert!(said.contains("(was 0.4.1)"), "{said}");

    let file = registry(home.path()).expect("a registry");
    assert_eq!(file["extensions"][0]["manifest"]["name"], "tower");
    assert_eq!(file["extensions"][0]["manifest"]["version"], "0.5.0");
    assert_eq!(file["extensions"][1]["manifest"]["name"], "bay");
}

#[test]
fn an_empty_registry_says_so_in_both_surfaces() {
    let (home, bin) = machine();

    let out = ff(home.path(), Some(bin.path()), &["extension"]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    let said = stdout(&out);
    assert!(
        said.contains("nothing is declared on this machine"),
        "{said}"
    );
    assert!(said.contains("ff extension add"), "{said}");

    let out = ff(
        home.path(),
        Some(bin.path()),
        &["extension", "list", "--json"],
    );
    let envelope = envelope(&out);
    assert_eq!(envelope["cmd"], "extension list");
    assert_eq!(envelope["data"]["declared"], serde_json::json!([]));
    assert_eq!(envelope["data"]["stale"], serde_json::json!([]));
    assert_eq!(envelope["data"]["unreadable"], Value::Null);
    assert!(
        envelope["data"]["file"]
            .as_str()
            .is_some_and(|file| file.ends_with("fufu/extensions.json")),
        "{envelope}"
    );
}

/// Bare `ff extension` is the list, on the same rule as bare `ff branch`.
#[test]
fn the_listing_names_each_extension_and_its_verbs() {
    let (home, bin) = machine();
    ext_bin(bin.path(), "tower", &manifest("tower", "0.4.1"));
    ext_bin(bin.path(), "bay", &manifest("bay", "0.1.0"));
    ff(
        home.path(),
        Some(bin.path()),
        &["extension", "add", "tower"],
    );
    ff(home.path(), Some(bin.path()), &["extension", "add", "bay"]);

    let bare = stdout(&ff(home.path(), Some(bin.path()), &["extension"]));
    let spelled = stdout(&ff(home.path(), Some(bin.path()), &["extension", "list"]));
    assert_eq!(bare, spelled, "bare ff extension is the list");
    let rows: Vec<&str> = bare.lines().collect();
    assert_eq!(rows.len(), 2, "{bare}");
    assert_eq!(rows[0], "tower  0.4.1  board, done");
    assert_eq!(rows[1], "bay    0.1.0  board, done");

    let envelope = envelope(&ff(
        home.path(),
        Some(bin.path()),
        &["extension", "list", "--json"],
    ));
    let declared = envelope["data"]["declared"]
        .as_array()
        .expect("a declared list");
    assert_eq!(declared.len(), 2);
    assert_eq!(declared[0]["manifest"]["name"], "tower");
    assert_eq!(
        declared[0]["resolved"],
        bin.path().join("ff-tower").to_str().unwrap()
    );
}

/// Dispatch is the PATH walk every time, so a record outliving its binary
/// stays and says so — the row is what `ff doctor` compares against.
#[test]
fn a_record_whose_binary_left_path_is_listed_and_marked() {
    let (home, bin) = machine();
    ext_bin(bin.path(), "tower", &manifest("tower", "0.4.1"));
    ff(
        home.path(),
        Some(bin.path()),
        &["extension", "add", "tower"],
    );
    std::fs::remove_file(bin.path().join("ff-tower")).expect("uninstall it");

    let said = stdout(&ff(home.path(), Some(bin.path()), &["extension", "list"]));
    assert!(said.contains("tower  0.4.1  board, done"), "{said}");
    assert!(said.contains("no ff-tower on PATH any more"), "{said}");

    let envelope = envelope(&ff(
        home.path(),
        Some(bin.path()),
        &["extension", "list", "--json"],
    ));
    assert_eq!(envelope["data"]["declared"][0]["resolved"], Value::Null);
}

#[test]
fn removing_takes_one_name_off_and_leaves_the_rest() {
    let (home, bin) = machine();
    ext_bin(bin.path(), "tower", &manifest("tower", "0.4.1"));
    ext_bin(bin.path(), "bay", &manifest("bay", "0.1.0"));
    ff(
        home.path(),
        Some(bin.path()),
        &["extension", "add", "tower"],
    );
    ff(home.path(), Some(bin.path()), &["extension", "add", "bay"]);

    let out = ff(
        home.path(),
        Some(bin.path()),
        &["extension", "remove", "tower"],
    );
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    let said = stdout(&out);
    assert!(said.starts_with("removed tower\n"), "{said}");
    assert!(said.contains("ff-tower still runs from a shell"), "{said}");

    let file = registry(home.path()).expect("a registry");
    assert_eq!(file["extensions"].as_array().expect("records").len(), 1);
    assert_eq!(file["extensions"][0]["manifest"]["name"], "bay");

    // The binary is untouched: removing is fufu forgetting, not an
    // uninstall.
    let ran = ff(home.path(), Some(bin.path()), &["tower", "board"]);
    assert!(ran.status.success(), "stderr: {}", stderr(&ran));
    assert_eq!(stdout(&ran), "board\n");
}

/// A name that was never declared is refused rather than answered as done:
/// the two are different facts about the machine, and a typo reads as the
/// first.
#[test]
fn removing_a_name_that_was_never_declared_is_refused() {
    let (home, bin) = machine();
    ext_bin(bin.path(), "tower", &manifest("tower", "0.4.1"));
    ff(
        home.path(),
        Some(bin.path()),
        &["extension", "add", "tower"],
    );

    let out = ff(
        home.path(),
        Some(bin.path()),
        &["extension", "remove", "bay", "--json"],
    );
    assert!(!out.status.success());
    assert_eq!(error_id(&out), "extension/not-declared");
    // tower is still on the list: the refusal wrote nothing.
    let file = registry(home.path()).expect("a registry");
    assert_eq!(file["extensions"][0]["manifest"]["name"], "tower");
}

/// A registry a person hand-edited is not one fufu writes over, and it is
/// not one fufu describes anything out of either.
#[test]
fn a_registry_that_does_not_read_as_one_stops_both_surfaces() {
    let (home, bin) = machine();
    let file = userdirs::registry(home.path());
    std::fs::create_dir_all(file.parent().expect("parent")).expect("create the fufu dir");
    std::fs::write(&file, "{ this was hand-edited").expect("write it");

    let out = ff(home.path(), Some(bin.path()), &["extension", "list"]);
    assert!(out.status.success(), "a listing is still a listing");
    assert!(
        stderr(&out).contains("the registry does not read as one"),
        "{}",
        stderr(&out)
    );
    assert!(
        stdout(&out).contains("nothing is declared on this machine"),
        "{}",
        stdout(&out)
    );

    // And a write refuses rather than clobbering whatever a person meant.
    ext_bin(bin.path(), "tower", &manifest("tower", "0.4.1"));
    let out = ff(
        home.path(),
        Some(bin.path()),
        &["extension", "add", "tower", "--json"],
    );
    assert_eq!(error_id(&out), "extension/registry-unreadable");
    assert_eq!(
        std::fs::read_to_string(&file).expect("still there"),
        "{ this was hand-edited"
    );
}

/// A record from a contract this fufu does not speak is described to
/// nobody, kept in the file, and named apart in the listing.
#[test]
fn a_record_from_another_contract_is_listed_apart() {
    let (home, bin) = machine();
    ext_bin(bin.path(), "tower", &manifest("tower", "0.4.1"));
    ff(
        home.path(),
        Some(bin.path()),
        &["extension", "add", "tower"],
    );

    let file = userdirs::registry(home.path());
    let mut written: Value =
        serde_json::from_str(&std::fs::read_to_string(&file).expect("read")).expect("json");
    written["extensions"][0]["manifest"]["contract"] = serde_json::json!(99);
    std::fs::write(&file, written.to_string()).expect("rewrite");

    let said = stdout(&ff(home.path(), Some(bin.path()), &["extension", "list"]));
    assert!(
        said.contains("nothing is declared on this machine"),
        "{said}"
    );
    assert!(
        said.contains("from a contract this fufu does not speak"),
        "{said}"
    );
    assert!(said.contains("tower  contract 99"), "{said}");

    let envelope = envelope(&ff(
        home.path(),
        Some(bin.path()),
        &["extension", "list", "--json"],
    ));
    assert_eq!(envelope["data"]["declared"], serde_json::json!([]));
    assert_eq!(envelope["data"]["stale"][0]["name"], "tower");
    assert_eq!(envelope["data"]["stale"][0]["contract"], 99);
}

/// The family answers outside a repository: the binary is on PATH, and
/// declaring it is a decision about the machine.
#[test]
fn the_family_answers_outside_a_repository() {
    let (home, bin) = machine();
    ext_bin(bin.path(), "tower", &manifest("tower", "0.4.1"));

    for args in [
        vec!["extension", "add", "tower"],
        vec!["extension", "list"],
        vec!["extension", "remove", "tower"],
    ] {
        let out = ff(home.path(), Some(bin.path()), &args);
        assert!(
            out.status.success(),
            "ff {}: {}",
            args.join(" "),
            stderr(&out)
        );
    }
}

// ---- help and explain delegate to a declared extension --------------------

/// [`ext_bin`] with a fallback of the caller's own, for a script that has
/// to behave one way at the handshake and another way when asked something
/// else — `ext_bin` itself always echoes.
fn ext_bin_with_fallback(dir: &Path, name: &str, data: &str, fallback: &str) -> PathBuf {
    let compact = serde_json::to_string(
        &serde_json::from_str::<Value>(data).expect("the manifest is valid json"),
    )
    .expect("compact");
    answering(
        dir,
        name,
        &format!(
            r#"if [ "$1" = "--ff-manifest" ]; then
  echo '{{"ff":1,"cmd":"{name} --ff-manifest","data":{compact}}}'
  exit 0
fi
{fallback}"#
        ),
    )
}

/// `ff help <name>` for a declared extension runs `ff-<name> help` and
/// hands back exactly what it printed.
#[test]
fn help_delegates_to_a_declared_extension() {
    let (home, bin) = machine();
    ext_bin(bin.path(), "tower", &manifest("tower", "0.4.1"));
    ff(
        home.path(),
        Some(bin.path()),
        &["extension", "add", "tower"],
    );

    let out = ff(home.path(), Some(bin.path()), &["help", "tower"]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    // `ext_bin`'s fallback echoes its argv, so this is `ff-tower help`'s
    // one word of argv coming straight back.
    assert_eq!(stdout(&out), "help\n");
}

/// `ff help <name>` for a name nobody declared does not reach the binary,
/// even when one answers to that name on PATH: it gets the same error a
/// name resolving to nothing at all gets, because `ff help` reaching an
/// undeclared extension is exactly what declaring is for.
#[test]
fn help_does_not_reach_an_undeclared_extension() {
    let (home, bin) = machine();
    answering(bin.path(), "tower", r#"echo "reached: $@""#);
    // tower is on PATH and answers, but nothing declared it.

    let declared_name = ff(home.path(), Some(bin.path()), &["help", "tower"]);
    let nothing_on_path = ff(
        home.path(),
        Some(bin.path()),
        &["help", "nothing-answers-to-this-name"],
    );
    assert!(!declared_name.status.success());
    assert!(
        !stdout(&declared_name).contains("reached:"),
        "the binary must not run: {}",
        stdout(&declared_name)
    );
    assert_eq!(
        declared_name.status.code(),
        nothing_on_path.status.code(),
        "an undeclared name gets today's usual error, the same one a name \
         resolving to nothing at all gets"
    );
    assert!(stderr(&declared_name).contains("unrecognized subcommand"));
    assert!(stderr(&nothing_on_path).contains("unrecognized subcommand"));
}

/// A person asked to see a page, so a declared extension whose binary has
/// left PATH since it was declared reports why instead of printing an
/// empty page.
#[test]
fn help_of_an_unresolvable_declared_extension_reports_rather_than_prints_nothing() {
    let (home, bin) = machine();
    ext_bin(bin.path(), "tower", &manifest("tower", "0.4.1"));
    ff(
        home.path(),
        Some(bin.path()),
        &["extension", "add", "tower"],
    );
    std::fs::remove_file(bin.path().join("ff-tower")).expect("uninstall it");

    let out = ff(home.path(), Some(bin.path()), &["help", "tower"]);
    assert!(!out.status.success());
    assert!(stdout(&out).is_empty(), "{}", stdout(&out));
    assert!(stderr(&out).contains("did not answer"), "{}", stderr(&out));
    assert!(stderr(&out).contains("ff doctor"), "{}", stderr(&out));
}

/// A declared extension that answers the handshake and then fails
/// everything else reports the same way a vanished binary does: a person
/// asked for a page, and both look the same from here.
#[test]
fn help_of_a_failing_declared_extension_reports_rather_than_prints_nothing() {
    let (home, bin) = machine();
    ext_bin_with_fallback(bin.path(), "tower", &manifest("tower", "0.4.1"), "exit 1");
    ff(
        home.path(),
        Some(bin.path()),
        &["extension", "add", "tower"],
    );

    let out = ff(home.path(), Some(bin.path()), &["help", "tower"]);
    assert!(!out.status.success());
    assert!(stdout(&out).is_empty(), "{}", stdout(&out));
    assert!(stderr(&out).contains("did not answer"), "{}", stderr(&out));
}

/// `ff explain <name>/<id>` hands the id after the slash to
/// `ff-<name> explain`, `--json` riding along last exactly as it would for
/// any other verb.
#[test]
fn explain_delegates_the_id_to_a_declared_extension() {
    let (home, bin) = machine();
    ext_bin(bin.path(), "tower", &manifest("tower", "0.4.1"));
    ff(
        home.path(),
        Some(bin.path()),
        &["extension", "add", "tower"],
    );

    let out = ff(
        home.path(),
        Some(bin.path()),
        &["explain", "tower/flight/not-found"],
    );
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    assert_eq!(stdout(&out), "explain flight/not-found\n");

    let out = ff(
        home.path(),
        Some(bin.path()),
        &["explain", "tower/flight/not-found", "--json"],
    );
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    assert_eq!(stdout(&out), "explain flight/not-found --json\n");
}

/// A prefix nothing is declared under is not an extension: it falls through
/// to the ordinary lookup and gets the ordinary unknown-id error, the way
/// it always has.
#[test]
fn explain_with_an_undeclared_prefix_falls_through_to_the_ordinary_lookup() {
    let (home, bin) = machine();
    let out = ff(
        home.path(),
        Some(bin.path()),
        &["explain", "tower/flight/not-found", "--json"],
    );
    assert!(!out.status.success());
    assert_eq!(error_id(&out), "usage/unknown-error-id");
}

/// `ff explain` on one of fufu's own ids is unaffected by the delegation
/// path: the registry has nothing declared under `branch`, so the split
/// falls through to the ordinary lookup.
#[test]
fn explain_of_a_builtin_id_is_unchanged() {
    let (home, bin) = machine();
    let out = ff(
        home.path(),
        Some(bin.path()),
        &["explain", "branch/not-found", "--json"],
    );
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    assert_eq!(envelope(&out)["data"]["id"], "branch/not-found");
}

/// A failed delegation is reported rather than handed back as an empty
/// page, the same decision `help` makes.
#[test]
fn explain_of_a_failing_declared_extension_reports_rather_than_prints_nothing() {
    let (home, bin) = machine();
    ext_bin_with_fallback(bin.path(), "tower", &manifest("tower", "0.4.1"), "exit 1");
    ff(
        home.path(),
        Some(bin.path()),
        &["extension", "add", "tower"],
    );

    let out = ff(
        home.path(),
        Some(bin.path()),
        &["explain", "tower/flight/not-found", "--json"],
    );
    assert!(!out.status.success());
    assert_eq!(error_id(&out), "extension/delegate-failed");
}
