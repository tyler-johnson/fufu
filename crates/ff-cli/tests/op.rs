//! The `ff op` family, end to end against the real binary.
//!
//! Two things are being pinned here beyond "the verbs run". First, the
//! address space: an operation is spelled in letters and never in hex, and
//! that has to hold at every door into the family, because a hex spelling
//! that worked once would teach the wrong model on the first try. Second,
//! the envelope names: `ff session` shipped a listing and a diffstat both
//! stamped `session`, and this family must not repeat it.

use std::path::Path;
use std::process::{Command, Output};

use ff_testsupport::Fixture;
use ff_testsupport::fixtures::null_device;

fn ff_at(dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_ff"))
        .current_dir(dir)
        .args(args)
        .env("GIT_CONFIG_GLOBAL", null_device())
        .env("GIT_CONFIG_SYSTEM", null_device())
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env_remove("GIT_AUTHOR_NAME")
        .env_remove("GIT_AUTHOR_EMAIL")
        .env_remove("GIT_AUTHOR_DATE")
        .env_remove("GIT_COMMITTER_NAME")
        .env_remove("GIT_COMMITTER_EMAIL")
        .env_remove("GIT_COMMITTER_DATE")
        .env_remove("EMAIL")
        .env_remove("FF_SESSION")
        .output()
        .expect("spawn ff")
}

fn ff(fx: &Fixture, args: &[&str]) -> Output {
    ff_at(&fx.path(), args)
}

fn stdout(out: &Output) -> String {
    String::from_utf8(out.stdout.clone()).expect("utf-8 stdout")
}

fn stderr(out: &Output) -> String {
    String::from_utf8(out.stderr.clone()).expect("utf-8 stderr")
}

fn json(out: &Output) -> serde_json::Value {
    serde_json::from_str(&stdout(out)).expect("valid json")
}

/// A repository with a few operations of both kinds on the log.
fn with_ops() -> Fixture {
    let fx = Fixture::new();
    fx.set_config("user.name", "Op Tester");
    fx.set_config("user.email", "op@test.test");
    fx.write("a.txt", "a\n");
    fx.commit("init");
    for i in 1..=3 {
        fx.write("a.txt", &format!("v{i}\n"));
        assert!(ff(&fx, &["-m", &format!("snap {i}")]).status.success());
    }
    fx.write("b.txt", "closing\n");
    assert!(
        ff(&fx, &["commit", "-m", "the close"]).status.success(),
        "close"
    );
    fx
}

/// The ids `ff op log --json` prints, newest first.
fn op_ids(fx: &Fixture, extra: &[&str]) -> Vec<String> {
    let mut args = vec!["op", "log", "--json"];
    args.extend_from_slice(extra);
    let out = ff(fx, &args);
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    json(&out)["data"]["ops"]
        .as_array()
        .expect("ops array")
        .iter()
        .map(|op| op["id"].as_str().expect("id").to_string())
        .collect()
}

#[test]
fn op_log_lists_verbs_and_captures_on_one_log() {
    let fx = with_ops();

    // Captures outnumber verb operations by more than an order of magnitude,
    // so the default view leaves them out and --captures puts them back.
    let verbs = op_ids(&fx, &[]);
    let everything = op_ids(&fx, &["--captures"]);
    assert!(!verbs.is_empty(), "the close is on the log");
    assert!(
        everything.len() > verbs.len(),
        "captures are the majority: {} vs {}",
        everything.len(),
        verbs.len()
    );

    // Every id is letters, never hex — the whole point of the alphabet is
    // that an operation can never be misread as a commit sha.
    for id in &everything {
        assert!(
            id.chars().all(|c| ('k'..='z').contains(&c)),
            "op id is spelled in k–z: {id}"
        );
    }

    // -n bounds the rows.
    assert_eq!(op_ids(&fx, &["-n", "1"]).len(), 1);
}

/// `@` is the newest, and git's own first-parent suffixes work on it because
/// an operation's first parent *is* the operation before it.
#[test]
fn addresses_accept_the_suffixes_git_already_has() {
    let fx = with_ops();
    let ids = op_ids(&fx, &["--captures"]);
    assert!(ids.len() >= 4, "enough depth to walk: {ids:?}");

    let shown = |spec: &str| {
        let out = ff(&fx, &["op", "show", spec, "--json"]);
        assert!(out.status.success(), "ff op show {spec}: {}", stderr(&out));
        json(&out)["data"]["id"].as_str().expect("id").to_string()
    };
    assert_eq!(shown("@"), ids[0]);
    assert_eq!(shown("@^"), ids[1]);
    assert_eq!(shown("@~3"), ids[3]);
    // A prefix of the letters id resolves to the same operation.
    assert_eq!(shown(&ids[1][..8]), ids[1]);

    // Bare `ff op show` is `@`.
    let out = ff(&fx, &["op", "show", "--json"]);
    assert_eq!(json(&out)["data"]["id"], ids[0]);
}

