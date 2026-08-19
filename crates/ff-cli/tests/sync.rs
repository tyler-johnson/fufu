//! `ff sync` and `ff publish`, end to end against the real `ff` binary.
//! Every test is offline — `--no-fetch` on a repository with no remote — so
//! none reaches the network. Covers the base-axis replay, the JSON
//! envelopes, the nothing-to-sync state, publish with nowhere to send, and
//! the two git words that now point at the pair.

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
