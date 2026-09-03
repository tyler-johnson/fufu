//! `ff edit <rev>` and `ff done` end to end against the real `ff` binary:
//! opening a session on a commit, the switch a branch name becomes, the
//! parking of the open change, the JSON envelopes, the no-op landing, the
//! abandon, and the refusals — nesting, `@`, and no session.

use std::path::Path;
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
        .env_remove("GIT_AUTHOR_NAME")
        .env_remove("GIT_AUTHOR_EMAIL")
        .env_remove("GIT_AUTHOR_DATE")
        .env_remove("GIT_COMMITTER_NAME")
        .env_remove("GIT_COMMITTER_EMAIL")
        .env_remove("GIT_COMMITTER_DATE")
        .env_remove("EMAIL")
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

/// Both streams concatenated, so an assertion never misses the one an
/// output actually landed on.
fn out(output: &Output) -> String {
    format!("{}{}", stdout(output), stderr(output))
}

fn json(output: &Output) -> serde_json::Value {
    serde_json::from_str(&stdout(output)).expect("valid json")
}

fn repo() -> Fixture {
    let fx = Fixture::new();
    fx.set_config("user.name", "Session Tester");
    fx.set_config("user.email", "session@test.test");
    fx
}

/// The shas the tests need off the shared base.
struct Stack {
    c0: String,
    c1: String,
    c3: String,
}

/// The shared base, leaving the fixture standing on `main`:
///
/// c0 ─ c1 ─ c2 ─ c3    (main, each touching its own file, `mid` at c1)
fn stack(fx: &Fixture) -> Stack {
    fx.write("c0.txt", "c0\n");
    let c0 = fx.commit("c0");
    fx.write("c1.txt", "c1\n");
    let c1 = fx.commit("c1");
    fx.git(&["branch", "mid", &c1]);
    fx.write("c2.txt", "c2\n");
    fx.commit("c2");
    fx.write("c3.txt", "c3\n");
    let c3 = fx.commit("c3");
    Stack { c0, c1, c3 }
}

fn tip(fx: &Fixture, branch: &str) -> String {
    fx.git(&["rev-parse", branch]).trim().to_string()
}

fn branch_of(fx: &Fixture) -> String {
    fx.git(&["rev-parse", "--abbrev-ref", "HEAD"])
        .trim()
        .to_string()
}

fn ff_branches(fx: &Fixture) -> Vec<String> {
    fx.git(&["for-each-ref", "--format=%(refname)", "refs/heads/ff/"])
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_string)
        .collect()
}

#[test]
fn edit_opens_a_session() {
    let fx = repo();
    let s = stack(&fx);

    let main_before = tip(&fx, "main");
    let output = ff(&fx, &["edit", &s.c1]);
    assert!(output.status.success(), "{}", out(&output));
    let text = stdout(&output);
    assert!(text.contains("editing"), "{text}");
    assert!(text.contains("c1"), "the subject of the commit: {text}");
    let branch = branch_of(&fx);
    assert!(
        branch.starts_with("ff/"),
        "HEAD must be on the session: {branch}"
    );
    assert_eq!(
        main_before,
        tip(&fx, "main"),
        "main must stand exactly where it was"
    );
}

#[test]
fn edit_says_what_waits_ahead() {
    let fx = repo();
    let s = stack(&fx);

    let output = ff(&fx, &["edit", &s.c1]);
    assert!(output.status.success(), "{}", out(&output));
    assert!(
        stdout(&output).contains("2 commit(s) wait ahead on main"),
        "{}",
        out(&output)
    );

    // A session at the tip has nothing ahead: it says nothing, not "0".
    let fx = repo();
    let s = stack(&fx);
    let output = ff(&fx, &["edit", &s.c3]);
    assert!(output.status.success(), "{}", out(&output));
    assert!(!stdout(&output).contains("wait ahead"), "{}", out(&output));
}

#[test]
fn edit_json_envelope() {
    let fx = repo();
    let s = stack(&fx);

    let output = ff(&fx, &["--json", "edit", &s.c1]);
    assert!(output.status.success(), "{}", out(&output));
    let v = json(&output);
    assert_eq!(v["ff"], 1);
    assert_eq!(v["cmd"], "edit");
    assert_eq!(v["data"]["edit"]["onto"], "main");
    assert_eq!(v["data"]["edit"]["ahead"], 2);
    let session = v["data"]["edit"]["session"]
        .as_str()
        .expect("a session name");
    assert!(session.starts_with("ff/"), "{session}");
}

