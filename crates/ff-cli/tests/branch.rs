//! The `ff branch` family, end to end against the real binary.
//!
//! What is being pinned is a split, not a rename. `ff branch` used to mean
//! three things at once — list, claim, delete — and the claim has moved to
//! `ff describe -b`, where naming a branch sits on the same axis as naming
//! a change. So the tests come in pairs: the family keeps only the
//! bookkeeping and answers the retired spelling by name, while describe
//! takes both halves of naming, petname and chosen name alike.

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
        .env_remove("GIT_COMMITTER_NAME")
        .env_remove("GIT_COMMITTER_EMAIL")
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

fn repo() -> Fixture {
    let fx = Fixture::new();
    fx.set_config("user.name", "Branch Tester");
    fx.set_config("user.email", "branch@test.test");
    fx.write("a.txt", "a\n");
    fx.commit("init");
    fx
}

/// The name of the current branch, straight from git.
fn current(fx: &Fixture) -> String {
    fx.git(&["symbolic-ref", "--short", "HEAD"]).trim().into()
}

/// Bare `ff branch` is the list — the one spelling kept from before the
/// split, because a family whose read is its default is how `git branch`
/// already reads and nothing about the split argues with that.
#[test]
fn bare_branch_is_the_list() {
    let fx = repo();
    fx.git(&["branch", "other"]);

    let bare = ff(&fx, &["branch"]);
    let spelled = ff(&fx, &["branch", "list"]);
    assert!(bare.status.success(), "stderr: {}", stderr(&bare));
    assert!(spelled.status.success(), "stderr: {}", stderr(&spelled));
    assert_eq!(stdout(&bare), stdout(&spelled), "one shape, two spellings");
    assert!(stdout(&bare).contains("other"));
}

