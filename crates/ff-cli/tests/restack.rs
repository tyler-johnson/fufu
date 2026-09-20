//! `ff restack`: moving a branch onto a different base, end to end against
//! the real `ff` binary. Covers the replay, the re-aim, the off-branch
//! restack, the JSON envelope, the nothing-happened exit, the refusals, the
//! undo round trip, and the published-remote disclosure.

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
    fx.set_config("user.name", "Absorb Tester");
    fx.set_config("user.email", "absorb@test.test");
    fx
}

/// The shared stack, leaving the fixture standing on `feature`:
///
/// m1 ─ m2 ─ m3               (main)
///       └─ f1 ─ f2 ─ f3      (feature, with `mid` at f2)
///
/// Distinct files throughout, so the replay is clean.
fn stack(fx: &Fixture) {
    fx.write("root.txt", "root\n");
    fx.commit("root");
    fx.write("m.txt", "m\n");
    fx.commit("m");

    fx.git(&["switch", "-q", "-c", "feature"]);
    fx.write("a.txt", "a\n");
    fx.commit("f1");
    fx.write("b.txt", "b\n");
    let f2 = fx.commit("f2");
    fx.write("c.txt", "c\n");
    fx.commit("f3");
    fx.git(&["branch", "mid", &f2]);

    fx.git(&["switch", "-q", "main"]);
    fx.write("d.txt", "d\n");
    fx.commit("m3");

    fx.git(&["switch", "-q", "feature"]);
}

/// Give `feature` and `main` different upstreams, both already holding the
/// branch's tip: a HEAD-derived name is then visibly the wrong one.
fn upstreamed(fx: &Fixture) {
    // gix resolves the tracking ref through the remote's config — no URL and
    // no fetch refspec means no tracking ref at all, so both must exist.
    fx.git(&[
        "config",
        "remote.origin.url",
        "https://example.test/origin.git",
    ]);
    fx.git(&[
        "config",
        "remote.origin.fetch",
        "+refs/heads/*:refs/remotes/origin/*",
    ]);

    let feature_tip = fx.git(&["rev-parse", "feature"]).trim().to_string();
    fx.git(&["update-ref", "refs/remotes/origin/feature", &feature_tip]);
    fx.git(&["config", "branch.feature.remote", "origin"]);
    fx.git(&["config", "branch.feature.merge", "refs/heads/feature"]);

    let main_tip = fx.git(&["rev-parse", "main"]).trim().to_string();
    fx.git(&["update-ref", "refs/remotes/origin/main", &main_tip]);
    fx.git(&["config", "branch.main.remote", "origin"]);
    fx.git(&["config", "branch.main.merge", "refs/heads/main"]);
}

#[test]
fn restack_replays_and_reports() {
    let fx = repo();
    stack(&fx);

    let feature_before = fx.git(&["rev-parse", "feature"]).trim().to_string();
    let output = ff(&fx, &["restack"]);
    assert!(output.status.success(), "{}", out(&output));
    let text = stdout(&output);
    assert!(text.contains("replayed"), "{text}");
    assert!(text.contains("onto main"), "{text}");
    let feature_after = fx.git(&["rev-parse", "feature"]).trim().to_string();
    assert_ne!(feature_before, feature_after, "the tip must move");
}

/// `ff restack` moves only the branch it was asked to move. `git rebase
/// --update-refs` would have carried `mid` along with `feature` out of the
/// rewritten range; fufu diverges deliberately, leaving a branch — one another
/// worktree may be standing on — where it stood and naming it.
#[test]
fn restack_leaves_descendants_where_they_stand() {
    let fx = repo();
    stack(&fx);

    let feature_before = fx.git(&["rev-parse", "feature"]).trim().to_string();
    let mid_before = fx.git(&["rev-parse", "mid"]).trim().to_string();

    let output = ff(&fx, &["restack"]);
    assert!(output.status.success(), "{}", out(&output));
    let text = stdout(&output);

    let feature_after = fx.git(&["rev-parse", "feature"]).trim().to_string();
    assert_ne!(
        feature_before, feature_after,
        "feature moved to the rewritten tip"
    );

    // The load-bearing assertion: the branch partway up the rewritten range
    // did not move.
    let mid_after = fx.git(&["rev-parse", "mid"]).trim().to_string();
    assert_eq!(
        mid_before, mid_after,
        "mid sits where it stood before the restack"
    );

    // And the command names it, saying what it now sits on.
    assert!(
        text.contains("mid"),
        "the divergence line names mid: {text}"
    );
    assert!(
        text.contains("now sits on commits this restack replaced"),
        "the divergence line says what mid now sits on: {text}"
    );
}

