//! Contract for `sync::preflight` and `sync::sync`: the two axes, the
//! divergence rule, and the exit — with the fetch's before/after tips
//! handed in as parameters and the network reached zero times.

use ff_core::gix;
use ff_core::sync::SyncOptions;
use ff_core::{BaseAxis, Provenance, Publish, RemoteAxis, RestackOutcome, SyncReport};
use ff_testsupport::Fixture;

const NOW: i64 = 1_799_999_999;

/// `restack` reads the committer identity from the repo config, which the
/// fixture's hermetic env does not set; git itself gets its identity from
/// env vars, so this is only for the gix side.
fn ident(fx: &Fixture) {
    fx.set_config("user.name", "Fixture Committer");
    fx.set_config("user.email", "committer@fixture.test");
}

fn prov() -> Provenance {
    Provenance::new("pre", Some("ff sync".into()))
}

fn sync_run(
    repo: &gix::Repository,
    pre: &ff_core::sync::Preflight,
    push: Option<bool>,
    fetched: bool,
    after: Option<&str>,
) -> SyncReport {
    let after = after.map(|h| gix::ObjectId::from_hex(h.as_bytes()).unwrap());
    ff_core::sync::sync(
        repo,
        pre,
        SyncOptions {
            push,
            fetched,
            tracking_after: after,
            now: Some(NOW),
            argv: vec!["ff".into(), "sync".into()],
        },
        &prov(),
    )
    .unwrap()
    .0
}

fn sync_call(fx: &Fixture, push: Option<bool>, fetched: bool, after: Option<&str>) -> SyncReport {
    let repo = fx.repo();
    let pre = ff_core::sync::preflight(&repo).unwrap();
    sync_run(&repo, &pre, push, fetched, after)
}

/// A ref's tip, for asserting that one did — or did not — move.
fn tip_of(fx: &Fixture, name: &str) -> String {
    fx.repo()
        .find_reference(name)
        .unwrap()
        .target()
        .try_id()
        .expect("the ref points at a commit")
        .to_string()
}

/// The tracking shape a fetch would have left behind, at `tip`. The URL is
/// never contacted — this suite reaches the network zero times — but the
/// remote must be configured for fufu to see it.
fn track(fx: &Fixture, tip: &str) {
    fx.git(&["update-ref", "refs/remotes/origin/feature", tip]);
    fx.set_config("branch.feature.remote", "origin");
    fx.set_config("branch.feature.merge", "refs/heads/feature");
    fx.set_config("remote.origin.url", "/nonexistent/remote.git");
    fx.set_config("remote.origin.fetch", "+refs/heads/*:refs/remotes/origin/*");
}

/// `main` with one commit, `feature` forked below with one of its own, and
/// the tracking ref at `feature`'s tip — a branch that has been pushed once.
/// Leaves the fixture standing on `feature`.
fn pushed_feature(fx: &Fixture) -> (String, String) {
    fx.write("root.txt", "root\n");
    let c0 = fx.commit("root");
    fx.git(&["switch", "-q", "-c", "feature"]);
    fx.write("f.txt", "f\n");
    let f1 = fx.commit("f1");
    track(fx, &f1);
    (c0, f1)
}

/// The test-1 shape: the collaborator committed on top of what we pushed
/// and so did we. `collab` is the tip the fetch would bring — the tracking
/// ref is left where it stands, because the "fetch" must run BETWEEN
/// preflight and sync: preflight records where the tracking ref stood
/// before, and the divergence rule compares against exactly that.
fn diverged(fx: &Fixture) -> String {
    let (_c0, _f1) = pushed_feature(fx);
    fx.git(&["switch", "-q", "-c", "collab"]);
    fx.write("collab.txt", "c\n");
    let collab = fx.commit("collab");
    fx.git(&["switch", "-q", "feature"]);
    fx.write("local.txt", "l\n");
    let _local = fx.commit("local");
    collab
}