/// One payload, one envelope name. The family names the full path the way
/// `ff op` does, and bare `ff branch` names the shape it emits rather than
/// the family — a listing and a deletion under one `branch` label is the
/// thing `ff session` did that the op family was built to avoid.
#[test]
fn envelope_names_the_full_path() {
    let fx = repo();
    fx.git(&["branch", "doomed"]);

    for spelling in [&["branch", "--json"][..], &["branch", "list", "--json"][..]] {
        let out = ff(&fx, spelling);
        assert!(out.status.success(), "stderr: {}", stderr(&out));
        assert_eq!(json(&out)["cmd"], "branch list", "{spelling:?}");
    }

    let out = ff(&fx, &["branch", "delete", "doomed", "--json"]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    assert_eq!(json(&out)["cmd"], "branch delete");
}

/// The retired claim is answered by name. A word that is not a subcommand
/// is nearly always the old `ff branch <name>`, so the family says where
/// naming went instead of letting the parser call it an unexpected
/// argument — which would teach that the act is gone rather than moved.
#[test]
fn the_retired_claim_redirects_to_describe() {
    let fx = repo();
    let out = ff(&fx, &["branch", "unicode-cleanup"]);
    assert_eq!(out.status.code(), Some(2), "a usage error");
    let text = stderr(&out);
    assert!(text.contains("ff describe -b"), "{text}");
    assert!(
        text.contains("unicode-cleanup"),
        "the name is carried: {text}"
    );
    assert_eq!(current(&fx), "main", "and nothing was renamed");
}

/// Delete is the family's one mutation, and it is undoable — the branch's
/// pointer into the log moves to trash rather than evaporating, so there is
/// no merged-check to argue with.
#[test]
fn delete_removes_a_branch_and_undo_puts_it_back() {
    let fx = repo();
    fx.git(&["branch", "old-experiment"]);

    let out = ff(&fx, &["branch", "delete", "old-experiment"]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    assert!(!stdout(&ff(&fx, &["branch"])).contains("old-experiment"));

    let out = ff(&fx, &["undo"]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    assert!(stdout(&ff(&fx, &["branch"])).contains("old-experiment"));
}

/// `ff describe -b` takes both halves of naming. An anonymous branch has no
/// name worth keeping, so taking one is a claim; a chosen name being
/// replaced is a rename. The difference is in the wording only — there is
/// no discipline separating them, which is the whole point of moving the
/// act here.
#[test]
fn describe_b_names_anonymous_and_named_branches_alike() {
    let fx = repo();
    assert!(
        ff(&fx, &["start"]).status.success(),
        "mint an anonymous one"
    );
    let minted = current(&fx);
    assert!(minted.starts_with("ff/"), "anonymous: {minted}");

    let out = ff(&fx, &["describe", "-b", "real-work"]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    assert!(stdout(&out).contains("claimed"), "{}", stdout(&out));
    assert_eq!(current(&fx), "real-work");

    let out = ff(&fx, &["describe", "-b", "renamed-again"]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    assert!(stdout(&out).contains("renamed"), "{}", stdout(&out));
    assert_eq!(current(&fx), "renamed-again");
}

/// Naming carries the pending description across, which is the part a bare
/// `git branch -m` would orphan and the reason the rename is fufu's rather
/// than git's.
#[test]
fn naming_carries_the_pending_description() {
    let fx = repo();
    assert!(ff(&fx, &["start"]).status.success());
    assert!(ff(&fx, &["describe", "-m", "the plan"]).status.success());
    assert!(ff(&fx, &["describe", "-b", "planned"]).status.success());

    let out = ff(&fx, &["branch", "--json"]);
    let named = json(&out)["data"]["named"].clone();
    let row = named
        .as_array()
        .expect("named array")
        .iter()
        .find(|b| b["name"] == "planned")
        .cloned()
        .expect("the named branch");
    assert_eq!(row["pending_description"], "the plan");
}

/// A name someone's work already holds is the one guess worth refusing,
/// and the refusal survived the move.
#[test]
fn naming_refuses_a_name_already_taken() {
    let fx = repo();
    fx.git(&["branch", "taken"]);
    let out = ff(&fx, &["describe", "-b", "taken"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(stderr(&out).contains("already exists"), "{}", stderr(&out));
    assert_eq!(current(&fx), "main");
}

/// A rewrite held on a branch is worth the same notice an unfinished
/// session is — standing work wherever branches are listed — so the listing
/// marks the held branch, and a branch with neither stays unmarked.
#[test]
fn branch_marks_a_held_branch() {
    let fx = repo();
    fx.write("f.txt", "one\n");
    fx.commit("base");
    fx.git(&["switch", "-q", "-c", "feature"]);
    fx.write("f.txt", "two\n");
    fx.commit("f1");
    fx.git(&["switch", "-q", "main"]);
    fx.write("f.txt", "three\n");
    fx.commit("m1");

    // The conflicting restack holds on feature — the branch is not moved.
    let held = ff(&fx, &["restack", "feature"]);
    assert_eq!(held.status.code(), Some(3), "{}", stderr(&held));

    let v = json(&ff(&fx, &["branch", "--json"]));
    let named = v["data"]["named"].as_array().expect("named array");
    let feature = named
        .iter()
        .find(|b| b["name"] == "feature")
        .expect("the feature row");
    let main = named
        .iter()
        .find(|b| b["name"] == "main")
        .expect("the main row");
    assert_eq!(feature["held"], serde_json::Value::Bool(true));
    assert_eq!(feature["resolving"], serde_json::Value::Bool(false));
    assert_eq!(main["held"], serde_json::Value::Bool(false));
    assert_eq!(main["resolving"], serde_json::Value::Bool(false));

    let text = stdout(&ff(&fx, &["branch"]));
    let lines: Vec<&str> = text.lines().collect();
    /// The note line under a row, when the row has one — rows carry a sigil
    /// and the bracketed name, so the bracketed name is what identifies them.
    fn note_after(lines: &[&str], name: &str) -> Option<String> {
        let i = lines
            .iter()
            .position(|l| l.contains(&format!("[{name}]")))?;
        lines
            .get(i + 1)
            .filter(|l| l.starts_with("    "))
            .map(|l| l.to_string())
    }
    let feature_note = note_after(&lines, "feature").unwrap_or_default();
    assert!(
        feature_note.contains("held"),
        "the held branch's row is marked: {text}"
    );
    let main_note = note_after(&lines, "main").unwrap_or_default();
    assert!(
        !main_note.contains("held"),
        "the clean branch's row is not: {text}"
    );
}

/// `--at-op` is declared on the read, and declared on both spellings of it:
/// before the subcommand and after. Either way it is the coded refusal
/// naming the follow-up, not an unknown argument.
#[test]
fn at_op_is_takeable_on_either_side_of_the_subcommand() {
    let fx = repo();
    for spelling in [
        &["branch", "--at-op", "@"][..],
        &["branch", "list", "--at-op", "@"][..],
        &["branch", "--at", "2h"][..],
        &["branch", "list", "--at", "2h"][..],
    ] {
        let out = ff(&fx, spelling);
        let text = stderr(&out);
        assert!(
            !text.contains("unexpected argument"),
            "{spelling:?} met the parser: {text}"
        );
        assert!(
            text.contains("does not read a past state yet"),
            "{spelling:?}: {text}"
        );
    }
}