#[test]
fn restack_json_envelope() {
    let fx = repo();
    stack(&fx);

    let output = ff(&fx, &["--json", "restack"]);
    assert!(output.status.success(), "{}", out(&output));
    let v = json(&output);
    assert_eq!(v["ff"], 1);
    assert_eq!(v["cmd"], "restack");
    assert_eq!(v["data"]["restack"]["replayed"], 3);
}

#[test]
fn restack_off_branch_leaves_the_worktree() {
    let fx = repo();
    stack(&fx);
    fx.git(&["switch", "-q", "main"]);

    let feature_before = fx.git(&["rev-parse", "feature"]).trim().to_string();
    let output = ff(&fx, &["restack", "feature"]);
    assert!(output.status.success(), "{}", out(&output));
    let feature_after = fx.git(&["rev-parse", "feature"]).trim().to_string();
    assert_ne!(feature_before, feature_after, "the named branch must move");
    assert!(
        fx.git(&["status", "--porcelain"]).trim().is_empty(),
        "an off-branch restack must leave the worktree untouched"
    );
}

#[test]
fn restack_onto_records_the_parent() {
    let fx = repo();
    stack(&fx);
    fx.git(&["switch", "-q", "main"]);
    fx.git(&["switch", "-q", "-c", "other"]);
    fx.write("o.txt", "o\n");
    fx.commit("other1");
    fx.git(&["switch", "-q", "main"]);

    let output = ff(&fx, &["restack", "feature", "--onto", "other"]);
    assert!(output.status.success(), "{}", out(&output));
    let text = stdout(&output);
    assert!(text.contains("re-aimed"), "{text}");
    assert!(text.contains("onto other"), "{text}");

    // Advance the new base so the next restack has something to replay:
    // standing on it already would read "already on top of".
    fx.git(&["switch", "-q", "other"]);
    fx.write("o2.txt", "o2\n");
    fx.commit("other2");
    fx.git(&["switch", "-q", "main"]);

    let again = ff(&fx, &["restack", "feature"]);
    assert!(again.status.success(), "{}", out(&again));
    let text = stdout(&again);
    assert!(
        text.contains("onto other"),
        "the recorded parent must be the base, not trunk: {text}"
    );
}

#[test]
fn restack_onto_self_is_exit_2() {
    let fx = repo();
    stack(&fx);

    let output = ff(&fx, &["--json", "restack", "feature", "--onto", "feature"]);
    assert_eq!(output.status.code(), Some(2), "{}", out(&output));
    let v = json(&output);
    assert_eq!(v["error"]["id"], "usage/restack-onto-self");
}

#[test]
fn restack_conflict_holds_at_exit_3() {
    let fx = repo();
    fx.write("f.txt", "x\nrest\n");
    fx.commit("base");
    fx.git(&["switch", "-q", "-c", "feature"]);
    fx.write("f.txt", "A\nrest\n");
    fx.commit("f1");
    fx.git(&["switch", "-q", "main"]);
    fx.write("f.txt", "B\nrest\n");
    fx.commit("m2");
    fx.git(&["switch", "-q", "feature"]);

    let feature_before = fx.git(&["rev-parse", "feature"]).trim().to_string();
    let output = ff(&fx, &["restack"]);
    assert_eq!(output.status.code(), Some(3), "{}", out(&output));

    // A hold is an outcome, so it reports on stdout — and still owes the
    // shell a 3, because nothing moved.
    let so = stdout(&output);
    assert!(
        so.contains("held:"),
        "a hold reports rather than refuses: {so}"
    );
    assert!(so.contains("f.txt"), "the report must name the path: {so}");
    assert!(
        so.contains("ff resolve"),
        "the report must name the way out: {so}"
    );

    assert_eq!(
        feature_before,
        fx.git(&["rev-parse", "feature"]).trim(),
        "the tip must not move"
    );

    // The second one meets the standing hold rather than recording another:
    // one hold per branch, until it lands or is dropped.
    let again = ff(&fx, &["--json", "restack"]);
    assert_eq!(again.status.code(), Some(3), "{}", out(&again));
    assert_eq!(json(&again)["error"]["id"], "held/already-held");
}

