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

/// The remote row wears the brackets like any other: they promise a name
/// `ff switch` takes, and a branch a remote holds is a target by name.
#[test]
fn remote_only_branches_are_listed_with_the_brackets() {
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
    assert!(
        text.contains("▸ [origin/spike]"),
        "brackets on a remote name: {text}"
    );
    assert!(text.contains("[main]"), "{text}");
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
    let rows = text.lines().filter(|l| l.contains("▸ [origin/")).count();
    assert_eq!(rows, 10, "the bound shows ten: {text}");
    assert!(text.contains("~ 2 more"), "{text}");

    let out = ff(&fx, &["branch", "--all"]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    let text = stdout(&out);
    let rows = text.lines().filter(|l| l.contains("▸ [origin/")).count();
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

// ── --prune ───────────────────────────────────────────────────────────────

/// A clone with `main` pushed and tracking, so a branch pushed from it has a
/// shared copy the bare remote beside it can lose.
fn cloned() -> Fixture {
    let fx = Fixture::new_cloned();
    fx.set_config("user.name", "Branch Tester");
    fx.set_config("user.email", "branch@test.test");
    fx.write("a.txt", "a\n");
    fx.commit("init");
    fx.git(&["push", "-q", "-u", "origin", "main"]);
    fx
}

/// Mint `name` at trunk with one commit of its own and push it through
/// `ff push`, which writes the seen and published records a plain git
/// push would not. Leaves the fixture standing on `main`.
fn publish(fx: &Fixture, name: &str) -> String {
    fx.git(&["switch", "-q", "-c", name, "main"]);
    fx.write(&format!("{name}.txt"), &format!("{name}\n"));
    let tip = fx.commit(name);
    let out = ff(fx, &["push"]);
    assert!(out.status.success(), "push {name}: {}", stderr(&out));
    fx.git(&["switch", "-q", "main"]);
    tip
}

/// The teammate's side of the merge: the forge deleted the branch.
fn remote_delete(fx: &Fixture, name: &str) {
    fx.remote_git(&["branch", "-D", name]);
}

/// The log's row count, after a read that reconciles whatever the git
/// switches in the setup left for fufu to observe, so the next count is
/// the verb's alone.
fn op_log_lines(fx: &Fixture) -> usize {
    let settle = ff(fx, &["status"]);
    assert!(settle.status.success(), "{}", stderr(&settle));
    let out = ff(fx, &["op", "log"]);
    assert!(out.status.success(), "{}", stderr(&out));
    stdout(&out).lines().count()
}

fn ref_exists(fx: &Fixture, name: &str) -> bool {
    fx.try_git(&["rev-parse", "--verify", "-q", name])
        .status
        .success()
}

/// Both branches go in one operation, and one undo brings both back with
/// the config section and the seen record intact, so `ff status` on one
/// still says its copy is gone.
#[test]
fn prune_deletes_every_gone_branch_in_one_operation_and_undo_restores_them() {
    let fx = cloned();
    publish(&fx, "alpha");
    publish(&fx, "beta");
    remote_delete(&fx, "alpha");
    remote_delete(&fx, "beta");
    let before = op_log_lines(&fx);

    let out = ff(&fx, &["branch", "--prune"]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    let text = stdout(&out);
    assert!(
        text.contains("pruned 2 branches whose shared copy is gone: alpha, beta"),
        "{text}"
    );
    assert!(text.contains("undo: ff undo"), "{text}");
    assert!(!ref_exists(&fx, "refs/heads/alpha"));
    assert!(!ref_exists(&fx, "refs/heads/beta"));
    assert!(
        !ref_exists(&fx, "refs/remotes/origin/alpha"),
        "the fetch pruned"
    );
    assert_eq!(op_log_lines(&fx), before + 1, "one operation");
    let log = ff(&fx, &["op", "log"]);
    assert!(
        stdout(&log).contains("prune 2 branch(es) whose shared copy is gone"),
        "{}",
        stdout(&log)
    );

    let undo = ff(&fx, &["undo"]);
    assert!(undo.status.success(), "stderr: {}", stderr(&undo));
    assert!(ref_exists(&fx, "refs/heads/alpha"));
    assert!(ref_exists(&fx, "refs/heads/beta"));
    assert!(ref_exists(&fx, "refs/fufu/seen/alpha"));
    assert_eq!(fx.git(&["config", "branch.alpha.remote"]).trim(), "origin");
    let sw = ff(&fx, &["switch", "alpha"]);
    assert!(sw.status.success(), "stderr: {}", stderr(&sw));
    let status = ff(&fx, &["status"]);
    assert!(
        stdout(&status).contains("remote is gone"),
        "{}",
        stdout(&status)
    );
}

#[test]
fn prune_json_carries_the_pruned_and_the_undo() {
    let fx = cloned();
    publish(&fx, "alpha");
    remote_delete(&fx, "alpha");

    let out = ff(&fx, &["branch", "--prune", "--json"]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    let v = json(&out);
    assert_eq!(v["cmd"], "branch prune");
    assert_eq!(v["data"]["prune"]["pruned"][0]["name"], "alpha");
    assert_eq!(
        v["data"]["prune"]["pruned"][0]["trash_ref"],
        "refs/fufu/trash/alpha"
    );
    assert_eq!(v["data"]["prune"]["fetched"], true);
    assert_eq!(v["data"]["prune"]["dry_run"], false);
    assert_eq!(v["data"]["undo"], "ff undo");
}

/// A commit past what the copy last held is work the deletion did not
/// take: the branch is kept, with the count and the way to take it on
/// purpose, and the others still go.
#[test]
fn prune_keeps_a_branch_ahead_of_its_copy() {
    let fx = cloned();
    publish(&fx, "alpha");
    publish(&fx, "beta");
    fx.git(&["switch", "-q", "alpha"]);
    fx.write("more.txt", "more\n");
    fx.commit("more");
    fx.git(&["switch", "-q", "main"]);
    remote_delete(&fx, "alpha");
    remote_delete(&fx, "beta");

    let out = ff(&fx, &["branch", "--prune"]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    let text = stdout(&out);
    assert!(
        text.contains("kept alpha: 1 commit its copy never held — ff branch -d alpha"),
        "{text}"
    );
    assert!(
        text.contains("pruned 1 branch whose shared copy is gone: beta"),
        "{text}"
    );
    assert!(ref_exists(&fx, "refs/heads/alpha"));
    assert!(!ref_exists(&fx, "refs/heads/beta"));

    let v = json(&ff(&fx, &["undo", "--json"]));
    assert!(v["ok"].as_bool().unwrap_or(true), "{v}");
    let out = ff(&fx, &["branch", "--prune", "--json", "--no-fetch"]);
    let v = json(&out);
    assert_eq!(v["data"]["prune"]["kept"][0]["name"], "alpha");
    assert_eq!(v["data"]["prune"]["kept"][0]["reason"]["kind"], "ahead");
    assert_eq!(v["data"]["prune"]["kept"][0]["reason"]["count"], 1);
}

#[test]
fn prune_keeps_the_branch_underfoot() {
    let fx = cloned();
    publish(&fx, "alpha");
    remote_delete(&fx, "alpha");
    fx.git(&["switch", "-q", "alpha"]);

    let out = ff(&fx, &["branch", "--prune"]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    let text = stdout(&out);
    assert!(text.contains("kept alpha: the branch you are on"), "{text}");
    assert!(!text.contains("pruned"), "{text}");
    assert!(!text.contains("undo: ff undo"), "{text}");
    assert!(ref_exists(&fx, "refs/heads/alpha"));
}

/// `beta` on `alpha`, alpha's copy gone: alpha goes, beta's parent becomes
/// what alpha's was — nothing recorded, so trunk — the report says so
/// inline, and a following pull replays beta onto trunk, which moved
/// meanwhile.
#[test]
fn prune_reaims_the_branch_stacked_on_a_pruned_one() {
    let fx = cloned();
    publish(&fx, "alpha");
    let out = ff(&fx, &["branch", "beta", "alpha"]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    let meta = fx.repo();
    assert_eq!(
        ff_core::branchmeta::read(&meta, "beta")
            .unwrap()
            .parent
            .as_deref(),
        Some("alpha")
    );
    remote_delete(&fx, "alpha");
    // Trunk moves on, so beta has somewhere to go once it sits on it.
    fx.write("m.txt", "m\n");
    fx.commit("m");

    let out = ff(&fx, &["branch", "--prune"]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    let text = stdout(&out);
    assert!(
        text.contains("pruned 1 branch whose shared copy is gone: alpha (beta now sits on main)"),
        "{text}"
    );
    assert_eq!(
        ff_core::branchmeta::read(&fx.repo(), "beta")
            .unwrap()
            .parent,
        None,
        "beta's parent is what alpha's was"
    );
    let v = json(&ff(&fx, &["branch", "--json"]));
    assert!(
        v["data"]["named"]
            .as_array()
            .unwrap()
            .iter()
            .all(|b| b["name"] != "alpha"),
        "{v}"
    );

    let pull = ff(&fx, &["pull", "beta", "--no-fetch"]);
    assert!(pull.status.success(), "stderr: {}", stderr(&pull));
    let text = stdout(&pull);
    assert!(text.contains("beta"), "{text}");
    assert!(
        text.contains("main moved ahead by 1 commit(s)") || text.contains("fast-forwarded to main"),
        "{text}"
    );

    // Undo of the pull, then of the prune: the parent link comes back.
    assert!(ff(&fx, &["undo"]).status.success());
    assert!(ff(&fx, &["undo"]).status.success());
    assert_eq!(
        ff_core::branchmeta::read(&fx.repo(), "beta")
            .unwrap()
            .parent
            .as_deref(),
        Some("alpha")
    );
}

/// A branch minted by `ff switch origin/spike` was never pushed from here,
/// and its seen record is what says the copy once stood.
#[test]
fn prune_takes_a_branch_minted_from_a_remote_and_never_pushed() {
    let fx = cloned();
    publish(&fx, "spike");
    // Forget the local branch entirely, keeping the tracking ref, so the
    // switch mints it fresh from the remote's copy.
    fx.git(&["branch", "-D", "spike"]);
    fx.git(&["update-ref", "-d", "refs/fufu/seen/spike"]);
    fx.git(&["update-ref", "-d", "refs/fufu/published/spike"]);
    let sw = ff(&fx, &["switch", "origin/spike"]);
    assert!(sw.status.success(), "stderr: {}", stderr(&sw));
    assert!(
        ref_exists(&fx, "refs/fufu/seen/spike"),
        "the switch marked it seen"
    );
    assert!(!ref_exists(&fx, "refs/fufu/published/spike"));
    let back = ff(&fx, &["switch", "main"]);
    assert!(back.status.success(), "stderr: {}", stderr(&back));
    remote_delete(&fx, "spike");

    let out = ff(&fx, &["branch", "--prune"]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    assert!(
        stdout(&out).contains("pruned 1 branch whose shared copy is gone: spike"),
        "{}",
        stdout(&out)
    );
}

/// An upstream section, no tracking ref, and no record: the fresh clone's
/// shape, a copy never created. Not gone, not touched, not listed.
#[test]
fn prune_leaves_a_branch_with_no_record_alone() {
    let fx = cloned();
    fx.git(&["branch", "fresh"]);
    fx.set_config("branch.fresh.remote", "origin");
    fx.set_config("branch.fresh.merge", "refs/heads/fresh");

    let out = ff(&fx, &["branch", "--prune"]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    let text = stdout(&out);
    assert!(text.contains("nothing to prune"), "{text}");
    assert!(!text.contains("fresh"), "{text}");
    assert!(ref_exists(&fx, "refs/heads/fresh"));
    let v = json(&ff(&fx, &["branch", "--json"]));
    assert_eq!(v["data"]["gone"], 0);
}

#[test]
fn prune_dry_run_writes_nothing() {
    let fx = cloned();
    publish(&fx, "alpha");
    remote_delete(&fx, "alpha");
    let before = op_log_lines(&fx);

    let out = ff(&fx, &["branch", "--prune", "--dry-run"]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    let text = stdout(&out);
    assert!(
        text.contains("would prune 1 branch whose shared copy is gone: alpha"),
        "{text}"
    );
    assert!(
        text.contains("nothing was written — drop --dry-run to prune"),
        "{text}"
    );
    assert!(ref_exists(&fx, "refs/heads/alpha"));
    assert_eq!(op_log_lines(&fx), before);

    let v = json(&ff(&fx, &["branch", "--prune", "-n", "--json"]));
    assert_eq!(v["data"]["prune"]["dry_run"], true);
    assert_eq!(v["data"]["undo"], serde_json::Value::Null);
}

/// `--no-fetch` prunes from the refs as they stand: a tracking ref deleted
/// by hand is gone, and the remote is never asked.
#[test]
fn prune_no_fetch_reads_the_refs_as_they_stand() {
    let fx = cloned();
    publish(&fx, "alpha");
    // The remote still holds alpha; only the tracking ref goes.
    fx.git(&["update-ref", "-d", "refs/remotes/origin/alpha"]);

    let out = ff(&fx, &["branch", "--prune", "--no-fetch"]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    let text = stdout(&out);
    assert!(!text.contains("fetching from"), "{text}");
    assert!(
        text.contains("pruned 1 branch whose shared copy is gone: alpha"),
        "{text}"
    );
    assert!(!ref_exists(&fx, "refs/heads/alpha"));
    assert!(
        !fx.remote_git(&["rev-parse", "--verify", "-q", "refs/heads/alpha"])
            .trim()
            .is_empty(),
        "the remote's copy was never touched"
    );
    let v = json(&ff(&fx, &["undo", "--json"]));
    assert!(v["ok"].as_bool().unwrap_or(true), "{v}");
    let v = json(&ff(&fx, &["branch", "--prune", "--no-fetch", "--json"]));
    assert_eq!(v["data"]["prune"]["fetched"], false);
}

#[test]
fn the_list_counts_the_gone_and_names_the_way_out() {
    let fx = cloned();
    publish(&fx, "alpha");
    remote_delete(&fx, "alpha");
    fx.git(&["fetch", "-q", "--prune", "origin"]);

    let out = ff(&fx, &["branch"]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    let text = stdout(&out);
    assert!(
        text.contains("1 whose shared copy is gone — ff branch --prune"),
        "{text}"
    );
    let v = json(&ff(&fx, &["branch", "--json"]));
    assert_eq!(v["data"]["gone"], 1);
}

/// `--prune` is its own shape: beside a name, `-d`, `--shared`, or `--all`
/// it is a usage error.
#[test]
fn prune_refuses_the_other_shapes() {
    let fx = repo();
    for args in [
        &["branch", "--prune", "spike"][..],
        &["branch", "--prune", "-d", "spike"],
        &["branch", "--prune", "--shared"],
        &["branch", "--prune", "--all"],
        &["branch", "--dry-run"],
    ] {
        let out = ff(&fx, args);
        assert_eq!(out.status.code(), Some(2), "{args:?}: {}", stderr(&out));
    }
}
