//! `ff worktree` — the family run through the real `ff` binary: the list
//! that reads the survey, and the two mutators that make and take a
//! worktree.
//!
//! The linked-worktree model lives in `tests/worktree.rs`; this file is the
//! rest: the bare form, the envelope name, the orphan section, the floor
//! the add lays, and the capture the remove makes before it destroys.

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

fn ok_at(dir: &Path, args: &[&str]) -> String {
    let output = ff_at(dir, args);
    assert!(
        output.status.success(),
        "ff {args:?} failed: {}",
        out(&output)
    );
    stdout(&output)
}

fn ok(fx: &Fixture, args: &[&str]) -> String {
    ok_at(&fx.path(), args)
}

fn repo() -> Fixture {
    let fx = Fixture::new();
    fx.set_config("user.name", "Worktree Tester");
    fx.set_config("user.email", "worktree@test.test");
    fx
}

/// The linked worktree beside the repository, on a branch of its own.
fn bay(fx: &Fixture) -> PathBuf {
    let bay = fx.root().join("bay");
    fx.git(&["worktree", "add", "-q", "-b", "side", bay.to_str().unwrap()]);
    bay
}

/// A bay with work in it, then removed: the setup every orphan test wants.
fn gone_bay(fx: &Fixture) -> PathBuf {
    fx.write("a.txt", "a\n");
    fx.commit("init");
    let bay = bay(fx);
    // A status inside the bay gives its chain a floor, so the orphan row
    // has a tip to name.
    ok_at(&bay, &["status"]);
    fx.git(&["worktree", "remove", "--force", bay.to_str().unwrap()]);
    bay
}

/// The bare form and the spelled-out one emit the same envelope: the cmd is
/// the full path, never the family — the rule the `ff op` family exists to
/// enforce.
#[test]
fn bare_worktree_is_the_list() {
    let fx = repo();
    fx.write("a.txt", "a\n");
    fx.commit("init");

    let a = json(&ff(&fx, &["worktree", "--json"]));
    let b = json(&ff(&fx, &["worktree", "list", "--json"]));
    assert_eq!(a["cmd"], "worktree list");
    assert_eq!(b["cmd"], "worktree list");
    assert_eq!(a["data"], b["data"]);
}

/// A repository with no bays still lists its one worktree, and marks it
/// current.
#[test]
fn a_lone_repository_lists_one_row() {
    let fx = repo();
    fx.write("a.txt", "a\n");
    fx.commit("init");

    let body = ok(&fx, &["worktree", "list"]);
    assert!(body.contains("main"), "names main: {body}");
    assert!(body.contains("* main"), "marks it current: {body}");
}

/// A bay is a row of its own: its id, and the branch it stands on.
#[test]
fn a_bay_is_listed_with_its_branch() {
    let fx = repo();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    bay(&fx);

    let body = ok(&fx, &["worktree", "list"]);
    assert!(body.contains("bay"), "names the bay: {body}");
    assert!(body.contains("side"), "names its branch: {body}");

    let v = json(&ff(&fx, &["worktree", "list", "--json"]));
    let worktrees = v["data"]["worktrees"].as_array().expect("worktrees array");
    assert_eq!(worktrees.len(), 2, "two entries: {v}");
}

/// A bay's chain outlives its checkout: after `git worktree remove` it is
/// the orphan section, and its tip survives for the reader to take.
#[test]
fn a_removed_bay_becomes_an_orphan_row() {
    let fx = repo();
    gone_bay(&fx);

    let body = ok(&fx, &["worktree", "list"]);
    assert!(
        body.contains("chains whose worktree is gone"),
        "the section header: {body}"
    );
    assert!(body.contains("bay"), "names the gone bay: {body}");

    let v = json(&ff(&fx, &["worktree", "list", "--json"]));
    let orphans = v["data"]["orphans"].as_array().expect("orphans array");
    assert_eq!(orphans.len(), 1, "one orphan: {v}");
    assert!(
        orphans[0]["tip"].is_string(),
        "tip is not null: {}",
        orphans[0]
    );
}