/// `main` and `feature` editing the same line of `f.txt`, standing on
/// `feature`: the replay conflicts at `f1`.
fn conflict_stack(fx: &Fixture) {
    fx.write("f.txt", "x\nrest\n");
    fx.commit("base");
    fx.git(&["switch", "-q", "-c", "feature"]);
    fx.write("f.txt", "A\nrest\n");
    fx.commit("f1");
    fx.git(&["switch", "-q", "main"]);
    fx.write("f.txt", "B\nrest\n");
    fx.commit("m2");
    fx.git(&["switch", "-q", "feature"]);
}

fn head_branch(fx: &Fixture) -> String {
    fx.git(&["symbolic-ref", "--short", "HEAD"])
        .trim()
        .to_string()
}

/// `--resolve` on a conflicting replay: the hold is recorded and named, the
/// session opens with the markers in the working copy, the exit is still 3,
/// and `ff done` lands the restack from the session.
#[test]
fn restack_resolve_opens_the_session_and_done_lands_it() {
    let fx = repo();
    conflict_stack(&fx);
    let feature_before = fx.git(&["rev-parse", "feature"]).trim().to_string();

    let output = ff(&fx, &["restack", "--resolve"]);
    assert_eq!(output.status.code(), Some(3), "{}", out(&output));
    let so = stdout(&output);
    assert!(so.contains("held: replaying "), "the hold's line: {so}");
    assert!(
        so.contains("resolving 1 conflict in f.txt on ff/"),
        "the session's line: {so}"
    );
    assert!(
        so.contains("fix the markers, then ff done"),
        "the ways out are the session's: {so}"
    );
    assert!(
        !so.contains("ff resolve to fix them"),
        "no held block under an open session: {so}"
    );
    let session = head_branch(&fx);
    assert!(
        session.starts_with("ff/"),
        "HEAD is on the session: {session}"
    );
    let f = std::fs::read_to_string(fx.path().join("f.txt")).unwrap();
    assert!(
        f.contains("<<<<<<<") && f.contains(">>>>>>>"),
        "markers: {f}"
    );
    assert_eq!(
        fx.git(&["rev-parse", "feature"]).trim(),
        feature_before,
        "the tip must not move"
    );

    fx.write("f.txt", "AB\nrest\n");
    let done = ff(&fx, &["done"]);
    assert!(done.status.success(), "{}", out(&done));
    assert_eq!(head_branch(&fx), "feature");
    assert_ne!(fx.git(&["rev-parse", "feature"]).trim(), feature_before);
    assert_eq!(fx.git(&["show", "feature:f.txt"]), "AB\nrest\n");
    let status = stdout(&ff(&fx, &["status"]));
    assert!(!status.contains("held:"), "the hold landed: {status}");
}

/// `ff done --abandon` from the session `--resolve` opened closes it and
/// keeps the hold, so status reads held on the branch again.
#[test]
fn restack_resolve_then_done_abandon_returns_to_the_held_branch() {
    let fx = repo();
    conflict_stack(&fx);
    let output = ff(&fx, &["restack", "--resolve"]);
    assert_eq!(output.status.code(), Some(3), "{}", out(&output));

    let abandon = ff(&fx, &["done", "--abandon"]);
    assert!(abandon.status.success(), "{}", out(&abandon));
    assert_eq!(head_branch(&fx), "feature");
    let status = stdout(&ff(&fx, &["status"]));
    assert!(status.contains("held:"), "the hold stands: {status}");
    assert!(
        !status.contains("resolving:"),
        "the session is closed: {status}"
    );
    assert_eq!(
        std::fs::read_to_string(fx.path().join("f.txt")).unwrap(),
        "A\nrest\n"
    );
}

