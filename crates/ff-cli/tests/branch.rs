//! The `ff branch` family, end to end against the real binary.
//!
//! Three shapes under one verb: bare is the list, `<name> [<rev>]` creates
//! a branch and leaves you where you stand, `-d` deletes one. Naming is
//! not here — it moved to `ff describe -b`, where naming a branch sits on
//! the same axis as naming a change — so the tests come in pairs: the
//! family keeps the bookkeeping and the mint, while describe takes both
//! halves of naming, petname and chosen name alike.

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
        .env_remove("CLAUDE_CODE_SESSION_ID")
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

/// Bare `ff branch` is the list, because a family whose read is its
/// default is how `git branch` already reads.
#[test]
fn bare_branch_is_the_list() {
    let fx = repo();
    fx.git(&["branch", "other"]);

    let bare = ff(&fx, &["branch"]);
    assert!(bare.status.success(), "stderr: {}", stderr(&bare));
    assert!(stdout(&bare).contains("other"));
}

/// One payload, one envelope name. The family names the shape it emits
/// rather than the family, the way `ff op` does — a listing and a deletion
/// under one `branch` label is the thing `ff session` did that the op
/// family was built to avoid.
#[test]
fn envelope_names_the_full_path() {
    let fx = repo();
    fx.git(&["branch", "doomed"]);

    let out = ff(&fx, &["branch", "--json"]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    assert_eq!(json(&out)["cmd"], "branch list");

    let out = ff(&fx, &["branch", "-d", "doomed", "--json"]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    assert_eq!(json(&out)["cmd"], "branch delete");
}

/// `ff branch <name>` mints at trunk and leaves you where you stand: the
/// verb that also moves there is `ff start`.
#[test]
fn create_mints_at_trunk_and_stays_put() {
    let fx = repo();
    fx.git(&["branch", "other"]);

    let out = ff(&fx, &["branch", "spike"]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    let text = stdout(&out);
    assert!(text.contains("created spike"), "{text}");
    assert!(text.contains("forked from main"), "{text}");
    assert_eq!(current(&fx), "main", "nothing moved");
    assert_eq!(
        fx.git(&["rev-parse", "spike"]).trim(),
        fx.git(&["rev-parse", "main"]).trim()
    );
}

/// `<rev>` says where: a revision puts the tip on that commit and records
/// no parent; a branch name puts the tip on that branch and records it as
/// the parent, so the list measures the new branch against it.
#[test]
fn create_at_a_revision_and_at_a_branch() {
    let fx = repo();
    fx.write("b.txt", "b\n");
    fx.commit("second");
    fx.git(&["branch", "other"]);
    fx.git(&["checkout", "-q", "other"]);
    fx.write("c.txt", "c\n");
    fx.commit("on other");
    fx.git(&["checkout", "-q", "main"]);

    let out = ff(&fx, &["branch", "a", "main~1", "--json"]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    let v = json(&out);
    assert_eq!(
        fx.git(&["rev-parse", "a"]).trim(),
        fx.git(&["rev-parse", "main~1"]).trim()
    );
    assert_eq!(v["data"]["create"]["parent"], serde_json::Value::Null);
    let forked = v["data"]["create"]["forked_from"]
        .as_str()
        .expect("a short sha");
    assert!(
        forked.len() < 40 && forked.chars().all(|c| c.is_ascii_hexdigit()),
        "{forked}"
    );

    let out = ff(&fx, &["branch", "b", "other", "--json"]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    let v = json(&out);
    assert_eq!(v["data"]["create"]["forked_from"], "other");
    assert_eq!(v["data"]["create"]["parent"], "other");
    assert_eq!(
        fx.git(&["rev-parse", "b"]).trim(),
        fx.git(&["rev-parse", "other"]).trim()
    );

    let list = json(&ff(&fx, &["branch", "--json"]));
    let b = list["data"]["named"]
        .as_array()
        .expect("named")
        .iter()
        .find(|row| row["name"] == "b")
        .expect("b is listed");
    assert_eq!(b["future"]["against"]["name"], "other", "{b}");
    assert_eq!(b["future"]["against"]["role"], "parent", "{b}");
}

/// `@` is the commit under the open change, and the open change stays
/// where it is: the tree is untouched, the branch underfoot is the same.
#[test]
fn create_at_the_open_change() {
    let fx = repo();
    fx.write("a.txt", "dirty\n");

    let out = ff(&fx, &["branch", "here", "@"]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    assert_eq!(
        fx.git(&["rev-parse", "here"]).trim(),
        fx.git(&["rev-parse", "HEAD"]).trim()
    );
    assert!(
        stdout(&out).contains("carried the open change onto here"),
        "{}",
        stdout(&out)
    );
    assert_eq!(
        std::fs::read_to_string(fx.path().join("a.txt")).expect("a.txt"),
        "dirty\n",
        "the working copy is untouched"
    );
    assert_eq!(current(&fx), "main");

    // The copy resumes there; the original resumes here.
    let out = ff(&fx, &["switch", "here"]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    assert_eq!(current(&fx), "here");
    assert_eq!(
        std::fs::read_to_string(fx.path().join("a.txt")).expect("a.txt"),
        "dirty\n",
        "the copy is the open change on the new branch"
    );
    let out = ff(&fx, &["switch", "main"]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    assert_eq!(
        std::fs::read_to_string(fx.path().join("a.txt")).expect("a.txt"),
        "dirty\n",
        "main keeps its own"
    );
}

/// The JSON names the copy.
#[test]
fn create_at_the_open_change_reports_the_copy() {
    let fx = repo();
    fx.write("a.txt", "dirty\n");
    let v = json(&ff(&fx, &["branch", "there", "@", "--json"]));
    let carried = v["data"]["create"]["carried"]
        .as_str()
        .expect("carried is a sha");
    assert_eq!(
        fx.git(&["rev-parse", "refs/fufu/open/there"]).trim(),
        carried,
        "{v}"
    );
    assert_eq!(
        fx.git(&["rev-parse", "refs/fufu/open/main"]).trim(),
        carried,
        "one sha on both branches"
    );
}

/// A taken name is refused by the same id `ff start -b` refuses it with.
#[test]
fn create_refuses_a_taken_name() {
    let fx = repo();
    let out = ff(&fx, &["branch", "main", "--json"]);
    assert!(!out.status.success());
    let v = json(&out);
    assert_eq!(v["error"]["id"], "branch/exists", "{v}");
    let exits = v["error"]["exits"].as_array().expect("exits");
    assert!(exits.iter().any(|e| e == "ff branch"), "{v}");
}

/// One operation, so one `ff undo` takes it back — and the log names the
/// verb that was typed.
#[test]
fn create_is_one_operation_and_undo_takes_it_back() {
    let fx = repo();
    let out = ff(&fx, &["branch", "spike"]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));

    let ops = json(&ff(&fx, &["--json", "op", "log", "-n", "0"]));
    let branch_ops: Vec<_> = ops["data"]["ops"]
        .as_array()
        .expect("ops")
        .iter()
        .filter(|op| op["verb"] == "branch")
        .collect();
    assert_eq!(branch_ops.len(), 1, "{ops}");

    let out = ff(&fx, &["undo"]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    let list = json(&ff(&fx, &["branch", "--json"]));
    assert!(
        !list["data"]["named"]
            .as_array()
            .expect("named")
            .iter()
            .any(|row| row["name"] == "spike"),
        "{list}"
    );
}

/// The envelope names the shape: `branch create`, carrying the report and
/// the way back.
#[test]
fn create_envelope_is_branch_create() {
    let fx = repo();
    let out = ff(&fx, &["branch", "spike", "--json"]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    let v = json(&out);
    assert_eq!(v["cmd"], "branch create");
    assert_eq!(v["data"]["create"]["name"], "spike");
    assert_eq!(v["data"]["undo"], "ff undo");
}

/// No subcommand words survive, hidden or visible: the positional is a
/// name, so `ff branch list` is a branch called `list`.
#[test]
fn a_branch_named_list_is_a_branch() {
    let fx = repo();
    let out = ff(&fx, &["branch", "list"]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    assert!(stdout(&ff(&fx, &["branch"])).contains("list"));
    assert_eq!(current(&fx), "main");
}

/// The list's flags belong to the list, and `--shared` to the delete: the
/// parser refuses the crossings rather than the verb.
#[test]
fn the_list_flags_refuse_the_mutators() {
    let fx = repo();
    for spelling in [
        &["branch", "spike", "--all"][..],
        &["branch", "-d", "spike", "--at-op", "@"][..],
        &["branch", "--shared"][..],
    ] {
        let out = ff(&fx, spelling);
        assert_eq!(out.status.code(), Some(2), "{spelling:?}: {}", stderr(&out));
    }
}

/// Delete is the family's one mutation, and it is undoable — the branch's
/// pointer into the log moves to trash rather than evaporating, so there is
/// no merged-check to argue with.
#[test]
fn delete_removes_a_branch_and_undo_puts_it_back() {
    let fx = repo();
    fx.git(&["branch", "old-experiment"]);

    let out = ff(&fx, &["branch", "-d", "old-experiment"]);
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

/// `--at-op` and `--at` are declared on the read, so either is the coded
/// refusal naming the follow-up, not an unknown argument.
#[test]
fn at_op_and_at_reach_the_list_refusal() {
    let fx = repo();
    for spelling in [
        &["branch", "--at-op", "@"][..],
        &["branch", "--at", "2h"][..],
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

/// The remote row keeps the sigil and drops the brackets: the brackets
/// promise a name `ff switch` takes, and `switch` resolves local names
/// only. The local rows keep theirs — the differing spellings on one
/// screen are the whole claim.
#[test]
fn remote_only_branches_are_listed_without_the_brackets() {
    let fx = repo();
    let head = fx.git(&["rev-parse", "HEAD"]);
    let sha = head.trim();
    fx.set_config("remote.origin.url", "file:///nonexistent");
    fx.set_config("remote.origin.fetch", "+refs/heads/*:refs/remotes/origin/*");
    fx.set_config("branch.main.remote", "origin");
    fx.set_config("branch.main.merge", "refs/heads/main");
    fx.git(&["update-ref", "refs/remotes/origin/main", sha]);
    fx.git(&["update-ref", "refs/remotes/origin/spike", sha]);
    fx.git(&["update-ref", "refs/remotes/origin/other", sha]);

    let out = ff(&fx, &["branch"]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    let text = stdout(&out);
    assert!(text.contains("remote only:"), "{text}");
    assert!(text.contains("▸ origin/spike"), "{text}");
    assert!(
        !text.contains("[origin/spike]"),
        "no brackets on a remote name: {text}"
    );
    assert!(
        text.contains("[main]"),
        "the local rows keep their brackets: {text}"
    );
}

#[test]
fn the_remote_section_is_bounded_and_says_what_it_left_out() {
    let fx = repo();
    let head = fx.git(&["rev-parse", "HEAD"]);
    let sha = head.trim();
    fx.set_config("remote.origin.url", "file:///nonexistent");
    fx.set_config("remote.origin.fetch", "+refs/heads/*:refs/remotes/origin/*");
    for i in 1..=12 {
        fx.git(&["update-ref", &format!("refs/remotes/origin/b{i:02}"), sha]);
    }

    let out = ff(&fx, &["branch"]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    let text = stdout(&out);
    let rows = text.lines().filter(|l| l.contains("▸ origin/")).count();
    assert_eq!(rows, 10, "the bound shows ten: {text}");
    assert!(text.contains("~ 2 more"), "{text}");

    let out = ff(&fx, &["branch", "--all"]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    let text = stdout(&out);
    let rows = text.lines().filter(|l| l.contains("▸ origin/")).count();
    assert_eq!(rows, 12, "`--all` unbounds the section: {text}");
    assert!(!text.contains("more"), "nothing was left out: {text}");
}

#[test]
fn json_carries_the_third_bucket_and_the_count() {
    let fx = repo();
    let head = fx.git(&["rev-parse", "HEAD"]);
    let sha = head.trim();
    fx.set_config("remote.origin.url", "file:///nonexistent");
    fx.set_config("remote.origin.fetch", "+refs/heads/*:refs/remotes/origin/*");
    for i in 1..=12 {
        fx.git(&["update-ref", &format!("refs/remotes/origin/b{i:02}"), sha]);
    }

    let out = ff(&fx, &["branch", "--json"]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    let v = json(&out);
    assert_eq!(v["data"]["remote_only"].as_array().map(Vec::len), Some(10));
    assert_eq!(v["data"]["remote_more"], 2);

    let out = ff(&fx, &["branch", "--all", "--json"]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    let v = json(&out);
    assert_eq!(v["data"]["remote_only"].as_array().map(Vec::len), Some(12));
    assert_eq!(v["data"]["remote_more"], 0);
}
