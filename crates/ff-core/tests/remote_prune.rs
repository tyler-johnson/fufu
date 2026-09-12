//! `remote::prune_tracking`: the step after a fetch that gix's fetch has
//! none of. What it deletes is decided by the fetch refspecs' destinations
//! and the set of refs the handshake named; what it never touches is the
//! symbolic `<remote>/HEAD`, a ref outside every spec's destination, and a
//! ref the handshake named.

use std::collections::HashSet;

use ff_core::gix;
use ff_core::remote;
use ff_testsupport::Fixture;

fn present(names: &[&str]) -> HashSet<gix::bstr::BString> {
    names
        .iter()
        .map(|name| gix::bstr::BString::from(*name))
        .collect()
}

fn exists(fx: &Fixture, name: &str) -> bool {
    fx.try_git(&["rev-parse", "--verify", "-q", name])
        .status
        .success()
}

/// A remote with `a`, `b` and a symbolic HEAD under its tracking prefix.
fn tracked() -> Fixture {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("one");
    fx.set_config("remote.origin.url", "/nonexistent/remote.git");
    fx.set_config("remote.origin.fetch", "+refs/heads/*:refs/remotes/origin/*");
    fx.git(&["update-ref", "refs/remotes/origin/a", "HEAD"]);
    fx.git(&["update-ref", "refs/remotes/origin/b", "HEAD"]);
    fx.git(&[
        "symbolic-ref",
        "refs/remotes/origin/HEAD",
        "refs/remotes/origin/a",
    ]);
    fx
}

#[test]
fn a_ref_the_handshake_did_not_name_goes_and_head_stays() {
    let fx = tracked();
    let deleted = remote::prune_tracking(
        &fx.repo(),
        "origin",
        &present(&["refs/remotes/origin/a"]),
        1_700_000_000,
    )
    .unwrap();
    assert_eq!(deleted, vec!["refs/remotes/origin/b".to_string()]);
    assert!(exists(&fx, "refs/remotes/origin/a"));
    assert!(!exists(&fx, "refs/remotes/origin/b"));
    assert_eq!(
        fx.git(&["symbolic-ref", "refs/remotes/origin/HEAD"]).trim(),
        "refs/remotes/origin/a",
        "the symbolic ref is not a copy"
    );
}

/// A ref outside every fetch spec's destination is not the fetch's to
/// prune, and a second remote's refs are that remote's.
#[test]
fn only_the_specs_destinations_are_walked() {
    let fx = tracked();
    fx.set_config("remote.upstream.url", "/nonexistent/upstream.git");
    fx.set_config(
        "remote.upstream.fetch",
        "+refs/heads/*:refs/remotes/upstream/*",
    );
    fx.git(&["update-ref", "refs/remotes/upstream/a", "HEAD"]);
    fx.git(&["update-ref", "refs/fufu/seen/a", "HEAD"]);
    let deleted =
        remote::prune_tracking(&fx.repo(), "origin", &present(&[]), 1_700_000_000).unwrap();
    assert_eq!(
        deleted,
        vec![
            "refs/remotes/origin/a".to_string(),
            "refs/remotes/origin/b".to_string()
        ]
    );
    assert!(exists(&fx, "refs/remotes/upstream/a"));
    assert!(exists(&fx, "refs/fufu/seen/a"));
}

/// A negative refspec has no destination and adds nothing to the walk; a
/// spec naming one ref covers that ref alone, not its longer neighbors.
#[test]
fn a_negative_spec_and_a_single_ref_spec_cover_what_they_say() {
    let fx = tracked();
    fx.git(&["config", "--add", "remote.origin.fetch", "^refs/heads/b"]);
    fx.set_config("remote.mirror.url", "/nonexistent/mirror.git");
    fx.set_config(
        "remote.mirror.fetch",
        "refs/heads/main:refs/remotes/mirror/main",
    );
    fx.git(&["update-ref", "refs/remotes/mirror/main", "HEAD"]);
    fx.git(&["update-ref", "refs/remotes/mirror/main2", "HEAD"]);

    let deleted = remote::prune_tracking(
        &fx.repo(),
        "origin",
        &present(&["refs/remotes/origin/a"]),
        1_700_000_000,
    )
    .unwrap();
    assert_eq!(deleted, vec!["refs/remotes/origin/b".to_string()]);

    let deleted =
        remote::prune_tracking(&fx.repo(), "mirror", &present(&[]), 1_700_000_000).unwrap();
    assert_eq!(deleted, vec!["refs/remotes/mirror/main".to_string()]);
    assert!(exists(&fx, "refs/remotes/mirror/main2"), "not the spec's");
}