/// The preflight → fetch → sync dance: preflight records where the tracking
/// ref stood before, the fetch moves it, and sync is handed the after tip.
fn synced_fetch(fx: &Fixture, push: Option<bool>, after: &str) -> SyncReport {
    let repo = fx.repo();
    let pre = ff_core::sync::preflight(&repo).unwrap();
    fx.git(&["update-ref", "refs/remotes/origin/feature", after]);
    sync_run(&repo, &pre, push, true, Some(after))
}

#[test]
fn a_fetch_that_moved_the_remote_is_theirs_and_replays() {
    let fx = Fixture::new();
    ident(&fx);
    let collab = diverged(&fx);

    let report = synced_fetch(&fx, None, &collab);

    let r = match report.remote {
        RemoteAxis::Ran { outcome, .. } => match outcome {
            RestackOutcome::Restacked(r) => *r,
            other => panic!("the replay must land, got {other:?}"),
        },
        other => panic!("the remote axis must act, got {other:?}"),
    };
    assert_eq!(r.base, "origin/feature");
    assert_eq!(r.replayed, 1);
}

#[test]
fn divergence_that_was_already_there_is_yours() {
    let fx = Fixture::new();
    ident(&fx);
    let (_c0, f1) = pushed_feature(&fx);

    // main moves and the restack rewrites feature's commit — exactly the
    // real case: the divergence below is fufu's own, and the tracking ref
    // never moves.
    fx.git(&["switch", "-q", "main"]);
    fx.write("m2.txt", "m2\n");
    let _m2 = fx.commit("m2");
    fx.git(&["switch", "-q", "feature"]);
    let repo = fx.repo();
    ff_core::restack::restack(
        &repo,
        None,
        None,
        &prov(),
        Some(NOW),
        vec!["ff".into(), "restack".into()],
    )
    .unwrap();

    let tip_before = tip_of(&fx, "refs/heads/feature");
    let report = sync_call(&fx, None, true, Some(&f1));

    let yours = match report.remote {
        RemoteAxis::Yours { ahead, behind, .. } => (ahead, behind),
        other => panic!("the divergence is yours, got {other:?}"),
    };
    assert!(yours.0 >= 1, "ahead {}", yours.0);
    assert!(yours.1 >= 1, "behind {}", yours.1);
    assert_eq!(
        tip_of(&fx, "refs/heads/feature"),
        tip_before,
        "sync must not replay your own rewrite back onto the stale remote"
    );
}

/// With no fetch, a divergence is yours only if the log accounts for it.
/// This one is a collaborator's own commit, which the log has never
/// touched, so sync must replay our side onto the remote instead of
/// force-publishing over it.
#[test]
fn no_fetch_does_not_make_a_stranger_commit_yours() {
    let fx = Fixture::new();
    ident(&fx);
    let collab = diverged(&fx);
    // The remote moved before this run — no fetch is part of it.
    fx.git(&["update-ref", "refs/remotes/origin/feature", &collab]);

    let tip_before = tip_of(&fx, "refs/heads/feature");
    let report = sync_call(&fx, None, false, Some(&collab));

    match report.remote {
        RemoteAxis::Ran { outcome, .. } => match outcome {
            RestackOutcome::Restacked(_) => {}
            other => panic!("an unaccounted divergence is replayed, got {other:?}"),
        },
        other => panic!("with no fetch, only an accounted divergence is yours, got {other:?}"),
    }
    assert_ne!(
        tip_of(&fx, "refs/heads/feature"),
        tip_before,
        "the replay is what preserves the collaborator's commit"
    );
}

#[test]
fn a_behind_remote_fast_forwards() {
    let fx = Fixture::new();
    ident(&fx);
    fx.write("root.txt", "root\n");
    let _c0 = fx.commit("root");
    fx.git(&["switch", "-q", "-c", "line"]);
    fx.write("f.txt", "f\n");
    let f1 = fx.commit("f1");
    fx.write("r1.txt", "r1\n");
    let _r1 = fx.commit("r1");
    fx.write("r2.txt", "r2\n");
    let r2 = fx.commit("r2");
    fx.git(&["branch", "feature", &f1]);
    fx.git(&["switch", "-q", "feature"]);
    track(&fx, &r2);

    let report = sync_call(&fx, None, true, Some(&r2));

    let r = match report.remote {
        RemoteAxis::Ran { outcome, .. } => match outcome {
            RestackOutcome::Restacked(r) => *r,
            other => panic!("a behind remote is a fast-forward, got {other:?}"),
        },
        other => panic!("the remote axis must act, got {other:?}"),
    };
    assert!(r.fast_forward);
    assert_eq!(tip_of(&fx, "refs/heads/feature"), r2);
}

