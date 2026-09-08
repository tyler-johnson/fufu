//! `ff pull` and `ff push`, end to end against the real `ff` binary.
//! Every test is offline: all but one run `--no-fetch` on a repository with
//! no remote, and the one that really fetches
//! ([`a_half_removed_worktree_admin_dir_does_not_stop_the_fetch`]) aims at a
//! bare remote on the filesystem beside it. Nothing here reaches the
//! network. Covers the base-axis replay, the JSON envelopes, the
//! nothing-to-pull state, the three scopes — bare, names, `--all` — and a
//! push with nowhere to send.

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
    fx.set_config("user.name", "Pull Tester");
    fx.set_config("user.email", "pull@test.test");
    fx
}

/// A no-remote stack with the fixture standing on `feature`: `main` moved two
/// commits ahead of the fork point, and `feature` carries one commit of its
/// own. Distinct files throughout, so the replay is clean.
fn moved_base(fx: &Fixture) {
    fx.write("root.txt", "root\n");
    fx.commit("root");
    fx.write("a.txt", "a\n");
    fx.commit("a");

    fx.git(&["switch", "-q", "-c", "feature"]);
    fx.write("f.txt", "f\n");
    fx.commit("f1");

    fx.git(&["switch", "-q", "main"]);
    fx.write("m1.txt", "m1\n");
    fx.commit("m1");
    fx.write("m2.txt", "m2\n");
    fx.commit("m2");

    fx.git(&["switch", "-q", "feature"]);
}

/// A commit's date says nothing about where it stands: a teammate's morning
/// commit pushed after lunch, or a cherry-pick of old work, sits on `main`
/// with a committer date older than the fork point, and the branch's own
/// commit can be older still. The count and the replay walk the range by
/// ancestry, not by clock, so neither drops it.
#[test]
fn a_commit_older_than_the_fork_point_still_counts_and_replays() {
    let fx = repo();
    fx.write("root.txt", "root\n");
    fx.commit("root");
    fx.write("a.txt", "a\n");
    fx.commit("a");
    // A day before the fixture clock's epoch, so older than the fork point.
    let old = "@1599913600 +0000";
    let dated = [("GIT_AUTHOR_DATE", old), ("GIT_COMMITTER_DATE", old)];
    fx.git(&["switch", "-q", "-c", "feature"]);
    fx.write("f.txt", "f\n");
    fx.git(&["add", "-A"]);
    fx.git_env_in(&fx.path(), &["commit", "-q", "-m", "f1"], &dated);
    fx.git(&["switch", "-q", "main"]);
    fx.write("m1.txt", "m1\n");
    fx.git(&["add", "-A"]);
    fx.git_env_in(&fx.path(), &["commit", "-q", "-m", "m1"], &dated);
    fx.git(&["switch", "-q", "feature"]);

    let output = ff(&fx, &["pull", "--no-fetch"]);
    assert!(output.status.success(), "{}", out(&output));
    let text = stdout(&output);
    assert!(text.contains("main moved ahead by 1 commit(s)"), "{text}");
    assert!(text.contains("replayed 1 commit(s) onto main"), "{text}");
    assert!(
        fx.try_git(&["merge-base", "--is-ancestor", "main", "feature"])
            .status
            .success(),
        "feature sits on the moved main"
    );
    assert!(
        fx.path().join("f.txt").exists(),
        "the branch's commit was replayed"
    );
    assert!(
        fx.path().join("m1.txt").exists(),
        "main's commit is beneath it"
    );
}

#[test]
fn pull_replays_onto_a_moved_base() {
    let fx = repo();
    moved_base(&fx);

    let feature_before = fx.git(&["rev-parse", "feature"]).trim().to_string();
    let output = ff(&fx, &["pull", "--no-fetch"]);
    assert!(output.status.success(), "{}", out(&output));
    let text = stdout(&output);
    assert!(text.contains("main moved ahead by 2 commit(s)"), "{text}");
    assert!(text.contains("replayed 1 commit(s) onto main"), "{text}");

    let feature_after = fx.git(&["rev-parse", "feature"]).trim().to_string();
    assert_ne!(feature_before, feature_after, "the tip must move");
}

#[test]
fn the_json_envelope_carries_the_report() {
    let fx = repo();
    moved_base(&fx);

    let output = ff(&fx, &["--json", "pull", "--no-fetch"]);
    assert!(output.status.success(), "{}", out(&output));
    let v = json(&output);
    assert_eq!(v["cmd"], "pull");
    assert_eq!(v["data"]["pull"]["branch"], "feature");
    assert!(v["data"]["pull"].get("remote").is_some());
    assert!(v["data"]["pull"].get("base").is_some());
    // Pull sends nothing, so the envelope carries no `pushed` at all — the
    // count of what is left for push is what replaced it.
    assert!(v["data"].get("pushed").is_none());
    assert!(v["data"]["pull"].get("pending").is_some());
}

#[test]
fn nothing_to_pull_says_so() {
    let fx = repo();
    fx.write("root.txt", "root\n");
    fx.commit("root");

    let output = ff(&fx, &["pull", "--no-fetch"]);
    assert!(output.status.success(), "{}", out(&output));
    let text = stdout(&output);
    assert!(text.contains("nothing to pull"), "{text}");
}

#[test]
fn push_with_no_remote_says_so_and_sends_nothing() {
    let fx = repo();
    fx.write("root.txt", "root\n");
    fx.commit("root");

    let output = ff(&fx, &["push"]);
    assert!(output.status.success(), "{}", out(&output));
    assert!(stdout(&output).contains("no remote"), "{}", stdout(&output));
}

#[test]
fn the_push_json_envelope_carries_its_own_report() {
    let fx = repo();
    fx.write("root.txt", "root\n");
    fx.commit("root");

    let output = ff(&fx, &["--json", "push"]);
    assert!(output.status.success(), "{}", out(&output));
    let v = json(&output);
    assert_eq!(v["cmd"], "push");
    assert_eq!(v["data"]["push"]["branch"], "main");
    assert_eq!(v["data"]["pushed"], false);
}

/// Pull names the other half rather than doing it: a branch that just lined
/// up and still has commits its shared copy lacks says so, and says which
/// verb sends them.
#[test]
fn pull_points_at_push_when_something_is_waiting() {
    let fx = repo();
    moved_base(&fx);
    // Give feature a shared copy that is one commit behind it.
    let base = fx.git(&["rev-parse", "feature~1"]).trim().to_string();
    fx.git(&["update-ref", "refs/remotes/origin/feature", &base]);
    fx.set_config("branch.feature.remote", "origin");
    fx.set_config("branch.feature.merge", "refs/heads/feature");
    fx.set_config("remote.origin.url", "/nonexistent/remote.git");
    fx.set_config("remote.origin.fetch", "+refs/heads/*:refs/remotes/origin/*");

    let output = ff(&fx, &["pull", "--no-fetch"]);
    assert!(output.status.success(), "{}", out(&output));
    let text = stdout(&output);
    assert!(text.contains("to push"), "{text}");
    assert!(text.contains("ff push"), "{text}");
}

