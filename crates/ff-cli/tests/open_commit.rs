//! The open change is a commit, and the close moves the branch to it.
//!
//! The acceptance sequence: edit on a branch, read the sha the `@` row
//! shows, `ff commit` — and the `●` row wears that exact sha. Around it, the
//! surfaces the open commit shows through: `refs/fufu/open/<branch>`,
//! `git log --all`, `ff show @`, the `re-minted` line when the close could
//! not land it, and `ff undo` bringing it back.

use std::path::Path;
use std::process::{Command, Output};

use ff_testsupport::Fixture;
use ff_testsupport::fixtures::null_device;
use ff_testsupport::hooks::install_hook;

fn ff_at(dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_ff"))
        .current_dir(dir)
        .args(args)
        .env("GIT_CONFIG_GLOBAL", null_device())
        .env("GIT_CONFIG_SYSTEM", null_device())
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .expect("spawn ff")
}

fn ff(fx: &Fixture, args: &[&str]) -> Output {
    ff_at(&fx.path(), args)
}

fn ok(out: Output) -> Output {
    assert!(
        out.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    out
}

fn stdout(out: &Output) -> String {
    String::from_utf8(out.stdout.clone()).expect("utf-8 stdout")
}

fn json(output: &Output) -> serde_json::Value {
    serde_json::from_str(&stdout(output)).expect("valid json")
}

fn repo() -> Fixture {
    let fx = Fixture::new();
    fx.set_config("user.name", "Open Commit Tester");
    fx.set_config("user.email", "open-commit@test.test");
    fx.write("a.txt", "a\n");
    fx.commit("init");
    fx
}

fn open_ref(fx: &Fixture) -> Option<String> {
    let out = fx.try_git_in(
        &fx.path(),
        &["rev-parse", "--verify", "-q", "refs/fufu/open/main"],
    );
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// The `@` row's sha, from the JSON log.
fn at_sha(fx: &Fixture) -> Option<String> {
    json(&ok(ff(fx, &["log", "--json"])))["data"]["open"]["pending"]
        .as_str()
        .map(str::to_string)
}

/// Dirty on `main`, `@` shows S, `ff commit` lands S on `main`.
#[test]
fn the_close_lands_the_sha_the_at_row_showed() {
    let fx = repo();
    fx.write("a.txt", "edited\n");
    ok(ff(&fx, &["describe", "-m", "the work"]));
    let shown = at_sha(&fx).expect("the @ row shows the open commit");
    assert_eq!(open_ref(&fx).as_deref(), Some(shown.as_str()));

    // git sees it too: an unsigned commit by the user, over HEAD, wearing
    // the description and the id, above the tip while the tree is dirty.
    let all = fx.git(&["log", "--all", "--oneline", "--no-decorate"]);
    assert!(all.starts_with(&shown[..7]), "{all}");
    let raw = fx.git(&["cat-file", "-p", &shown]);
    assert!(raw.contains("Open Commit Tester"), "{raw}");
    assert!(raw.contains("\nchange-id "), "{raw}");
    assert!(raw.ends_with("\n\nthe work\n"), "{raw}");
    assert!(!raw.contains("gpgsig"), "{raw}");

    let out = ok(ff(&fx, &["commit"]));
    let text = stdout(&out);
    assert!(!text.contains("re-minted"), "{text}");
    assert_eq!(fx.git(&["rev-parse", "HEAD"]).trim(), shown);
    assert_eq!(open_ref(&fx), None, "nothing open after the close");
    let log = json(&ok(ff(&fx, &["log", "--json"])));
    assert_eq!(log["data"]["commits"][0]["id"], shown, "{log}");
    assert!(log["data"]["open"]["pending"].is_null(), "{log}");

    // And `ff undo` brings the open commit back at the same sha.
    ok(ff(&fx, &["undo"]));
    assert_eq!(open_ref(&fx).as_deref(), Some(shown.as_str()));
    assert_eq!(at_sha(&fx).as_deref(), Some(shown.as_str()));
}

#[test]
fn ff_show_names_the_open_commit() {
    let fx = repo();
    fx.write("a.txt", "edited\n");
    let shown = at_sha(&fx).expect("the open commit");
    let text = stdout(&ok(ff(&fx, &["show", "@"])));
    assert!(
        text.starts_with(&format!("@  {} the open change on main", &shown[..8])),
        "{text}"
    );
    let payload = json(&ok(ff(&fx, &["--json", "show", "@"])));
    assert_eq!(payload["data"]["pending"], shown, "{payload}");

    // `@^` is the commit under it.
    let head = fx.git(&["rev-parse", "HEAD"]).trim().to_string();
    let payload = json(&ok(ff(&fx, &["--json", "show", "@^"])));
    assert_eq!(payload["data"]["id"], head, "{payload}");
}

#[test]
fn a_hook_that_changes_the_tree_says_so() {
    let fx = repo();
    install_hook(
        &fx,
        "pre-commit",
        "#!/bin/sh\nprintf 'formatted\\n' >> a.txt\n",
    );
    fx.write("a.txt", "unformatted\n");
    ok(ff(&fx, &["describe", "-m", "the work"]));
    let shown = at_sha(&fx).expect("the open commit");

    let out = ok(ff(&fx, &["commit"]));
    let text = stdout(&out);
    assert!(
        text.contains("re-minted: a hook changed the tree"),
        "{text}"
    );
    assert_ne!(fx.git(&["rev-parse", "HEAD"]).trim(), shown);

    // The JSON carries the reason.
    fx.write("a.txt", "again\n");
    ok(ff(&fx, &["describe", "-m", "more work"]));
    let payload = json(&ok(ff(&fx, &["--json", "commit"])));
    assert_eq!(
        payload["data"]["commit"]["reminted"], "hook_tree",
        "{payload}"
    );
}

#[test]
fn a_partial_close_says_so() {
    let fx = repo();
    fx.write("a.txt", "a2\n");
    fx.write("b.txt", "b\n");
    ok(ff(&fx, &["describe", "-m", "both"]));
    let out = ok(ff(&fx, &["commit", "a.txt"]));
    assert!(
        stdout(&out).contains("re-minted: partial close"),
        "{}",
        stdout(&out)
    );
    // The remainder is open, with its own commit on the new HEAD.
    let remainder = open_ref(&fx).expect("the remainder is open");
    assert_eq!(
        fx.git(&["rev-parse", &format!("{remainder}^")]).trim(),
        fx.git(&["rev-parse", "HEAD"]).trim()
    );
}

/// A `-m` that differs from what the open commit carries mints a commit
/// without a re-mint line: the message is the user's own choice.
#[test]
fn a_dash_m_mints_quietly() {
    let fx = repo();
    fx.write("a.txt", "edited\n");
    let shown = at_sha(&fx).expect("the open commit");
    let out = ok(ff(&fx, &["commit", "-m", "landed"]));
    assert!(!stdout(&out).contains("re-minted"), "{}", stdout(&out));
    assert_ne!(fx.git(&["rev-parse", "HEAD"]).trim(), shown);
    let payload = json(&ok(ff(&fx, &["log", "--json"])));
    assert!(
        payload["data"]["commits"][0]["subject"] == "landed",
        "{payload}"
    );
}
