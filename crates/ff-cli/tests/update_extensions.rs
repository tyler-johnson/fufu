//! `ff update` over declared extensions: the walk after fufu's own step,
//! by the recipes each manifest carries and the channel each binary sits
//! on.
//!
//! Unix only, for the reason `tests/extension.rs` is: the extensions are
//! shell scripts, and the one move that runs is a shell pipeline.
//!
//! The test binary is never official, so fufu's own step is the source
//! build's `cargo install` line every time, and the assertions below read
//! the block after it. Nothing here reaches the network: the walk checks
//! no release for an extension, and the one install that runs is a fake
//! `curl` on PATH printing the script it would have fetched.

#![cfg(unix)]

use std::fs::Permissions;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use ff_testsupport::userdirs;
use serde_json::Value;
use tempfile::TempDir;

/// Run `ff` with the user roots pinned under `home` and PATH set to `path`
/// alone. Nothing prompts: stdin is not a terminal, so a printed command
/// is the whole answer unless `-y` says otherwise.
fn ff(home: &Path, path: &str, args: &[&str]) -> Output {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_ff"));
    cmd.current_dir(home).args(args);
    userdirs::pin(&mut cmd, home)
        .env_remove("FF_SESSION")
        .env_remove("CLAUDE_CODE_SESSION_ID")
        .env("PATH", path)
        .output()
        .expect("spawn ff")
}

fn stdout(out: &Output) -> String {
    String::from_utf8(out.stdout.clone()).expect("utf-8 stdout")
}

fn stderr(out: &Output) -> String {
    String::from_utf8(out.stderr.clone()).expect("utf-8 stderr")
}

/// A manifest with the `update` block and `build` spelled as given, as
/// JSON fragments; empty strings leave the field out.
fn manifest(name: &str, version: &str, update: &str, build: &str) -> Value {
    let mut manifest = serde_json::json!({
        "name": name,
        "version": version,
        "contract": 1,
        "verbs": [{"name": "board", "read_only": true}],
        "undoable": true,
    });
    if !update.is_empty() {
        manifest["update"] = serde_json::from_str(update).expect("the block is json");
    }
    if !build.is_empty() {
        manifest["build"] = Value::String(build.to_string());
    }
    manifest
}

/// An `ff-<name>` at `dir` that answers the handshake with `manifest`, on
/// one line, and echoes anything else.
fn ext_bin(dir: &Path, name: &str, manifest: &Value) -> PathBuf {
    std::fs::create_dir_all(dir).expect("create bin dir");
    let path = dir.join(format!("ff-{name}"));
    std::fs::write(&path, ext_script(name, manifest)).expect("write script");
    std::fs::set_permissions(&path, Permissions::from_mode(0o755)).expect("chmod script");
    path
}

fn ext_script(name: &str, manifest: &Value) -> String {
    format!(
        r#"#!/bin/sh
if [ "$1" = "--ff-manifest" ]; then
  echo '{{"ff":1,"cmd":"{name} --ff-manifest","data":{manifest}}}'
  exit 0
fi
echo "$@"
"#
    )
}

/// Write the registry this machine reads, recording each manifest at the
/// path its binary sits, in the order given.
fn declare(home: &Path, entries: &[(&Path, &Value)]) {
    let records: Vec<Value> = entries
        .iter()
        .map(|(path, manifest)| {
            serde_json::json!({
                "path": path,
                "declared_at": 1_788_462_398_i64,
                "manifest": manifest,
            })
        })
        .collect();
    let file = userdirs::registry(home);
    std::fs::create_dir_all(file.parent().expect("parent")).expect("create config dir");
    std::fs::write(
        &file,
        serde_json::json!({"ff": 1, "extensions": records}).to_string(),
    )
    .expect("write registry");
}

fn registry(home: &Path) -> Value {
    let body = std::fs::read_to_string(userdirs::registry(home)).expect("a registry");
    serde_json::from_str(&body).expect("the registry is json")
}