#[test]
fn restack_resolve_json_carries_the_hold_and_the_session() {
    let fx = repo();
    conflict_stack(&fx);
    let output = ff(&fx, &["--json", "restack", "--resolve"]);
    assert_eq!(output.status.code(), Some(3), "{}", out(&output));
    let v = json(&output);
    assert_eq!(v["cmd"], "restack", "{v}");
    assert!(v["data"]["restack"].is_null(), "{v}");
    assert_eq!(v["data"]["held"]["verb"], "restack", "{v}");
    assert_eq!(v["data"]["held"]["branch"], "feature", "{v}");
    let session = v["data"]["resolve"]["session"]
        .as_str()
        .expect("the session name");
    assert!(session.starts_with("ff/"), "{v}");
    assert_eq!(v["data"]["resolve"]["held"]["verb"], "restack", "{v}");
    assert_eq!(v["data"]["resolve"]["branch"], "feature", "{v}");
    assert_eq!(v["data"]["resolve"]["files"][0], "f.txt", "{v}");
    assert_eq!(v["data"]["undo"], "ff undo", "{v}");
    assert_eq!(head_branch(&fx), session);
}

/// `fufu.onConflict resolve` opens without the flag; `--no-resolve` holds
/// for one run under it.
#[test]
fn on_conflict_resolve_is_the_standing_choice_and_no_resolve_overrides_it() {
    let fx = repo();
    conflict_stack(&fx);
    fx.set_config("fufu.onConflict", "resolve");
    let output = ff(&fx, &["restack"]);
    assert_eq!(output.status.code(), Some(3), "{}", out(&output));
    let so = stdout(&output);
    assert!(so.contains("resolving 1 conflict in f.txt on ff/"), "{so}");
    assert!(head_branch(&fx).starts_with("ff/"));

    let fx = repo();
    conflict_stack(&fx);
    fx.set_config("fufu.onConflict", "resolve");
    let output = ff(&fx, &["restack", "--no-resolve"]);
    assert_eq!(output.status.code(), Some(3), "{}", out(&output));
    let so = stdout(&output);
    assert!(so.contains("held: replaying "), "{so}");
    assert!(
        so.contains("ff resolve to fix them, all at once"),
        "the plain held block: {so}"
    );
    assert!(!so.contains("resolving "), "no session: {so}");
    assert_eq!(head_branch(&fx), "feature");
    let status = stdout(&ff(&fx, &["status"]));
    assert!(status.contains("held:"), "{status}");
    assert!(!status.contains("resolving:"), "{status}");
}

#[test]
fn resolve_and_no_resolve_together_are_a_usage_error() {
    let fx = repo();
    conflict_stack(&fx);
    let output = ff(&fx, &["restack", "--resolve", "--no-resolve"]);
    assert_eq!(output.status.code(), Some(2), "{}", out(&output));
    assert_eq!(head_branch(&fx), "feature");
}

/// A replay that lands reads the same with and without the flag: the flag
/// only decides what a conflict does.
#[test]
fn a_clean_restack_with_resolve_reads_like_one_without() {
    let fx = repo();
    stack(&fx);
    let with = ff(&fx, &["restack", "--resolve"]);
    assert!(with.status.success(), "{}", out(&with));

    let fx = repo();
    stack(&fx);
    let without = ff(&fx, &["restack"]);
    assert!(without.status.success(), "{}", out(&without));
    assert_eq!(stdout(&with), stdout(&without));
    assert!(
        stdout(&with).contains("replayed 3 commit(s) onto main"),
        "{}",
        stdout(&with)
    );
}

#[test]
fn a_restack_hold_is_an_outcome_in_json() {
    let fx = repo();
    fx.write("f.txt", "x\nrest\n");
    fx.commit("base");
    fx.git(&["switch", "-q", "-c", "feature"]);
    fx.write("f.txt", "A\nrest\n");
    fx.commit("f1");
    fx.git(&["switch", "-q", "main"]);
    fx.write("f.txt", "B\nrest\n");
    fx.commit("m2");
    fx.git(&["switch", "-q", "feature"]);

    let output = ff(&fx, &["--json", "restack"]);
    assert_eq!(output.status.code(), Some(3), "{}", out(&output));
    let v = json(&output);
    assert!(v["error"].is_null(), "a hold is not an error envelope: {v}");
    assert_eq!(v["data"]["held"]["verb"], "restack");
    assert_eq!(v["data"]["held"]["branch"], "feature");
    assert_eq!(v["data"]["held"]["at"]["what"], "commit");
    assert_eq!(v["data"]["held"]["paths"][0], "f.txt");
}

