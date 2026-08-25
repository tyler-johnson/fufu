//! `-C <dir>` (`--cwd`): run the command as if fufu had been started there.
//!
//! The mechanism is a chdir, so what these tests hold it to is not "the verb
//! found another repository" but the whole consequence of moving: a relative
//! path argument reads from the new directory, the passthrough inherits it,
//! and the pre-command capture is taken there rather than where the command
//! was typed.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use ff_testsupport::Fixture;
use ff_testsupport::fixtures::null_device;

fn ff_at(dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_ff"))
        .current_dir(dir)
        .args(args)
        // Hermetic like the fixtures: production discover() reads these.
        .env("GIT_CONFIG_GLOBAL", null_device())
        .env("GIT_CONFIG_SYSTEM", null_device())
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env_remove("FF_SESSION")
        .output()
        .expect("spawn ff")
}

fn stdout(out: &Output) -> String {
    String::from_utf8(out.stdout.clone()).expect("utf-8 stdout")
}

fn stderr(out: &Output) -> String {
    String::from_utf8(out.stderr.clone()).expect("utf-8 stderr")
}

fn out(output: &Output) -> String {
    format!("{}{}", stdout(output), stderr(output))
}

fn ok_at(dir: &Path, args: &[&str]) -> String {
    let output = ff_at(dir, args);
    assert!(
        output.status.success(),
        "ff {args:?} failed: {}",
        out(&output)
    );
    stdout(&output)
}

fn repo() -> Fixture {
    let fx = Fixture::new();
    fx.set_config("user.name", "Cwd Tester");
    fx.set_config("user.email", "cwd@test.test");
    fx.write("a.txt", "a\n");
    fx.commit("init");
    fx
}

/// A linked worktree beside the repository, on a branch of its own.
fn bay(fx: &Fixture, name: &str, branch: &str) -> PathBuf {
    let bay = fx.root().join(name);
    fx.git(&["worktree", "add", "-q", "-b", branch, bay.to_str().unwrap()]);
    bay
}

/// Somewhere that is not a repository at all, which is where the tower case
/// runs from: a supervisor process asking about trees it does not stand in.
fn outside() -> tempfile::TempDir {
    tempfile::TempDir::new().expect("create tempdir")
}

#[test]
fn it_runs_the_verb_against_another_repository() {
    let fx = repo();
    let away = outside();

    // Without it, there is no repository under foot at all.
    let bare = ff_at(away.path(), &["status"]);
    assert!(!bare.status.success(), "stdout: {}", out(&bare));

    let text = ok_at(away.path(), &["-C", fx.path().to_str().unwrap(), "status"]);
    assert!(
        text.contains("main"),
        "status did not name the branch: {text}"
    );
}

#[test]
fn the_long_form_says_the_same_thing() {
    let fx = repo();
    let away = outside();
    let short = ok_at(away.path(), &["-C", fx.path().to_str().unwrap(), "status"]);
    let long = ok_at(
        away.path(),
        &[&format!("--cwd={}", fx.path().display()), "status"],
    );
    assert_eq!(short, long);
}

/// The `--json` half of the same reach, and the shape tower asks for: one
/// process, every bay in the pool, no spawn-from-the-directory dance.
#[test]
fn it_answers_json_from_another_repository() {
    let fx = repo();
    let bay = bay(&fx, "bay", "side");
    let away = outside();

    let text = ok_at(
        away.path(),
        &[
            "-C",
            fx.path().to_str().unwrap(),
            "worktree",
            "list",
            "--json",
        ],
    );
    let value: serde_json::Value = serde_json::from_str(&text).expect("valid json");
    let rows = value["data"]["worktrees"]
        .as_array()
        .expect("worktrees array");
    assert!(
        rows.iter().any(|row| {
            row["path"]
                .as_str()
                .is_some_and(|path| ff_testsupport::paths::is(path, &bay))
        }),
        "the listing did not name the bay: {text}"
    );
}

/// Tower's actual case: two bays on two branches, each addressed by path from
/// a third place, each answering for itself.
#[test]
fn it_reaches_a_linked_worktree() {
    let fx = repo();
    let a = bay(&fx, "bay-a", "branch-a");
    let b = bay(&fx, "bay-b", "branch-b");
    let away = outside();

    let first = ok_at(away.path(), &["-C", a.to_str().unwrap(), "status"]);
    assert!(first.contains("branch-a"), "{first}");
    assert!(!first.contains("branch-b"), "{first}");

    let second = ok_at(away.path(), &["-C", b.to_str().unwrap(), "status"]);
    assert!(second.contains("branch-b"), "{second}");
    assert!(!second.contains("branch-a"), "{second}");
}