/// `--dry-run` says which push this would be and spends nothing. The tail
/// line is the tell: the real run says the push left the machine, and this
/// one must not, because it did not.
#[test]
fn push_dry_run_says_would_and_sends_nothing() {
    let fx = repo();
    fx.write("root.txt", "root\n");
    fx.commit("root");
    fx.set_config("remote.origin.url", "/nonexistent/remote.git");
    fx.set_config("remote.origin.fetch", "+refs/heads/*:refs/remotes/origin/*");

    // A real push here would spawn git and fail against a dead remote.
    // The dry run must not reach it at all, which is why success is the
    // assertion that matters.
    let output = ff(&fx, &["push", "--dry-run"]);
    assert!(output.status.success(), "{}", out(&output));
    let text = stdout(&output);
    assert!(text.contains("would create"), "{text}");
    assert!(text.contains("nothing was sent"), "{text}");
    assert!(
        !text.contains("left the machine"),
        "a dry run must not claim the irreversible act: {text}"
    );

    // -n is the same flag, matching ff trim.
    let short = ff(&fx, &["push", "-n"]);
    assert_eq!(stdout(&short), text, "-n and --dry-run are one flag");
}

#[test]
fn the_dry_run_envelope_says_it_sent_nothing() {
    let fx = repo();
    fx.write("root.txt", "root\n");
    fx.commit("root");
    fx.set_config("remote.origin.url", "/nonexistent/remote.git");
    fx.set_config("remote.origin.fetch", "+refs/heads/*:refs/remotes/origin/*");

    let output = ff(&fx, &["--json", "push", "-n"]);
    assert!(output.status.success(), "{}", out(&output));
    let v = json(&output);
    assert_eq!(v["cmd"], "push");
    assert_eq!(v["data"]["pushed"], false);
    assert_eq!(v["data"]["push"]["dry_run"], true);
}

/// A sibling worktree's admin dir caught mid-removal — a `gitdir` file with
/// no `commondir` beside it — used to fail the whole pull: gix's fetch opens
/// every admin dir `worktrees()` lists to find the branches checked out
/// elsewhere, and propagates the open error. git's own walk skips such a
/// directory and fetches anyway, so fufu hands the fetch to git there.
///
/// The assertion is the fetch, not the fallback: the run exits 0 and the
/// tracking ref carries a commit this repository had never seen.
/// `tests/zero_spawn.rs` is where *which* process did it is pinned.
#[test]
fn a_half_removed_worktree_admin_dir_does_not_stop_the_fetch() {
    let fx = Fixture::new_cloned();
    fx.write("a.txt", "a\n");
    fx.commit("one");
    fx.git(&["push", "-q", "-u", "origin", "main"]);

    // A second clone is where the commit this repository has never seen
    // comes from — a bare remote has no worktree to make one in.
    let other = fx.root().join("other");
    fx.git_in(
        fx.root(),
        &[
            "clone",
            "-q",
            &fx.remote_path().to_string_lossy(),
            &other.to_string_lossy(),
        ],
    );
    std::fs::write(other.join("b.txt"), "b\n").unwrap();
    fx.git_in(&other, &["add", "-A"]);
    fx.git_in(&other, &["commit", "-q", "-m", "theirs"]);
    fx.git_in(&other, &["push", "-q", "origin", "main"]);
    let theirs = fx.git_in(&other, &["rev-parse", "HEAD"]).trim().to_string();
    assert!(
        fx.try_git(&["cat-file", "-e", &theirs]).status.code() != Some(0),
        "test fixture: the commit must be one this repository has never seen"
    );

    // The half-removed admin dir: `gitdir` present, `commondir` gone. git
    // ignores it; gix's fetch used to stop on it.
    let ghost = fx.path().join(".git/worktrees/ghost");
    std::fs::create_dir_all(&ghost).unwrap();
    std::fs::write(
        ghost.join("gitdir"),
        format!("{}/.git\n", fx.root().join("ghost").display()),
    )
    .unwrap();

    let output = ff(&fx, &["pull"]);
    assert!(output.status.success(), "{}", out(&output));
    assert_eq!(
        fx.git(&["rev-parse", "refs/remotes/origin/main"]).trim(),
        theirs,
        "the tracking ref must carry what the remote advertised"
    );
}

/// A branch you are not standing on follows its shared copy: `side` is
/// pushed, a second clone moves it, and `--all` from `main` fast-forwards
/// it without a switch. The envelope carries the move under `branches`.
#[test]
fn pull_moves_a_branch_you_are_not_on() {
    let fx = Fixture::new_cloned();
    fx.write("a.txt", "a\n");
    fx.commit("one");
    fx.git(&["push", "-q", "-u", "origin", "main"]);
    fx.git(&["switch", "-q", "-c", "side"]);
    fx.write("s.txt", "s\n");
    let s1 = fx.commit("s1");
    fx.git(&["push", "-q", "-u", "origin", "side"]);
    fx.git(&["switch", "-q", "main"]);

    let other = fx.root().join("other");
    fx.git_in(
        fx.root(),
        &[
            "clone",
            "-q",
            &fx.remote_path().to_string_lossy(),
            &other.to_string_lossy(),
        ],
    );
    fx.git_in(&other, &["switch", "-q", "side"]);
    std::fs::write(other.join("t.txt"), "t\n").unwrap();
    fx.git_in(&other, &["add", "-A"]);
    fx.git_in(&other, &["commit", "-q", "-m", "theirs"]);
    fx.git_in(&other, &["push", "-q", "origin", "side"]);
    let theirs = fx.git_in(&other, &["rev-parse", "HEAD"]).trim().to_string();
    assert_ne!(theirs, s1);

    let output = ff(&fx, &["--json", "pull", "--all"]);
    assert!(output.status.success(), "{}", out(&output));
    let v = json(&output);
    assert_eq!(v["cmd"], "pull");
    assert_eq!(v["data"]["pull"]["branch"], "main");
    let row = &v["data"]["pull"]["branches"][0]["Pulled"];
    assert_eq!(row["branch"], "side", "{v}");
    let moved = &row["remote"]["Moved"];
    assert_eq!(moved["fast_forward"], true, "{v}");
    assert_eq!(moved["behind"], 1, "{v}");
    assert_eq!(moved["old"], s1, "{v}");
    assert_eq!(moved["new"], theirs, "{v}");
    assert_eq!(
        fx.git(&["rev-parse", "side"]).trim(),
        theirs,
        "side followed its shared copy without a switch"
    );
}