/// The orphan row says how to use its tip: it names `ff restore` and
/// `--at-op`. The row is the front door for a deleted bay, and the test
/// that it says so is the point.
#[test]
fn the_orphan_row_names_the_way_back() {
    let fx = repo();
    gone_bay(&fx);

    let body = ok(&fx, &["worktree", "list"]);
    assert!(body.contains("ff restore"), "names ff restore: {body}");
    assert!(body.contains("--at-op"), "names --at-op: {body}");
}

/// The `branch/checked-out-elsewhere` exit has pointed at `git worktree
/// list` since fufu had no worktree verb; it names fufu's own verb now.
#[test]
fn the_exit_now_names_fufus_own_verb() {
    let fx = repo();

    let body = ok(&fx, &["explain", "branch/checked-out-elsewhere"]);
    assert!(
        body.contains("ff worktree list"),
        "names ff worktree list: {body}"
    );
}

/// The layout fufu wrote is git's own, so git's listing is the oracle: the
/// add exits 0 and `git worktree list` names the path it made.
#[test]
fn add_makes_a_worktree_git_agrees_with() {
    let fx = repo();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    let bay = fx.root().join("bay");

    let body = ok(&fx, &["worktree", "add", bay.to_str().unwrap(), "side"]);
    assert!(body.contains("made bay"), "names the worktree: {body}");
    assert!(bay.is_dir(), "the checkout stands: {bay:?}");

    let listing = fx.git(&["worktree", "list"]);
    assert!(
        ff_testsupport::paths::names(&listing, &bay),
        "git names the path: {listing}"
    );
}

/// The earn over `git worktree add`: the floor is laid by the add itself, so
/// the new row already carries a tip the moment it is listed.
#[test]
fn add_lays_the_floor() {
    let fx = repo();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    let bay = fx.root().join("bay");

    ok(&fx, &["worktree", "add", bay.to_str().unwrap(), "side"]);

    let v = json(&ff(&fx, &["worktree", "list", "--json"]));
    let worktrees = v["data"]["worktrees"].as_array().expect("worktrees array");
    let row = worktrees
        .iter()
        .find(|w| w["id"] == "bay")
        .expect("the new row is listed");
    assert!(
        row["tip"].is_string(),
        "the floor gave it a tip, not null: {row}"
    );
}

/// With no branch named, the directory's name becomes a new branch — and
/// the output says that is what happened.
#[test]
fn add_says_when_it_made_the_branch() {
    let fx = repo();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    let bay = fx.root().join("meadow");

    let body = ok(&fx, &["worktree", "add", bay.to_str().unwrap()]);
    assert!(
        body.contains("on a new branch"),
        "says the branch is new: {body}"
    );

    let branches = fx.git(&["branch", "--list", "meadow"]);
    assert!(
        branches.contains("meadow"),
        "the branch is named after the directory: {branches:?}"
    );
}

/// git allows a branch in one tree, and fufu enforces it: a second add on a
/// branch a live worktree holds is refused.
#[test]
fn add_refuses_a_branch_another_worktree_holds() {
    let fx = repo();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    bay(&fx);
    let other = fx.root().join("other");

    let output = ff(&fx, &["worktree", "add", other.to_str().unwrap(), "side"]);
    assert!(
        !output.status.success(),
        "took the branch the bay holds: {}",
        out(&output)
    );
    let v = json(&ff(
        &fx,
        &["--json", "worktree", "add", other.to_str().unwrap(), "side"],
    ));
    assert_eq!(v["error"]["id"], "branch/checked-out-elsewhere");
    assert!(!other.exists(), "no checkout was made");
}

/// The load-bearing earn: the capture lands in the bay's own chain before
/// the teardown, so the uncommitted work survives, the output says where it
/// went, and the bay stands under the orphan section afterwards.
#[test]
fn remove_captures_before_it_destroys() {
    let fx = repo();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    let bay = bay(&fx);
    std::fs::write(bay.join("flight.txt"), "work in flight\n").expect("write in the bay");

    let body = ok(&fx, &["worktree", "remove", bay.to_str().unwrap()]);
    assert!(!bay.exists(), "the checkout is gone: {bay:?}");
    assert!(
        body.contains("captured first as"),
        "says where the work went: {body}"
    );

    let v = json(&ff(&fx, &["worktree", "list", "--json"]));
    let orphans = v["data"]["orphans"].as_array().expect("orphans array");
    assert!(
        orphans.iter().any(|o| o["id"] == "bay"),
        "the bay stands under the orphan section: {v}"
    );
}

