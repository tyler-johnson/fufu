//! Contract for `publish::publish`: the plan it hands back, and nothing
//! else. The network is the CLI's job, so every case here is decided from
//! refs alone and reaches it zero times.

use ff_core::model::Publish;
use ff_core::{Provenance, PublishReport};
use ff_testsupport::Fixture;

const NOW: i64 = 1_799_999_999;

fn ident(fx: &Fixture) {
    fx.set_config("user.name", "Fixture Committer");
    fx.set_config("user.email", "committer@fixture.test");
}

fn prov() -> Provenance {
    Provenance::new("pre", Some("ff publish".into()))
}

fn publish_call(fx: &Fixture) -> PublishReport {
    let repo = fx.repo();
    let pre = ff_core::preflight::preflight(&repo, ff_core::preflight::Verb::Publish).unwrap();
    ff_core::publish::publish(
        &repo,
        &pre,
        ff_core::publish::PublishOptions {
            now: Some(NOW),
            argv: vec!["ff".into(), "publish".into()],
        },
        &prov(),
    )
    .unwrap()
    .0
}

/// A branch on `feature`, one commit deep, with origin configured.
fn feature(fx: &Fixture) -> String {
    ident(fx);
    fx.write("root.txt", "root\n");
    let _c0 = fx.commit("root");
    fx.git(&["switch", "-q", "-c", "feature"]);
    fx.write("f.txt", "f\n");
    let f1 = fx.commit("f1");
    fx.set_config("remote.origin.url", "/nonexistent/remote.git");
    fx.set_config("remote.origin.fetch", "+refs/heads/*:refs/remotes/origin/*");
    f1
}

fn track(fx: &Fixture, tip: &str) {
    fx.git(&["update-ref", "refs/remotes/origin/feature", tip]);
    fx.set_config("branch.feature.remote", "origin");
    fx.set_config("branch.feature.merge", "refs/heads/feature");
}

#[test]
fn a_repository_with_no_remote_has_nowhere_to_send_it() {
    let fx = Fixture::new();
    ident(&fx);
    fx.write("root.txt", "root\n");
    let _c0 = fx.commit("root");

    match publish_call(&fx).publish {
        Publish::NoRemote => {}
        other => panic!("nowhere to send it, got {other:?}"),
    }
}

#[test]
fn a_branch_with_no_upstream_is_created_and_tracked() {
    let fx = Fixture::new();
    feature(&fx);

    match publish_call(&fx).publish {
        Publish::Create {
            remote,
            remote_branch,
            ..
        } => {
            assert_eq!(remote, "origin");
            assert_eq!(remote_branch, "feature");
        }
        other => panic!("the push creates the remote branch, got {other:?}"),
    }
}

#[test]
fn a_remote_that_holds_everything_is_up_to_date() {
    let fx = Fixture::new();
    let f1 = feature(&fx);
    track(&fx, &f1);

    match publish_call(&fx).publish {
        Publish::UpToDate => {}
        other => panic!("the remote already holds it, got {other:?}"),
    }
}

/// The lease is the tracking ref as it stands — what you last saw. Publish
/// never fetches, so this is the only value it could honestly offer.
#[test]
fn a_commit_the_remote_lacks_is_pushed_under_the_last_seen_tip() {
    let fx = Fixture::new();
    let f1 = feature(&fx);
    track(&fx, &f1);
    fx.write("g.txt", "g\n");
    let f2 = fx.commit("f2");
    assert_ne!(f1, f2);

    match publish_call(&fx).publish {
        Publish::Push {
            remote,
            remote_branch,
            lease,
            tip,
        } => {
            assert_eq!(remote, "origin");
            assert_eq!(remote_branch, "feature");
            assert_eq!(lease, f1, "the lease is the tip as last seen");
            assert_eq!(tip, f2);
        }
        other => panic!("the commit must be sent, got {other:?}"),
    }
}

/// Somebody deleted the shared copy. Publishing puts it back — typing the
/// verb is saying so out loud, which is what a flag used to be for when
/// publishing was a default. The empty lease is git's *must not exist*, so a
/// racing re-create still loses rather than being overwritten.
#[test]
fn a_deleted_shared_copy_is_re_created_under_an_empty_lease() {
    let fx = Fixture::new();
    let f1 = feature(&fx);
    track(&fx, &f1);
    fx.git(&["update-ref", "-d", "refs/remotes/origin/feature"]);

    match publish_call(&fx).publish {
        Publish::Push {
            remote_branch,
            lease,
            ..
        } => {
            assert_eq!(remote_branch, "feature");
            assert_eq!(lease, "", "must not exist");
        }
        other => panic!("the branch is put back, got {other:?}"),
    }
}

/// The exits-blocked discipline: what a held rewrite refuses is passage, and
/// it refuses it before anything about remotes is even considered.
#[test]
fn a_held_rewrite_blocks_the_exit() {
    let fx = Fixture::new();
    ident(&fx);
    fx.write("root.txt", "root\n");
    let _c0 = fx.commit("root");
    fx.git(&["switch", "-q", "-c", "feature"]);
    fx.write("shared.txt", "base\n");
    let f1 = fx.commit("f1");
    fx.set_config("remote.origin.url", "/nonexistent/remote.git");
    fx.set_config("remote.origin.fetch", "+refs/heads/*:refs/remotes/origin/*");

    // Both sides rewrite the same line, so replaying onto the shared copy
    // cannot succeed and the rewrite is held.
    fx.git(&["switch", "-q", "-c", "collab"]);
    fx.write("shared.txt", "theirs\n");
    let collab = fx.commit("collab");
    fx.git(&["switch", "-q", "feature"]);
    fx.write("shared.txt", "mine\n");
    let _local = fx.commit("local");
    track(&fx, &f1);
    fx.git(&["update-ref", "refs/remotes/origin/feature", &collab]);

    let repo = fx.repo();
    let (outcome, _) = ff_core::restack::restack(
        &repo,
        None,
        Some("refs/remotes/origin/feature".to_string()),
        &prov(),
        Some(NOW),
        vec!["ff".into(), "restack".into()],
    )
    .unwrap();
    assert!(
        matches!(outcome, ff_core::RestackOutcome::Held(_)),
        "the setup must actually hold, got {outcome:?}"
    );

    match publish_call(&fx).publish {
        Publish::Blocked => {}
        other => panic!("a held rewrite blocks the exit, got {other:?}"),
    }
}