#[test]
fn restack_missing_branch() {
    let fx = repo();
    stack(&fx);

    let output = ff(&fx, &["--json", "restack", "no-such-branch"]);
    assert!(!output.status.success(), "{}", out(&output));
    let v = json(&output);
    assert_eq!(v["error"]["id"], "branch/not-found");
}

#[test]
fn restack_nothing_to_do() {
    let fx = repo();
    stack(&fx);

    let first = ff(&fx, &["restack"]);
    assert!(first.status.success(), "{}", out(&first));

    let second = ff(&fx, &["restack"]);
    assert!(second.status.success(), "{}", out(&second));
    let text = stdout(&second);
    assert!(text.contains("already on top of"), "{text}");
}

#[test]
fn restack_undo_round_trip() {
    let fx = repo();
    stack(&fx);

    let feature_before = fx.git(&["rev-parse", "feature"]).trim().to_string();
    let output = ff(&fx, &["restack"]);
    assert!(output.status.success(), "{}", out(&output));

    let undone = ff(&fx, &["undo"]);
    assert!(undone.status.success(), "{}", out(&undone));
    assert_eq!(
        feature_before,
        fx.git(&["rev-parse", "feature"]).trim(),
        "ff undo must put the tip back"
    );
}

#[test]
fn explain_knows_the_new_ids() {
    let fx = repo();
    fx.write("f1.txt", "one\n");
    fx.commit("base");

    let no_base = ff(&fx, &["explain", "restack/no-base"]);
    assert!(no_base.status.success(), "{}", out(&no_base));
    let text = stdout(&no_base);
    assert!(
        text.contains("no base branch was found for this restack"),
        "{text}"
    );

    let unrelated = ff(&fx, &["explain", "restack/unrelated"]);
    assert!(unrelated.status.success(), "{}", out(&unrelated));
    let text = stdout(&unrelated);
    assert!(
        text.contains("the branch and requested base have no common ancestor"),
        "{text}"
    );

    let onto_self = ff(&fx, &["explain", "usage/restack-onto-self"]);
    assert!(onto_self.status.success(), "{}", out(&onto_self));
    let text = stdout(&onto_self);
    assert!(
        text.contains("a branch cannot be restacked onto itself"),
        "{text}"
    );
}

#[test]
fn restack_says_what_it_dropped() {
    // The stack with one commit of each empty kind in `feature`'s range:
    // `dup` adds the exact same `shared.txt` bytes `main` adds next, and
    // `marker` started empty — both are dropped by the restack.
    let fx = repo();
    fx.write("root.txt", "root\n");
    fx.commit("root");
    fx.write("m.txt", "m\n");
    fx.commit("m");

    fx.git(&["switch", "-q", "-c", "feature"]);
    fx.write("a.txt", "a\n");
    fx.commit("f1");
    fx.write("shared.txt", "shared\n");
    let dup = fx.commit("dup");
    fx.git(&["branch", "mid", &dup]);
    fx.commit("marker");
    fx.write("c.txt", "c\n");
    fx.commit("f3");

    fx.git(&["switch", "-q", "main"]);
    fx.write("shared.txt", "shared\n");
    fx.commit("m3");

    fx.git(&["switch", "-q", "feature"]);

    let output = ff(&fx, &["restack"]);
    assert!(output.status.success(), "{}", out(&output));
    let text = stdout(&output);
    assert!(
        text.contains("dropped 2 commit(s) that change nothing"),
        "the drop must be announced: {text}"
    );
}