/// The listing prints ids and a person thinks in paths; both are the same
/// worktree, so both remove it the same way.
#[test]
fn remove_takes_a_path_or_an_id() {
    // By path.
    let fx = repo();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    let bay = bay(&fx);
    ok(&fx, &["worktree", "remove", bay.to_str().unwrap()]);
    assert!(!bay.exists(), "gone by path: {bay:?}");

    // By the id `ff worktree list` shows.
    let fx2 = repo();
    fx2.write("a.txt", "a\n");
    fx2.commit("init");
    let harbor = fx2.root().join("harbor");
    ok(&fx2, &["worktree", "add", harbor.to_str().unwrap()]);

    let v = json(&ff(&fx2, &["worktree", "list", "--json"]));
    let worktrees = v["data"]["worktrees"].as_array().expect("worktrees array");
    let id = worktrees
        .iter()
        .find(|w| w["path"] == ff_testsupport::paths::real(&harbor))
        .expect("the harbor's row")["id"]
        .as_str()
        .expect("an id")
        .to_string();

    ok(&fx2, &["worktree", "remove", &id]);
    assert!(!harbor.exists(), "gone by id: {harbor:?}");
}

/// The worktree you are standing in cannot be taken from under your feet;
/// the refusal is coded, and the tree is still there afterwards.
#[test]
fn remove_refuses_the_worktree_you_are_in() {
    let fx = repo();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    let bay = bay(&fx);

    let output = ff_at(&bay, &["worktree", "remove", bay.to_str().unwrap()]);
    assert!(
        !output.status.success(),
        "removed the worktree it stands in: {}",
        out(&output)
    );
    let v = json(&ff_at(
        &bay,
        &["--json", "worktree", "remove", bay.to_str().unwrap()],
    ));
    assert_eq!(v["error"]["id"], "worktree/is-current");
    assert!(bay.exists(), "the worktree still stands: {bay:?}");
}

/// The path a person types and the path fufu records are not the same string
/// whenever the checkout is reached through a symlink — which is every macOS,
/// where the temporary directory lives under `/var` and resolves to
/// `/private/var`. A removal has to find the worktree anyway.
///
/// Unix only: making a symlink on Windows needs a privilege the test runner
/// does not have. The platform this reproduces is covered either way.
#[test]
#[cfg(unix)]
fn a_worktree_reached_through_a_symlink_is_still_found() {
    let fx = repo();
    fx.write("a.txt", "a\n");
    fx.commit("init");

    let real = fx.root().join("real");
    std::fs::create_dir(&real).expect("create the real directory");
    let link = fx.root().join("link");
    std::os::unix::fs::symlink(&real, &link).expect("symlink");

    // Added through the link, so what fufu records resolves past it.
    let typed = link.join("bay");
    ok(&fx, &["worktree", "add", typed.to_str().unwrap(), "side"]);

    let v = json(&ff(&fx, &["worktree", "list", "--json"]));
    let row = v["data"]["worktrees"]
        .as_array()
        .expect("worktrees array")
        .iter()
        .find(|w| w["id"] == "bay")
        .expect("the bay's row")
        .clone();
    assert_eq!(
        row["path"],
        ff_testsupport::paths::real(&typed),
        "the row holds the resolved path"
    );
    assert_ne!(
        row["path"],
        *typed.to_str().unwrap(),
        "the two spellings differ, or this test proves nothing"
    );

    // Removed by the spelling a person would have typed.
    ok(&fx, &["worktree", "remove", typed.to_str().unwrap()]);
    assert!(!typed.exists(), "gone through the link: {typed:?}");
    assert!(!real.join("bay").exists(), "gone for real: {real:?}");
}
