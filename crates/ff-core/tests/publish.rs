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
    plan(fx, false).0
}

/// The plan, plus whether a verb context came back — which is how "a dry run
/// writes nothing" is observable from here: no context means no capture.
fn plan(fx: &Fixture, dry_run: bool) -> (PublishReport, bool) {
    let repo = fx.repo();
    let pre = ff_core::preflight::preflight(&repo, ff_core::preflight::Verb::Publish).unwrap();
    let (report, ctx) = ff_core::publish::publish(
        &repo,
        &pre,
        ff_core::publish::PublishOptions {
            dry_run,
            now: Some(NOW),
            argv: vec!["ff".into(), "publish".into()],
        },
        &prov(),
    )
    .unwrap();
    (report, ctx.is_some())
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
            shape,
        } => {
            assert_eq!(shape, ff_core::PushShape::Replace);
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
    // Deleted, not never-created: a sibling tracking ref is what says a copy
    // once stood there, and a clone of a non-empty remote always has some.
    fx.git(&["update-ref", "refs/remotes/origin/main", &f1]);

    match publish_call(&fx).publish {
        Publish::Push {
            remote_branch,
            lease,
            shape,
            ..
        } => {
            assert_eq!(remote_branch, "feature");
            assert_eq!(lease, "", "must not exist");
            assert_eq!(shape, ff_core::PushShape::Recreate);
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
    // cannot succeed and the rewrite is held. Aiming at the shared copy is
    // `ff sync`'s remote axis, which this stands in for, so the call is
    // spelled the way sync spells it: `Aim::Settled`, which is what the
    // refusal `--onto` owes a person does not apply to.
    fx.git(&["switch", "-q", "-c", "collab"]);
    fx.write("shared.txt", "theirs\n");
    let collab = fx.commit("collab");
    fx.git(&["switch", "-q", "feature"]);
    fx.write("shared.txt", "mine\n");
    let _local = fx.commit("local");
    track(&fx, &f1);
    fx.git(&["update-ref", "refs/remotes/origin/feature", &collab]);

    let repo = fx.repo();
    let (outcome, _) = ff_core::restack::restack_with(
        &repo,
        None,
        Some("refs/remotes/origin/feature".to_string()),
        &prov(),
        (Some(NOW), vec!["ff".into(), "restack".into()]),
        &ff_core::rewrite::Decided::none(),
        ff_core::Aim::Settled,
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

/// A dry run decides the same plan and writes nothing to get there — no
/// capture, no operation. Same rule as `ff trim -n`.
#[test]
fn a_dry_run_plans_the_same_push_and_writes_nothing() {
    let fx = Fixture::new();
    let f1 = feature(&fx);
    track(&fx, &f1);
    fx.write("g.txt", "g\n");
    let f2 = fx.commit("f2");

    let ops_before = fx.git(&[
        "for-each-ref",
        "--format=%(refname) %(objectname)",
        "refs/fufu/",
    ]);
    let (report, captured) = plan(&fx, true);

    assert!(report.dry_run, "the report says it sent nothing");
    assert!(!captured, "a dry run takes no capture");
    match report.publish {
        Publish::Push { lease, tip, .. } => {
            assert_eq!(lease, f1, "the same lease the real run would offer");
            assert_eq!(tip, f2);
        }
        other => panic!("the plan is unchanged by previewing it, got {other:?}"),
    }
    assert_eq!(
        fx.git(&[
            "for-each-ref",
            "--format=%(refname) %(objectname)",
            "refs/fufu/"
        ]),
        ops_before,
        "a dry run writes nothing under refs/fufu"
    );
}

// --- The record ------------------------------------------------------------
//
// A real bare remote, because an undone publish only exists on the far side
// of a push and every other test here fakes the remote with `update-ref`.

/// Push `branch` to the fixture's real remote, then record it the way
/// `ff publish` does: plan, push, record.
fn publish_for_real(fx: &Fixture, branch: &str) -> PublishReport {
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
    let spec = format!("{branch}:{branch}");
    fx.git(&["push", "--force", "origin", &spec]);
    ff_core::publish::record(&repo, &pre, &report, ctx.as_ref().unwrap(), &prov()).unwrap();
    report
}

/// The newest publish row on `branch`, if the log holds one.
fn published_row(fx: &Fixture, branch: &str) -> Option<ff_core::ops::Published> {
    let repo = fx.repo();
    let log = ff_core::ops::OpLog::open(&repo).unwrap();
    for op in log.iter_branch(branch) {
        let op = op.unwrap();
        if let Some(record) = op.record().unwrap()
            && let Some(published) = &record.published
        {
            return Some(published.clone());
        }
    }
    None
}

#[test]
fn a_push_is_recorded_as_a_note_naming_where_it_left_the_remote() {
    let fx = Fixture::new_cloned();
    fx.write("a.txt", "a\n");
    let one = fx.commit("one");
    publish_for_real(&fx, "main");

    let row = published_row(&fx, "main").expect("the push is on the log");
    assert_eq!(row.remote, "origin");
    assert_eq!(row.remote_branch, "main");
    assert_eq!(
        row.from, None,
        "the first push had nothing to lease against"
    );
    assert_eq!(row.to, one);

    // The kind is the whole argument for it: a note is something that
    // happened, so undo steps over it and revert refuses it.
    let repo = fx.repo();
    let log = ff_core::ops::OpLog::open(&repo).unwrap();
    let tip = log.branch_tip("main").unwrap().unwrap();
    assert_eq!(log.get(tip).unwrap().kind(), ff_core::ops::OpKind::Note);
    assert!(ff_core::published_tip(&repo, "main", &one).unwrap());
    assert!(ff_core::ever_published(&repo, "main").unwrap());
}

/// The second push leases against the first: `from` is where the remote
/// stood, `to` is where it stands now.
#[test]
fn the_second_push_records_the_lease_it_went_out_under() {
    let fx = Fixture::new_cloned();
    fx.write("a.txt", "a\n");
    let one = fx.commit("one");
    publish_for_real(&fx, "main");
    fx.write("a.txt", "aa\n");
    let two = fx.commit("two");
    publish_for_real(&fx, "main");

    let row = published_row(&fx, "main").expect("the push is on the log");
    assert_eq!(row.from, Some(one));
    assert_eq!(row.to, two);
}

/// A dry run sends nothing, so there is nothing to remember: no note, and no
/// pointer for the next sync to read.
#[test]
fn a_dry_run_records_no_push() {
    let fx = Fixture::new_cloned();
    fx.write("a.txt", "a\n");
    fx.commit("one");

    let (report, captured) = plan(&fx, true);
    assert!(report.dry_run);
    assert!(!captured);
    assert!(published_row(&fx, "main").is_none(), "nothing happened");
    assert!(!ff_core::ever_published(&fx.repo(), "main").unwrap());
}

/// The memory of a push must outlive `ff undo`, and that is the whole reason
/// it is a ref beside the note rather than only the note: undo is a pointer
/// move, so everything above the landing leaves the log — the publish row
/// with it — and undo is precisely the thing that cannot reach the remote.
#[test]
fn undo_rewinds_the_log_past_the_note_and_not_past_the_memory() {
    let fx = Fixture::new_cloned();
    fx.write("a.txt", "a\n");
    fx.commit("one");
    publish_for_real(&fx, "main");
    fx.write("a.txt", "aa\n");
    let two = fx.commit("two");
    publish_for_real(&fx, "main");

    let repo = fx.repo();
    ff_core::undo(
        &repo,
        &ff_core::RewindOptions {
            now: Some(NOW),
            ..Default::default()
        },
        &prov(),
    )
    .unwrap();

    let repo = fx.repo();
    assert_ne!(
        published_row(&fx, "main").map(|r| r.to),
        Some(two.clone()),
        "the newest note is above the landing and steps off with it"
    );
    assert!(
        ff_core::published_tip(&repo, "main", &two).unwrap(),
        "the remote is still where the push left it, and fufu still knows"
    );
}

/// A tip that is an ancestor of the shared copy does not send commits, it
/// takes them off — which is what `ff undo` then `ff publish` does, and the
/// only way back across the wire fufu has.
#[test]
fn a_tip_behind_the_shared_copy_is_a_retraction() {
    let fx = Fixture::new_cloned();
    fx.write("a.txt", "a\n");
    fx.commit("one");
    publish_for_real(&fx, "main");
    fx.write("a.txt", "aa\n");
    fx.commit("two");
    publish_for_real(&fx, "main");
    fx.git(&["reset", "--hard", "-q", "HEAD~1"]);

    match publish_call(&fx).publish {
        Publish::Push { shape, .. } => assert_eq!(shape, ff_core::PushShape::Retract),
        other => panic!("the shared copy is rolled back, got {other:?}"),
    }
}

/// The absent tracking ref used to mean one thing. A clone of an empty
/// remote wears it too, and nothing was lost there.
#[test]
fn an_absent_shared_copy_with_no_evidence_is_a_first_push() {
    let fx = Fixture::new_cloned();
    fx.write("a.txt", "a\n");
    fx.commit("one");

    match publish_call(&fx).publish {
        Publish::Push { lease, shape, .. } => {
            assert_eq!(lease, "", "must not exist, either way");
            assert_eq!(shape, ff_core::PushShape::First);
        }
        other => panic!("the first copy is created, got {other:?}"),
    }
}

/// And with evidence it still means a deletion. Either half of the evidence
/// does it; this is the log's half, with the remote's refs pruned away.
#[test]
fn an_absent_shared_copy_with_a_push_on_record_is_a_re_creation() {
    let fx = Fixture::new_cloned();
    fx.write("a.txt", "a\n");
    fx.commit("one");
    publish_for_real(&fx, "main");
    fx.remote_git(&["update-ref", "-d", "refs/heads/main"]);
    fx.git(&["fetch", "--prune", "-q", "origin"]);

    match publish_call(&fx).publish {
        Publish::Push { lease, shape, .. } => {
            assert_eq!(lease, "");
            assert_eq!(shape, ff_core::PushShape::Recreate);
        }
        other => panic!("the deleted copy is put back, got {other:?}"),
    }
}

/// Two remotes, neither named origin — the state that puts the
/// `sync/ambiguous-remote` refusal on the table, because the ladder has no
/// branch upstream to fall back on.
fn two_remotes(fx: &Fixture) {
    ident(fx);
    fx.write("root.txt", "root\n");
    fx.commit("root");
    fx.set_config("remote.one.url", "/nonexistent/one.git");
    fx.set_config("remote.one.fetch", "+refs/heads/*:refs/remotes/one/*");
    fx.set_config("remote.two.url", "/nonexistent/two.git");
    fx.set_config("remote.two.fetch", "+refs/heads/*:refs/remotes/two/*");
}

#[test]
fn an_explicit_remote_resolves_past_the_ambiguity() {
    let fx = Fixture::new();
    two_remotes(&fx);

    // `Preflight` does not derive `Debug`, so `unwrap_err` is not an option;
    // the expectation carries the claim.
    let err = ff_core::preflight::preflight(&fx.repo(), ff_core::preflight::Verb::Publish)
        .err()
        .expect("two remotes, no origin: the ladder has to refuse");
    assert_eq!(
        err.id(),
        "sync/ambiguous-remote",
        "with no upstream set, the default path still refuses to guess"
    );

    let pre = ff_core::preflight::preflight_to(
        &fx.repo(),
        ff_core::preflight::Verb::Publish,
        Some("two"),
    )
    .unwrap();
    assert_eq!(
        pre.remote,
        Some("two".to_string()),
        "naming the remote settles what the ladder would not"
    );
}

#[test]
fn an_unknown_remote_is_refused() {
    let fx = Fixture::new();
    two_remotes(&fx);

    let err = ff_core::preflight::preflight_to(
        &fx.repo(),
        ff_core::preflight::Verb::Publish,
        Some("nope"),
    )
    .err()
    .expect("a remote that does not exist has to be refused");
    assert_eq!(
        err.id(),
        "publish/unknown-remote",
        "fufu will not invent a remote to publish to"
    );
}

#[test]
fn a_branch_that_answers_elsewhere_refuses_the_retarget() {
    let fx = Fixture::new();
    two_remotes(&fx);
    fx.set_config("branch.main.remote", "one");

    let err = ff_core::preflight::preflight_to(
        &fx.repo(),
        ff_core::preflight::Verb::Publish,
        Some("two"),
    )
    .err()
    .expect("a branch already answering elsewhere has to be refused");
    assert_eq!(
        err.id(),
        "publish/retarget",
        "a branch already answering to one remote is not pointed at a second"
    );
}

#[test]
fn naming_the_remote_already_tracked_changes_nothing() {
    let fx = Fixture::new();
    two_remotes(&fx);
    let sha = fx.git(&["rev-parse", "HEAD"]).trim().to_string();
    fx.set_config("branch.main.remote", "one");
    fx.set_config("branch.main.merge", "refs/heads/main");
    fx.git(&["update-ref", "refs/remotes/one/main", &sha]);

    let plain =
        ff_core::preflight::preflight(&fx.repo(), ff_core::preflight::Verb::Publish).unwrap();
    let named = ff_core::preflight::preflight_to(
        &fx.repo(),
        ff_core::preflight::Verb::Publish,
        Some("one"),
    )
    .unwrap();

    assert_eq!(
        named.remote, plain.remote,
        "naming the remote the branch already answers to changes nothing"
    );
    // `Tracking` derives neither `PartialEq` nor `Debug`, so the claim is
    // spelled out per field.
    let tracked = |p: &ff_core::preflight::Preflight| {
        p.tracking.as_ref().map(|t| {
            (
                t.full.clone(),
                t.name.clone(),
                t.remote_branch.clone(),
                t.tip,
            )
        })
    };
    assert_eq!(
        tracked(&named),
        tracked(&plain),
        "… and neither does the shared copy it points at"
    );
}