#[test]
fn restack_names_the_branchs_own_remote() {
    let fx = repo();
    stack(&fx);
    upstreamed(&fx);
    fx.git(&["switch", "-q", "main"]);

    let output = ff(&fx, &["restack", "feature"]);
    assert!(output.status.success(), "{}", out(&output));
    let text = stdout(&output);
    assert!(
        text.contains("origin/feature"),
        "the disclosure must name feature's own remote: {text}"
    );
    assert!(
        !text.contains("origin/main"),
        "a HEAD-derived name would be visibly wrong: {text}"
    );

    // The restack moved feature, so the JSON form gets a fresh fixture.
    let fx = repo();
    stack(&fx);
    upstreamed(&fx);
    fx.git(&["switch", "-q", "main"]);

    let v = json(&ff(&fx, &["--json", "restack", "feature"]));
    assert_eq!(v["data"]["restack"]["published"], 3);
    assert_eq!(v["data"]["restack"]["published_on"], "origin/feature");
}

// ---- The cascade: the branches stacked above the one that moved ----

/// Two branches stacked above `feature` through `ff start`, which records
/// the branch beneath each. Leaves the fixture standing on `feature`.
fn stack_above(fx: &Fixture) {
    let started = ff(fx, &["start", "feature", "-b", "child"]);
    assert!(started.status.success(), "{}", out(&started));
    fx.write("x.txt", "x\n");
    fx.commit("x1");
    let started = ff(fx, &["start", "child", "-b", "grandchild"]);
    assert!(started.status.success(), "{}", out(&started));
    fx.write("y.txt", "y\n");
    fx.commit("y1");
    let back = ff(fx, &["switch", "feature"]);
    assert!(back.status.success(), "{}", out(&back));
}

fn rev(fx: &Fixture, name: &str) -> String {
    fx.git(&["rev-parse", name]).trim().to_string()
}

fn is_ancestor(fx: &Fixture, ancestor: &str, of: &str) -> bool {
    fx.try_git(&["merge-base", "--is-ancestor", ancestor, of])
        .status
        .success()
}

#[test]
fn restack_says_what_followed_above() {
    let fx = repo();
    stack(&fx);
    stack_above(&fx);
    let child_before = rev(&fx, "child");
    let grandchild_before = rev(&fx, "grandchild");

    let output = ff(&fx, &["restack"]);
    assert!(output.status.success(), "{}", out(&output));
    let text = stdout(&output);
    assert!(
        text.contains("child followed feature: replayed 1 commit(s)"),
        "{text}"
    );
    assert!(
        text.contains("grandchild followed child: replayed 1 commit(s)"),
        "{text}"
    );

    assert_ne!(child_before, rev(&fx, "child"), "child moved");
    assert_ne!(
        grandchild_before,
        rev(&fx, "grandchild"),
        "grandchild moved"
    );
    assert!(is_ancestor(&fx, "feature", "child"));
    assert!(is_ancestor(&fx, "child", "grandchild"));
}

#[test]
fn the_cascade_is_in_the_json_and_one_undo_takes_it_back() {
    let fx = repo();
    stack(&fx);
    stack_above(&fx);
    let feature_before = rev(&fx, "feature");
    let child_before = rev(&fx, "child");
    let grandchild_before = rev(&fx, "grandchild");

    let output = ff(&fx, &["--json", "restack"]);
    assert!(output.status.success(), "{}", out(&output));
    let v = json(&output);
    let moved = &v["data"]["restack"]["cascade"]["moved"];
    assert_eq!(moved[0]["branch"], "child");
    assert_eq!(moved[0]["base"], "feature");
    assert_eq!(moved[0]["replayed"], 1);
    assert_eq!(moved[1]["branch"], "grandchild");
    assert_eq!(moved[1]["base"], "child");
    assert_eq!(
        v["data"]["restack"]["cascade"]["held"]
            .as_array()
            .unwrap()
            .len(),
        0
    );

    let undone = ff(&fx, &["undo"]);
    assert!(undone.status.success(), "{}", out(&undone));
    assert_eq!(feature_before, rev(&fx, "feature"));
    assert_eq!(child_before, rev(&fx, "child"), "one undo puts child back");
    assert_eq!(grandchild_before, rev(&fx, "grandchild"), "and grandchild");
}

