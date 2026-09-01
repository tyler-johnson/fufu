//! `ff describe <rev>`: the reword surface, end to end against the real
//! `ff` binary. Covers rewording a closed commit, the bare-form/`@`
//! equivalence, the JSON envelope, refusals, and the publish note.

use std::path::Path;
use std::process::{Command, Output};

use ff_testsupport::Fixture;
use ff_testsupport::fixtures::null_device;
use ff_testsupport::hooks::install_hook;

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

/// `ff` with no terminal to open an editor on, on top of the hermetic env.
fn ff_noninteractive(fx: &Fixture, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_ff"))
        .current_dir(fx.path())
        .args(args)
        .env("FF_NONINTERACTIVE", "1")
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

/// Four commits on `main`: `mid` at `c2`, `other` at `c3`. Nobody switches
/// branches, so `main` stays HEAD throughout.
fn stack(fx: &Fixture) -> [String; 4] {
    fx.write("f1.txt", "one\n");
    let c1 = fx.commit("c1");
    fx.write("f2.txt", "two\n");
    let c2 = fx.commit("c2");
    fx.git(&["branch", "mid"]);
    fx.write("f3.txt", "three\n");
    let c3 = fx.commit("c3");
    fx.git(&["branch", "other"]);
    fx.write("f4.txt", "four\n");
    let c4 = fx.commit("c4");
    [c1, c2, c3, c4]
}

fn repo() -> Fixture {
    let fx = Fixture::new();
    fx.set_config("user.name", "Reword Tester");
    fx.set_config("user.email", "reword@test.test");
    fx
}

#[test]
fn describe_at_a_rev_rewords_and_names_what_moved() {
    let fx = repo();
    stack(&fx);
    let mid_before = fx.git(&["rev-parse", "mid"]).trim().to_string();
    let other_before = fx.git(&["rev-parse", "other"]).trim().to_string();

    let output = ff(&fx, &["describe", "HEAD~2", "-m", "c2 reworded"]);
    assert!(output.status.success(), "{}", out(&output));

    let text = stdout(&output);
    let lines: Vec<&str> = text.lines().collect();
    let first = lines.first().expect("at least one line of output");
    assert!(first.starts_with("reworded "), "{text}");
    assert!(first.contains(" on main: c2 reworded"), "{text}");
    assert!(text.contains("restacked 2 commit"), "{text}");
    assert!(text.contains("moved"), "{text}");
    assert!(text.contains("mid"), "{text}");
    assert!(text.contains("other"), "{text}");
    let last = lines.last().expect("at least one line of output");
    assert_eq!(*last, "undo: ff undo");

    let subjects = fx.git(&["log", "--format=%s", "main"]);
    assert!(subjects.contains("c2 reworded"), "{subjects}");

    let mid_after = fx.git(&["rev-parse", "mid"]).trim().to_string();
    let other_after = fx.git(&["rev-parse", "other"]).trim().to_string();
    assert_ne!(mid_before, mid_after);
    assert_ne!(other_before, other_after);
}

#[test]
fn describe_at_the_open_change_is_the_bare_form() {
    let fx_bare = repo();
    stack(&fx_bare);
    let head_before = fx_bare.git(&["rev-parse", "HEAD"]).trim().to_string();
    let bare_output = ff(&fx_bare, &["describe", "-m", "planned work"]);
    assert!(bare_output.status.success(), "{}", out(&bare_output));

    let fx_at = repo();
    stack(&fx_at);
    let at_output = ff(&fx_at, &["describe", "@", "-m", "planned work"]);
    assert!(at_output.status.success(), "{}", out(&at_output));

    assert_eq!(stdout(&bare_output), stdout(&at_output));
    assert!(stdout(&bare_output).contains("pending description on "));

    let head_after = fx_bare.git(&["rev-parse", "HEAD"]).trim().to_string();
    assert_eq!(head_before, head_after);
}

#[test]
fn the_reword_json_envelope_carries_the_report() {
    let fx = repo();
    stack(&fx);

    let output = ff(&fx, &["--json", "describe", "HEAD~2", "-m", "c2 reworded"]);
    assert!(output.status.success(), "{}", out(&output));
    let v = json(&output);
    assert_eq!(v["ff"], 1);
    assert_eq!(v["cmd"], "describe");
    assert_eq!(v["data"]["undo"], "ff undo");

    let reword = &v["data"]["reword"];
    assert_eq!(reword["branch"], "main");
    assert_eq!(reword["subject"], "c2 reworded");
    assert_eq!(reword["restacked"], 2);
    assert_eq!(reword["moved"], serde_json::json!(["mid", "other"]));
    assert_eq!(reword["published"], 0);
    let old = reword["old"].as_str().expect("old is a string");
    let new = reword["new"].as_str().expect("new is a string");
    assert_ne!(old, new);
    assert_eq!(old.len(), 40);
    assert_eq!(new.len(), 40);

    let fx2 = repo();
    stack(&fx2);
    let bare = ff(&fx2, &["--json", "describe", "-m", "x"]);
    assert!(bare.status.success(), "{}", out(&bare));
    let v2 = json(&bare);
    assert!(v2["data"]["describe"].is_object());
    assert!(v2["data"]["reword"].is_null());
}

#[test]
fn a_rev_off_this_line_is_refused_and_explained() {
    let fx = repo();
    stack(&fx);
    fx.git(&["switch", "-c", "sidetrack", "HEAD~2"]);
    fx.write("side.txt", "side\n");
    fx.commit("side commit");
    fx.git(&["switch", "main"]);

    let main_before = fx.git(&["rev-parse", "main"]).trim().to_string();
    let mid_before = fx.git(&["rev-parse", "mid"]).trim().to_string();
    let other_before = fx.git(&["rev-parse", "other"]).trim().to_string();
    let sidetrack_before = fx.git(&["rev-parse", "sidetrack"]).trim().to_string();

    let output = ff(&fx, &["--json", "describe", "sidetrack", "-m", "nope"]);
    assert_eq!(output.status.code(), Some(1), "{}", out(&output));
    let v = json(&output);
    assert_eq!(v["error"]["id"], "rewrite/not-in-history");

    assert_eq!(main_before, fx.git(&["rev-parse", "main"]).trim());
    assert_eq!(mid_before, fx.git(&["rev-parse", "mid"]).trim());
    assert_eq!(other_before, fx.git(&["rev-parse", "other"]).trim());
    assert_eq!(sidetrack_before, fx.git(&["rev-parse", "sidetrack"]).trim());

    let explain = ff(&fx, &["explain", "rewrite/not-in-history"]);
    assert!(explain.status.success(), "{}", out(&explain));
    assert!(stdout(&explain).contains("ff log"), "{}", stdout(&explain));
}

#[test]
fn the_published_note_names_the_upstream() {
    let fx = repo();
    stack(&fx);

    let bare = Fixture::new_bare();
    let remote = bare.path();
    fx.git(&[
        "remote",
        "add",
        "origin",
        remote.to_str().expect("utf-8 path"),
    ]);
    fx.git(&["push", "-u", "origin", "main"]);

    let mid_before = fx.git(&["rev-parse", "mid"]).trim().to_string();
    let other_before = fx.git(&["rev-parse", "other"]).trim().to_string();

    let output = ff(&fx, &["describe", "HEAD~2", "-m", "c2 reworded"]);
    assert!(output.status.success(), "{}", out(&output));
    let text = stdout(&output);
    assert!(text.contains("already on origin/main"), "{text}");
    assert!(text.contains('3'), "{text}");

    let mid_after = fx.git(&["rev-parse", "mid"]).trim().to_string();
    let other_after = fx.git(&["rev-parse", "other"]).trim().to_string();
    assert_ne!(mid_before, mid_after);
    assert_ne!(other_before, other_after);
}

#[test]
fn a_rev_and_dash_b_cannot_be_given_together() {
    let fx = repo();
    stack(&fx);

    let output = ff(&fx, &["describe", "HEAD~1", "-b", "newname"]);
    assert_eq!(output.status.code(), Some(2), "{}", out(&output));
    let text = out(&output);
    assert!(text.contains("rev"), "{text}");
    assert!(text.contains("--branch") || text.contains("-b"), "{text}");
}

#[test]
fn a_rev_with_no_message_and_no_terminal_refuses() {
    let fx = repo();
    stack(&fx);
    let head_before = fx.git(&["rev-parse", "HEAD"]).trim().to_string();

    let plain = ff_noninteractive(&fx, &["describe", "HEAD~1"]);
    assert_eq!(plain.status.code(), Some(2), "{}", out(&plain));

    let json_out = ff_noninteractive(&fx, &["--json", "describe", "HEAD~1"]);
    let v = json(&json_out);
    assert_eq!(v["error"]["id"], "usage/needs-message");

    let head_after = fx.git(&["rev-parse", "HEAD"]).trim().to_string();
    assert_eq!(head_before, head_after);
}

#[test]
fn a_reword_runs_the_message_hooks_and_can_be_declined() {
    let fx = repo();
    stack(&fx);
    let head_before = fx.git(&["rev-parse", "HEAD"]).trim().to_string();
    install_hook(&fx, "commit-msg", "#!/bin/sh\nexit 1\n");
    // A pre-commit hook that would refuse: a reword moves no tree, so it
    // must never be asked.
    install_hook(&fx, "pre-commit", "#!/bin/sh\nexit 1\n");

    let output = ff(&fx, &["describe", "HEAD~2", "-m", "c2 reworded"]);
    assert!(!output.status.success(), "{}", out(&output));
    assert!(out(&output).contains("commit-msg hook"), "{}", out(&output));
    assert_eq!(
        fx.git(&["rev-parse", "HEAD"]).trim(),
        head_before,
        "nothing was rewritten"
    );

    // --no-verify skips it, and the rewrite lands.
    let output = ff(
        &fx,
        &["describe", "HEAD~2", "-m", "c2 reworded", "--no-verify"],
    );
    assert!(output.status.success(), "{}", out(&output));
    assert!(
        fx.git(&["log", "--format=%s", "main"])
            .contains("c2 reworded"),
        "the reword landed"
    );
}

#[test]
fn the_open_changes_description_runs_no_hook() {
    let fx = repo();
    stack(&fx);
    // Both message hooks would refuse. A pending description is not a
    // commit, so neither is asked; they fire when the change closes.
    install_hook(&fx, "commit-msg", "#!/bin/sh\nexit 1\n");
    install_hook(&fx, "prepare-commit-msg", "#!/bin/sh\nexit 1\n");

    let output = ff(&fx, &["describe", "-m", "still open"]);
    assert!(output.status.success(), "{}", out(&output));
    assert!(stdout(&output).contains("still open"), "{}", out(&output));
}
