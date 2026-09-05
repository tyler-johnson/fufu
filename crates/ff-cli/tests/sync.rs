//! `ff sync` and `ff publish`, end to end against the real `ff` binary.
//! Every test is offline: all but one run `--no-fetch` on a repository with
//! no remote, and the one that really fetches
//! ([`a_half_removed_worktree_admin_dir_does_not_stop_the_fetch`]) aims at a
//! bare remote on the filesystem beside it. Nothing here reaches the
//! network. Covers the base-axis replay, the JSON envelopes, the
//! nothing-to-sync state, publish with nowhere to send, and the two git
//! words that now point at the pair.

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
    fx.set_config("user.name", "Sync Tester");
    fx.set_config("user.email", "sync@test.test");
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

#[test]
fn sync_replays_onto_a_moved_base() {
    let fx = repo();
    moved_base(&fx);

    let feature_before = fx.git(&["rev-parse", "feature"]).trim().to_string();
    let output = ff(&fx, &["sync", "--no-fetch"]);
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

    let output = ff(&fx, &["--json", "sync", "--no-fetch"]);
    assert!(output.status.success(), "{}", out(&output));
    let v = json(&output);
    assert_eq!(v["cmd"], "sync");
    assert_eq!(v["data"]["sync"]["branch"], "feature");
    assert!(v["data"]["sync"].get("remote").is_some());
    assert!(v["data"]["sync"].get("base").is_some());
    // Sync sends nothing, so the envelope carries no `pushed` at all — the
    // count of what is left for publish is what replaced it.
    assert!(v["data"].get("pushed").is_none());
    assert!(v["data"]["sync"].get("pending").is_some());
}

#[test]
fn nothing_to_sync_says_so() {
    let fx = repo();
    fx.write("root.txt", "root\n");
    fx.commit("root");

    let output = ff(&fx, &["sync", "--no-fetch"]);
    assert!(output.status.success(), "{}", out(&output));
    let text = stdout(&output);
    assert!(text.contains("nothing to sync"), "{text}");
}

#[test]
fn ff_pull_now_points_at_sync() {
    let fx = repo();

    let output = ff(&fx, &["pull"]);
    assert_eq!(output.status.code(), Some(2), "{}", out(&output));
    let said = stderr(&output);
    assert!(said.contains("ff sync"), "{said}");
}

/// The word fufu deliberately does not have, now that the outgoing half is
/// its own verb: `ff push` is a question, and `ff publish` is the answer.
#[test]
fn ff_push_now_points_at_publish() {
    let fx = repo();

    let output = ff(&fx, &["push"]);
    assert_eq!(output.status.code(), Some(2), "{}", out(&output));
    let said = stderr(&output);
    assert!(said.contains("ff publish"), "{said}");
    assert!(
        said.contains("lease"),
        "the reason it is not git's push: {said}"
    );
}

#[test]
fn publish_with_no_remote_says_so_and_sends_nothing() {
    let fx = repo();
    fx.write("root.txt", "root\n");
    fx.commit("root");

    let output = ff(&fx, &["publish"]);
    assert!(output.status.success(), "{}", out(&output));
    assert!(stdout(&output).contains("no remote"), "{}", stdout(&output));
}

#[test]
fn the_publish_json_envelope_carries_its_own_report() {
    let fx = repo();
    fx.write("root.txt", "root\n");
    fx.commit("root");

    let output = ff(&fx, &["--json", "publish"]);
    assert!(output.status.success(), "{}", out(&output));
    let v = json(&output);
    assert_eq!(v["cmd"], "publish");
    assert_eq!(v["data"]["publish"]["branch"], "main");
    assert_eq!(v["data"]["pushed"], false);
}

/// Sync names the other half rather than doing it: a branch that just lined
/// up and still has commits its shared copy lacks says so, and says which
/// verb sends them.
#[test]
fn sync_points_at_publish_when_something_is_waiting() {
    let fx = repo();
    moved_base(&fx);
    // Give feature a shared copy that is one commit behind it.
    let base = fx.git(&["rev-parse", "feature~1"]).trim().to_string();
    fx.git(&["update-ref", "refs/remotes/origin/feature", &base]);
    fx.set_config("branch.feature.remote", "origin");
    fx.set_config("branch.feature.merge", "refs/heads/feature");
    fx.set_config("remote.origin.url", "/nonexistent/remote.git");
    fx.set_config("remote.origin.fetch", "+refs/heads/*:refs/remotes/origin/*");

    let output = ff(&fx, &["sync", "--no-fetch"]);
    assert!(output.status.success(), "{}", out(&output));
    let text = stdout(&output);
    assert!(text.contains("to publish"), "{text}");
    assert!(text.contains("ff publish"), "{text}");
}

