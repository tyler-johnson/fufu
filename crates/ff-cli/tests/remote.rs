//! `ff publish` and `ff sync` against a real bare remote.
//!
//! Deliberately its own file. `tests/sync.rs` promises every case is offline
//! — `--no-fetch` on a repository whose remote is a URL nobody contacts —
//! and the states here cannot be reached that way: an undone publish exists
//! only on the far side of a push, and a fresh clone's "no copy yet" is the
//! shape `git clone` leaves behind rather than one `update-ref` can fake.
//!
//! Nothing here reaches the network. The remote is a bare repository beside
//! the clone, on a path.

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

fn both(out: &Output) -> String {
    format!(
        "{}{}",
        stdout(out),
        String::from_utf8_lossy(&out.stderr).into_owned()
    )
}

fn json(out: &Output) -> serde_json::Value {
    serde_json::from_str(&stdout(out)).expect("valid json")
}

fn ok(out: &Output) -> String {
    assert!(out.status.success(), "{}", both(out));
    stdout(out)
}

/// Two commits published, then the second undone: the shared copy stands one
/// commit ahead of a branch that is now a strict ancestor of it.
fn published_then_undone() -> Fixture {
    let fx = Fixture::new_cloned();
    fx.write("a.txt", "a\n");
    fx.commit("one");
    ok(&ff(&fx, &["publish"]));
    fx.write("a.txt", "aa\n");
    fx.commit("two");
    ok(&ff(&fx, &["publish"]));
    ok(&ff(&fx, &["undo"]));
    fx
}

#[test]
fn status_says_the_remote_holds_what_you_undid() {
    let fx = published_then_undone();
    let text = ok(&ff(&fx, &["status"]));
    assert!(text.contains("remote holds 1 you undid"), "{text}");
    assert!(
        !text.contains("to sync"),
        "there is nothing to take in: {text}"
    );
}

#[test]
fn sync_takes_nothing_in_and_names_publish() {
    let fx = published_then_undone();
    let before = fx.git(&["rev-parse", "main"]).trim().to_string();

    let text = ok(&ff(&fx, &["sync"]));
    assert!(text.contains("you undid"), "{text}");
    assert!(text.contains("ff publish"), "{text}");
    assert_eq!(
        fx.git(&["rev-parse", "main"]).trim(),
        before,
        "the fast-forward would have reversed the undo: {text}"
    );
}

#[test]
fn the_sync_envelope_carries_the_undone_state() {
    let fx = published_then_undone();
    let v = json(&ff(&fx, &["--json", "sync"]));
    assert_eq!(v["cmd"], "sync");
    assert_eq!(v["data"]["sync"]["remote"]["Undone"]["behind"], 1);
    assert_eq!(v["data"]["sync"]["pending"]["Undone"], 1);
}

#[test]
fn the_status_envelope_carries_the_undone_verdict() {
    let fx = published_then_undone();
    let v = json(&ff(&fx, &["status", "--json"]));
    assert_eq!(v["data"]["futures"]["remote"]["verdict"]["kind"], "undone");
    assert_eq!(v["data"]["futures"]["remote"]["verdict"]["behind"], 1);
}

/// Publishing again rolls the shared copy back, and the line says so: this
/// push takes commits off the far side rather than sending any.
#[test]
fn publishing_again_reads_as_a_retraction_and_rolls_the_remote_back() {
    let fx = published_then_undone();
    let one = fx.git(&["rev-parse", "main"]).trim().to_string();

    let text = ok(&ff(&fx, &["publish"]));
    assert!(text.contains("rolled origin/main back to main"), "{text}");
    assert_eq!(
        fx.remote_git(&["rev-parse", "refs/heads/main"]).trim(),
        one,
        "the shared copy is where the branch now stands"
    );

    // And the axis settles: nothing left undone, nothing left to send.
    let after = ok(&ff(&fx, &["status"]));
    assert!(!after.contains("you undid"), "{after}");
}

