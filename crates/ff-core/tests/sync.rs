//! Contract for `sync::sync`: the two axes, the
//! divergence rule, and the exit — with the fetch's before/after tips
//! handed in as parameters and the network reached zero times.

use ff_core::gix;
use ff_core::sync::{OtherBranch, SyncOptions};
use ff_core::{
    BaseAxis, BranchRemote, BranchSync, Provenance, RemoteAxis, RestackOutcome, SkipReason,
    SyncReport,
};
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
            others: Vec::new(),
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

/// The tracking shape a fetch would have left behind for `branch`, at
/// `tip`. The URL is never contacted — this suite reaches the network zero
/// times — but the remote must be configured for fufu to see it.
fn track_branch(fx: &Fixture, branch: &str, tip: &str) {
    fx.git(&["update-ref", &format!("refs/remotes/origin/{branch}"), tip]);
    fx.set_config(&format!("branch.{branch}.remote"), "origin");
    fx.set_config(
        &format!("branch.{branch}.merge"),
        &format!("refs/heads/{branch}"),
    );
    fx.set_config("remote.origin.url", "/nonexistent/remote.git");
    fx.set_config("remote.origin.fetch", "+refs/heads/*:refs/remotes/origin/*");
}

/// [`track_branch`] for `feature`, the branch most of this suite stands on.
fn track(fx: &Fixture, tip: &str) {
    track_branch(fx, "feature", tip);
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

/// Push the branch underfoot to the fixture's real remote and record it,
/// the way `ff publish` does: plan, push, record.
fn publish_for_real(fx: &Fixture) {
    publish_branch_for_real(fx, "main");
}

/// [`publish_for_real`] for `branch`, which must be the one underfoot.
fn publish_branch_for_real(fx: &Fixture, branch: &str) {
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
    fx.git(&[
        "push",
        "--force",
        "-q",
        "origin",
        &format!("{branch}:{branch}"),
    ]);
    ff_core::publish::record(&repo, &pre, &report, ctx.as_ref().unwrap(), &prov()).unwrap();
}

/// Two commits published, then the second undone: the shared copy stands one
/// commit ahead of a branch that is a strict ancestor of it.
fn published_then_undone(fx: &Fixture) -> (String, String) {
    published_then_undone_on(fx, "main")
}

/// [`published_then_undone`] on `branch`, which must be the one underfoot.
fn published_then_undone_on(fx: &Fixture, branch: &str) -> (String, String) {
    fx.write("a.txt", "a\n");
    let one = fx.commit("one");
    publish_branch_for_real(fx, branch);
    fx.write("a.txt", "aa\n");
    let two = fx.commit("two");
    publish_branch_for_real(fx, branch);
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
        ff_core::Verify::Run,
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

/// Record `parent` as the branch `branch` sits on, the way
/// `ff start <parent> -b <branch>` does.
fn stacked_on(fx: &Fixture, branch: &str, parent: &str) {
    let repo = fx.repo();
    let mut meta = ff_core::branchmeta::read(&repo, branch).unwrap();
    meta.parent = Some(parent.to_string());
    ff_core::branchmeta::write(&repo, branch, &meta).unwrap();
}

#[test]
fn the_base_axis_cascades_onto_the_branches_above() {
    let fx = Fixture::new();
    ident(&fx);
    fx.write("root.txt", "root\n");
    fx.commit("root");
    fx.git(&["switch", "-q", "-c", "feature"]);
    fx.write("f.txt", "f\n");
    fx.commit("f1");
    fx.git(&["switch", "-q", "-c", "child"]);
    fx.write("g.txt", "g\n");
    let g1 = fx.commit("g1");
    stacked_on(&fx, "child", "feature");
    fx.git(&["switch", "-q", "main"]);
    fx.write("m2.txt", "m2\n");
    fx.commit("m2");
    fx.git(&["switch", "-q", "feature"]);

    let report = sync_call(&fx, false, None);

    let r = match report.base {
        BaseAxis::Ran {
            outcome: RestackOutcome::Restacked(r),
            ..
        } => *r,
        other => panic!("the base axis must replay, got {other:?}"),
    };
    assert_eq!(r.replayed, 1);
    assert_eq!(r.cascade.moved.len(), 1, "{:?}", r.cascade);
    assert_eq!(r.cascade.moved[0].branch, "child");
    assert_eq!(r.cascade.moved[0].base, "feature");
    assert_eq!(r.cascade.moved[0].replayed, 1);

    let child = fx.git(&["rev-parse", "child"]).trim().to_string();
    assert_ne!(child, g1, "child followed");
    assert_eq!(r.cascade.moved[0].new_tip, child);
    assert!(
        fx.try_git(&["merge-base", "--is-ancestor", "feature", "child"])
            .status
            .success(),
        "child sits on the synced feature"
    );
}

// --- The remote axis over the branches not underfoot ----------------------
//
// The branch underfoot has two axes; every other local branch gets the
// remote one: a fast-forward, a branch that stood exactly where the
// tracking ref stood before the fetch following the remote wherever it
// went, and the divergence rule the branch underfoot gets, replay included.
// Nothing here reaches the network: the "fetch" is `update-ref` between
// the two readings. A run that did not fetch cannot trust any tracking tip
// and reads every other branch as `NotFetched`, so the "nothing arrived"
// shape here is a fetch that moved nothing.

/// The CLI's dance for the branches not underfoot, mirrored exactly:
/// preflight and the other branches read before the fetch, `fetch` runs
/// (moving tracking refs the way a real one would), both are read again, and
/// the second reading's tips are carried into the first.
fn sync_around(fx: &Fixture, fetched: bool, fetch: impl FnOnce()) -> SyncReport {
    let repo = fx.repo();
    let pre = ff_core::preflight::preflight(&repo, ff_core::preflight::Verb::Sync).unwrap();
    let before = ff_core::sync::other_branches(&repo, &pre.branch).unwrap();
    fetch();
    let after = ff_core::preflight::preflight(&repo, ff_core::preflight::Verb::Sync).unwrap();
    let others_after = ff_core::sync::other_branches(&repo, &pre.branch).unwrap();
    let others: Vec<OtherBranch> = ff_core::sync::after_fetch(before, &others_after);
    ff_core::sync::sync(
        &repo,
        &pre,
        SyncOptions {
            fetched,
            tracking_after: after.tracking.as_ref().and_then(|t| t.tip),
            others,
            now: Some(NOW),
            argv: vec!["ff".into(), "sync".into()],
        },
        &prov(),
    )
    .unwrap()
    .0
}

/// The row the report carries for `branch`.
fn row_for(report: &SyncReport, branch: &str) -> BranchSync {
    report
        .branches
        .iter()
        .find(|b| match b {
            BranchSync::Elsewhere { branch: b, .. }
            | BranchSync::Held { branch: b, .. }
            | BranchSync::Synced { branch: b, .. } => b == branch,
        })
        .cloned()
        .unwrap_or_else(|| panic!("no row for {branch}: {:?}", report.branches))
}

/// How many `sync` operations the log holds.
fn sync_ops(fx: &Fixture) -> usize {
    ff_core::ops::read_ops(&fx.repo(), 100)
        .unwrap()
        .iter()
        .filter(|op| op.verb == "sync")
        .count()
}

/// The record of the newest operation.
fn tip_record(repo: &gix::Repository) -> ff_core::ops::OpRecord {
    let log = ff_core::ops::OpLog::open(repo).unwrap();
    let op = log.get(log.tip().unwrap().unwrap()).unwrap();
    op.record()
        .unwrap()
        .cloned()
        .expect("a verb op has a record")
}

/// `main` with one commit and `side` forked from it with one of its own,
/// standing on `main`. Returns `(c0, s1)`.
fn main_and_side(fx: &Fixture) -> (String, String) {
    fx.write("root.txt", "root\n");
    let c0 = fx.commit("root");
    fx.git(&["switch", "-q", "-c", "side"]);
    fx.write("s.txt", "s\n");
    let s1 = fx.commit("s1");
    fx.git(&["switch", "-q", "main"]);
    (c0, s1)
}

/// A commit on its own branch off `main`, unrelated to `side`'s: what a
/// force-push to the shared copy of `side` would leave the tracking ref at.
fn unrelated_commit(fx: &Fixture) -> String {
    fx.git(&["switch", "-q", "-c", "other", "main"]);
    fx.write("x.txt", "x\n");
    let x1 = fx.commit("x1");
    fx.git(&["switch", "-q", "main"]);
    x1
}

#[test]
fn an_other_branch_behind_its_remote_fast_forwards() {
    let fx = Fixture::new();
    ident(&fx);
    let (c0, s1) = main_and_side(&fx);
    // side stands one commit behind its shared copy, brought in by an
    // earlier fetch: nothing arrives this run.
    fx.git(&["branch", "-f", "side", &c0]);
    track_branch(&fx, "side", &s1);

    let report = sync_around(&fx, true, || {});

    assert_eq!(
        row_for(&report, "side"),
        BranchSync::Synced {
            branch: "side".into(),
            remote: BranchRemote::Moved {
                name: "origin/side".into(),
                fast_forward: true,
                behind: 1,
                old: c0.clone(),
                new: s1.clone(),
            },
            base: Box::new(on_base("side", "main")),
        }
    );
    assert_eq!(
        tip_of(&fx, "refs/heads/side"),
        s1,
        "side followed its remote"
    );

    let repo = fx.repo();
    assert_eq!(
        sync_ops(&fx),
        1,
        "one operation for the branches not underfoot"
    );
    let record = tip_record(&repo);
    assert_eq!(record.verb, "sync");
    assert_eq!(record.refs.len(), 1);
    assert_eq!(record.refs[0].name, "refs/heads/side");
    assert_eq!(record.refs[0].old.as_deref(), Some(c0.as_str()));
    assert_eq!(record.refs[0].new.as_deref(), Some(s1.as_str()));

    let opts = ff_core::RewindOptions {
        force: false,
        now: Some(NOW + 100),
        argv: vec!["ff".into(), "undo".into()],
    };
    ff_core::undo(&repo, &opts, &prov()).unwrap();
    assert_eq!(
        tip_of(&fx, "refs/heads/side"),
        c0,
        "one undo puts side back where it stood"
    );
}

#[test]
fn an_other_branch_on_the_old_tracking_tip_follows_a_force_push() {
    let fx = Fixture::new();
    ident(&fx);
    let (_c0, s1) = main_and_side(&fx);
    let x1 = unrelated_commit(&fx);
    // side stands exactly where the tracking ref stood before the fetch...
    track_branch(&fx, "side", &s1);

    // ...and the fetch finds the shared copy force-pushed somewhere unrelated.
    let report = sync_around(&fx, true, || {
        fx.git(&["update-ref", "refs/remotes/origin/side", &x1]);
    });

    assert_eq!(
        row_for(&report, "side"),
        BranchSync::Synced {
            branch: "side".into(),
            remote: BranchRemote::Moved {
                name: "origin/side".into(),
                fast_forward: false,
                behind: 1,
                old: s1,
                new: x1.clone(),
            },
            base: Box::new(on_base("side", "main")),
        }
    );
    assert_eq!(
        tip_of(&fx, "refs/heads/side"),
        x1,
        "a branch on the old tracking tip follows wherever the remote went"
    );
    assert_eq!(sync_ops(&fx), 1);
}

/// Whether `ancestor` is reachable from `rev`, by git's own reckoning.
fn is_ancestor(fx: &Fixture, ancestor: &str, rev: &str) -> bool {
    fx.try_git(&["merge-base", "--is-ancestor", ancestor, rev])
        .status
        .success()
}

/// The landed replay a row carries, or a panic naming what it carries
/// instead.
fn replayed_row(report: &SyncReport, branch: &str) -> ff_core::RestackReport {
    match row_for(report, branch) {
        BranchSync::Synced {
            remote:
                BranchRemote::Ran {
                    name,
                    outcome: RestackOutcome::Restacked(r),
                },
            ..
        } => {
            assert_eq!(name, format!("origin/{branch}"));
            *r
        }
        other => panic!("the divergence is theirs and replays, got {other:?}"),
    }
}

#[test]
fn an_other_branch_the_fetch_moved_replays_onto_its_remote() {
    let fx = Fixture::new();
    ident(&fx);
    let (c0, s1) = main_and_side(&fx);
    let x1 = unrelated_commit(&fx);
    // side has moved on from the tracking tip, and the fetch finds that the
    // remote has too: divergence this run's fetch created is theirs.
    track_branch(&fx, "side", &c0);

    let report = sync_around(&fx, true, || {
        fx.git(&["update-ref", "refs/remotes/origin/side", &x1]);
    });

    let r = replayed_row(&report, "side");
    assert_eq!(r.base, "origin/side");
    assert_eq!(r.replayed, 1);
    assert_eq!(r.files, 0, "a branch nobody stands on moves no file");
    let tip = tip_of(&fx, "refs/heads/side");
    assert_ne!(tip, s1, "side was rewritten onto the new remote tip");
    assert!(
        is_ancestor(&fx, &x1, "side"),
        "the replay is what keeps their commit"
    );
    assert!(!report.blocked());
    assert_eq!(sync_ops(&fx), 1, "the replay rides the run's one operation");
    let record = tip_record(&fx.repo());
    assert_eq!(record.verb, "sync");
    assert_eq!(record.rewrites.len(), 1, "{:?}", record.rewrites);

    let repo = fx.repo();
    let opts = ff_core::RewindOptions {
        force: false,
        now: Some(NOW + 100),
        argv: vec!["ff".into(), "undo".into()],
    };
    ff_core::undo(&repo, &opts, &prov()).unwrap();
    assert_eq!(
        tip_of(&fx, "refs/heads/side"),
        s1,
        "one undo puts side back where it stood"
    );
}

/// `main` moves and a restack of `side`, run from `main`, rewrites its
/// commit: the divergence below is fufu's own, and the tracking ref never
/// moves. The log accounts for the stale copy, so the branch is left where
/// it stands rather than replayed back onto its own original.
#[test]
fn an_other_branch_rewritten_by_the_log_is_yours() {
    let fx = Fixture::new();
    ident(&fx);
    let (_c0, s1) = main_and_side(&fx);
    track_branch(&fx, "side", &s1);
    fx.write("m2.txt", "m2\n");
    fx.commit("m2");
    let repo = fx.repo();
    ff_core::restack::restack(
        &repo,
        Some("side".into()),
        None,
        &prov(),
        Some(NOW),
        vec!["ff".into(), "restack".into()],
    )
    .unwrap();
    let rewritten = tip_of(&fx, "refs/heads/side");
    assert_ne!(rewritten, s1, "test fixture: the restack rewrote side");

    let report = sync_around(&fx, true, || {});

    assert_eq!(
        row_for(&report, "side"),
        BranchSync::Synced {
            branch: "side".into(),
            remote: BranchRemote::Yours {
                name: "origin/side".into(),
                ahead: 2,
                behind: 1,
            },
            base: Box::new(on_base("side", "main")),
        }
    );
    assert_eq!(
        tip_of(&fx, "refs/heads/side"),
        rewritten,
        "sync must not replay your own rewrite back onto the stale remote"
    );
    assert!(!report.blocked());
    assert_eq!(sync_ops(&fx), 0, "nothing moved, so nothing was recorded");
}

/// Their commit reached the tracking ref through an earlier fetch, so this
/// run's fetch moves nothing. That silence does not make the divergence
/// yours: the log holds no record of the stranger's commit, so `side`
/// replays onto it instead of being left for a force-publish over it.
#[test]
fn an_other_branch_with_a_stranger_commit_replays_without_a_fetch() {
    let fx = Fixture::new();
    ident(&fx);
    let (_c0, s1) = main_and_side(&fx);
    let x1 = unrelated_commit(&fx);
    // The earlier fetch already left the shared copy at the stranger's
    // commit, before this run's first reading.
    track_branch(&fx, "side", &x1);

    let report = sync_around(&fx, true, || {});

    let r = replayed_row(&report, "side");
    assert_eq!(r.replayed, 1);
    assert_ne!(tip_of(&fx, "refs/heads/side"), s1);
    assert!(
        is_ancestor(&fx, &x1, "side"),
        "the replay is what keeps the stranger's commit"
    );
}

/// `first` and `second` off `main`'s one commit, standing on `main`.
/// `first` holds `mine`, which rewrites the same line of the same file as
/// `theirs`, the commit its shared copy will hold after the fetch; `second`
/// stands at the root one commit behind its shared copy. Returns
/// `(c0, mine, theirs, t1)`. Both tracking refs stand at the root, so
/// the fetch closure is what moves `origin/first`.
fn first_conflicts_second_follows(fx: &Fixture) -> (String, String, String, String) {
    fx.write("root.txt", "root\n");
    let c0 = fx.commit("root");
    fx.git(&["switch", "-q", "-c", "first"]);
    fx.write("shared.txt", "mine\n");
    let mine = fx.commit("mine");
    fx.git(&["switch", "-q", "-c", "scratch", &c0]);
    fx.write("shared.txt", "theirs\n");
    let theirs = fx.commit("theirs");
    fx.git(&["switch", "-q", "-c", "scratch2", &c0]);
    fx.write("t.txt", "t\n");
    let t1 = fx.commit("t1");
    fx.git(&["switch", "-q", "main"]);
    fx.git(&["branch", "-q", "-D", "scratch", "scratch2"]);
    fx.git(&["branch", "-q", "second", &c0]);
    track_branch(fx, "first", &c0);
    track_branch(fx, "second", &t1);
    (c0, mine, theirs, t1)
}

#[test]
fn an_other_branch_that_conflicts_holds_and_the_run_continues() {
    let fx = Fixture::new();
    ident(&fx);
    let (c0, mine, theirs, t1) = first_conflicts_second_follows(&fx);

    let report = sync_around(&fx, true, || {
        fx.git(&["update-ref", "refs/remotes/origin/first", &theirs]);
    });

    let held = match row_for(&report, "first") {
        BranchSync::Synced {
            remote:
                BranchRemote::Ran {
                    outcome: RestackOutcome::Held(h),
                    ..
                },
            ..
        } => h,
        other => panic!("a replay that conflicts holds, got {other:?}"),
    };
    assert_eq!(held.branch, "first");
    assert_eq!(held.paths, vec!["shared.txt".to_string()]);
    let repo = fx.repo();
    assert!(
        ff_core::held::of(&repo, "first").unwrap().is_some(),
        "the hold stands on first"
    );
    assert_eq!(
        tip_of(&fx, "refs/heads/first"),
        mine,
        "a hold touches nothing"
    );

    assert_eq!(
        row_for(&report, "second"),
        BranchSync::Synced {
            branch: "second".into(),
            remote: BranchRemote::Moved {
                name: "origin/second".into(),
                fast_forward: true,
                behind: 1,
                old: c0,
                new: t1.clone(),
            },
            base: Box::new(on_base("second", "main")),
        },
        "the run goes on past a hold"
    );
    assert_eq!(
        base_of(&report, "first"),
        BaseAxis::Skipped,
        "a branch whose remote axis held gets no base axis"
    );
    assert_eq!(tip_of(&fx, "refs/heads/second"), t1);
    assert_eq!(sync_ops(&fx), 1, "the hold and the move ride one operation");
    let record = tip_record(&repo);
    assert_eq!(record.refs.len(), 1, "{:?}", record.refs);
    assert_eq!(record.cascade_held.len(), 1, "{:?}", record.cascade_held);
    assert_eq!(record.cascade_held[0].branch, "first");
    assert!(record.held.is_none(), "the branch underfoot did not hold");
    assert!(
        report.blocked(),
        "a hold anywhere in the run needs a person"
    );
}

/// The branch underfoot's own report is untouched by a hold on a branch it
/// is not: its axes answer for themselves, and only the exit changes.
#[test]
fn a_hold_elsewhere_leaves_the_current_branch_report_alone() {
    let fx = Fixture::new();
    ident(&fx);
    let (c0, _mine, theirs, _t1) = first_conflicts_second_follows(&fx);
    // main's shared copy is level with it.
    track_branch(&fx, "main", &c0);

    let report = sync_around(&fx, true, || {
        fx.git(&["update-ref", "refs/remotes/origin/first", &theirs]);
    });

    assert!(matches!(
        row_for(&report, "first"),
        BranchSync::Synced {
            remote: BranchRemote::Ran {
                outcome: RestackOutcome::Held(_),
                ..
            },
            ..
        }
    ));
    assert_eq!(report.branch, "main");
    assert!(report.fetched);
    assert!(
        matches!(
            report.remote,
            RemoteAxis::Ran {
                outcome: RestackOutcome::NothingToRestack { .. },
                ..
            }
        ),
        "{:?}",
        report.remote
    );
    assert_eq!(report.base, BaseAxis::NoBase);
    assert_eq!(report.pending, ff_core::Pending::Ahead(0));
    assert_eq!(tip_of(&fx, "refs/heads/main"), c0);
    assert!(report.blocked());
}

#[test]
fn an_other_branch_whose_remote_was_undone_is_not_taken_back_in() {
    let fx = Fixture::new_cloned();
    fx.write("root.txt", "root\n");
    fx.commit("root");
    fx.git(&["switch", "-q", "-c", "side"]);
    let (one, two) = published_then_undone_on(&fx, "side");
    fx.git(&["switch", "-q", "main"]);
    fx.set_config("branch.side.remote", "origin");
    fx.set_config("branch.side.merge", "refs/heads/side");
    assert_eq!(
        tip_of(&fx, "refs/remotes/origin/side"),
        two,
        "test fixture: the push left the tracking ref at the undone tip"
    );

    let report = sync_around(&fx, true, || {});

    assert_eq!(
        row_for(&report, "side"),
        BranchSync::Synced {
            branch: "side".into(),
            remote: BranchRemote::Undone {
                name: "origin/side".into(),
                behind: 1,
            },
            base: Box::new(on_base("side", "main")),
        }
    );
    assert_eq!(
        tip_of(&fx, "refs/heads/side"),
        one,
        "the fast-forward would reverse the undo"
    );
    assert_eq!(sync_ops(&fx), 0);
}

#[test]
fn an_other_branch_tracking_an_unfetched_remote_gets_no_remote_axis() {
    let fx = Fixture::new();
    ident(&fx);
    let (c0, s1) = main_and_side(&fx);
    fx.git(&["branch", "-f", "side", &c0]);
    // origin is the remote the run fetches from; side answers to a second
    // one, whose tracking ref this run never refreshed.
    fx.set_config("remote.origin.url", "/nonexistent/remote.git");
    fx.set_config("remote.origin.fetch", "+refs/heads/*:refs/remotes/origin/*");
    fx.set_config("remote.upstream.url", "/nonexistent/upstream.git");
    fx.set_config(
        "remote.upstream.fetch",
        "+refs/heads/*:refs/remotes/upstream/*",
    );
    fx.set_config("branch.side.remote", "upstream");
    fx.set_config("branch.side.merge", "refs/heads/side");
    fx.git(&["update-ref", "refs/remotes/upstream/side", &s1]);

    let report = sync_around(&fx, true, || {});

    assert_eq!(
        row_for(&report, "side"),
        BranchSync::Synced {
            branch: "side".into(),
            remote: BranchRemote::NotFetched {
                name: "upstream/side".into(),
            },
            base: Box::new(on_base("side", "main")),
        }
    );
    assert_eq!(
        tip_of(&fx, "refs/heads/side"),
        c0,
        "a tip this run did not fetch is not one it acts on"
    );
    assert_eq!(sync_ops(&fx), 0);
}

#[test]
fn a_branch_held_by_another_worktree_is_skipped_and_named() {
    let fx = Fixture::new();
    ident(&fx);
    let (c0, s1) = main_and_side(&fx);
    fx.git(&["branch", "-f", "side", &c0]);
    track_branch(&fx, "side", &s1);
    let bay = fx.root().join("bay");
    let created = ff_core::linked::add::create(&fx.repo(), &bay, "side", 0).expect("create");

    let report = sync_around(&fx, true, || {});

    let path = match row_for(&report, "side") {
        BranchSync::Elsewhere { branch, path } => {
            assert_eq!(branch, "side");
            path
        }
        other => panic!("a branch another worktree holds is skipped, got {other:?}"),
    };
    assert!(
        ff_testsupport::paths::is(&path, &created.path),
        "the row names the worktree: {path} vs {}",
        created.path.display()
    );
    assert_eq!(
        tip_of(&fx, "refs/heads/side"),
        c0,
        "sync must not move a branch out from under another worktree"
    );
    assert_eq!(sync_ops(&fx), 0);
}

// --- The base axis over the whole repository, parent first ----------------
//
// Once the remote phase has run everywhere, every branch with a row gets
// its base axis: `restack` with `onto: None`, in an order that puts a
// parent before its child. A restack cascades into the branches stacked
// above it, so a child reached later in the order finds its base already
// moved and reads up to date, and nothing replays twice.

/// The base axis a branch already sitting on its base reads.
fn on_base(branch: &str, base: &str) -> BaseAxis {
    BaseAxis::Ran {
        name: base.into(),
        outcome: RestackOutcome::NothingToRestack {
            branch: branch.into(),
            base: base.into(),
        },
    }
}

/// The base axis a row carries, or a panic naming what the row is instead.
fn base_of(report: &SyncReport, branch: &str) -> BaseAxis {
    match row_for(report, branch) {
        BranchSync::Synced { base, .. } => *base,
        other => panic!("{branch} has no base axis: {other:?}"),
    }
}

/// A commit on top of `main` that `main` itself does not move to: what a
/// collaborator's push would leave the shared copy of trunk at. Made on a
/// scratch branch that is deleted again, standing on `main` throughout.
fn ahead_of_main(fx: &Fixture, file: &str) -> String {
    fx.git(&["switch", "-q", "-c", "scratch", "main"]);
    fx.write(file, "x\n");
    let sha = fx.commit(file);
    fx.git(&["switch", "-q", "main"]);
    fx.git(&["branch", "-q", "-D", "scratch"]);
    sha
}

/// `main` at `c0`, `a` and `b` each one commit off it with no recorded
/// parent, standing on `main`. Returns `(c0, a1, b1)`.
fn two_bare_starts(fx: &Fixture) -> (String, String, String) {
    fx.write("root.txt", "root\n");
    let c0 = fx.commit("root");
    fx.git(&["switch", "-q", "-c", "a"]);
    fx.write("a.txt", "a\n");
    let a1 = fx.commit("a1");
    fx.git(&["switch", "-q", "-c", "b", "main"]);
    fx.write("b.txt", "b\n");
    let b1 = fx.commit("b1");
    fx.git(&["switch", "-q", "main"]);
    (c0, a1, b1)
}

/// `main` at `c0`, `a` one commit off it, `b` one commit off `a` with `a`
/// recorded as its parent, standing on `main`. Returns `(c0, a1, b1)`.
fn stack_of_two(fx: &Fixture) -> (String, String, String) {
    fx.write("root.txt", "root\n");
    let c0 = fx.commit("root");
    fx.git(&["switch", "-q", "-c", "a"]);
    fx.write("a.txt", "a\n");
    let a1 = fx.commit("a1");
    fx.git(&["switch", "-q", "-c", "b"]);
    fx.write("b.txt", "b\n");
    let b1 = fx.commit("b1");
    stacked_on(fx, "b", "a");
    fx.git(&["switch", "-q", "main"]);
    (c0, a1, b1)
}

#[test]
fn a_fast_forwarded_trunk_moves_every_bare_started_branch() {
    let fx = Fixture::new();
    ident(&fx);
    let (c0, a1, b1) = two_bare_starts(&fx);
    track_branch(&fx, "main", &c0);
    let m2 = ahead_of_main(&fx, "m2.txt");

    let report = sync_around(&fx, true, || {
        fx.git(&["update-ref", "refs/remotes/origin/main", &m2]);
    });

    assert!(
        matches!(
            &report.remote,
            RemoteAxis::Ran {
                outcome: RestackOutcome::Restacked(r),
                ..
            } if r.fast_forward
        ),
        "{:?}",
        report.remote
    );
    assert_eq!(tip_of(&fx, "refs/heads/main"), m2);
    assert_eq!(report.base, BaseAxis::NoBase, "trunk sits on nothing");
    for (branch, before) in [("a", &a1), ("b", &b1)] {
        let tip = tip_of(&fx, &format!("refs/heads/{branch}"));
        assert_ne!(&tip, before, "{branch} followed main");
        assert!(is_ancestor(&fx, &m2, &tip), "{branch} sits on the new main");
        match base_of(&report, branch) {
            BaseAxis::Ran {
                name,
                outcome: RestackOutcome::Restacked(_) | RestackOutcome::NothingToRestack { .. },
            } => assert_eq!(name, "main"),
            other => panic!("{branch}'s base axis ran, got {other:?}"),
        }
    }
    assert!(!report.blocked());
}

/// The verb operations in the log, captures and notes excluded, so the
/// count says how many things the user asked for happened.
fn verb_ops(fx: &Fixture) -> usize {
    let repo = fx.repo();
    let log = ff_core::ops::OpLog::open(&repo).unwrap();
    log.iter()
        .flatten()
        .filter(|op| op.kind() == ff_core::ops::OpKind::Op)
        .count()
}

/// Whatever the run moved, it is one operation, and one undo takes every
/// branch back: trunk's fast-forward off the remote, both bare-started
/// branches' replays onto it, and the open change carried along on the
/// branch underfoot.
#[test]
fn a_whole_sync_is_one_operation_and_one_undo() {
    let fx = Fixture::new();
    ident(&fx);
    let (c0, a1, b1) = two_bare_starts(&fx);
    track_branch(&fx, "main", &c0);
    let m2 = ahead_of_main(&fx, "m2.txt");
    fx.write("wip.txt", "open\n");
    let before = verb_ops(&fx);

    let report = sync_around(&fx, true, || {
        fx.git(&["update-ref", "refs/remotes/origin/main", &m2]);
    });

    assert!(!report.blocked());
    assert_eq!(tip_of(&fx, "refs/heads/main"), m2);
    let a = tip_of(&fx, "refs/heads/a");
    let b = tip_of(&fx, "refs/heads/b");
    assert_ne!(a, a1);
    assert_ne!(b, b1);
    assert_eq!(
        verb_ops(&fx),
        before + 1,
        "three branches moved and the log grew by one operation"
    );
    let repo = fx.repo();
    let record = tip_record(&repo);
    assert_eq!(record.verb, "sync");
    let mut moved: Vec<&str> = record.refs.iter().map(|t| t.name.as_str()).collect();
    moved.sort();
    assert_eq!(
        moved,
        ["refs/heads/a", "refs/heads/b", "refs/heads/main"],
        "one transition per branch the run moved"
    );
    assert_eq!(
        record.rewrites.len(),
        2,
        "a's and b's replays ride the record: {:?}",
        record.rewrites
    );
    assert!(record.held.is_none() && record.cascade_held.is_empty());
    assert_eq!(
        std::fs::read_to_string(fx.path().join("wip.txt")).unwrap(),
        "open\n",
        "the open change came along"
    );
    assert!(
        fx.path().join("m2.txt").exists(),
        "the worktree moved with main"
    );

    let opts = ff_core::RewindOptions {
        force: false,
        now: Some(NOW + 100),
        argv: vec!["ff".into(), "undo".into()],
    };
    ff_core::undo(&repo, &opts, &prov()).unwrap();
    assert_eq!(tip_of(&fx, "refs/heads/main"), c0);
    assert_eq!(tip_of(&fx, "refs/heads/a"), a1);
    assert_eq!(tip_of(&fx, "refs/heads/b"), b1);
    assert_eq!(
        std::fs::read_to_string(fx.path().join("wip.txt")).unwrap(),
        "open\n",
        "the open change is still open"
    );
    assert!(
        !fx.path().join("m2.txt").exists(),
        "the worktree came back with main"
    );
}

#[test]
fn a_stack_replays_parent_before_child() {
    let fx = Fixture::new();
    ident(&fx);
    let (c0, a1, b1) = stack_of_two(&fx);
    track_branch(&fx, "main", &c0);
    let m2 = ahead_of_main(&fx, "m2.txt");
    assert_eq!(
        ff_core::sync::base_order(&fx.repo()).unwrap(),
        ["main", "a", "b"]
    );

    let report = sync_around(&fx, true, || {
        fx.git(&["update-ref", "refs/remotes/origin/main", &m2]);
    });

    let a = tip_of(&fx, "refs/heads/a");
    let b = tip_of(&fx, "refs/heads/b");
    assert_ne!(a, a1, "a followed main");
    assert_ne!(b, b1, "b followed a");
    assert!(is_ancestor(&fx, &m2, &a), "a sits on the new main");
    assert!(is_ancestor(&fx, &a, &b), "b sits on the new a");
    assert_eq!(
        base_of(&report, "b"),
        on_base("b", "a"),
        "b found a already moved, so nothing replayed twice"
    );
    assert!(!report.blocked());
}

/// The same stack with `main` moved by a commit of its own and no remote in
/// the picture: nothing cascades in the remote phase, so the base phase is
/// what replays the stack, parent first. A third branch off `main` comes
/// after the whole of `a`'s subtree.
#[test]
fn a_stale_stack_is_replayed_by_the_base_phase_parent_first() {
    let fx = Fixture::new();
    ident(&fx);
    let (_c0, a1, b1) = stack_of_two(&fx);
    fx.git(&["switch", "-q", "-c", "c", "main"]);
    fx.write("c.txt", "c\n");
    let c1 = fx.commit("c1");
    fx.git(&["switch", "-q", "main"]);
    fx.write("m2.txt", "m2\n");
    let m2 = fx.commit("m2");
    assert_eq!(
        ff_core::sync::base_order(&fx.repo()).unwrap(),
        ["main", "a", "b", "c"]
    );

    let report = sync_around(&fx, true, || {});

    let r = match base_of(&report, "a") {
        BaseAxis::Ran {
            name,
            outcome: RestackOutcome::Restacked(r),
        } => {
            assert_eq!(name, "main");
            *r
        }
        other => panic!("a replays onto the moved main, got {other:?}"),
    };
    assert_eq!(r.replayed, 1);
    assert_eq!(r.cascade.moved.len(), 1, "{:?}", r.cascade);
    assert_eq!(r.cascade.moved[0].branch, "b");
    assert_eq!(base_of(&report, "b"), on_base("b", "a"));
    let a = tip_of(&fx, "refs/heads/a");
    let b = tip_of(&fx, "refs/heads/b");
    let c = tip_of(&fx, "refs/heads/c");
    assert_ne!(a, a1);
    assert_ne!(b, b1);
    assert_ne!(c, c1);
    assert!(is_ancestor(&fx, &m2, &a));
    assert!(is_ancestor(&fx, &a, &b));
    assert!(is_ancestor(&fx, &m2, &c));
    assert!(!report.blocked());
}

/// Standing on `b`, the child of `a`, when `main` moved: the base phase
/// replays `a` first and its cascade carries `b` with the open change, so
/// the branch underfoot's own base axis reads up to date. The worktree
/// still moved, and the report says so at the top level, since no axis of
/// the branch underfoot landed to carry the count.
#[test]
fn a_branch_underfoot_carried_by_a_cascade_reports_the_worktree_write() {
    let fx = Fixture::new();
    ident(&fx);
    let (_c0, a1, b1) = stack_of_two(&fx);
    fx.write("m2.txt", "m2\n");
    let m2 = fx.commit("m2");
    fx.git(&["switch", "-q", "b"]);
    fx.write("wip.txt", "open\n");

    let report = sync_around(&fx, true, || {});

    assert_eq!(report.base, on_base("b", "a"), "{:?}", report.base);
    let r = match base_of(&report, "a") {
        BaseAxis::Ran {
            outcome: RestackOutcome::Restacked(r),
            ..
        } => *r,
        other => panic!("a replays onto the moved main, got {other:?}"),
    };
    assert_eq!(r.cascade.moved.len(), 1, "{:?}", r.cascade);
    assert_eq!(r.cascade.moved[0].branch, "b");
    assert_eq!(r.files, 0, "the count is the run's, not a's");
    assert_eq!(report.files, 1, "m2.txt arrived in the working tree");
    assert!(report.still_open, "wip.txt is still open");
    assert!(fx.path().join("m2.txt").exists());
    assert_eq!(
        std::fs::read_to_string(fx.path().join("wip.txt")).unwrap(),
        "open\n"
    );
    let a = tip_of(&fx, "refs/heads/a");
    let b = tip_of(&fx, "refs/heads/b");
    assert_ne!(a, a1);
    assert_ne!(b, b1);
    assert!(is_ancestor(&fx, &m2, &a));
    assert!(is_ancestor(&fx, &a, &b));
    assert!(!report.blocked());
}

#[test]
fn an_onto_loop_terminates() {
    let fx = Fixture::new();
    ident(&fx);
    let (_c0, _a1, _b1) = two_bare_starts(&fx);
    stacked_on(&fx, "a", "b");
    stacked_on(&fx, "b", "a");

    assert_eq!(
        ff_core::sync::base_order(&fx.repo()).unwrap(),
        ["main", "a", "b"],
        "a loop has no root: its members come after every rooted tree, by name, once"
    );

    let report = sync_around(&fx, true, || {});

    assert_eq!(report.branches.len(), 2, "{:?}", report.branches);
    for branch in ["a", "b"] {
        assert!(
            matches!(base_of(&report, branch), BaseAxis::Ran { .. }),
            "{branch} was visited once"
        );
    }
}

#[test]
fn a_held_branch_leaves_its_subtree_alone() {
    let fx = Fixture::new();
    ident(&fx);
    fx.write("shared.txt", "base\n");
    fx.commit("root");
    fx.git(&["switch", "-q", "-c", "a"]);
    fx.write("shared.txt", "mine\n");
    let a1 = fx.commit("a1");
    fx.git(&["switch", "-q", "-c", "b"]);
    fx.write("b.txt", "b\n");
    let b1 = fx.commit("b1");
    stacked_on(&fx, "b", "a");
    fx.git(&["switch", "-q", "main"]);
    fx.write("shared.txt", "theirs\n");
    fx.commit("m2");

    let report = sync_around(&fx, true, || {});

    let held = match base_of(&report, "a") {
        BaseAxis::Ran {
            name,
            outcome: RestackOutcome::Held(h),
        } => {
            assert_eq!(name, "main");
            h
        }
        other => panic!("a conflicts with the moved main and holds, got {other:?}"),
    };
    assert_eq!(held.branch, "a");
    assert_eq!(held.paths, vec!["shared.txt".to_string()]);
    assert!(
        ff_core::held::of(&fx.repo(), "a").unwrap().is_some(),
        "the hold stands on a"
    );
    assert_eq!(tip_of(&fx, "refs/heads/a"), a1, "a hold touches nothing");
    assert_eq!(
        tip_of(&fx, "refs/heads/b"),
        b1,
        "b sits on a, which did not move"
    );
    assert_eq!(base_of(&report, "b"), on_base("b", "a"));
    assert!(
        report.blocked(),
        "a hold anywhere in the run needs a person"
    );
}

/// A branch whose one commit its base already holds has nothing of its own:
/// the base moved past it, so the ref fast-forwards and nothing is replayed,
/// and in particular nothing is replayed to an empty commit and dropped.
#[test]
fn a_branch_with_nothing_of_its_own_is_not_replayed_to_nothing() {
    let fx = Fixture::new();
    ident(&fx);
    fx.write("root.txt", "root\n");
    fx.commit("root");
    fx.git(&["switch", "-q", "-c", "a"]);
    fx.write("a.txt", "a\n");
    let a1 = fx.commit("a1");
    fx.git(&["switch", "-q", "main"]);
    fx.git(&["merge", "-q", "--ff-only", "a"]);
    fx.write("m2.txt", "m2\n");
    let m2 = fx.commit("m2");

    let report = sync_around(&fx, true, || {});

    let r = match base_of(&report, "a") {
        BaseAxis::Ran {
            outcome: RestackOutcome::Restacked(r),
            ..
        } => *r,
        other => panic!("a strict ancestor of its base fast-forwards, got {other:?}"),
    };
    assert!(r.fast_forward);
    assert_eq!(r.replayed, 0);
    assert!(
        r.dropped.is_empty(),
        "nothing was replayed, so nothing was dropped"
    );
    assert_eq!(tip_of(&fx, "refs/heads/a"), m2);
    assert!(is_ancestor(&fx, &a1, &m2));
}

#[test]
fn a_branch_with_no_base_gets_no_base_axis() {
    let fx = Fixture::new();
    ident(&fx);
    let (_c0, _s1) = main_and_side(&fx);
    fx.git(&["switch", "-q", "side"]);

    let report = sync_around(&fx, true, || {});

    assert_eq!(report.branch, "side");
    assert_eq!(
        base_of(&report, "main"),
        BaseAxis::NoBase,
        "trunk sits on nothing"
    );
    assert_eq!(report.base, on_base("side", "main"));
}

/// A replay `restack` refuses before anything moves is named and left where
/// it stands, and the run goes on: one branch's merge or orphan history is
/// no reason to leave the rest stale.
#[test]
fn a_merge_in_range_and_an_orphan_are_named_and_the_run_goes_on() {
    let fx = Fixture::new();
    ident(&fx);
    fx.write("root.txt", "root\n");
    fx.commit("root");
    fx.git(&["switch", "-q", "-c", "x"]);
    fx.write("x.txt", "x\n");
    fx.commit("x1");
    fx.git(&["switch", "-q", "-c", "merged", "main"]);
    fx.write("m.txt", "m\n");
    fx.commit("m1");
    fx.git(&["merge", "-q", "--no-ff", "-m", "merge x", "x"]);
    let merged = tip_of(&fx, "refs/heads/merged");
    fx.git(&["branch", "-q", "-D", "x"]);
    fx.git(&["switch", "-q", "--orphan", "orphan"]);
    fx.write("o.txt", "o\n");
    let orphan = fx.commit("o1");
    fx.git(&["switch", "-q", "-c", "a", "main"]);
    fx.write("a.txt", "a\n");
    let a1 = fx.commit("a1");
    fx.git(&["switch", "-q", "main"]);
    fx.write("m2.txt", "m2\n");
    let m2 = fx.commit("m2");

    let report = sync_around(&fx, true, || {});

    assert_eq!(
        base_of(&report, "merged"),
        BaseAxis::Refused {
            name: "main".into(),
            reason: SkipReason::MergeInRange,
        }
    );
    assert_eq!(tip_of(&fx, "refs/heads/merged"), merged);
    assert_eq!(
        base_of(&report, "orphan"),
        BaseAxis::Refused {
            name: "main".into(),
            reason: SkipReason::Unrelated,
        }
    );
    assert_eq!(tip_of(&fx, "refs/heads/orphan"), orphan);
    let a = tip_of(&fx, "refs/heads/a");
    assert_ne!(a, a1, "the run went on to a");
    assert!(is_ancestor(&fx, &m2, &a));
    assert!(!report.blocked());
}