/// A replay on a branch you are not standing on conflicts: the hold stands
/// on that branch alone, the run finishes, the envelope still comes out,
/// and the exit is 3, the same word a hold underfoot says.
#[test]
fn a_hold_on_another_branch_exits_3() {
    let fx = Fixture::new_cloned();
    fx.set_config("user.name", "Pull Tester");
    fx.set_config("user.email", "pull@test.test");
    fx.write("a.txt", "a\n");
    fx.commit("one");
    fx.git(&["push", "-q", "-u", "origin", "main"]);
    fx.git(&["switch", "-q", "-c", "side"]);
    fx.write("shared.txt", "base\n");
    fx.commit("s1");
    fx.git(&["push", "-q", "-u", "origin", "side"]);

    // A second clone rewrites the shared line and pushes it...
    let other = fx.root().join("other");
    fx.git_in(
        fx.root(),
        &[
            "clone",
            "-q",
            &fx.remote_path().to_string_lossy(),
            &other.to_string_lossy(),
        ],
    );
    fx.git_in(&other, &["switch", "-q", "side"]);
    std::fs::write(other.join("shared.txt"), "theirs\n").unwrap();
    fx.git_in(&other, &["add", "-A"]);
    fx.git_in(&other, &["commit", "-q", "-m", "theirs"]);
    fx.git_in(&other, &["push", "-q", "origin", "side"]);

    // ...and so does this one, on the branch it then steps off.
    fx.write("shared.txt", "mine\n");
    let mine = fx.commit("mine");
    fx.git(&["switch", "-q", "main"]);

    let output = ff(&fx, &["--json", "pull", "--all"]);
    assert_eq!(output.status.code(), Some(3), "{}", out(&output));
    let v = json(&output);
    assert_eq!(v["cmd"], "pull", "{v}");
    assert_eq!(v["data"]["pull"]["branch"], "main");
    let row = &v["data"]["pull"]["branches"][0]["Pulled"];
    assert_eq!(row["branch"], "side", "{v}");
    let held = &row["remote"]["Ran"]["outcome"]["held"];
    assert_eq!(held["branch"], "side", "{v}");
    assert_eq!(held["paths"][0], "shared.txt", "{v}");
    assert_eq!(
        fx.git(&["rev-parse", "side"]).trim(),
        mine,
        "a hold touches nothing"
    );
}

/// One branch stacked above `feature` through `ff start`, which records the
/// branch beneath it. Leaves the fixture standing on `feature`.
fn stacked_child(fx: &Fixture) {
    let started = ff(fx, &["start", "feature", "-b", "child"]);
    assert!(started.status.success(), "{}", out(&started));
    fx.write("g.txt", "g\n");
    fx.commit("g1");
    let back = ff(fx, &["switch", "feature"]);
    assert!(back.status.success(), "{}", out(&back));
}

#[test]
fn pull_says_what_followed_above() {
    let fx = repo();
    moved_base(&fx);
    stacked_child(&fx);
    let child_before = fx.git(&["rev-parse", "child"]).trim().to_string();

    let output = ff(&fx, &["pull", "--no-fetch"]);
    assert!(output.status.success(), "{}", out(&output));
    let text = stdout(&output);
    assert!(text.contains("replayed 1 commit(s) onto main"), "{text}");
    assert!(
        text.contains("child followed feature: replayed 1 commit(s)"),
        "{text}"
    );

    let child_after = fx.git(&["rev-parse", "child"]).trim().to_string();
    assert_ne!(child_before, child_after, "child followed");
    assert!(
        fx.try_git(&["merge-base", "--is-ancestor", "feature", "child"])
            .status
            .success(),
        "child sits on the pulled feature"
    );
}

/// Standing on `child` when `main` moved: `feature` replays first and its
/// cascade carries `child` with the open change, so the branch underfoot
/// says nothing of its own. The working-tree line still prints, under the
/// branch underfoot's lines, and one undo puts the tree and every tip back.
#[test]
fn the_working_tree_line_prints_when_a_cascade_carried_the_branch_underfoot() {
    let fx = repo();
    moved_base(&fx);
    stacked_child(&fx);
    let switched = ff(&fx, &["switch", "child"]);
    assert!(switched.status.success(), "{}", out(&switched));
    fx.write("wip.txt", "open\n");
    let feature_before = fx.git(&["rev-parse", "feature"]).trim().to_string();
    let child_before = fx.git(&["rev-parse", "child"]).trim().to_string();

    let output = ff(&fx, &["pull", "--no-fetch"]);
    assert!(output.status.success(), "{}", out(&output));
    let text = stdout(&output);
    assert!(
        text.starts_with("updated the working copy (2 file(s)); your change is still open\n"),
        "{text}"
    );
    assert!(
        text.contains("feature\n    main moved ahead by 2 commit(s)\n    replayed 1 commit(s) onto main\n    child followed feature: replayed 1 commit(s)\n"),
        "{text}"
    );
    assert!(text.contains("undo: ff undo"), "{text}");
    assert!(
        fx.path().join("m2.txt").exists(),
        "the tree moved with child"
    );
    assert_eq!(
        std::fs::read_to_string(fx.path().join("wip.txt")).unwrap(),
        "open\n",
        "the open change came along"
    );

    let undone = ff(&fx, &["undo"]);
    assert!(undone.status.success(), "{}", out(&undone));
    assert_eq!(fx.git(&["rev-parse", "feature"]).trim(), feature_before);
    assert_eq!(fx.git(&["rev-parse", "child"]).trim(), child_before);
    assert!(!fx.path().join("m2.txt").exists(), "the tree came back");
    assert_eq!(
        std::fs::read_to_string(fx.path().join("wip.txt")).unwrap(),
        "open\n",
        "the open change is still open"
    );
}

#[test]
fn the_pull_envelope_carries_the_cascade() {
    let fx = repo();
    moved_base(&fx);
    stacked_child(&fx);

    let output = ff(&fx, &["--json", "pull", "--no-fetch"]);
    assert!(output.status.success(), "{}", out(&output));
    let v = json(&output);
    let cascade = &v["data"]["pull"]["base"]["Ran"]["outcome"]["restacked"]["cascade"];
    assert_eq!(cascade["moved"][0]["branch"], "child", "{v}");
    assert_eq!(cascade["moved"][0]["base"], "feature", "{v}");
}

/// A busy repository on a bare remote, left standing on `main`: `side` is
/// pushed and a second clone advances it by one commit, `a` is started off
/// `main` with one commit of its own and no shared copy, and then `main`
/// moves one commit ahead locally (writing `shared.txt`), so both sit on a
/// stale base. Returns `side`'s tip before the run and the second clone's.
fn side_moved_and_a_stale(fx: &Fixture) -> (String, String) {
    fx.write("a.txt", "a\n");
    fx.commit("one");
    fx.git(&["push", "-q", "-u", "origin", "main"]);
    fx.git(&["switch", "-q", "-c", "side"]);
    fx.write("s.txt", "s\n");
    let s1 = fx.commit("s1");
    fx.git(&["push", "-q", "-u", "origin", "side"]);
    fx.git(&["switch", "-q", "-c", "a", "main"]);
    fx.write("a1.txt", "a1\n");
    fx.commit("a1");
    fx.git(&["switch", "-q", "main"]);

    let other = fx.root().join("other");
    fx.git_in(
        fx.root(),
        &[
            "clone",
            "-q",
            &fx.remote_path().to_string_lossy(),
            &other.to_string_lossy(),
        ],
    );
    fx.git_in(&other, &["switch", "-q", "side"]);
    std::fs::write(other.join("t.txt"), "t\n").unwrap();
    fx.git_in(&other, &["add", "-A"]);
    fx.git_in(&other, &["commit", "-q", "-m", "theirs"]);
    fx.git_in(&other, &["push", "-q", "origin", "side"]);
    let theirs = fx.git_in(&other, &["rev-parse", "HEAD"]).trim().to_string();

    fx.write("shared.txt", "theirs\n");
    fx.commit("two");
    (s1, theirs)
}