/// The pushes are on the log, as notes. `ff undo` steps over them and
/// `ff op revert` refuses them, because a push is something that happened
/// rather than something that was done here.
#[test]
fn op_log_shows_the_pushes() {
    let fx = Fixture::new_cloned();
    fx.write("a.txt", "a\n");
    fx.commit("one");
    ok(&ff(&fx, &["publish"]));
    fx.write("a.txt", "aa\n");
    fx.commit("two");
    ok(&ff(&fx, &["publish"]));

    let text = ok(&ff(&fx, &["op", "log", "-n", "10"]));
    let rows: Vec<&str> = text
        .lines()
        .filter(|l| l.contains("published main to origin/main"))
        .collect();
    assert_eq!(rows.len(), 2, "one row per push: {text}");
    assert!(
        rows.iter().all(|row| row.contains("note")),
        "a push is a note: {text}"
    );
}

/// A clone of an empty remote has `branch.main.merge` and no
/// `refs/remotes/*`, which is the same shape a deleted shared copy wears.
/// Only one of the two is a loss.
#[test]
fn a_fresh_clone_reports_no_copy_rather_than_a_deletion() {
    let fx = Fixture::new_cloned();
    fx.write("a.txt", "a\n");
    fx.commit("one");

    let text = ok(&ff(&fx, &["status"]));
    assert!(text.contains("remote has no copy yet"), "{text}");
    assert!(!text.contains("gone"), "nothing was lost: {text}");

    let preview = ok(&ff(&fx, &["publish", "-n"]));
    assert!(preview.contains("would create origin/main"), "{preview}");
    assert!(!preview.contains("gone"), "{preview}");
}

/// And a real deletion still says gone, in both surfaces.
#[test]
fn a_deleted_shared_copy_still_says_gone() {
    let fx = Fixture::new_cloned();
    fx.write("a.txt", "a\n");
    fx.commit("one");
    ok(&ff(&fx, &["publish"]));
    fx.remote_git(&["update-ref", "-d", "refs/heads/main"]);
    fx.git(&["fetch", "--prune", "-q", "origin"]);

    let text = ok(&ff(&fx, &["status"]));
    assert!(text.contains("remote is gone"), "{text}");

    let preview = ok(&ff(&fx, &["publish", "-n"]));
    assert!(
        preview.contains("would re-create origin/main, which is gone"),
        "{preview}"
    );
}

/// The tail under a real push names both halves: what cannot be taken back,
/// and the one way back that is not a way to erase it.
#[test]
fn the_tail_names_the_recovery_beside_the_irreversibility() {
    let fx = Fixture::new_cloned();
    fx.write("a.txt", "a\n");
    fx.commit("one");

    let text = ok(&ff(&fx, &["publish"]));
    assert!(text.contains("ff undo cannot reach it"), "{text}");
    assert!(
        text.contains("ff undo then ff publish rolls the shared copy back"),
        "{text}"
    );
}

/// `--to` sends to a remote the branch has never answered to, and records
/// that it does now: the bare `ff sync` that could not run before runs after.
#[test]
fn publishing_to_a_named_remote_records_it() {
    let fx = Fixture::new_cloned();
    fx.write("a.txt", "a\n");
    fx.commit("one");
    let tip = fx.git(&["rev-parse", "main"]).trim().to_string();

    // Two remotes and no `origin`, and the upstream `git clone` wrote is
    // gone: there is nothing for `ff sync` to name.
    fx.git(&["remote", "rename", "origin", "one"]);
    let second = fx.root().join("second.git");
    fx.git_in(
        fx.root(),
        &["init", "-q", "--bare", "-b", "main", "second.git"],
    );
    fx.git(&["remote", "add", "two", &second.to_string_lossy()]);
    fx.git(&["config", "--unset", "branch.main.remote"]);
    fx.git(&["config", "--unset", "branch.main.merge"]);

    let out = ff(&fx, &["--json", "sync"]);
    assert!(
        !out.status.success(),
        "a sync with two remotes and no upstream is not a success: {}",
        both(&out)
    );
    // The id rides the envelope, not the human line.
    let v = json(&out);
    assert_eq!(
        v["error"]["id"], "sync/ambiguous-remote",
        "the refusal is the ambiguity, not some other state: {v}"
    );

    let text = ok(&ff(&fx, &["publish", "--to", "two"]));
    assert!(text.contains("two/main"), "{text}");

    assert_eq!(fx.git(&["config", "branch.main.remote"]).trim(), "two");
    assert_eq!(
        fx.git(&["config", "branch.main.merge"]).trim(),
        "refs/heads/main"
    );
    assert_eq!(
        fx.git_in(&second, &["rev-parse", "refs/heads/main"]).trim(),
        tip,
        "the commit must have arrived on the far side"
    );

    // The payoff: the sync that was ambiguous is now the branch's own.
    let out = ff(&fx, &["sync"]);
    assert!(
        out.status.success(),
        "the payoff: a bare ff sync that needed --to a moment ago now names the remote itself: {}",
        both(&out)
    );
}