#[test]
fn edit_a_branch_is_a_switch() {
    let fx = repo();
    stack(&fx);

    assert!(ff_branches(&fx).is_empty(), "the fixture starts with none");
    let output = ff(&fx, &["edit", "mid"]);
    assert!(output.status.success(), "{}", out(&output));
    let text = stdout(&output);
    assert!(text.contains("that is a branch, not a commit"), "{text}");
    assert!(text.contains("switched to mid"), "{text}");
    assert_eq!(branch_of(&fx), "mid");
    assert!(
        ff_branches(&fx).is_empty(),
        "a branch name must mint no session"
    );
}

#[test]
fn edit_parks_a_dirty_tree() {
    let fx = repo();
    let s = stack(&fx);

    fx.write("loose.txt", "loose\n");
    let output = ff(&fx, &["edit", &s.c1]);
    assert!(output.status.success(), "{}", out(&output));
    assert!(
        stdout(&output).contains("parked the open change on main"),
        "{}",
        out(&output)
    );
    assert!(
        !fx.path().join("loose.txt").exists(),
        "the parked file must not follow into the session"
    );
    // Panics if the ref is missing.
    fx.git(&["rev-parse", "--verify", "refs/fufu/parked/main"]);
}

#[test]
fn edit_the_open_change_is_refused() {
    let fx = repo();
    stack(&fx);

    let main_before = tip(&fx, "main");
    let output = ff(&fx, &["edit", "@"]);
    assert_eq!(output.status.code(), Some(1), "{}", out(&output));
    let text = out(&output);
    assert!(text.contains("ff edit HEAD"), "{text}");
    assert_eq!(main_before, tip(&fx, "main"), "a refusal must move nothing");

    let v = json(&ff(&fx, &["--json", "edit", "@"]));
    assert_eq!(v["error"]["id"], "target/unresolvable");
}

#[test]
fn edit_cannot_nest() {
    let fx = repo();
    let s = stack(&fx);

    let opened = ff(&fx, &["edit", &s.c1]);
    assert!(opened.status.success(), "{}", out(&opened));

    let output = ff(&fx, &["edit", &s.c0]);
    assert_eq!(output.status.code(), Some(1), "{}", out(&output));
    let text = out(&output);
    assert!(
        text.contains("ff done"),
        "the exit must name the way out: {text}"
    );

    let v = json(&ff(&fx, &["--json", "edit", &s.c0]));
    assert_eq!(v["error"]["id"], "session/open");

    let branches = ff_branches(&fx);
    assert_eq!(
        branches.len(),
        1,
        "the refusal must mint nothing: {branches:?}"
    );
}

#[test]
fn done_lands_the_session() {
    let fx = repo();
    let s = stack(&fx);

    let main_before = tip(&fx, "main");
    let mid_before = tip(&fx, "mid");
    let opened = ff(&fx, &["edit", &s.c1]);
    assert!(opened.status.success(), "{}", out(&opened));

    fx.write("c1.txt", "c1, edited\n");
    let output = ff(&fx, &["done"]);
    assert!(output.status.success(), "{}", out(&output));
    let text = stdout(&output);
    assert!(text.contains("amended"), "{text}");
    assert!(text.contains("replayed 2 commit(s)"), "{text}");
    assert!(text.contains("moved mid"), "{text}");
    assert!(text.contains("back on main"), "{text}");

    assert_eq!(branch_of(&fx), "main");
    assert!(ff_branches(&fx).is_empty(), "no session branch may survive");
    assert_ne!(
        main_before,
        tip(&fx, "main"),
        "the amended history must move main"
    );
    assert_ne!(
        mid_before,
        tip(&fx, "mid"),
        "the branch standing at the anchor must be carried"
    );
    assert!(
        fx.git(&["status", "--porcelain"]).trim().is_empty(),
        "the landing must leave a clean worktree"
    );
}

#[test]
fn done_json_envelope() {
    let fx = repo();
    let s = stack(&fx);

    let opened = ff(&fx, &["edit", &s.c1]);
    assert!(opened.status.success(), "{}", out(&opened));
    fx.write("c1.txt", "c1, edited\n");

    let output = ff(&fx, &["--json", "done"]);
    assert!(output.status.success(), "{}", out(&output));
    let v = json(&output);
    assert_eq!(v["ff"], 1);
    assert_eq!(v["cmd"], "done");
    assert_eq!(v["data"]["done"]["replayed"], 2);
    assert_eq!(v["data"]["done"]["onto"], "main");
    assert_eq!(v["data"]["done"]["unchanged"], false);
}