/// The human render names every branch that did something, in report
/// order, with what happened to it indented beneath: `a` replayed onto the
/// moved `main` by its base axis, `side` fast-forwarded to its shared copy
/// and then replayed too. One undo hint for the whole run.
#[test]
fn the_human_render_names_every_branch_that_moved() {
    let fx = Fixture::new_cloned();
    let (s1, theirs) = side_moved_and_a_stale(&fx);

    let output = ff(&fx, &["pull", "--all"]);
    assert!(output.status.success(), "{}", out(&output));
    let text = stdout(&output);
    let a_block = "a\n    main moved ahead by 1 commit(s)\n    replayed 1 commit(s) onto main\n";
    let side_block = "side\n    fast-forwarded to origin/side (1 commit(s))\n    main moved ahead by \
                      1 commit(s)\n    replayed 2 commit(s) onto main\n";
    assert!(text.contains(a_block), "{text}");
    assert!(text.contains(side_block), "{text}");
    assert!(
        text.find(a_block).unwrap() < text.find(side_block).unwrap(),
        "report order: {text}"
    );
    assert_eq!(text.matches("undo: ff undo").count(), 1, "{text}");
    assert!(
        text.trim_end().ends_with("undo: ff undo"),
        "the undo hint closes the run: {text}"
    );
    assert!(!text.contains("nothing to pull"), "{text}");

    assert_ne!(fx.git(&["rev-parse", "side"]).trim(), s1, "side moved");
    // The replay onto main rewrote what arrived, so the tree is the witness.
    assert_eq!(
        fx.git(&["rev-parse", "side:t.txt"]).trim().len(),
        40,
        "side carries what arrived"
    );
    assert_eq!(
        fx.git(&["rev-parse", "refs/remotes/origin/side"]).trim(),
        theirs,
        "the tracking ref carries what the remote advertised"
    );
    assert!(
        fx.try_git(&["merge-base", "--is-ancestor", "main", "side"])
            .status
            .success(),
        "side sits on the moved main"
    );
    assert!(
        fx.try_git(&["merge-base", "--is-ancestor", "main", "a"])
            .status
            .success(),
        "a sits on the moved main"
    );
}

/// A branch checked out in another worktree is named with where it is, and
/// left alone: the block says which verb to run there.
#[test]
fn a_skipped_worktree_branch_is_named_with_its_path() {
    let fx = repo();
    fx.write("root.txt", "root\n");
    fx.commit("root");
    fx.git(&["branch", "side"]);
    let bay = fx.root().join("bay");
    let added = ff(&fx, &["worktree", "add", &bay.to_string_lossy(), "side"]);
    assert!(added.status.success(), "{}", out(&added));
    fx.write("m.txt", "m\n");
    fx.commit("m1");

    let output = ff(&fx, &["pull", "--all", "--no-fetch"]);
    assert!(output.status.success(), "{}", out(&output));
    let text = stdout(&output);
    assert!(text.contains("side\n    checked out in "), "{text}");
    assert!(text.contains("bay"), "the path is named: {text}");
    assert!(
        text.contains("— skipped; run ff restack side there"),
        "{text}"
    );
    assert!(!text.contains("undo: ff undo"), "nothing moved: {text}");
    assert_eq!(
        fx.git(&["rev-parse", "side"]).trim(),
        fx.git(&["rev-parse", "main~1"]).trim(),
        "side stayed where it stood"
    );
}

/// A replay on a branch not underfoot that conflicts holds that branch, the
/// block says so under its name, the closing line says where to go, and
/// the exit is 3.
#[test]
fn a_held_branch_is_named_and_the_exit_is_3() {
    let fx = repo();
    fx.write("shared.txt", "base\n");
    fx.commit("root");
    fx.git(&["switch", "-q", "-c", "side"]);
    fx.write("shared.txt", "mine\n");
    let mine = fx.commit("mine");
    fx.git(&["switch", "-q", "main"]);
    fx.write("shared.txt", "theirs\n");
    fx.commit("theirs");

    let output = ff(&fx, &["pull", "--all", "--no-fetch"]);
    assert_eq!(output.status.code(), Some(3), "{}", out(&output));
    let text = stdout(&output);
    assert!(text.contains("side\n    held: replaying "), "{text}");
    assert!(text.contains("conflicts in shared.txt"), "{text}");
    assert!(
        text.contains("the restack of 1 commit on side is waiting — nothing was written"),
        "{text}"
    );
    assert!(
        text.trim_end()
            .ends_with("1 branch(es) held — ff switch side, then ff resolve"),
        "{text}"
    );
    assert!(!text.contains("undo: ff undo"), "nothing landed: {text}");
    assert_eq!(
        fx.git(&["rev-parse", "side"]).trim(),
        mine,
        "a hold touches nothing"
    );

    let status = ff(&fx, &["--json", "status"]);
    assert!(status.status.success(), "{}", out(&status));
    let v = json(&status);
    assert!(
        v["data"]["held"].is_null(),
        "the hold stands on side, not on the branch underfoot: {v}"
    );
}

/// Three branches with nothing to do say nothing: the render stays the one
/// line it was before it knew about them.
#[test]
fn nothing_to_pull_stays_one_line() {
    let fx = repo();
    fx.write("root.txt", "root\n");
    fx.commit("root");
    for name in ["a", "b", "c"] {
        fx.git(&["branch", name]);
    }

    let output = ff(&fx, &["pull", "--all", "--no-fetch"]);
    assert!(output.status.success(), "{}", out(&output));
    assert_eq!(stdout(&output), "nothing to pull\n");
}

/// The keys of a JSON object, sorted, so a shape can be pinned in one
/// comparison.
fn keys(v: &serde_json::Value) -> Vec<&str> {
    let mut keys: Vec<&str> = v
        .as_object()
        .unwrap_or_else(|| panic!("an object, got {v}"))
        .keys()
        .map(String::as_str)
        .collect();
    keys.sort_unstable();
    keys
}

/// The row for `branch` in `branches`, whatever its variant, as the pair of
/// variant name and payload.
fn row_of<'a>(v: &'a serde_json::Value, branch: &str) -> (&'a str, &'a serde_json::Value) {
    v["data"]["pull"]["branches"]
        .as_array()
        .expect("branches is an array")
        .iter()
        .map(|row| {
            let (tag, payload) = row.as_object().unwrap().iter().next().unwrap();
            (tag.as_str(), payload)
        })
        .find(|(_, payload)| payload["branch"] == branch)
        .unwrap_or_else(|| panic!("no row for {branch} in {v}"))
}

