//! Gitlinks through the real binary (#23): a change to one measures as git's
//! `Subproject commit` line on every diff surface.

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

fn ok(out: Output) -> Output {
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    out
}

/// `main` with a gitlink `sub` pointing at the init commit, and `side` forked
/// from before the gitlink. The working copy has no `sub` directory, as in
/// the issue. Returns the fixture and the gitlink's target.
fn repo() -> (Fixture, String) {
    let fx = Fixture::new();
    fx.set_config("user.name", "Gitlink Tester");
    fx.set_config("user.email", "gitlink@test.test");
    fx.write("a.txt", "a\n");
    let sha = fx.commit("init");
    fx.git(&["branch", "side"]);
    fx.git(&[
        "update-index",
        "--add",
        "--cacheinfo",
        &format!("160000,{sha},sub"),
    ]);
    fx.git(&["commit", "-qm", "add sub"]);
    (fx, sha)
}

/// The issue's reproduction: a gitlink missing from the working copy.
#[test]
fn status_and_diff_measure_a_missing_gitlink() {
    let (fx, sha) = repo();
    let status = ok(ff(&fx, &["status"]));
    assert!(stdout(&status).contains("sub"), "{}", stdout(&status));
    let diff = stdout(&ok(ff(&fx, &["diff"])));
    assert!(diff.contains("deleted file mode 160000"), "{diff}");
    assert!(
        diff.contains(&format!("-Subproject commit {sha}")),
        "{diff}"
    );
}