#[test]
fn done_with_no_edits_says_so() {
    let fx = repo();
    let s = stack(&fx);

    let main_before = tip(&fx, "main");
    let opened = ff(&fx, &["edit", &s.c1]);
    assert!(opened.status.success(), "{}", out(&opened));

    let output = ff(&fx, &["done"]);
    assert!(output.status.success(), "{}", out(&output));
    assert!(
        stdout(&output).contains("changed nothing"),
        "{}",
        out(&output)
    );
    assert_eq!(
        main_before,
        tip(&fx, "main"),
        "a no-op landing must leave main byte-identical"
    );
}

#[test]
fn done_without_a_session_is_refused() {
    let fx = repo();
    stack(&fx);

    let main_before = tip(&fx, "main");
    let output = ff(&fx, &["done"]);
    assert_eq!(output.status.code(), Some(1), "{}", out(&output));
    let text = out(&output);
    assert!(
        text.contains("ff edit <rev>"),
        "the exit must name the way in: {text}"
    );
    assert_eq!(main_before, tip(&fx, "main"), "a refusal must move nothing");

    let v = json(&ff(&fx, &["--json", "done"]));
    assert_eq!(v["error"]["id"], "session/none");
}

#[test]
fn done_abandon_says_where_the_edits_went() {
    let fx = repo();
    let s = stack(&fx);

    let main_before = tip(&fx, "main");
    let opened = ff(&fx, &["edit", &s.c1]);
    assert!(opened.status.success(), "{}", out(&opened));
    fx.write("c1.txt", "c1, edited\n");

    let output = ff(&fx, &["done", "--abandon"]);
    assert!(output.status.success(), "{}", out(&output));
    let text = stdout(&output);
    assert!(text.contains("abandoned"), "{text}");
    assert!(text.contains("stashed"), "{text}");

    assert_eq!(branch_of(&fx), "main");
    assert_eq!(
        main_before,
        tip(&fx, "main"),
        "abandoning must leave main exactly where it stood"
    );
    assert!(ff_branches(&fx).is_empty(), "no session branch may survive");
    let stash = fx.git(&["stash", "list"]);
    assert!(
        !stash.trim().is_empty(),
        "the edits must be in the stash, not thrown away: {stash}"
    );
}

#[test]
fn commit_inside_a_session_is_refused() {
    let fx = repo();
    let s = stack(&fx);

    let opened = ff(&fx, &["edit", &s.c1]);
    assert!(opened.status.success(), "{}", out(&opened));
    let branch = branch_of(&fx);
    assert!(branch.starts_with("ff/"), "{branch}");
    let tip_before = tip(&fx, &branch);

    fx.write("c1.txt", "c1, edited\n");
    let output = ff(&fx, &["commit", "-m", "nope"]);
    assert_eq!(output.status.code(), Some(1), "{}", out(&output));
    let text = out(&output);
    assert!(
        text.contains("ff done"),
        "the exit must name the way out: {text}"
    );

    let v = json(&ff(&fx, &["--json", "commit", "-m", "nope"]));
    assert_eq!(v["error"]["id"], "session/open");

    assert_eq!(
        tip_before,
        tip(&fx, &branch),
        "a refusal must not land anything on the session branch"
    );
    assert!(
        !fx.git(&["status", "--porcelain"]).trim().is_empty(),
        "the open change must still be in the working tree"
    );
}

#[test]
fn commit_outside_a_session_still_works() {
    let fx = repo();
    stack(&fx);

    fx.write("loose.txt", "loose\n");
    let main_before = tip(&fx, "main");
    let output = ff(&fx, &["commit", "-m", "fine"]);
    assert!(output.status.success(), "{}", out(&output));
    assert_ne!(main_before, tip(&fx, "main"), "the close must move main");
}

#[test]
fn status_shows_the_session() {
    let fx = repo();
    let s = stack(&fx);

    let opened = ff(&fx, &["edit", &s.c1]);
    assert!(opened.status.success(), "{}", out(&opened));

    let output = ff(&fx, &["status"]);
    assert!(output.status.success(), "{}", out(&output));
    let text = stdout(&output);
    assert!(text.contains("editing"), "{text}");
    assert!(
        text.contains("c1"),
        "the subject of the edited commit: {text}"
    );
    assert!(text.contains("lands back on main"), "{text}");
    assert!(
        text.contains("ff done"),
        "the exits line must name the way out: {text}"
    );

    let v = json(&ff(&fx, &["--json", "status"]));
    assert_eq!(v["data"]["session"]["onto"], "main");
    let branch = v["data"]["session"]["branch"]
        .as_str()
        .expect("a session branch");
    assert!(branch.starts_with("ff/"), "{branch}");

    // A session line when no session is running is worse than none.
    let fx = repo();
    stack(&fx);
    let v = json(&ff(&fx, &["--json", "status"]));
    assert!(v["data"]["session"].is_null(), "{}", v);
}