/// The envelope carries every other branch under `branches`, one row each,
/// tagged by variant and naming the branch; a `Pulled` row carries its two
/// axes tagged the same way. The exact keys serde produces are pinned here
/// so a script can rely on them.
#[test]
fn the_json_envelope_lists_the_other_branches() {
    let fx = Fixture::new_cloned();
    let (s1, theirs) = side_moved_and_a_stale(&fx);
    // A hold from an earlier run: `h` rewrote the line `main` then rewrote.
    fx.git(&["switch", "-q", "-c", "h", "main~1"]);
    fx.write("shared.txt", "mine\n");
    fx.commit("h1");
    fx.git(&["switch", "-q", "main"]);
    let held = ff(&fx, &["restack", "h"]);
    assert_eq!(held.status.code(), Some(3), "{}", out(&held));
    // A branch open in another worktree.
    fx.git(&["branch", "w"]);
    let bay = fx.root().join("bay");
    let added = ff(&fx, &["worktree", "add", &bay.to_string_lossy(), "w"]);
    assert!(added.status.success(), "{}", out(&added));

    let output = ff(&fx, &["--json", "pull", "--all"]);
    assert!(output.status.success(), "{}", out(&output));
    let v = json(&output);
    assert_eq!(v["cmd"], "pull");
    assert_eq!(v["data"]["undo"], "ff undo");
    assert_eq!(
        v["data"]["pull"]["branches"].as_array().map(Vec::len),
        Some(4),
        "{v}"
    );
    for row in v["data"]["pull"]["branches"].as_array().unwrap() {
        assert_eq!(keys(row).len(), 1, "one variant tag per row: {row}");
    }

    let (tag, side) = row_of(&v, "side");
    assert_eq!(tag, "Pulled");
    assert_eq!(keys(side), ["base", "branch", "remote"], "{v}");
    assert_eq!(keys(&side["remote"]), ["Moved"], "{v}");
    let moved = &side["remote"]["Moved"];
    assert_eq!(
        keys(moved),
        ["behind", "fast_forward", "name", "new", "old"],
        "{v}"
    );
    assert_eq!(moved["name"], "origin/side");
    assert_eq!(moved["fast_forward"], true);
    assert_eq!(moved["behind"], 1);
    assert_eq!(moved["old"], s1);
    assert_eq!(moved["new"], theirs);

    let (tag, a) = row_of(&v, "a");
    assert_eq!(tag, "Pulled");
    assert_eq!(a["remote"], "NoRemote", "a unit variant is its name: {v}");
    assert_eq!(keys(&a["base"]), ["Ran"], "{v}");
    let ran = &a["base"]["Ran"];
    assert_eq!(keys(ran), ["name", "outcome"], "{v}");
    assert_eq!(ran["name"], "main");
    assert_eq!(keys(&ran["outcome"]), ["restacked"], "{v}");
    assert_eq!(ran["outcome"]["restacked"]["branch"], "a");
    assert_eq!(ran["outcome"]["restacked"]["replayed"], 1);

    let (tag, w) = row_of(&v, "w");
    assert_eq!(tag, "Elsewhere");
    assert_eq!(keys(w), ["branch", "path"], "{v}");
    assert!(
        w["path"].as_str().unwrap().contains("bay"),
        "the worktree is named: {v}"
    );

    let (tag, h) = row_of(&v, "h");
    assert_eq!(tag, "Held");
    assert_eq!(keys(h), ["branch", "verb"], "{v}");
    assert_eq!(h["verb"], "restack");
}

/// Standing on `topic`, off a pushed `main`, with `other` a sibling on the
/// same base: a teammate's commit lands on `main` from a second clone.
/// Returns `other`'s tip and the teammate's.
fn topic_on_a_moved_main(fx: &Fixture) -> (String, String) {
    fx.set_config("user.name", "Pull Tester");
    fx.set_config("user.email", "pull@test.test");
    fx.write("a.txt", "a\n");
    fx.commit("one");
    fx.git(&["push", "-q", "-u", "origin", "main"]);
    fx.git(&["switch", "-q", "-c", "other"]);
    fx.write("o.txt", "o\n");
    let other_before = fx.commit("o1");
    fx.git(&["switch", "-q", "-c", "topic", "main"]);
    fx.write("t.txt", "t\n");
    fx.commit("t1");

    let teammate = fx.root().join("teammate");
    fx.git_in(
        fx.root(),
        &[
            "clone",
            "-q",
            &fx.remote_path().to_string_lossy(),
            &teammate.to_string_lossy(),
        ],
    );
    std::fs::write(teammate.join("m.txt"), "m\n").unwrap();
    fx.git_in(&teammate, &["add", "-A"]);
    fx.git_in(&teammate, &["commit", "-q", "-m", "theirs"]);
    fx.git_in(&teammate, &["push", "-q", "origin", "main"]);
    let theirs = fx
        .git_in(&teammate, &["rev-parse", "HEAD"])
        .trim()
        .to_string();
    (other_before, theirs)
}

/// Bare pull is the branch you stand on, and the base beneath it comes
/// level with its own shared copy first: standing on `topic`, a teammate's
/// commit on `main` fast-forwards `main` and `topic` replays onto it, while
/// `other`, a sibling on the same stale base, is left where it stood — its
/// shared copy unread, its block absent.
#[test]
fn bare_pull_is_the_branch_underfoot_and_the_base_beneath_it() {
    let fx = Fixture::new_cloned();
    let (other_before, theirs) = topic_on_a_moved_main(&fx);

    let output = ff(&fx, &["pull"]);
    assert!(output.status.success(), "{}", out(&output));
    let text = stdout(&output);
    assert!(text.contains("main moved ahead by 1 commit(s)\n"), "{text}");
    assert!(text.contains("replayed 1 commit(s) onto main\n"), "{text}");
    assert!(
        text.contains("main\n    fast-forwarded to origin/main (1 commit(s))\n"),
        "the base beneath is in the run: {text}"
    );
    assert!(!text.contains("other\n"), "a sibling is not: {text}");
    assert_eq!(
        fx.git(&["rev-parse", "main"]).trim(),
        theirs,
        "main is level"
    );
    assert!(
        fx.try_git(&["merge-base", "--is-ancestor", "main", "topic"])
            .status
            .success(),
        "topic sits on the moved main"
    );
    assert_eq!(
        fx.git(&["rev-parse", "other"]).trim(),
        other_before,
        "other stayed where it stood"
    );
}