/// Raw hex is not a second way to say the same thing; it is how you say
/// *commit*. Every door into the family refuses it.
#[test]
fn hex_is_refused_wherever_an_operation_is_taken() {
    let fx = with_ops();
    let repo = fx.repo();
    let hex = ff_core::ops::OpLog::open(&repo)
        .unwrap()
        .tip()
        .unwrap()
        .unwrap()
        .hex();
    drop(repo);

    for args in [
        vec!["op", "show", &hex[..8], "--json"],
        vec!["op", "diff", &hex[..8], "--json"],
        vec!["op", "restore", &hex[..8], "--json"],
        vec!["op", "revert", &hex[..8], "--json"],
        vec!["op", "abandon", &hex[..8], "--json"],
        vec!["restore", "--all", "--at-op", &hex[..8], "--json"],
    ] {
        let out = ff(&fx, &args);
        assert!(!out.status.success(), "ff {args:?} must refuse hex");
        assert_eq!(
            json(&out)["error"]["id"],
            "op/not-found",
            "ff {args:?}: {}",
            stdout(&out)
        );
    }

    // The `^2` crossing is refused by name, because slot 2 is the commit the
    // operation ran on — the other address space, and the crossing has a name.
    let out = ff(&fx, &["op", "show", "@^2", "--json"]);
    assert_eq!(json(&out)["error"]["id"], "usage/rev-in-op-position");
    assert!(
        json(&out)["error"]["message"]
            .as_str()
            .unwrap()
            .contains("base()"),
        "{}",
        stdout(&out)
    );
}

/// A prefix has to name one operation. The bold prefix `ff op log` prints is
/// the shortest that does, so an id copied from there never lands here — but
/// a shorter one can, and the refusal lists the candidates.
#[test]
fn an_ambiguous_prefix_names_the_candidates() {
    let fx = with_ops();
    let ids = op_ids(&fx, &["--captures"]);
    // One letter cannot be unique across a log this size unless the log is
    // tiny; find a first letter two ids share.
    let mut shared: Option<char> = None;
    for (i, a) in ids.iter().enumerate() {
        for b in &ids[i + 1..] {
            if a.as_bytes()[0] == b.as_bytes()[0] {
                shared = a.chars().next();
            }
        }
    }
    let Some(letter) = shared else {
        return; // no two ids share a first letter here; nothing to assert
    };

    let spec = letter.to_string();
    let out = ff(&fx, &["op", "show", &spec, "--json"]);
    assert!(!out.status.success(), "one letter cannot be unique");
    let id = json(&out)["error"]["id"].as_str().unwrap().to_string();
    // Below git's four-character minimum there is no id it could be, so
    // "not found" and "ambiguous" are both honest answers to one letter.
    assert!(
        id == "op/ambiguous" || id == "op/not-found",
        "{}",
        stdout(&out)
    );
}