#[test]
fn branch_marks_the_session_row() {
    let fx = repo();
    let s = stack(&fx);

    let opened = ff(&fx, &["edit", &s.c1]);
    assert!(opened.status.success(), "{}", out(&opened));

    let output = ff(&fx, &["branch"]);
    assert!(output.status.success(), "{}", out(&output));
    let text = stdout(&output);
    assert!(text.contains("editing session, lands on main"), "{text}");

    let done = ff(&fx, &["done"]);
    assert!(done.status.success(), "{}", out(&done));
    let output = ff(&fx, &["branch"]);
    assert!(output.status.success(), "{}", out(&output));
    assert!(
        !stdout(&output).contains("editing session"),
        "no row may still claim a session after the landing: {}",
        out(&output)
    );
}

#[test]
fn explain_knows_the_new_ids() {
    let fx = repo();
    fx.write("f1.txt", "one\n");
    fx.commit("base");

    let none = ff(&fx, &["explain", "session/none"]);
    assert!(none.status.success(), "{}", out(&none));
    let text = stdout(&none);
    assert!(
        text.contains("there is no editing session to finish"),
        "{text}"
    );

    let open = ff(&fx, &["explain", "session/open"]);
    assert!(open.status.success(), "{}", out(&open));
    let text = stdout(&open);
    assert!(
        text.contains("you are already inside an editing session"),
        "{text}"
    );

    let moved = ff(&fx, &["explain", "session/moved"]);
    assert!(moved.status.success(), "{}", out(&moved));
    let text = stdout(&moved);
    assert!(
        text.contains("the session branch has commits of its own now"),
        "{text}"
    );

    let unreachable = ff(&fx, &["explain", "session/unreachable"]);
    assert!(unreachable.status.success(), "{}", out(&unreachable));
    let text = stdout(&unreachable);
    assert!(
        text.contains("the edited commit has left the branch the session lands on"),
        "{text}"
    );

    let not_in_history = ff(&fx, &["explain", "edit/not-in-history"]);
    assert!(not_in_history.status.success(), "{}", out(&not_in_history));
    let text = stdout(&not_in_history);
    assert!(
        text.contains("that commit is not in the branch you are standing on"),
        "{text}"
    );
}

#[test]
fn done_says_what_followed_and_the_json_carries_it() {
    let fx = repo();
    fx.write("a.txt", "base\n");
    fx.commit("base");
    fx.git(&["switch", "-q", "-c", "feat"]);
    fx.write("a.txt", "one\n");
    let c1 = fx.commit("c1");
    fx.write("c.txt", "c\n");
    fx.commit("c2");
    let started = ff(&fx, &["start", "feat", "-b", "top"]);
    assert!(started.status.success(), "{}", out(&started));
    fx.write("x.txt", "x\n");
    let x1 = fx.commit("x1");
    let back = ff(&fx, &["switch", "feat"]);
    assert!(back.status.success(), "{}", out(&back));

    let opened = ff(&fx, &["edit", c1.trim()]);
    assert!(opened.status.success(), "{}", out(&opened));
    fx.write("a.txt", "one, edited\n");
    let output = ff(&fx, &["done"]);
    assert!(output.status.success(), "{}", out(&output));
    let text = stdout(&output);
    assert!(text.contains("amended"), "{text}");
    assert!(
        text.contains("top followed feat: replayed 1 commit(s)"),
        "{text}"
    );
    assert!(text.contains("back on feat"), "{text}");
    assert_ne!(tip(&fx, "top"), x1.trim(), "top followed");

    let undone = ff(&fx, &["undo"]);
    assert!(undone.status.success(), "{}", out(&undone));
    assert_eq!(tip(&fx, "top"), x1.trim());

    let output = ff(&fx, &["--json", "done"]);
    assert!(output.status.success(), "{}", out(&output));
    let v = json(&output);
    assert_eq!(v["cmd"], "done");
    let moved = &v["data"]["done"]["cascade"]["moved"];
    assert_eq!(moved[0]["branch"], "top");
    assert_eq!(moved[0]["base"], "feat");
    assert_eq!(moved[0]["replayed"], 1);
    assert_eq!(
        v["data"]["done"]["cascade"]["held"]
            .as_array()
            .unwrap()
            .len(),
        0
    );
}