/// The same with trunk read off `origin/HEAD`, the way every clone of a
/// repository with a HEAD has it: `topic`'s base is `refs/remotes/origin/
/// main`, and local `main`, whose shared copy that is, is still in the run,
/// so it fast-forwards rather than being left behind the branch that
/// replayed onto what arrived.
#[test]
fn bare_pull_carries_local_main_when_trunk_is_origin_main() {
    let fx = Fixture::new_cloned();
    let (other_before, theirs) = topic_on_a_moved_main(&fx);
    fx.git(&[
        "symbolic-ref",
        "refs/remotes/origin/HEAD",
        "refs/remotes/origin/main",
    ]);

    let output = ff(&fx, &["pull"]);
    assert!(output.status.success(), "{}", out(&output));
    let text = stdout(&output);
    assert!(text.contains("main moved ahead by 1 commit(s)\n"), "{text}");
    assert!(
        text.contains("main\n    fast-forwarded to origin/main (1 commit(s))\n"),
        "local main is in the run: {text}"
    );
    assert!(!text.contains("other\n"), "a sibling is not: {text}");
    assert_eq!(
        fx.git(&["rev-parse", "main"]).trim(),
        theirs,
        "main is level"
    );
    assert_eq!(fx.git(&["rev-parse", "other"]).trim(), other_before);
}

/// Standing on `main` with nothing beneath it, bare pull is `main` alone:
/// `side` and `a` keep their tips and their blocks stay out of the report,
/// and what is waiting to push is still named.
#[test]
fn bare_pull_from_main_leaves_the_other_branches_alone() {
    let fx = Fixture::new_cloned();
    let (s1, _theirs) = side_moved_and_a_stale(&fx);
    let a_before = fx.git(&["rev-parse", "a"]).trim().to_string();

    let output = ff(&fx, &["pull"]);
    assert!(output.status.success(), "{}", out(&output));
    let text = stdout(&output);
    assert!(text.contains("1 commit(s) to push — ff push"), "{text}");
    assert!(!text.contains("side"), "{text}");
    assert!(!text.contains("undo: ff undo"), "nothing moved: {text}");
    assert_eq!(fx.git(&["rev-parse", "side"]).trim(), s1);
    assert_eq!(fx.git(&["rev-parse", "a"]).trim(), a_before);
}

/// One name is that branch and the base beneath it: `side` fast-forwards to
/// its shared copy and replays onto the moved `main`, `a` is left alone,
/// and one undo hint closes the run.
#[test]
fn one_name_pulls_that_branch() {
    let fx = Fixture::new_cloned();
    let (s1, _theirs) = side_moved_and_a_stale(&fx);
    let a_before = fx.git(&["rev-parse", "a"]).trim().to_string();

    let output = ff(&fx, &["pull", "side"]);
    assert!(output.status.success(), "{}", out(&output));
    let text = stdout(&output);
    let side_block = "side\n    fast-forwarded to origin/side (1 commit(s))\n    main moved ahead by \
                      1 commit(s)\n    replayed 2 commit(s) onto main\n";
    assert!(text.contains(side_block), "{text}");
    assert!(!text.contains("a\n    "), "a is not in the run: {text}");
    assert!(text.trim_end().ends_with("undo: ff undo"), "{text}");
    assert_ne!(fx.git(&["rev-parse", "side"]).trim(), s1, "side moved");
    assert_eq!(fx.git(&["rev-parse", "a"]).trim(), a_before, "a stayed");
    assert!(
        fx.try_git(&["merge-base", "--is-ancestor", "main", "side"])
            .status
            .success(),
        "side sits on the moved main"
    );

    let undone = ff(&fx, &["undo"]);
    assert!(undone.status.success(), "{}", out(&undone));
    assert_eq!(fx.git(&["rev-parse", "side"]).trim(), s1, "one undo");
}

/// Several names are each pulled, reported in the order the ref namespace
/// lists them whatever order they were typed in, and a name resolves the
/// way `ff restack` resolves one: an unambiguous prefix names the branch.
#[test]
fn several_names_pull_each_in_namespace_order() {
    let fx = Fixture::new_cloned();
    let (s1, _theirs) = side_moved_and_a_stale(&fx);
    let a_before = fx.git(&["rev-parse", "a"]).trim().to_string();

    let output = ff(&fx, &["pull", "sid", "a"]);
    assert!(output.status.success(), "{}", out(&output));
    let text = stdout(&output);
    let a_block = "a\n    main moved ahead by 1 commit(s)\n    replayed 1 commit(s) onto main\n";
    let side_block = "side\n    fast-forwarded to origin/side (1 commit(s))\n";
    assert!(text.contains(a_block), "{text}");
    assert!(text.contains(side_block), "{text}");
    assert!(
        text.find(a_block).unwrap() < text.find(side_block).unwrap(),
        "report order: {text}"
    );
    assert_eq!(text.matches("undo: ff undo").count(), 1, "{text}");
    assert_ne!(fx.git(&["rev-parse", "side"]).trim(), s1, "side moved");
    assert_ne!(fx.git(&["rev-parse", "a"]).trim(), a_before, "a moved");
}

/// A named branch whose replay conflicts holds, the others in the run still
/// move, the report names the hold and where to go, and the exit is 3.
#[test]
fn a_named_branch_that_holds_exits_3_and_the_rest_still_move() {
    let fx = repo();
    fx.write("shared.txt", "base\n");
    fx.commit("root");
    fx.git(&["switch", "-q", "-c", "side"]);
    fx.write("shared.txt", "mine\n");
    let mine = fx.commit("mine");
    fx.git(&["switch", "-q", "-c", "clean", "main"]);
    fx.write("c.txt", "c\n");
    let clean_before = fx.commit("c1");
    fx.git(&["switch", "-q", "main"]);
    fx.write("shared.txt", "theirs\n");
    fx.commit("theirs");

    let output = ff(&fx, &["pull", "--no-fetch", "side", "clean"]);
    assert_eq!(output.status.code(), Some(3), "{}", out(&output));
    let text = stdout(&output);
    assert!(
        text.contains(
            "clean\n    main moved ahead by 1 commit(s)\n    replayed 1 commit(s) onto main\n"
        ),
        "{text}"
    );
    assert!(text.contains("side\n    held: replaying "), "{text}");
    assert!(text.contains("undo: ff undo"), "clean landed: {text}");
    assert!(
        text.trim_end()
            .ends_with("1 branch(es) held — ff switch side, then ff resolve"),
        "{text}"
    );
    assert_eq!(
        fx.git(&["rev-parse", "side"]).trim(),
        mine,
        "a hold touches nothing"
    );
    assert_ne!(
        fx.git(&["rev-parse", "clean"]).trim(),
        clean_before,
        "clean moved"
    );

    let v = json(&ff(&fx, &["--json", "status"]));
    assert!(v["data"]["held"].is_null(), "the hold is on side: {v}");
}

/// A name that resolves to nothing is refused before the fetch, with the
/// error a misspelled branch gets everywhere else.
#[test]
fn an_unknown_name_is_refused_before_the_fetch() {
    let fx = Fixture::new_cloned();
    fx.write("a.txt", "a\n");
    fx.commit("one");
    fx.git(&["push", "-q", "-u", "origin", "main"]);

    let output = ff(&fx, &["pull", "nope"]);
    assert_eq!(output.status.code(), Some(1), "{}", out(&output));
    assert!(
        stderr(&output).contains("no branch named nope"),
        "{}",
        out(&output)
    );
    assert!(
        !stdout(&output).contains("fetching from"),
        "refused ahead of the network: {}",
        out(&output)
    );

    let output = ff(&fx, &["--json", "pull", "nope"]);
    assert_eq!(output.status.code(), Some(1), "{}", out(&output));
    assert_eq!(json(&output)["error"]["id"], "branch/not-found");
}