/// `ff remote` answers both halves of the question the publish and sync
/// refusals used to deflect: the name, and where it points — from the config
/// fufu already reads.
#[test]
fn remote_names_the_remote_and_where_it_points() {
    let fx = Fixture::new_cloned();
    let text = ok(&ff(&fx, &["remote"]));
    assert!(text.contains("origin"), "{text}");
    // The final path component, so a temp-dir prefix cannot make this brittle.
    let tail = fx
        .remote_path()
        .file_name()
        .map(|name| name.to_string_lossy().into_owned());
    assert!(
        tail.as_deref().is_some_and(|tail| text.contains(tail)),
        "the row says where origin points: {text}"
    );
}

/// `--json` carries the same fact as fields, under a named array the
/// envelope can grow into.
#[test]
fn remote_json_carries_the_name_and_the_url() {
    let fx = Fixture::new_cloned();
    let v = json(&ff(&fx, &["remote", "--json"]));
    let remotes = v["data"]["remotes"].as_array().expect("a remotes array");
    assert_eq!(remotes.len(), 1, "{v}");
    assert_eq!(remotes[0]["name"], "origin", "{v}");
    let url = remotes[0]["fetch_url"]
        .as_str()
        .expect("a fetch_url string");
    assert!(
        url.contains("remote.git"),
        "the url is where origin points: {url}"
    );
}

/// A repository with no remote is a state, not a failure: the listing says
/// so and exits 0, in both surfaces.
#[test]
fn a_repository_with_no_remotes_says_so() {
    let fx = Fixture::new();
    let text = ok(&ff(&fx, &["remote"]));
    assert!(text.contains("no remotes configured"), "{text}");

    let v = json(&ff(&fx, &["remote", "--json"]));
    assert!(
        v["data"]["remotes"]
            .as_array()
            .expect("a remotes array")
            .is_empty(),
        "{v}"
    );
}

/// The shape `--shared` deletes: `main` published, a second branch `shared`
/// with a commit of its own, published too, standing on `main` again.
fn published_branches() -> Fixture {
    let fx = Fixture::new_cloned();
    fx.write("a.txt", "a\n");
    fx.commit("one");
    ok(&ff(&fx, &["publish"]));
    fx.git(&["checkout", "-q", "-b", "shared"]);
    fx.write("b.txt", "b\n");
    fx.commit("two");
    ok(&ff(&fx, &["publish"]));
    fx.git(&["checkout", "-q", "main"]);
    fx
}

