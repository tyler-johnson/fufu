//! `ref/contended` is the one failure whose answer is running the same
//! command again, so it carries its own exit code and touches nothing.
//! Contention is simulated the way ff-core's capture test does it: a lock
//! file left on the snapshot ref.

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
        .env_remove("FF_SESSION")
        .output()
        .expect("spawn ff")
}

fn stdout(out: &Output) -> String {
    String::from_utf8(out.stdout.clone()).expect("utf-8 stdout")
}

/// A verb that loses the ref to another writer exits 4 with `ref/contended`,
/// leaves history and the worktree exactly as they were, and reports
/// promptly rather than sitting out the verb's two-second wait. Once the lock
/// clears, the same command succeeds unchanged.
#[test]
fn a_contended_verb_exits_4_and_touches_nothing() {
    let fx = Fixture::new();
    fx.set_config("user.name", "Fixture Committer");
    fx.set_config("user.email", "committer@fixture.test");
    fx.write("a.txt", "a\n");
    fx.commit("one");
    fx.write("a.txt", "b\n");

    let lock = fx.path().join(".git/refs/fufu/snap/main.lock");
    std::fs::create_dir_all(lock.parent().unwrap()).unwrap();
    std::fs::write(&lock, "held by someone else").unwrap();

    let start = std::time::Instant::now();
    let out = ff_at(&fx.path(), &["commit", "-m", "two", "--json"]);
    let took = start.elapsed();
    assert_eq!(out.status.code(), Some(4), "stdout: {}", stdout(&out));
    let v: serde_json::Value = serde_json::from_str(&stdout(&out)).expect("valid json");
    assert_eq!(v["error"]["id"], "ref/contended");
    assert!(
        took < std::time::Duration::from_secs(2),
        "contention must report promptly, not block: took {took:?}"
    );

    assert_eq!(
        fx.git(&["rev-list", "--count", "HEAD"]).trim(),
        "1",
        "no commit landed"
    );
    assert_eq!(
        std::fs::read_to_string(fx.path().join("a.txt")).unwrap(),
        "b\n",
        "the edit is still in the worktree"
    );

    std::fs::remove_file(&lock).unwrap();
    let out = ff_at(&fx.path(), &["commit", "-m", "two", "--json"]);
    assert_eq!(out.status.code(), Some(0), "stdout: {}", stdout(&out));
    assert_eq!(fx.git(&["rev-list", "--count", "HEAD"]).trim(), "2");
}