/// Names and `--all` say two different things about which branches to
/// visit, so together they are a usage error.
#[test]
fn names_and_all_are_refused_together() {
    let fx = repo();
    fx.write("root.txt", "root\n");
    fx.commit("root");
    fx.git(&["branch", "side"]);

    let output = ff(&fx, &["pull", "--all", "side"]);
    assert_eq!(output.status.code(), Some(2), "{}", out(&output));
}

/// Standing on `side` and naming `a`, a sibling on `main`: the run is `a`
/// and `main` beneath it, and the branch underfoot is not in it. Both of
/// its axes read `NotNamed`, no row is filed for it, and the human render
/// says nothing about what it has waiting to push.
#[test]
fn the_envelope_says_when_the_branch_underfoot_was_not_named() {
    let fx = Fixture::new_cloned();
    let (s1, _theirs) = side_moved_and_a_stale(&fx);
    fx.git(&["switch", "-q", "side"]);

    let output = ff(&fx, &["--json", "pull", "a"]);
    assert!(output.status.success(), "{}", out(&output));
    let v = json(&output);
    assert_eq!(v["data"]["pull"]["branch"], "side");
    assert_eq!(v["data"]["pull"]["remote"], "NotNamed", "{v}");
    assert_eq!(v["data"]["pull"]["base"], "NotNamed", "{v}");
    let rows: Vec<&str> = v["data"]["pull"]["branches"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| {
            row.as_object().unwrap().values().next().unwrap()["branch"]
                .as_str()
                .unwrap()
        })
        .collect();
    assert_eq!(rows, ["a", "main"], "the name and the base beneath it: {v}");
    let (_, a) = row_of(&v, "a");
    assert_eq!(
        a["base"]["Ran"]["outcome"]["restacked"]["replayed"], 1,
        "{v}"
    );
    assert_eq!(fx.git(&["rev-parse", "side"]).trim(), s1, "side stayed");

    let undone = ff(&fx, &["undo"]);
    assert!(undone.status.success(), "{}", out(&undone));
    let output = ff(&fx, &["pull", "a"]);
    assert!(output.status.success(), "{}", out(&output));
    let text = stdout(&output);
    assert!(
        !text.contains("ff push"),
        "side is not being talked about: {text}"
    );
    assert!(
        text.contains("a\n    main moved ahead by 1 commit(s)\n"),
        "{text}"
    );
}

/// The ids `ff op log --json` prints, captures included — the whole log,
/// so a verb that wrote anything at all shows up here.
fn op_count(fx: &Fixture) -> usize {
    let output = ff(fx, &["op", "log", "--json", "-n", "0"]);
    assert!(output.status.success(), "{}", out(&output));
    json(&output)["data"]["ops"]
        .as_array()
        .expect("ops array")
        .len()
}

/// The tips of `names`, for asserting that none of them moved.
fn tips(fx: &Fixture, names: &[&str]) -> Vec<String> {
    names
        .iter()
        .map(|name| fx.git(&["rev-parse", name]).trim().to_string())
        .collect()
}

/// A bare dry run says what would fast-forward and what would replay, in
/// the conditional, and writes nothing: standing on `topic` over a `main` a
/// teammate moved, `main` would fast-forward and `topic` would replay onto
/// it, and afterwards every tip, the working copy, and the operation log
/// stand where they stood. `-n` is the same flag.
#[test]
fn a_dry_run_says_would_and_writes_nothing() {
    let fx = Fixture::new_cloned();
    let (_other_before, _theirs) = topic_on_a_moved_main(&fx);
    let before = tips(&fx, &["main", "topic", "other"]);
    let ops = op_count(&fx);

    let output = ff(&fx, &["pull", "--dry-run"]);
    assert!(output.status.success(), "{}", out(&output));
    assert_eq!(
        stdout(&output),
        "fetching from origin\n\
         main moved ahead by 1 commit(s)\n\
         would replay 1 commit(s) onto main\n\
         would update the working copy (1 file(s))\n\
         not published yet — ff push\n\
         main\n    would fast-forward to origin/main (1 commit(s))\n\
         nothing was written — drop --dry-run to pull\n"
    );
    assert_eq!(
        tips(&fx, &["main", "topic", "other"]),
        before,
        "no tip moved"
    );
    assert!(!fx.path().join("m.txt").exists(), "the working copy stayed");
    assert_eq!(
        fx.git(&["status", "--porcelain"]).trim(),
        "",
        "nothing open"
    );
    assert_eq!(op_count(&fx), ops, "nothing on the operation log");

    let short = ff(&fx, &["pull", "-n"]);
    assert_eq!(
        stdout(&short),
        stdout(&output),
        "-n and --dry-run are one flag"
    );
}

/// The dry run fetches: the report cannot say what the shared copy holds
/// without it, and a fetch moves the remote-tracking ref and nothing a
/// person stands on. So `origin/main` carries the teammate's commit
/// afterwards while local `main` does not, and FETCH_HEAD is not written.
#[test]
fn a_dry_run_fetches_and_moves_only_the_tracking_ref() {
    let fx = Fixture::new_cloned();
    let (_other_before, theirs) = topic_on_a_moved_main(&fx);
    let main_before = fx.git(&["rev-parse", "main"]).trim().to_string();
    assert_ne!(
        fx.git(&["rev-parse", "refs/remotes/origin/main"]).trim(),
        theirs,
        "test fixture: the tracking ref must not know the commit yet"
    );

    let output = ff(&fx, &["pull", "-n"]);
    assert!(output.status.success(), "{}", out(&output));
    assert_eq!(
        fx.git(&["rev-parse", "refs/remotes/origin/main"]).trim(),
        theirs,
        "the fetch moved the tracking ref"
    );
    assert_eq!(
        fx.git(&["rev-parse", "main"]).trim(),
        main_before,
        "the local branch stayed"
    );
    assert!(
        !fx.path().join(".git/FETCH_HEAD").exists(),
        "the fetch writes tracking refs and objects and nothing else"
    );
}

/// `--no-fetch` beside the dry run reads what is already here and writes
/// nothing at all: no fetch line, the tracking ref where it stood, and a
/// report with nothing to say about a base whose move has not arrived.
#[test]
fn a_dry_run_with_no_fetch_reads_what_you_have() {
    let fx = Fixture::new_cloned();
    let (_other_before, theirs) = topic_on_a_moved_main(&fx);
    let tracking_before = fx
        .git(&["rev-parse", "refs/remotes/origin/main"])
        .trim()
        .to_string();
    let before = tips(&fx, &["main", "topic", "other"]);

    let output = ff(&fx, &["pull", "-n", "--no-fetch"]);
    assert!(output.status.success(), "{}", out(&output));
    assert_eq!(stdout(&output), "not published yet — ff push\n");
    assert_eq!(
        fx.git(&["rev-parse", "refs/remotes/origin/main"]).trim(),
        tracking_before,
        "no fetch ran"
    );
    assert_ne!(tracking_before, theirs);
    assert_eq!(tips(&fx, &["main", "topic", "other"]), before);

    let v = json(&ff(&fx, &["--json", "pull", "-n", "--no-fetch"]));
    assert_eq!(v["data"]["pull"]["fetched"], false, "{v}");
    assert_eq!(v["data"]["pull"]["dry_run"], true, "{v}");
}