/// `--shared` deletes the copy on the remote and all three local traces —
/// the tracking ref, the `[branch "shared"]` section, and the published
/// note — and says both halves.
#[test]
fn shared_delete_removes_the_copy_and_the_traces() {
    let fx = published_branches();
    let text = ok(&ff(&fx, &["branch", "delete", "shared", "--shared"]));

    let heads = fx.remote_git(&["for-each-ref", "refs/heads"]);
    assert!(
        !heads.lines().any(|l| l.contains("shared")),
        "the copy on the remote is gone: {heads}"
    );
    let remotes = fx.git(&["for-each-ref", "refs/remotes"]);
    assert!(
        !remotes.lines().any(|l| l.contains("shared")),
        "the tracking ref is gone: {remotes}"
    );
    // Exits 1 when there is no match, so this reads the raw output instead.
    let config = fx.try_git(&["config", "--get-regexp", "^branch\\.shared\\."]);
    assert!(
        String::from_utf8_lossy(&config.stdout).trim().is_empty(),
        "the [branch \"shared\"] section is gone: {}",
        String::from_utf8_lossy(&config.stdout)
    );
    let fufu = fx.git(&["for-each-ref", "refs/fufu"]);
    assert!(
        !fufu.lines().any(|l| l.contains("published/shared")),
        "the published note is gone: {fufu}"
    );

    assert!(
        text.contains("removed the shared copy origin/shared"),
        "{text}"
    );
    assert!(text.contains("the delete left the machine"), "{text}");
}

/// A plain delete touches none of the three local traces and leaves the
/// copy on the remote standing — the shape `--shared` exists to change.
#[test]
fn a_plain_delete_removes_none_of_them() {
    let fx = published_branches();
    let text = ok(&ff(&fx, &["branch", "delete", "shared"]));

    let heads = fx.remote_git(&["for-each-ref", "refs/heads"]);
    assert!(
        heads.lines().any(|l| l.contains("shared")),
        "the copy stands: {heads}"
    );
    let remotes = fx.git(&["for-each-ref", "refs/remotes"]);
    assert!(
        remotes.lines().any(|l| l.contains("shared")),
        "the tracking ref survives: {remotes}"
    );
    let config = fx.try_git(&["config", "--get-regexp", "^branch\\.shared\\."]);
    assert!(
        config.status.success()
            && String::from_utf8_lossy(&config.stdout).contains("branch.shared.remote"),
        "the section survives: {}",
        String::from_utf8_lossy(&config.stdout)
    );
    let fufu = fx.git(&["for-each-ref", "refs/fufu"]);
    assert!(
        fufu.lines().any(|l| l.contains("published/shared")),
        "the published note survives: {fufu}"
    );

    assert!(
        text.contains("the shared copy origin/shared is still there"),
        "{text}"
    );
    assert!(text.contains("undo: ff undo"), "{text}");
}

/// A copy that moved since the last look refuses the leased delete, and the
/// far side stands where it moved to — the stale lease is the wire's own
/// refusal, not fufu's.
#[test]
fn a_moved_copy_refuses_and_leaves_the_far_side_standing() {
    let fx = published_branches();

    // The move, behind fufu's back: a second clone commits on `shared` and
    // pushes it. No fetch afterwards — the stale tracking ref is the point.
    let mover = fx.root().join("mover");
    fx.git_in(
        fx.root(),
        &["clone", "-q", &fx.remote_path().to_string_lossy(), "mover"],
    );
    fx.git_in(&mover, &["checkout", "-q", "shared"]);
    std::fs::write(mover.join("c.txt"), "c\n").expect("write in the mover");
    fx.git_in(&mover, &["add", "-A"]);
    fx.git_in(&mover, &["commit", "-q", "-m", "move"]);
    fx.git_in(&mover, &["push", "origin", "shared"]);

    let out = ff(&fx, &["--json", "branch", "delete", "shared", "--shared"]);
    assert!(
        !out.status.success(),
        "the stale lease is refused: {}",
        both(&out)
    );
    // The id rides the envelope, not the human line.
    let v = json(&out);
    assert_eq!(
        v["error"]["id"], "branch/shared-lease-refused",
        "the refusal is the lease, not some other state: {v}"
    );

    let heads = fx.remote_git(&["for-each-ref", "refs/heads"]);
    assert!(
        heads.lines().any(|l| l.contains("shared")),
        "the far side stands where it moved: {heads}"
    );
}