#[test]
fn the_base_axis_is_a_plain_restack() {
    let fx = Fixture::new();
    ident(&fx);
    fx.write("root.txt", "root\n");
    let _c0 = fx.commit("root");
    fx.git(&["switch", "-q", "-c", "feature"]);
    fx.write("f.txt", "f\n");
    let _f1 = fx.commit("f1");
    fx.git(&["switch", "-q", "main"]);
    fx.write("m2.txt", "m2\n");
    let _m2 = fx.commit("m2");
    fx.git(&["switch", "-q", "feature"]);

    let report = sync_call(&fx, None, false, None);

    match report.remote {
        RemoteAxis::NoRemote => {}
        other => panic!("no remote is configured, got {other:?}"),
    }
    let r = match report.base {
        BaseAxis::Ran { name, outcome, .. } => {
            assert_eq!(name, "main");
            match outcome {
                RestackOutcome::Restacked(r) => *r,
                other => panic!("the base axis must replay, got {other:?}"),
            }
        }
        other => panic!("the base axis must act, got {other:?}"),
    };
    assert_eq!(r.base, "main");
    assert_eq!(r.replayed, 1);
    match report.publish {
        Publish::Off { .. } => {}
        other => panic!("nowhere to send it, got {other:?}"),
    }
}

#[test]
fn a_held_remote_axis_skips_the_base_and_blocks_the_exit() {
    let fx = Fixture::new();
    ident(&fx);
    fx.write("root.txt", "root\n");
    let _c0 = fx.commit("root");
    fx.git(&["switch", "-q", "-c", "feature"]);
    fx.write("shared.txt", "base\n");
    let f1 = fx.commit("f1");
    track(&fx, &f1);

    // Both sides rewrite the same line of the same file...
    fx.git(&["switch", "-q", "-c", "collab"]);
    fx.write("shared.txt", "theirs\n");
    let collab = fx.commit("collab");
    fx.git(&["switch", "-q", "feature"]);
    fx.write("shared.txt", "mine\n");
    let _local = fx.commit("local");

    // ...and main also moves, so the base axis has work it never gets.
    fx.git(&["switch", "-q", "main"]);
    fx.write("m2.txt", "m2\n");
    let _m2 = fx.commit("m2");
    fx.git(&["switch", "-q", "feature"]);

    let tip_before = tip_of(&fx, "refs/heads/feature");
    let report = synced_fetch(&fx, None, &collab);

    match report.remote {
        RemoteAxis::Ran { outcome, .. } => match outcome {
            RestackOutcome::Held(_) => {}
            other => panic!("the replay must hold, got {other:?}"),
        },
        other => panic!("the remote axis must act, got {other:?}"),
    }
    match report.base {
        BaseAxis::Skipped => {}
        other => panic!("a hold stops the run, got {other:?}"),
    }
    match report.publish {
        Publish::Blocked => {}
        other => panic!("a held rewrite blocks the exit, got {other:?}"),
    }
    assert_eq!(tip_of(&fx, "refs/heads/feature"), tip_before);
}