/// `--dry-run` says which push this would be and spends nothing. The tail
/// line is the tell: the real run says the push left the machine, and this
/// one must not, because it did not.
#[test]
fn publish_dry_run_says_would_and_sends_nothing() {
    let fx = repo();
    fx.write("root.txt", "root\n");
    fx.commit("root");
    fx.set_config("remote.origin.url", "/nonexistent/remote.git");
    fx.set_config("remote.origin.fetch", "+refs/heads/*:refs/remotes/origin/*");

    // A real publish here would spawn git and fail against a dead remote.
    // The dry run must not reach it at all, which is why success is the
    // assertion that matters.
    let output = ff(&fx, &["publish", "--dry-run"]);
    assert!(output.status.success(), "{}", out(&output));
    let text = stdout(&output);
    assert!(text.contains("would create"), "{text}");
    assert!(text.contains("nothing was sent"), "{text}");
    assert!(
        !text.contains("left the machine"),
        "a dry run must not claim the irreversible act: {text}"
    );

    // -n is the same flag, matching ff trim.
    let short = ff(&fx, &["publish", "-n"]);
    assert_eq!(stdout(&short), text, "-n and --dry-run are one flag");
}

#[test]
fn the_dry_run_envelope_says_it_sent_nothing() {
    let fx = repo();
    fx.write("root.txt", "root\n");
    fx.commit("root");
    fx.set_config("remote.origin.url", "/nonexistent/remote.git");
    fx.set_config("remote.origin.fetch", "+refs/heads/*:refs/remotes/origin/*");

    let output = ff(&fx, &["--json", "publish", "-n"]);
    assert!(output.status.success(), "{}", out(&output));
    let v = json(&output);
    assert_eq!(v["cmd"], "publish");
    assert_eq!(v["data"]["pushed"], false);
    assert_eq!(v["data"]["publish"]["dry_run"], true);
}

/// A sibling worktree's admin dir caught mid-removal — a `gitdir` file with
/// no `commondir` beside it — used to fail the whole sync: gix's fetch opens
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

    let output = ff(&fx, &["sync"]);
    assert!(output.status.success(), "{}", out(&output));
    assert_eq!(
        fx.git(&["rev-parse", "refs/remotes/origin/main"]).trim(),
        theirs,
        "the tracking ref must carry what the remote advertised"
    );
}

/// A branch you are not standing on follows its shared copy: `side` is
/// pushed, a second clone moves it, and a sync from `main` fast-forwards it
/// without a switch. The envelope carries the move under `branches`.
#[test]
fn sync_moves_a_branch_you_are_not_on() {
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

    let output = ff(&fx, &["--json", "sync"]);
    assert!(output.status.success(), "{}", out(&output));
    let v = json(&output);
    assert_eq!(v["cmd"], "sync");
    assert_eq!(v["data"]["sync"]["branch"], "main");
    let row = &v["data"]["sync"]["branches"][0]["Synced"];
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
    fx.set_config("user.name", "Sync Tester");
    fx.set_config("user.email", "sync@test.test");
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

    let output = ff(&fx, &["--json", "sync"]);
    assert_eq!(output.status.code(), Some(3), "{}", out(&output));
    let v = json(&output);
    assert_eq!(v["cmd"], "sync", "{v}");
    assert_eq!(v["data"]["sync"]["branch"], "main");
    let row = &v["data"]["sync"]["branches"][0]["Synced"];
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
fn sync_says_what_followed_above() {
    let fx = repo();
    moved_base(&fx);
    stacked_child(&fx);
    let child_before = fx.git(&["rev-parse", "child"]).trim().to_string();

    let output = ff(&fx, &["sync", "--no-fetch"]);
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
        "child sits on the synced feature"
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

    let output = ff(&fx, &["sync", "--no-fetch"]);
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
fn the_sync_envelope_carries_the_cascade() {
    let fx = repo();
    moved_base(&fx);
    stacked_child(&fx);

    let output = ff(&fx, &["--json", "sync", "--no-fetch"]);
    assert!(output.status.success(), "{}", out(&output));
    let v = json(&output);
    let cascade = &v["data"]["sync"]["base"]["Ran"]["outcome"]["restacked"]["cascade"];
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

    let output = ff(&fx, &["sync"]);
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
    assert!(!text.contains("nothing to sync"), "{text}");

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

    let output = ff(&fx, &["sync", "--no-fetch"]);
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

    let output = ff(&fx, &["sync", "--no-fetch"]);
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
fn nothing_to_sync_stays_one_line() {
    let fx = repo();
    fx.write("root.txt", "root\n");
    fx.commit("root");
    for name in ["a", "b", "c"] {
        fx.git(&["branch", name]);
    }

    let output = ff(&fx, &["sync", "--no-fetch"]);
    assert!(output.status.success(), "{}", out(&output));
    assert_eq!(stdout(&output), "nothing to sync\n");
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
    v["data"]["sync"]["branches"]
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
/// tagged by variant and naming the branch; a `Synced` row carries its two
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

    let output = ff(&fx, &["--json", "sync"]);
    assert!(output.status.success(), "{}", out(&output));
    let v = json(&output);
    assert_eq!(v["cmd"], "sync");
    assert_eq!(v["data"]["undo"], "ff undo");
    assert_eq!(
        v["data"]["sync"]["branches"].as_array().map(Vec::len),
        Some(4),
        "{v}"
    );
    for row in v["data"]["sync"]["branches"].as_array().unwrap() {
        assert_eq!(keys(row).len(), 1, "one variant tag per row: {row}");
    }

    let (tag, side) = row_of(&v, "side");
    assert_eq!(tag, "Synced");
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
    assert_eq!(tag, "Synced");
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