#[test]
fn a_branch_above_that_conflicts_holds_and_exits_3() {
    let fx = repo();
    stack(&fx);
    // main's m3 added d.txt; child adds its own d.txt, an add/add conflict.
    let started = ff(&fx, &["start", "feature", "-b", "child"]);
    assert!(started.status.success(), "{}", out(&started));
    fx.write("d.txt", "child\n");
    fx.commit("x1");
    let started = ff(&fx, &["start", "child", "-b", "grandchild"]);
    assert!(started.status.success(), "{}", out(&started));
    fx.write("y.txt", "y\n");
    fx.commit("y1");
    let back = ff(&fx, &["switch", "feature"]);
    assert!(back.status.success(), "{}", out(&back));
    let feature_before = rev(&fx, "feature");
    let child_before = rev(&fx, "child");
    let grandchild_before = rev(&fx, "grandchild");

    let output = ff(&fx, &["restack"]);
    assert_eq!(output.status.code(), Some(3), "{}", out(&output));
    let so = stdout(&output);
    assert!(so.contains("replayed 3 commit(s) onto main"), "{so}");
    assert!(so.contains("child held:"), "{so}");
    assert!(so.contains("d.txt"), "{so}");
    assert!(so.contains("grandchild left alone"), "{so}");
    assert!(so.contains("ff resolve"), "{so}");

    assert_ne!(feature_before, rev(&fx, "feature"), "feature itself moved");
    assert_eq!(child_before, rev(&fx, "child"), "the held branch stays put");
    assert_eq!(grandchild_before, rev(&fx, "grandchild"), "and its subtree");
}

#[test]
fn a_branch_above_in_another_worktree_is_skipped_and_named() {
    let fx = repo();
    stack(&fx);
    stack_above(&fx);
    let wt = fx.root().join("linked-wt");
    fx.git(&["worktree", "add", "-q", wt.to_str().unwrap(), "child"]);
    let child_before = rev(&fx, "child");
    let grandchild_before = rev(&fx, "grandchild");

    let output = ff(&fx, &["restack"]);
    assert!(output.status.success(), "{}", out(&output));
    let text = stdout(&output);
    assert!(text.contains("child skipped: checked out in"), "{text}");
    assert!(text.contains("linked-wt"), "{text}");
    assert!(text.contains("grandchild left alone"), "{text}");

    assert_eq!(child_before, rev(&fx, "child"));
    assert_eq!(grandchild_before, rev(&fx, "grandchild"));
}

/// gh #5 end to end, with the base's commit closed by fufu: the child's stale
/// copy of a commit the base rewrote drops by change id, with no reflog to
/// read, and both the text and the JSON say what superseded it.
#[test]
fn restack_onto_drops_a_stale_copy_the_base_already_holds() {
    let fx = repo();
    fx.write("a.txt", "a\n");
    fx.commit("A: trunk");
    fx.git(&["switch", "-q", "-c", "base"]);
    fx.write("b.txt", "b\n");
    let closed = ff(&fx, &["commit", "-m", "B: base work"]);
    assert!(closed.status.success(), "{}", out(&closed));
    let b = fx.git(&["rev-parse", "HEAD"]).trim().to_string();
    fx.git(&["switch", "-q", "-c", "child"]);
    fx.write("c.txt", "c\n");
    fx.commit("C: child work");
    fx.git(&["switch", "-q", "main"]);
    fx.write("a.txt", "a\na2\n");
    fx.commit("A2: trunk moves");
    fx.git(&["switch", "-q", "base"]);
    let moved = ff(&fx, &["restack"]);
    assert!(moved.status.success(), "{}", out(&moved));
    fx.write("b.txt", "b RESOLVED DIFFERENTLY\n");
    let absorbed = ff(&fx, &["absorb"]);
    assert!(absorbed.status.success(), "{}", out(&absorbed));
    let b_rewritten = fx.git(&["rev-parse", "base"]).trim().to_string();
    fx.git(&["reflog", "expire", "--expire=now", "--all"]);
    fx.git(&["switch", "-q", "child"]);

    let output = ff(&fx, &["--json", "restack", "--onto", "base"]);
    assert!(output.status.success(), "{}", out(&output));
    let v = json(&output);
    let restack = &v["data"]["restack"];
    assert_eq!(restack["replayed"], 1);
    assert_eq!(restack["dropped"].as_array().map(Vec::len), Some(1));
    assert_eq!(restack["dropped"][0]["old"], b);
    assert_eq!(restack["dropped"][0]["reason"], "superseded");
    assert_eq!(restack["dropped"][0]["by"], b_rewritten);
    assert_eq!(
        fx.git(&["show", "child:b.txt"]),
        "b RESOLVED DIFFERENTLY\n",
        "the child sits on the base's rewrite"
    );

    let undone = ff(&fx, &["undo"]);
    assert!(undone.status.success(), "{}", out(&undone));
    let output = ff(&fx, &["restack", "--onto", "base"]);
    assert!(output.status.success(), "{}", out(&output));
    let text = out(&output);
    assert!(
        text.contains(&format!(
            "dropped {} \"B: base work\" — superseded by {} in the base",
            &b[..8],
            &b_rewritten[..8]
        )),
        "{text}"
    );
}