/// The part of stdout after fufu's own block: what the walk said about the
/// extensions.
fn after_fufu(said: &str) -> String {
    let at = said
        .find("\n\n")
        .unwrap_or_else(|| panic!("no extension block after fufu's own: {said}"));
    said[at + 2..].to_string()
}

const INSTALL: &str = "https://raw.githubusercontent.com/tyler-johnson/tower/main/install.sh";
const RELEASES: &str = "https://github.com/tyler-johnson/tower/releases/latest";

fn full_block() -> String {
    format!(r#"{{"brew":"tyler-johnson/tap/tower","install":"{INSTALL}","releases":"{RELEASES}"}}"#)
}

/// The walk prints fufu first, unchanged, and then nothing when nothing is
/// declared: a machine with no extensions reads exactly what it read.
#[test]
fn nothing_declared_is_fufu_alone() {
    let home = TempDir::new().expect("home");
    let out = ff(home.path(), "", &["update"]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let said = stdout(&out);
    assert!(said.starts_with("ff was built from source.\n"), "{said}");
    assert!(!said.contains("\n\n"), "no extension block: {said}");
}

/// A binary under a Homebrew prefix gets the brew recipe, printed and not
/// run.
#[test]
fn a_homebrew_binary_gets_the_brew_recipe() {
    let home = TempDir::new().expect("home");
    let bin = home.path().join("opt/homebrew/bin");
    let manifest = manifest("tower", "0.4.1", &full_block(), "");
    let path = ext_bin(&bin, "tower", &manifest);
    declare(home.path(), &[(&path, &manifest)]);

    let out = ff(home.path(), &bin.display().to_string(), &["update"]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let block = after_fufu(&stdout(&out));
    assert_eq!(
        block,
        "ff-tower 0.4.1 was installed with Homebrew.\nupdate with:\n  brew upgrade tyler-johnson/tap/tower\n"
    );
}

/// A binary in the directory its install script places it — `~/.local/bin`
/// when the block does not say — gets the install recipe, and with nobody
/// there to ask, the printed command is the whole answer.
#[test]
fn a_script_binary_gets_the_install_recipe_and_nobody_to_ask_prints_it() {
    let home = TempDir::new().expect("home");
    let bin = home.path().join(".local/bin");
    let manifest = manifest("tower", "0.4.1", &full_block(), "");
    let path = ext_bin(&bin, "tower", &manifest);
    declare(home.path(), &[(&path, &manifest)]);

    let out = ff(home.path(), &bin.display().to_string(), &["update"]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let block = after_fufu(&stdout(&out));
    assert!(
        block
            .starts_with("ff-tower 0.4.1 sits where its install script puts it.\nupdate with:\n  "),
        "{block}"
    );
    assert!(block.contains(&format!("{INSTALL} | sh\n")), "{block}");
    assert!(!block.contains("run it now?"), "nobody to ask: {block}");
    // Nothing moved: the binary and its record stand.
    assert_eq!(
        registry(home.path())["extensions"][0]["manifest"]["version"],
        "0.4.1"
    );
}

/// The block's `bin` names where the script places the binary, and a binary
/// sitting there is on the script channel wherever that is.
#[test]
fn the_blocks_bin_says_where_the_script_channel_is() {
    let home = TempDir::new().expect("home");
    let bin = home.path().join("tools/bin");
    let block = format!(
        r#"{{"install":"{INSTALL}","bin":"{}","releases":"{RELEASES}"}}"#,
        bin.display()
    );
    let manifest = manifest("tower", "0.4.1", &block, "");
    let path = ext_bin(&bin, "tower", &manifest);
    declare(home.path(), &[(&path, &manifest)]);

    let out = ff(home.path(), &bin.display().to_string(), &["update"]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let said = after_fufu(&stdout(&out));
    assert!(
        said.contains("sits where its install script puts it"),
        "{said}"
    );

    // A `~` in `bin` is the home directory.
    let block = format!(r#"{{"install":"{INSTALL}","bin":"~/tools/bin"}}"#);
    let tilde = self::manifest("tower", "0.4.1", &block, "");
    ext_bin(&bin, "tower", &tilde);
    declare(home.path(), &[(&path, &tilde)]);
    let said = after_fufu(&stdout(&ff(
        home.path(),
        &bin.display().to_string(),
        &["update"],
    )));
    assert!(
        said.contains("sits where its install script puts it"),
        "{said}"
    );
}

/// A binary anywhere else — a hand copy, a tool's own directory — gets the
/// releases page: whatever placed it replaces it.
#[test]
fn a_binary_anywhere_else_gets_the_releases_page() {
    let home = TempDir::new().expect("home");
    let bin = home.path().join("hand");
    let manifest = manifest("tower", "0.4.1", &full_block(), "");
    let path = ext_bin(&bin, "tower", &manifest);
    declare(home.path(), &[(&path, &manifest)]);

    let out = ff(home.path(), &bin.display().to_string(), &["update"]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let block = after_fufu(&stdout(&out));
    assert_eq!(
        block,
        format!(
            "ff-tower 0.4.1 was installed by something else — whatever placed this binary replaces it.\nupdate with:\n  {RELEASES}\n"
        )
    );
}

/// A binary at `~/.local/bin` whose block has no install recipe is a hand
/// copy: nothing places binaries there without a script, so it is read as
/// anywhere else.
#[test]
fn the_script_directory_without_an_install_recipe_is_anywhere_else() {
    let home = TempDir::new().expect("home");
    let bin = home.path().join(".local/bin");
    let manifest = manifest(
        "tower",
        "0.4.1",
        &format!(r#"{{"releases":"{RELEASES}"}}"#),
        "",
    );
    let path = ext_bin(&bin, "tower", &manifest);
    declare(home.path(), &[(&path, &manifest)]);

    let said = after_fufu(&stdout(&ff(
        home.path(),
        &bin.display().to_string(),
        &["update"],
    )));
    assert!(said.contains("was installed by something else"), "{said}");
    assert!(said.contains(RELEASES), "{said}");
}

/// A `source` build is named as one to rebuild and nothing else: no
/// recipe, no release check, and the block is not read.
#[test]
fn a_source_build_is_told_to_rebuild_and_nothing_else() {
    let home = TempDir::new().expect("home");
    let bin = home.path().join("opt/homebrew/bin");
    let manifest = manifest("tower", "0.4.1", &full_block(), "source");
    let path = ext_bin(&bin, "tower", &manifest);
    declare(home.path(), &[(&path, &manifest)]);

    let out = ff(home.path(), &bin.display().to_string(), &["update"]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let block = after_fufu(&stdout(&out));
    assert_eq!(
        block,
        "ff-tower 0.4.1 was built from source — rebuild it the way you built it.\n"
    );
}

/// A manifest with no block is one fufu cannot move, named with the path
/// the binary sits at.
#[test]
fn no_block_is_named_as_unmovable_with_its_path() {
    let home = TempDir::new().expect("home");
    let bin = home.path().join("bin");
    let manifest = manifest("tower", "0.4.1", "", "");
    let path = ext_bin(&bin, "tower", &manifest);
    declare(home.path(), &[(&path, &manifest)]);

    let out = ff(home.path(), &bin.display().to_string(), &["update"]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let block = after_fufu(&stdout(&out));
    assert_eq!(
        block,
        format!(
            "ff-tower 0.4.1 is one fufu cannot move: its manifest carries no update recipes.\n  at {}\n",
            path.display()
        )
    );
}

/// A channel the block has no recipe for is named the same way, with the
/// channel and the recipe it lacks.
#[test]
fn a_channel_with_no_recipe_is_named() {
    let home = TempDir::new().expect("home");
    let bin = home.path().join("opt/homebrew/bin");
    let manifest = manifest(
        "tower",
        "0.4.1",
        &format!(r#"{{"releases":"{RELEASES}"}}"#),
        "",
    );
    let path = ext_bin(&bin, "tower", &manifest);
    declare(home.path(), &[(&path, &manifest)]);

    let out = ff(home.path(), &bin.display().to_string(), &["update"]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let block = after_fufu(&stdout(&out));
    assert_eq!(
        block,
        format!(
            "ff-tower 0.4.1 was installed with Homebrew, and its manifest has no brew recipe, so fufu cannot move it.\n  at {}\n",
            path.display()
        )
    );
}

/// A record whose binary has left PATH is said, with the path it was
/// recorded at.
#[test]
fn a_binary_off_path_is_named_with_its_recorded_path() {
    let home = TempDir::new().expect("home");
    let bin = home.path().join("bin");
    let manifest = manifest("tower", "0.4.1", &full_block(), "");
    let path = ext_bin(&bin, "tower", &manifest);
    declare(home.path(), &[(&path, &manifest)]);

    let out = ff(home.path(), "", &["update"]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let block = after_fufu(&stdout(&out));
    assert_eq!(
        block,
        format!(
            "ff-tower 0.4.1 is not on PATH any more.\n  recorded at {}\n",
            path.display()
        )
    );
}

/// The walk is in registry order, one block per extension.
#[test]
fn the_walk_is_in_registry_order() {
    let home = TempDir::new().expect("home");
    let bin = home.path().join("bin");
    let tower = manifest("tower", "0.4.1", &full_block(), "");
    let bay = manifest("bay", "1.0.0", "", "source");
    let tower_path = ext_bin(&bin, "tower", &tower);
    let bay_path = ext_bin(&bin, "bay", &bay);
    declare(home.path(), &[(&bay_path, &bay), (&tower_path, &tower)]);

    let said = stdout(&ff(home.path(), &bin.display().to_string(), &["update"]));
    let bay_at = said.find("ff-bay 1.0.0").expect("bay");
    let tower_at = said.find("ff-tower 0.4.1").expect("tower");
    assert!(bay_at < tower_at, "{said}");
}

/// `-y` is one answer for the whole walk, and what it could not move is
/// named at the end with an exit of 1: fufu's own channel and the
/// extension's alike, and the walk still reached every extension.
#[test]
fn yes_names_what_it_could_not_move_and_exits_one_at_the_end() {
    let home = TempDir::new().expect("home");
    let bin = home.path().join("bin");
    let tower = manifest("tower", "0.4.1", &full_block(), "");
    let bay = manifest("bay", "1.0.0", "", "");
    let tower_path = ext_bin(&bin, "tower", &tower);
    let bay_path = ext_bin(&bin, "bay", &bay);
    declare(home.path(), &[(&tower_path, &tower), (&bay_path, &bay)]);

    let out = ff(home.path(), &bin.display().to_string(), &["update", "-y"]);
    assert_eq!(out.status.code(), Some(1));
    let said = stdout(&out);
    assert!(
        said.contains("ff-tower 0.4.1 was installed by something else"),
        "{said}"
    );
    assert!(
        said.contains("ff-bay 1.0.0 is one fufu cannot move"),
        "{said}"
    );
    let err = stderr(&out);
    assert!(err.contains("ff: not everything moved:"), "{err}");
    assert!(
        err.contains("ff was built from source — update with: cargo install"),
        "{err}"
    );
    assert!(err.contains(&format!("ff-tower 0.4.1 was installed by something else — whatever placed this binary replaces it — update with: {RELEASES}")), "{err}");
    assert!(
        err.contains("ff-bay is one fufu cannot move: its manifest carries no update recipes"),
        "{err}"
    );
}

/// The one move that runs: `-y` on the script channel runs the install
/// recipe, and after it the manifest is re-asked, re-recorded, and
/// `ff hook -u` runs.
///
/// The recipe is `curl -fsSL <url> | sh`, and `curl` here is a script on
/// PATH that prints the installer it would have fetched: one that writes a
/// new `ff-tower` answering a new version over the old.
#[test]
fn yes_runs_the_install_recipe_then_re_records_and_refreshes() {
    let home = TempDir::new().expect("home");
    let bin = home.path().join(".local/bin");
    let old = manifest("tower", "0.4.1", &full_block(), "");
    let path = ext_bin(&bin, "tower", &old);
    declare(home.path(), &[(&path, &old)]);

    let new = manifest("tower", "0.5.0", &full_block(), "");
    let installer = home.path().join("installer.sh");
    std::fs::write(
        &installer,
        format!(
            "#!/bin/sh\ncat > \"$HOME/.local/bin/ff-tower\" <<'FF_NEW'\n{}FF_NEW\necho installed tower 0.5.0\n",
            ext_script("tower", &new)
        ),
    )
    .expect("write installer");
    let curl = bin.join("curl");
    std::fs::write(
        &curl,
        format!(
            "#!/bin/sh\necho \"curl $*\" >> \"$HOME/curl.log\"\ncat {}\n",
            installer.display()
        ),
    )
    .expect("write curl");
    std::fs::set_permissions(&curl, Permissions::from_mode(0o755)).expect("chmod curl");

    // `sh` inside the pipe is found through PATH, so the system's bin
    // directories ride behind the test's own.
    let path_env = format!("{}:/bin:/usr/bin", bin.display());
    let out = ff(home.path(), &path_env, &["update", "-y"]);
    let said = stdout(&out);
    let err = stderr(&out);
    // fufu's own step is the source build, which `-y` cannot move: the exit
    // is 1 for that alone, and the extension moved all the same.
    assert_eq!(out.status.code(), Some(1), "{said}\n{err}");
    assert_eq!(
        err.trim(),
        "ff: ff was built from source — update with: cargo install --git https://github.com/tyler-johnson/fufu ff-cli"
    );

    let log = std::fs::read_to_string(home.path().join("curl.log")).expect("curl ran");
    assert_eq!(log.trim(), format!("curl -fsSL {INSTALL}"));
    assert!(said.contains("installed tower 0.5.0"), "{said}");
    assert!(
        said.contains("re-declared tower 0.5.0 (was 0.4.1)"),
        "{said}"
    );
    assert!(
        said.contains("nothing is wired on this machine"),
        "hook -u ran: {said}"
    );
    assert_eq!(
        registry(home.path())["extensions"][0]["manifest"]["version"],
        "0.5.0"
    );
}

/// An installer that fails is that extension's alone: the walk goes on,
/// the failure is said, and the exit is 1 at the end.
#[test]
fn a_failed_install_is_that_extensions_alone() {
    let home = TempDir::new().expect("home");
    let bin = home.path().join(".local/bin");
    let tower = manifest("tower", "0.4.1", &full_block(), "");
    let tower_path = ext_bin(&bin, "tower", &tower);
    let bay = manifest(
        "bay",
        "1.0.0",
        &format!(r#"{{"releases":"{RELEASES}"}}"#),
        "",
    );
    let bay_bin = home.path().join("hand");
    let bay_path = ext_bin(&bay_bin, "bay", &bay);
    declare(home.path(), &[(&tower_path, &tower), (&bay_path, &bay)]);

    // The pipe's exit is `sh`'s, so the failure has to be the script's own.
    let curl = bin.join("curl");
    std::fs::write(
        &curl,
        "#!/bin/sh\necho \"echo 'no such host' >&2; exit 6\"\n",
    )
    .expect("write curl");
    std::fs::set_permissions(&curl, Permissions::from_mode(0o755)).expect("chmod curl");

    let path_env = format!("{}:{}:/bin:/usr/bin", bin.display(), bay_bin.display());
    let out = ff(home.path(), &path_env, &["update", "-y"]);
    assert_eq!(out.status.code(), Some(1));
    let said = stdout(&out);
    assert!(
        said.contains("ff-bay 1.0.0 was installed by something else"),
        "the walk went on: {said}"
    );
    let err = stderr(&out);
    assert!(err.contains("ff-tower: the installer failed"), "{err}");
    assert!(err.contains("run it yourself: curl -fsSL"), "{err}");
    assert!(!said.contains("re-declared"), "nothing moved: {said}");
    assert_eq!(
        registry(home.path())["extensions"][0]["manifest"]["version"],
        "0.4.1"
    );
}
