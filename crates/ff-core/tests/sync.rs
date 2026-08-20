//! Contract for `sync::sync`: the two axes, the
//! divergence rule, and the exit — with the fetch's before/after tips
//! handed in as parameters and the network reached zero times.

use ff_core::gix;
use ff_core::sync::SyncOptions;
use ff_core::{BaseAxis, Provenance, RemoteAxis, RestackOutcome, SyncReport};
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
    pre: &ff_core::preflight::Preflight,
    fetched: bool,
    after: Option<&str>,
) -> SyncReport {
    let after = after.map(|h| gix::ObjectId::from_hex(h.as_bytes()).unwrap());
    ff_core::sync::sync(
        repo,
        pre,
        SyncOptions {
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

fn sync_call(fx: &Fixture, fetched: bool, after: Option<&str>) -> SyncReport {
    let repo = fx.repo();
    let pre = ff_core::preflight::preflight(&repo, ff_core::preflight::Verb::Sync).unwrap();
    sync_run(&repo, &pre, fetched, after)
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
fn synced_fetch(fx: &Fixture, after: &str) -> SyncReport {
    let repo = fx.repo();
    let pre = ff_core::preflight::preflight(&repo, ff_core::preflight::Verb::Sync).unwrap();
    fx.git(&["update-ref", "refs/remotes/origin/feature", after]);
    sync_run(&repo, &pre, true, Some(after))
}

#[test]
fn a_fetch_that_moved_the_remote_is_theirs_and_replays() {
    let fx = Fixture::new();
    ident(&fx);
    let collab = diverged(&fx);

    let report = synced_fetch(&fx, &collab);

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
    let report = sync_call(&fx, true, Some(&f1));

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
    let report = sync_call(&fx, false, Some(&collab));

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

    let report = sync_call(&fx, true, Some(&r2));

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

    let report = sync_call(&fx, false, None);

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
    assert_eq!(
        report.pending,
        ff_core::Pending::NoRemote,
        "no remote to be ahead of"
    );
}

#[test]
fn a_held_remote_axis_skips_the_base() {
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
    let report = synced_fetch(&fx, &collab);

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
    assert_eq!(tip_of(&fx, "refs/heads/feature"), tip_before);
}

/// Sync sends nothing, so what it has to say about the outgoing half is what
/// is left of it. Never published, nothing waiting, and something waiting are
/// three different answers, and the tail line depends on which.
#[test]
fn sync_counts_what_is_left_for_publish() {
    let fx = Fixture::new();
    ident(&fx);
    fx.write("root.txt", "root\n");
    let _c0 = fx.commit("root");
    fx.git(&["switch", "-q", "-c", "feature"]);
    fx.write("f.txt", "f\n");
    let f1 = fx.commit("f1");
    track(&fx, &f1);

    // Before there is a shared copy at all: not a count, a state.
    fx.set_config("remote.origin.url", "/nonexistent/remote.git");
    fx.set_config("remote.origin.fetch", "+refs/heads/*:refs/remotes/origin/*");
    let fresh = sync_call(&fx, true, None);
    assert_eq!(
        fresh.pending,
        ff_core::Pending::Unpublished,
        "a branch with no shared copy has everything to publish"
    );

    let level = sync_call(&fx, true, Some(&f1));
    assert_eq!(
        level.pending,
        ff_core::Pending::Ahead(0),
        "the remote holds everything"
    );

    // One more commit the remote has never seen.
    fx.write("g.txt", "g\n");
    let f2 = fx.commit("f2");
    assert_ne!(f1, f2);
    let ahead = sync_call(&fx, true, Some(&f1));
    assert_eq!(
        ahead.pending,
        ff_core::Pending::Ahead(1),
        "one commit is waiting for the remote"
    );
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
    let report = sync_call(&fx, true, Some(&collab));

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
    let report = sync_call(&fx, false, Some(&f1));

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

    let report = sync_call(&fx, false, Some(&f1));

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

    let report = sync_call(&fx, false, Some(&f1));

    match report.remote {
        RemoteAxis::Ran { outcome, .. } => match outcome {
            RestackOutcome::Restacked(_) => {}
            other => panic!("a rewrite fufu never recorded is replayed, got {other:?}"),
        },
        other => panic!("a rewrite fufu never recorded is replayed, got {other:?}"),
    }
}

// --- The undone clause -----------------------------------------------------
//
// A real bare remote, because a published-then-undone tracking tip only
// exists on the far side of a push, and the shape it wears — local a strict
// *ancestor* of the tracking ref — never reaches the accounted-for check at
// all. It took the plain fast-forward path and put the undo straight back.

/// Push `main` to the fixture's real remote and record it, the way
/// `ff publish` does: plan, push, record.
fn publish_for_real(fx: &Fixture) {
    let repo = fx.repo();
    let pre = ff_core::preflight::preflight(&repo, ff_core::preflight::Verb::Publish).unwrap();
    let (report, ctx) = ff_core::publish::publish(
        &repo,
        &pre,
        ff_core::publish::PublishOptions {
            dry_run: false,
            now: Some(NOW),
            argv: vec!["ff".into(), "publish".into()],
        },
        &prov(),
    )
    .unwrap();
    fx.git(&["push", "--force", "-q", "origin", "main:main"]);
    ff_core::publish::record(&repo, &pre, &report, ctx.as_ref().unwrap(), &prov()).unwrap();
}

/// Two commits published, then the second undone: the shared copy stands one
/// commit ahead of a branch that is a strict ancestor of it.
fn published_then_undone(fx: &Fixture) -> (String, String) {
    fx.write("a.txt", "a\n");
    let one = fx.commit("one");
    publish_for_real(fx);
    fx.write("a.txt", "aa\n");
    let two = fx.commit("two");
    publish_for_real(fx);
    fx.git(&["reset", "--hard", "-q", "HEAD~1"]);
    (one, two)
}

#[test]
fn a_published_then_undone_tip_is_undone_and_not_a_fast_forward() {
    let fx = Fixture::new_cloned();
    let (one, two) = published_then_undone(&fx);

    let report = sync_call(&fx, false, Some(&two));

    match report.remote {
        RemoteAxis::Undone { behind, .. } => assert_eq!(behind, 1),
        other => panic!("the remote holds what you undid, got {other:?}"),
    }
    assert_eq!(
        tip_of(&fx, "refs/heads/main"),
        one,
        "nothing may be taken in: the fast-forward would reverse the undo"
    );
    assert_eq!(report.pending, ff_core::Pending::Undone(1));
}

/// The guard is all-or-nothing, exactly as it is for the accounted clause: a
/// colleague's commit on top of your published tip means the tracking ref no
/// longer stands where you left it, so the whole answer reverts to replay.
#[test]
fn a_colleague_on_top_of_your_published_tip_is_still_theirs() {
    let fx = Fixture::new_cloned();
    let (_one, _two) = published_then_undone(&fx);
    // Somebody else pushed on top of what we published. From here the only
    // visible difference is that the tracking ref moved off our tip.
    fx.git(&["switch", "-q", "-c", "collab", "origin/main"]);
    fx.write("collab.txt", "c\n");
    let collab = fx.commit("collab");
    fx.git(&["switch", "-q", "main"]);
    fx.git(&["update-ref", "refs/remotes/origin/main", &collab]);

    let tip_before = tip_of(&fx, "refs/heads/main");
    let report = sync_call(&fx, false, Some(&collab));

    match report.remote {
        RemoteAxis::Ran { .. } => {}
        other => panic!("a tip we did not publish is theirs, got {other:?}"),
    }
    assert_ne!(
        tip_of(&fx, "refs/heads/main"),
        tip_before,
        "their commit is what the replay preserves"
    );
}

/// Publish then rewrite satisfies both outgoing clauses, and the accounted
/// one runs first: "stale copies of your own" is the truer sentence there
/// than "you undid this".
#[test]
fn publish_then_rewrite_is_still_yours_with_its_own_message() {
    let fx = Fixture::new_cloned();
    ident(&fx);
    fx.write("a.txt", "a\n");
    fx.commit("one");
    fx.write("b.txt", "b\n");
    fx.commit("two");
    publish_for_real(&fx);
    let published = fx.git(&["rev-parse", "main"]).trim().to_string();

    // A rewrite fufu records: reword the tip through the operation log, so
    // the old sha is the `old` side of a rewrite the log accounts for.
    let repo = fx.repo();
    let target = gix::ObjectId::from_hex(published.as_bytes()).unwrap();
    ff_core::describe::reword(
        &repo,
        target,
        "two, reworded".into(),
        &prov(),
        Some(NOW),
        vec!["ff".into(), "describe".into()],
    )
    .unwrap();

    let report = sync_call(&fx, false, Some(&published));

    match report.remote {
        RemoteAxis::Yours { behind, .. } => assert_eq!(behind, 1),
        other => panic!("an accounted rewrite is yours, got {other:?}"),
    }
}