/// A feature branch that merged trunk once, with trunk moved on since, HEAD
/// on `feature`:
///
/// ```text
/// T0 ─ T1 ──────── T2          (main)
///  └─ f1 ─ f2 ─ M ─ f3         (feature; M merges T1)
/// ```
///
/// `extra` gives M an edit of its own, `extra.txt`, that T2 leaves alone.
/// Returns M's sha.
fn trunk_merge(fx: &Fixture, extra: bool) -> String {
    fx.write("main.txt", "main\n");
    fx.commit("T0");
    fx.git(&["switch", "-q", "-c", "feature"]);
    fx.write("a.txt", "a\n");
    fx.commit("f1");
    fx.write("b.txt", "b\n");
    fx.commit("f2");
    fx.git(&["switch", "-q", "main"]);
    fx.write("t1.txt", "t1\n");
    fx.commit("T1");
    fx.git(&["switch", "-q", "feature"]);
    fx.git(&["merge", "-q", "--no-commit", "main"]);
    if extra {
        fx.write("extra.txt", "ours\n");
    }
    let m = fx.commit("M: merge main");
    fx.write("c.txt", "c\n");
    fx.commit("f3");
    fx.git(&["switch", "-q", "main"]);
    fx.write("t2.txt", "t2\n");
    fx.commit("T2");
    fx.git(&["switch", "-q", "feature"]);
    m
}

/// A merge of trunk flattens away when the branch moves onto newer trunk:
/// the restack lands, the merge is reported dropped, and the branch is a
/// straight line.
#[test]
fn restack_flattens_a_trunk_merge_and_says_so() {
    let fx = repo();
    let m = trunk_merge(&fx, false);

    let output = ff(&fx, &["restack"]);
    assert!(output.status.success(), "{}", out(&output));
    let text = stdout(&output);
    assert!(text.contains("replayed 3 commit(s) onto main"), "{text}");
    assert!(
        text.contains(&format!("dropped {} \"M: merge main\"", &m[..8])),
        "{text}"
    );
    assert!(
        fx.git(&["rev-list", "--merges", "feature"])
            .trim()
            .is_empty(),
        "no merge left on the branch"
    );
    assert_eq!(
        fx.git(&["log", "--format=%s", "feature"])
            .lines()
            .collect::<Vec<_>>(),
        ["f3", "f2", "f1", "T2", "T1", "T0"]
    );
}

/// A merge that carried an edit of its own becomes an ordinary commit that
/// holds the edit, and the restack names it.
#[test]
fn restack_flattens_a_merge_that_carried_an_edit_and_names_it() {
    let fx = repo();
    let m = trunk_merge(&fx, true);

    let output = ff(&fx, &["restack"]);
    assert!(output.status.success(), "{}", out(&output));
    let text = stdout(&output);
    assert!(
        text.contains(&format!("flattened {} \"M: merge main\"", &m[..8])),
        "{text}"
    );
    assert!(!text.contains("dropped"), "{text}");
    assert!(
        fx.git(&["rev-list", "--merges", "feature"])
            .trim()
            .is_empty(),
        "no merge left on the branch"
    );
    assert_eq!(fx.git(&["show", "feature~1:extra.txt"]), "ours\n");
    assert_eq!(
        fx.git(&["log", "--format=%s", "feature"])
            .lines()
            .collect::<Vec<_>>(),
        ["f3", "M: merge main", "f2", "f1", "T2", "T1", "T0"]
    );
}
