//! Upstream ahead/behind differential tests. Tracking refs are created with
//! `update-ref` plus branch/remote config — no network. Counts are checked both
//! through the porcelain `branch.ab` comparison (via assert_status_matches) and
//! directly against `git rev-list --left-right --count`.

use ff_testsupport::Fixture;
use ff_testsupport::porcelain::assert_status_matches;

/// Configure `origin/main` as the upstream of `main`, pointing at `target`.
fn set_upstream(fx: &Fixture, target: &str) {
    fx.git(&["config", "remote.origin.url", "file:///nonexistent"]);
    fx.git(&[
        "config",
        "remote.origin.fetch",
        "+refs/heads/*:refs/remotes/origin/*",
    ]);
    fx.git(&["config", "branch.main.remote", "origin"]);
    fx.git(&["config", "branch.main.merge", "refs/heads/main"]);
    if !target.is_empty() {
        fx.git(&["update-ref", "refs/remotes/origin/main", target]);
    }
}

fn native_upstream(fx: &Fixture) -> Option<ff_core::Upstream> {
    ff_core::upstream(&fx.repo()).expect("compute upstream")
}

fn assert_counts(fx: &Fixture, ahead: usize, behind: usize) {
    let u = native_upstream(fx).expect("upstream configured");
    assert!(!u.gone);
    assert_eq!((u.ahead, u.behind), (ahead, behind), "ahead/behind");
    let out = fx.git(&[
        "rev-list",
        "--left-right",
        "--count",
        "main...refs/remotes/origin/main",
    ]);
    let (l, r) = out.trim().split_once('\t').expect("left-right counts");
    assert_eq!(
        (l.parse::<usize>().unwrap(), r.parse::<usize>().unwrap()),
        (ahead, behind),
        "sanity: rev-list agrees with the expectation itself"
    );
    assert_status_matches(fx);
}

#[test]
fn no_upstream_configured() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("one");
    assert_eq!(native_upstream(&fx), None);
    assert_status_matches(&fx);
}

#[test]
fn in_sync() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    let id = fx.commit("one");
    set_upstream(&fx, &id);
    assert_counts(&fx, 0, 0);
}

#[test]
fn ahead_only() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    let first = fx.commit("one");
    set_upstream(&fx, &first);
    fx.write("a.txt", "b\n");
    fx.commit("two");
    fx.write("a.txt", "c\n");
    fx.commit("three");
    assert_counts(&fx, 2, 0);
}

#[test]
fn behind_only() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    let first = fx.commit("one");
    fx.write("a.txt", "b\n");
    fx.commit("two");
    fx.write("a.txt", "c\n");
    let tip = fx.commit("three");
    set_upstream(&fx, &tip);
    fx.git(&["reset", "-q", "--hard", &first]);
    assert_counts(&fx, 0, 2);
}

#[test]
fn diverged() {
    let fx = Fixture::new();
    fx.write("base.txt", "base\n");
    fx.commit("base");
    fx.git(&["checkout", "-q", "-b", "remote-side"]);
    fx.write("r.txt", "r1\n");
    fx.commit("remote one");
    fx.write("r.txt", "r2\n");
    fx.commit("remote two");
    fx.write("r.txt", "r3\n");
    let remote_tip = fx.commit("remote three");
    fx.git(&["checkout", "-q", "main"]);
    fx.write("l.txt", "l1\n");
    fx.commit("local one");
    set_upstream(&fx, &remote_tip);
    fx.git(&["branch", "-q", "-D", "remote-side"]);
    assert_counts(&fx, 1, 3);
}

#[test]
fn criss_cross_merge_bases() {
    let fx = Fixture::new();
    fx.write("base.txt", "base\n");
    fx.commit("base");
    fx.git(&["checkout", "-q", "-b", "b1"]);
    fx.write("f1.txt", "1\n");
    fx.commit("b1 work");
    fx.git(&["checkout", "-q", "-b", "b2", "main"]);
    fx.write("f2.txt", "2\n");
    fx.commit("b2 work");
    // criss-cross: main merges b1 then b2's line merges b1 the other way round
    fx.git(&["checkout", "-q", "main"]);
    fx.git(&["merge", "-q", "--no-edit", "b1"]);
    fx.git(&["merge", "-q", "--no-edit", "b2"]);
    fx.git(&["checkout", "-q", "b2"]);
    fx.git(&["merge", "-q", "--no-edit", "b1"]);
    let remote_tip = fx.git(&["rev-parse", "HEAD"]).trim().to_string();
    fx.git(&["checkout", "-q", "main"]);
    set_upstream(&fx, &remote_tip);
    // Verify against git rather than a hand-computed expectation.
    let out = fx.git(&[
        "rev-list",
        "--left-right",
        "--count",
        "main...refs/remotes/origin/main",
    ]);
    let (l, r) = out.trim().split_once('\t').unwrap();
    let u = native_upstream(&fx).expect("upstream configured");
    assert_eq!(
        (u.ahead, u.behind),
        (l.parse().unwrap(), r.parse().unwrap()),
        "criss-cross counts must match git"
    );
    assert_status_matches(&fx);
}

#[test]
fn unrelated_histories() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("one");
    fx.write("a.txt", "b\n");
    fx.commit("two");
    fx.git(&["checkout", "-q", "--orphan", "orphan"]);
    fx.git(&["rm", "-rq", "-f", "."]);
    fx.write("other.txt", "other\n");
    let orphan_tip = fx.commit("orphan root");
    fx.git(&["checkout", "-q", "-f", "main"]);
    fx.git(&["branch", "-q", "-D", "orphan"]);
    set_upstream(&fx, &orphan_tip);
    assert_counts(&fx, 2, 1);
}

#[test]
fn gone_upstream() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("one");
    set_upstream(&fx, ""); // config only, no tracking ref
    let u = native_upstream(&fx).expect("upstream configured");
    assert!(u.gone);
    assert_eq!(u.r#ref, "origin/main");
    assert_status_matches(&fx);
}

#[test]
fn detached_has_no_upstream() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    let id = fx.commit("one");
    set_upstream(&fx, &id);
    fx.git(&["checkout", "-q", &id]);
    assert_eq!(native_upstream(&fx), None);
    assert_status_matches(&fx);
}