/// A dry run under a name is that branch and the base beneath it, in the
/// conditional: `side` would fast-forward to its shared copy and replay
/// onto the moved `main`, `a` is not in the run, and nothing moved.
#[test]
fn a_dry_run_under_names_says_would_for_each() {
    let fx = Fixture::new_cloned();
    let (_s1, _theirs) = side_moved_and_a_stale(&fx);
    let before = tips(&fx, &["main", "side", "a"]);
    let ops = op_count(&fx);

    let output = ff(&fx, &["pull", "-n", "side"]);
    assert!(output.status.success(), "{}", out(&output));
    assert_eq!(
        stdout(&output),
        "fetching from origin\n\
         1 commit(s) to push — ff push\n\
         side\n    would fast-forward to origin/side (1 commit(s))\n    main moved ahead by \
         1 commit(s)\n    would replay 2 commit(s) onto main\n\
         nothing was written — drop --dry-run to pull\n"
    );
    assert_eq!(tips(&fx, &["main", "side", "a"]), before, "no tip moved");
    assert_eq!(op_count(&fx), ops, "nothing on the operation log");
}

/// A dry run under `--all` names every branch that would do something, in
/// report order, and moves none of them.
#[test]
fn a_dry_run_under_all_names_every_branch() {
    let fx = Fixture::new_cloned();
    let (_s1, _theirs) = side_moved_and_a_stale(&fx);
    let before = tips(&fx, &["main", "side", "a"]);
    let ops = op_count(&fx);

    let output = ff(&fx, &["pull", "-n", "--all"]);
    assert!(output.status.success(), "{}", out(&output));
    assert_eq!(
        stdout(&output),
        "fetching from origin\n\
         1 commit(s) to push — ff push\n\
         a\n    main moved ahead by 1 commit(s)\n    would replay 1 commit(s) onto main\n\
         side\n    would fast-forward to origin/side (1 commit(s))\n    main moved ahead by \
         1 commit(s)\n    would replay 2 commit(s) onto main\n\
         nothing was written — drop --dry-run to pull\n"
    );
    assert_eq!(tips(&fx, &["main", "side", "a"]), before, "no tip moved");
    assert_eq!(op_count(&fx), ops, "nothing on the operation log");
}

/// A dry run says which branch would hold and where, beside the one that
/// would move, exits 3 the way a real run does, and records no hold: the
/// real run afterwards still finds the conflict for itself rather than a
/// hold already standing.
#[test]
fn a_dry_run_shows_a_would_hold_beside_a_would_move() {
    let fx = repo();
    fx.write("shared.txt", "base\n");
    fx.commit("root");
    fx.git(&["switch", "-q", "-c", "side"]);
    fx.write("shared.txt", "mine\n");
    let mine = fx.commit("mine");
    fx.git(&["switch", "-q", "-c", "clean", "main"]);
    fx.write("c.txt", "c\n");
    fx.commit("c1");
    fx.git(&["switch", "-q", "main"]);
    fx.write("shared.txt", "theirs\n");
    fx.commit("theirs");
    let before = tips(&fx, &["main", "side", "clean"]);
    let ops = op_count(&fx);

    let output = ff(&fx, &["pull", "-n", "--no-fetch", "side", "clean"]);
    assert_eq!(output.status.code(), Some(3), "{}", out(&output));
    assert_eq!(
        stdout(&output),
        format!(
            "clean\n    main moved ahead by 1 commit(s)\n    would replay 1 commit(s) onto main\n\
             side\n    would hold: replaying {} \"mine\" conflicts in shared.txt\n\
             nothing was written — drop --dry-run to pull\n\
             1 branch(es) would hold\n",
            &mine[..8]
        )
    );
    assert_eq!(
        tips(&fx, &["main", "side", "clean"]),
        before,
        "no tip moved"
    );
    assert_eq!(op_count(&fx), ops, "nothing on the operation log");

    let v = json(&ff(
        &fx,
        &["--json", "pull", "-n", "--no-fetch", "side", "clean"],
    ));
    assert_eq!(v["data"]["pull"]["dry_run"], true, "{v}");
    let (_, side) = row_of(&v, "side");
    assert_eq!(
        side["base"]["Ran"]["outcome"]["held"]["paths"][0], "shared.txt",
        "{v}"
    );

    let real = ff(&fx, &["pull", "--no-fetch", "side", "clean"]);
    assert_eq!(real.status.code(), Some(3), "{}", out(&real));
    assert!(
        stdout(&real).contains("side\n    held: replaying "),
        "the dry run recorded no hold: {}",
        stdout(&real)
    );
}

/// The dry run's envelope is the real run's with `dry_run` true and `undo`
/// null: the same rows under `branches`, the same axes, the same numbers,
/// and no tip moved.
#[test]
fn the_dry_run_envelope_carries_the_rows_and_says_so() {
    let fx = Fixture::new_cloned();
    let (s1, theirs) = side_moved_and_a_stale(&fx);
    let before = tips(&fx, &["main", "side", "a"]);

    let output = ff(&fx, &["--json", "pull", "-n", "--all"]);
    assert!(output.status.success(), "{}", out(&output));
    let v = json(&output);
    assert_eq!(v["cmd"], "pull");
    assert_eq!(v["data"]["pull"]["dry_run"], true, "{v}");
    assert_eq!(v["data"]["pull"]["fetched"], true, "{v}");
    assert!(v["data"]["undo"].is_null(), "nothing to undo: {v}");
    let (tag, side) = row_of(&v, "side");
    assert_eq!(tag, "Pulled");
    let moved = &side["remote"]["Moved"];
    assert_eq!(moved["fast_forward"], true, "{v}");
    assert_eq!(moved["behind"], 1, "{v}");
    assert_eq!(moved["old"], s1, "{v}");
    assert_eq!(moved["new"], theirs, "{v}");
    assert_eq!(
        side["base"]["Ran"]["outcome"]["restacked"]["replayed"], 2,
        "{v}"
    );
    let (_, a) = row_of(&v, "a");
    assert_eq!(
        a["base"]["Ran"]["outcome"]["restacked"]["replayed"], 1,
        "{v}"
    );
    assert_eq!(tips(&fx, &["main", "side", "a"]), before, "no tip moved");

    let real = json(&ff(&fx, &["--json", "pull", "--all"]));
    assert_eq!(real["data"]["pull"]["dry_run"], false, "{real}");
    assert_eq!(real["data"]["undo"], "ff undo", "{real}");
    assert_ne!(
        fx.git(&["rev-parse", "side"]).trim(),
        s1,
        "the real run moved it"
    );
}