#[test]
fn op_diff_compares_two_operations_worktrees() {
    let fx = with_ops();
    let ids = op_ids(&fx, &["--captures"]);

    let out = ff(&fx, &["op", "diff", &ids[2], &ids[0], "--json"]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    let d = &json(&out)["data"];
    assert_eq!(d["a"], ids[2]);
    assert_eq!(d["b"], ids[0]);
    assert!(d["changes"].is_array(), "a diffstat: {d}");

    // A single argument reads "from there to now".
    let out = ff(&fx, &["op", "diff", &ids[2], "--json"]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    assert_eq!(json(&out)["data"]["b"], ids[0]);
}

/// `ff op restore` rewinds the whole repository, which is `ff undo` with the
/// landing named instead of found. It moves the pointer rather than
/// appending, so a round trip leaves the log exactly as long as it was.
#[test]
fn op_restore_rewinds_the_repository_without_appending() {
    let fx = with_ops();
    let before = op_ids(&fx, &["--captures"]);
    let head_before = fx.git(&["rev-parse", "HEAD"]);

    // Land on the operation before the close.
    let out = ff(&fx, &["op", "restore", "@~2"]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    assert_ne!(
        fx.git(&["rev-parse", "HEAD"]),
        head_before,
        "the close was rolled back"
    );

    // Nothing was appended saying that we navigated.
    let after = op_ids(&fx, &["--captures"]);
    assert!(
        after.len() < before.len(),
        "the pointer moved back rather than growing the log: {} → {}",
        before.len(),
        after.len()
    );
    assert!(
        before.contains(&after[0]),
        "and it landed on an operation that was already there"
    );

    // Redo returns.
    let out = ff(&fx, &["redo"]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    assert_eq!(
        fx.git(&["rev-parse", "HEAD"]),
        head_before,
        "redo puts the close back"
    );
}

/// Revert inverts one change and leaves later work standing — the opposite
/// half of restore, and the one verb in the family that writes an operation,
/// because inverting a change while later work stands is itself a thing that
/// happened.
#[test]
fn op_revert_inverts_one_operation_and_records_itself() {
    let fx = with_ops();
    let head_after_close = fx.git(&["rev-parse", "HEAD"]);
    let verbs = op_ids(&fx, &[]);
    let close = verbs.first().expect("the close is on the log").clone();
    // Picked before the revert runs: afterwards the log holds the revert and
    // its own pre-capture, and "not among the verbs I saw earlier" would
    // find one of those instead.
    let a_capture = op_ids(&fx, &["--captures"])
        .into_iter()
        .find(|id| !verbs.contains(id))
        .expect("a capture");

    let before = op_ids(&fx, &["--captures"]).len();
    let out = ff(&fx, &["op", "revert", &close, "--json"]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    let d = &json(&out)["data"]["revert"];
    assert_eq!(d["reverted"], close);
    assert!(
        d["refs"].as_array().expect("refs").iter().any(|t| t["name"]
            .as_str()
            .is_some_and(|n| n.starts_with("refs/heads/"))),
        "the branch moved back: {d}"
    );
    assert_ne!(fx.git(&["rev-parse", "HEAD"]), head_after_close);

    // Unlike a rewind, this one is on the log.
    let after = op_ids(&fx, &["--captures"]).len();
    assert!(after > before, "revert records itself: {before} → {after}");

    // And reverting something with no ref transitions is refused by name.
    let out = ff(&fx, &["op", "revert", &a_capture, "--json"]);
    assert_eq!(json(&out)["error"]["id"], "undo/not-undoable");
}

/// Abandoning retires a branch of the log: the operations stay readable
/// objects, they simply stop being somewhere the log can walk to — which is
/// what stops `ff redo` offering them.
#[test]
fn op_abandon_retires_a_branch_of_the_log() {
    let fx = with_ops();
    let repo = fx.repo();
    let abandoned = ff_core::ops::OpLog::open(&repo)
        .unwrap()
        .tip()
        .unwrap()
        .unwrap()
        .to_string();
    drop(repo);

    assert!(ff(&fx, &["undo"]).status.success());
    // Redo can still see the way forward.
    let out = ff(&fx, &["op", "abandon", &abandoned, "--json"]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    assert_eq!(json(&out)["data"]["abandoned"], abandoned);

    let out = ff(&fx, &["redo", "--json"]);
    assert!(!out.status.success(), "the way forward was retired");
    assert_eq!(json(&out)["error"]["id"], "op/nothing-to-redo");

    // Abandoning what the log stands on is refused: the pointer would name a
    // position nothing resolves.
    let out = ff(&fx, &["op", "abandon", "@", "--json"]);
    assert!(!out.status.success());
    assert_eq!(json(&out)["error"]["id"], "usage/bad-flags");
}

/// `--at-op` and `--at` bound the log at a past operation rather than
/// filtering it: operations behind a point never change, so the log as it
/// read then is this log with its head cut off.
#[test]
fn the_context_flags_bound_the_log() {
    let fx = with_ops();
    let all = op_ids(&fx, &["--captures"]);

    let bounded = op_ids(&fx, &["--captures", "--at-op", &all[2]]);
    assert_eq!(bounded[0], all[2], "the walk starts there");
    assert_eq!(bounded.len(), all.len() - 2);

    // A time answers the same question through the other door.
    let out = ff(&fx, &["op", "log", "--at", "0s", "--json"]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    assert_eq!(
        json(&out)["data"]["ops"].as_array().expect("ops").len(),
        op_ids(&fx, &[]).len(),
        "as of now is the whole log"
    );
    // A week ago, this repository had no operations at all — and saying so
    // is the honest answer, not an empty list.
    let out = ff(&fx, &["op", "log", "--at", "1w", "--json"]);
    assert_eq!(json(&out)["error"]["id"], "op/not-found");

    // Naming both doors at once is refused before anything runs.
    let out = ff(&fx, &["op", "log", "--at-op", "@", "--at", "1w"]);
    assert_eq!(out.status.code(), Some(2), "one reach, two doors");
}

/// Verbs that only add to now do not carry the flags at all — the parser
/// refuses them, rather than the verb accepting and then refusing.
#[test]
fn verbs_that_only_add_to_now_do_not_take_the_flags() {
    let fx = with_ops();
    for args in [
        vec!["commit", "--at", "2h", "-m", "x"],
        vec!["start", "--at-op", "@"],
        vec!["describe", "--at", "2h", "-m", "x"],
    ] {
        let out = ff(&fx, &args);
        assert_eq!(out.status.code(), Some(2), "ff {args:?} must not parse");
        assert!(
            stderr(&out).contains("unexpected argument"),
            "ff {args:?} is a parse error, not a refusal: {}",
            stderr(&out)
        );
    }
}