#[test]
fn a_branch_with_no_upstream_is_created_and_tracked() {
    let fx = Fixture::new();
    ident(&fx);
    fx.write("root.txt", "root\n");
    let _c0 = fx.commit("root");
    fx.git(&["switch", "-q", "-c", "feature"]);
    fx.write("f.txt", "f\n");
    let _f1 = fx.commit("f1");
    fx.set_config("remote.origin.url", "/nonexistent/remote.git");
    fx.set_config("remote.origin.fetch", "+refs/heads/*:refs/remotes/origin/*");

    let report = sync_call(&fx, None, true, None);

    match report.remote {
        RemoteAxis::NoRemote => {}
        other => panic!("no upstream is configured, got {other:?}"),
    }
    match report.publish {
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
fn push_off_and_up_to_date() {
    let fx = Fixture::new();
    ident(&fx);
    fx.write("root.txt", "root\n");
    let _c0 = fx.commit("root");
    fx.git(&["switch", "-q", "-c", "feature"]);
    fx.write("f.txt", "f\n");
    let f1 = fx.commit("f1");
    track(&fx, &f1);

    let up_to_date = sync_call(&fx, None, true, Some(&f1));
    match up_to_date.publish {
        Publish::UpToDate => {}
        other => panic!("the remote already holds everything, got {other:?}"),
    }
    let off = sync_call(&fx, Some(false), true, Some(&f1));
    match off.publish {
        // Off wins over UpToDate — the knob is read before anything is sent —
        // but it still carries what a push would have done, and here that is
        // nothing.
        Publish::Off { pending: false } => {}
        other => panic!("Off wins over UpToDate, with nothing pending, got {other:?}"),
    }
}

/// `--no-push` with commits waiting is not an empty run, and must not read
/// like one. Unpushed commits are precisely what sync exists to send, so a
/// run that deliberately kept them has something to say — which is why
/// `Publish::Off` carries whether a push would have sent anything rather than
/// collapsing every declined publish into one silent answer.
#[test]
fn push_off_with_work_waiting_is_pending() {
    let fx = Fixture::new();
    ident(&fx);
    fx.write("root.txt", "root\n");
    let _c0 = fx.commit("root");
    fx.git(&["switch", "-q", "-c", "feature"]);
    fx.write("f.txt", "f\n");
    let f1 = fx.commit("f1");
    track(&fx, &f1);
    // One more commit the remote has never seen.
    fx.write("g.txt", "g\n");
    let f2 = fx.commit("f2");
    assert_ne!(f1, f2);

    let off = sync_call(&fx, Some(false), true, Some(&f1));
    match off.publish {
        Publish::Off { pending: true } => {}
        other => panic!("a commit is waiting for the remote, got {other:?}"),
    }
}

/// Their commit arrived in an earlier fetch, so this run's fetch finds
/// nothing new — but that silence does not make the divergence ours. The
/// log holds no record of the collaborator's commit, so sync replays our
/// side on top of theirs instead of force-publishing over it.
#[test]
fn a_commit_from_an_earlier_fetch_is_replayed_not_overwritten() {
    let fx = Fixture::new();
    ident(&fx);
    let collab = diverged(&fx);
    // The earlier fetch already brought the commit in: the tracking ref is
    // at `collab` before this run's preflight records where it stood.
    fx.git(&["update-ref", "refs/remotes/origin/feature", &collab]);

    let tip_before = tip_of(&fx, "refs/heads/feature");
    let report = sync_call(&fx, None, true, Some(&collab));

    match report.remote {
        RemoteAxis::Ran { outcome, .. } => match outcome {
            RestackOutcome::Restacked(_) => {}
            other => panic!("a commit the log never accounted for is replayed, got {other:?}"),
        },
        other => panic!("an unaccounted divergence is replayed, got {other:?}"),
    }
    assert_ne!(
        tip_of(&fx, "refs/heads/feature"),
        tip_before,
        "the replay is what keeps the collaborator's commit"
    );
    // Ancestry, not just the tip: the collaborator's commit must survive
    // under the replay even though our commit now rides on top of it.
    let out = fx.try_git(&["merge-base", "--is-ancestor", &collab, "feature"]);
    assert!(
        out.status.success(),
        "the collaborator's commit is still reachable from the local branch"
    );
}

/// The guard against over-correcting into timidity on the `--no-fetch`
/// path: a divergence the log does account for — a recorded rewrite — is
/// still ours, and sync does not move the branch to "take in" its own work.
#[test]
fn a_recorded_rewrite_is_still_yours_without_a_fetch() {
    let fx = Fixture::new();
    ident(&fx);
    let (_c0, f1) = pushed_feature(&fx);

    // main moves and the restack rewrites feature's commit — exactly the
    // real case: the divergence below is fufu's own, and the tracking ref
    // never moves.
    fx.git(&["switch", "-q", "main"]);
    fx.write("m2.txt", "m2\n");
    let _m2 = fx.commit("m2");
    fx.git(&["switch", "-q", "feature"]);
    let repo = fx.repo();
    ff_core::restack::restack(
        &repo,
        None,
        None,
        &prov(),
        Some(NOW),
        vec!["ff".into(), "restack".into()],
    )
    .unwrap();

    let tip_before = tip_of(&fx, "refs/heads/feature");
    let report = sync_call(&fx, None, false, Some(&f1));

    match report.remote {
        RemoteAxis::Yours { .. } => {}
        other => panic!("a recorded rewrite is yours even without a fetch, got {other:?}"),
    }
    assert_eq!(
        tip_of(&fx, "refs/heads/feature"),
        tip_before,
        "sync must not replay your own rewrite back onto the stale remote"
    );
}

/// A commit the replay deliberately dropped as empty is fufu's own removal,
/// not somebody else's work: a remote still holding it is divergence that
/// is ours, so the force-publish stands.
#[test]
fn a_commit_dropped_as_empty_is_accounted_for() {
    let fx = Fixture::new();
    ident(&fx);
    fx.write("root.txt", "root\n");
    let _c0 = fx.commit("root");
    fx.git(&["switch", "-q", "-c", "feature"]);
    fx.write("dup.txt", "dup\n");
    let f1 = fx.commit("dup");
    track(&fx, &f1);

    // main introduces the same file with the same contents, so the replay
    // of the feature commit introduces nothing over the new base...
    fx.git(&["switch", "-q", "main"]);
    fx.write("dup.txt", "dup\n");
    let m2 = fx.commit("main dup");
    fx.git(&["switch", "-q", "feature"]);
    let repo = fx.repo();
    ff_core::restack::restack(
        &repo,
        None,
        None,
        &prov(),
        Some(NOW),
        vec!["ff".into(), "restack".into()],
    )
    .unwrap();

    // ...and the fixture must actually produce the drop, or the test
    // proves nothing: the commit is gone from the branch's history and the
    // branch sits on main's tip.
    let log = fx.git(&["log", "--format=%H", "feature"]);
    assert!(
        !log.lines().any(|h| h.trim() == f1),
        "the replay dropped the commit that now introduces nothing"
    );
    assert_eq!(
        tip_of(&fx, "refs/heads/feature"),
        m2,
        "with the only commit dropped, the branch sits on main's tip"
    );

    let report = sync_call(&fx, None, false, Some(&f1));

    match report.remote {
        RemoteAxis::Yours { .. } => {}
        other => {
            panic!("a dropped commit is accounted for, so the divergence is yours, got {other:?}")
        }
    }
}

/// The log is the authority on what fufu did, not on history: a rewrite
/// performed outside fufu leaves no record, so it is unaccounted for, and
/// sync takes the conservative direction — replay, not force.
#[test]
fn a_rewrite_fufu_never_saw_falls_back_to_replay() {
    let fx = Fixture::new();
    ident(&fx);
    let (_c0, f1) = pushed_feature(&fx);

    // A rewrite with no fufu operation behind it: plain git amends the
    // pushed commit into a new sha the log has never seen. The amend keeps
    // to a file the remote's commit does not touch, so the replay the test
    // is expecting can actually land.
    fx.write("root.txt", "root amended\n");
    fx.git(&["add", "-A"]);
    fx.git(&["commit", "--amend", "-q", "-m", "amended"]);
    let f2 = fx.git(&["rev-parse", "HEAD"]).trim().to_string();
    assert_ne!(f1, f2, "the amend produced a new sha");

    let report = sync_call(&fx, None, false, Some(&f1));

    match report.remote {
        RemoteAxis::Ran { outcome, .. } => match outcome {
            RestackOutcome::Restacked(_) => {}
            other => panic!("a rewrite fufu never recorded is replayed, got {other:?}"),
        },
        other => panic!("a rewrite fufu never recorded is replayed, got {other:?}"),
    }
}