/// The chdir semantic, both halves: the flag's own value is read from where
/// the command was typed, and a path argument after it is read from where the
/// command landed.
#[test]
fn relative_paths_resolve_the_way_a_chdir_makes_them() {
    let fx = repo();
    let bay = bay(&fx, "bay", "side");
    std::fs::write(bay.join("only-here.txt"), "bay\n").expect("write bay file");

    // `-C ../bay` from the main worktree — the flag's value against the
    // caller's directory.
    let text = ok_at(&fx.path(), &["-C", "../bay", "status"]);
    assert!(text.contains("side"), "{text}");

    // And `only-here.txt` exists in the bay and nowhere else, so a log that
    // resolved the path against the caller would have refused it.
    ok_at(&fx.path(), &["-C", "../bay", "log", "only-here.txt"]);
    let refused = ff_at(&fx.path(), &["log", "only-here.txt"]);
    assert!(
        !refused.status.success(),
        "the path exists only in the bay: {}",
        out(&refused)
    );
}

/// `global = true`, so the flag rides a verb as happily as it precedes one.
#[test]
fn it_rides_a_verb_as_well_as_preceding_one() {
    let fx = repo();
    let bay = bay(&fx, "bay", "side");
    let away = outside();

    let before = ok_at(away.path(), &["-C", bay.to_str().unwrap(), "status"]);
    let after = ok_at(away.path(), &["status", "-C", bay.to_str().unwrap()]);
    assert_eq!(before, after);
    assert!(before.contains("side"), "{before}");
}

/// git accumulates repeated `-C`, each relative to the last. fufu takes
/// clap's default, which is the one `--session` already has: a second
/// occurrence is refused rather than ranked.
#[test]
fn a_repeated_flag_is_refused() {
    let fx = repo();
    let a = bay(&fx, "bay-a", "branch-a");
    let b = bay(&fx, "bay-b", "branch-b");
    let away = outside();

    let output = ff_at(
        away.path(),
        &[
            "-C",
            a.to_str().unwrap(),
            "-C",
            b.to_str().unwrap(),
            "status",
        ],
    );
    assert!(!output.status.success(), "{}", out(&output));
    assert!(
        stderr(&output).contains("multiple times"),
        "{}",
        stderr(&output)
    );
}

/// A directory that is not there is a usage error with an id behind it, and
/// nothing was captured on the way: the move lands before the pre-command
/// snapshot precisely so a snapshot is never taken against the wrong tree.
#[test]
fn a_missing_directory_is_refused_and_captures_nothing() {
    let fx = repo();
    let before = ok_at(&fx.path(), &["op", "log", "--json"]);

    let missing = fx.root().join("no-such-bay");
    let output = ff_at(&fx.path(), &["-C", missing.to_str().unwrap(), "status"]);
    assert_eq!(output.status.code(), Some(2), "{}", out(&output));
    assert!(
        stderr(&output).contains("-C"),
        "the message does not name the flag: {}",
        stderr(&output)
    );

    let json = ff_at(
        &fx.path(),
        &["-C", missing.to_str().unwrap(), "--json", "status"],
    );
    let value: serde_json::Value = serde_json::from_str(&stdout(&json)).expect("valid json");
    assert_eq!(value["error"]["id"], "usage/no-such-directory");

    assert_eq!(
        before,
        ok_at(&fx.path(), &["op", "log", "--json"]),
        "a refused -C must not leave an operation behind"
    );
}

/// `ff explain` knows the id, which is the registry promise from the user's
/// side rather than from the walk's.
#[test]
fn the_error_id_explains_itself() {
    let fx = repo();
    let text = ok_at(&fx.path(), &["explain", "usage/no-such-directory"]);
    assert!(text.contains("-C"), "{text}");
}

/// The passthrough inherits the chdir, because there is nothing to inherit —
/// the process moved before git was ever spawned.
#[test]
fn the_git_passthrough_inherits_the_move() {
    let fx = repo();
    let bay = bay(&fx, "bay", "side");
    let away = outside();

    let text = ok_at(
        away.path(),
        &[
            "-C",
            bay.to_str().unwrap(),
            "git",
            "rev-parse",
            "--show-toplevel",
        ],
    );
    assert!(
        ff_testsupport::paths::is(text.trim(), &bay),
        "rev-parse answered {text:?}, not the bay"
    );
}
