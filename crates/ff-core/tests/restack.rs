//! Contract for `restack::restack`: a branch's commits replayed onto a
//! different base, with the open change carried onto the new tip exactly
//! when the worktree stands on — or inside — the branch being moved.

use ff_core::futures::At;
use ff_core::gix;
use ff_core::{Provenance, RestackOutcome};
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
    Provenance::new("pre", Some("ff restack".into()))
}

fn restack_call(
    fx: &Fixture,
    branch: Option<&str>,
    onto: Option<&str>,
    now: i64,
) -> (RestackOutcome, ff_core::ops::VerbContext) {
    let repo = fx.repo();
    ff_core::restack::restack(
        &repo,
        branch.map(String::from),
        onto.map(String::from),
        &prov(),
        Some(now),
        vec!["ff".into(), "restack".into()],
    )
    .unwrap()
}

/// Every worktree file as (repo-relative path, bytes), sorted by path.
fn worktree_files(fx: &Fixture) -> Vec<(String, Vec<u8>)> {
    let root = fx.path();
    let mut out = Vec::new();
    let mut dirs = vec![root.clone()];
    while let Some(dir) = dirs.pop() {
        for entry in std::fs::read_dir(&dir).unwrap() {
            let entry = entry.unwrap();
            let name = entry.file_name();
            if name == ".git" {
                continue;
            }
            let path = entry.path();
            if path.is_dir() {
                dirs.push(path);
            } else {
                let rel = path
                    .strip_prefix(&root)
                    .unwrap()
                    .to_string_lossy()
                    .replace(std::path::MAIN_SEPARATOR, "/");
                out.push((rel, std::fs::read(&path).unwrap()));
            }
        }
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

/// The newest operation's record, read through the public reader.
fn tip_record(repo: &gix::Repository) -> ff_core::ops::OpRecord {
    let log = ff_core::ops::OpLog::open(repo).unwrap();
    let op = log.get(log.tip().unwrap().unwrap()).unwrap();
    op.record()
        .unwrap()
        .cloned()
        .expect("a verb op has a record")
}

/// The shared stack:
///
/// c0 ─ c1 ─────────── m2        (main)
///       └─ f1 ─ f2 ─ f3         (feature, with `mid` at f2)
///
/// Distinct files throughout, so the replay is clean. Leaves the fixture
/// standing on `feature`.
fn stack(fx: &Fixture) -> (String, String, String, String, String, String) {
    fx.write("root.txt", "root\n");
    let c0 = fx.commit("root");
    fx.write("m.txt", "m\n");
    let c1 = fx.commit("m");

    fx.git(&["switch", "-q", "-c", "feature"]);
    fx.write("a.txt", "a\n");
    let f1 = fx.commit("f1");
    fx.write("b.txt", "b\n");
    let f2 = fx.commit("f2");
    fx.write("c.txt", "c\n");
    let f3 = fx.commit("f3");
    fx.git(&["branch", "mid", &f2]);

    fx.git(&["switch", "-q", "main"]);
    fx.write("d.txt", "d\n");
    let m2 = fx.commit("m2");

    fx.git(&["switch", "-q", "feature"]);
    (c0, c1, f1, f2, f3, m2)
}

#[test]
fn restack_replays_onto_the_moved_base() {
    let fx = Fixture::new();
    ident(&fx);
    let (_c0, _c1, _f1, _f2, _f3, m2) = stack(&fx);

    let (outcome, _ctx) = restack_call(&fx, None, None, NOW);
    let report = match outcome {
        RestackOutcome::Restacked(r) => r,
        other => panic!("the restack must land, got {other:?}"),
    };

    assert_eq!(report.replayed, 3);
    assert!(!report.fast_forward);
    assert_eq!(
        fx.git(&["rev-parse", "feature^^^"]).trim(),
        m2,
        "three hops back from the new tip lands on the moved base"
    );
}

#[test]
fn restack_carries_an_inner_branch() {
    let fx = Fixture::new();
    ident(&fx);
    stack(&fx);

    let (outcome, _ctx) = restack_call(&fx, None, None, NOW);
    let report = match outcome {
        RestackOutcome::Restacked(r) => r,
        other => panic!("the restack must land, got {other:?}"),
    };

    assert!(
        report.moved.contains(&"mid".to_string()),
        "mid sits inside the range and must be carried: {:?}",
        report.moved
    );
}

#[test]
fn restack_carries_the_open_change() {
    let fx = Fixture::new();
    ident(&fx);
    stack(&fx);

    // No commit in feature's range touches this path.
    fx.write("open.txt", "wip\n");

    let (outcome, _ctx) = restack_call(&fx, None, None, NOW);
    let report = match outcome {
        RestackOutcome::Restacked(r) => r,
        other => panic!("the restack must land, got {other:?}"),
    };

    assert_eq!(
        std::fs::read_to_string(fx.path().join("open.txt")).unwrap(),
        "wip\n",
        "the open change must survive the replay"
    );
    assert!(
        fx.path().join("d.txt").exists(),
        "d.txt from the new base must be on disk"
    );
    assert!(report.still_open);
    assert!(report.files > 0);
}

/// A replay onto a base that moved rewrites the worktree — that is the whole
/// job — and a clean tree is still clean afterwards.
///
/// The bug this guards: measuring `still_open` as "the tree changed" makes it
/// true of every replay that did anything, so a restack with nothing open
/// would announce "your change is still open" over a tree git calls clean.
/// What is open is the difference between the worktree and the commit it now
/// sits on, and after a clean replay there is none.
#[test]
fn a_clean_tree_is_still_clean_after_a_replay() {
    let fx = Fixture::new();
    ident(&fx);
    stack(&fx);

    let (outcome, _ctx) = restack_call(&fx, None, None, NOW);
    let report = match outcome {
        RestackOutcome::Restacked(r) => r,
        other => panic!("the restack must land, got {other:?}"),
    };

    assert!(
        report.files > 0,
        "the replay really did rewrite the worktree"
    );
    assert!(
        !report.still_open,
        "nothing was open before the replay, so nothing is open after it"
    );
    assert_eq!(
        fx.git(&["status", "--porcelain"]),
        "",
        "and git agrees the tree is clean"
    );
}

#[test]
fn restack_off_branch_touches_no_file() {
    let fx = Fixture::new();
    ident(&fx);
    stack(&fx);
    fx.git(&["switch", "-q", "main"]);

    let feature_before = fx.git(&["rev-parse", "feature"]).trim().to_string();
    let mid_before = fx.git(&["rev-parse", "mid"]).trim().to_string();
    let before = worktree_files(&fx);

    let (outcome, _ctx) = restack_call(&fx, Some("feature"), None, NOW);
    let report = match outcome {
        RestackOutcome::Restacked(r) => r,
        other => panic!("the restack must land, got {other:?}"),
    };

    assert_ne!(
        fx.git(&["rev-parse", "feature"]).trim(),
        feature_before,
        "feature must move"
    );
    assert_ne!(
        fx.git(&["rev-parse", "mid"]).trim(),
        mid_before,
        "mid must move"
    );
    assert_eq!(report.files, 0);

    let after = worktree_files(&fx);
    assert_eq!(
        before, after,
        "an off-branch restack must leave the worktree byte-identical"
    );
}

#[test]
fn restack_up_to_date_appends_no_operation() {
    let fx = Fixture::new();
    ident(&fx);
    stack(&fx);

    let (outcome, _ctx) = restack_call(&fx, None, None, NOW);
    assert!(matches!(outcome, RestackOutcome::Restacked(_)));

    let ops_tip_before = fx.git(&["rev-parse", "refs/fufu/ops"]).trim().to_string();
    let (outcome, _ctx) = restack_call(&fx, None, None, NOW + 100);
    match outcome {
        RestackOutcome::NothingToRestack { branch, base } => {
            assert_eq!(branch, "feature");
            assert_eq!(base, "main");
        }
        other => panic!("the second restack must be a no-op, got {other:?}"),
    }
    let ops_tip_after = fx.git(&["rev-parse", "refs/fufu/ops"]).trim().to_string();
    assert_eq!(
        ops_tip_before, ops_tip_after,
        "up to date with no re-aim must append nothing"
    );
}

#[test]
fn restack_fast_forward_moves_the_ref() {
    let fx = Fixture::new();
    ident(&fx);
    fx.write("root.txt", "root\n");
    fx.commit("root");
    fx.git(&["switch", "-q", "-c", "topic"]);
    fx.git(&["switch", "-q", "main"]);
    fx.write("m1.txt", "m1\n");
    fx.commit("m1");
    fx.git(&["switch", "-q", "topic"]);

    let (outcome, _ctx) = restack_call(&fx, None, None, NOW);
    let report = match outcome {
        RestackOutcome::Restacked(r) => r,
        other => panic!("the restack must land, got {other:?}"),
    };

    assert!(report.fast_forward);
    assert_eq!(report.replayed, 0);
    assert_eq!(
        fx.git(&["rev-parse", "topic"]).trim(),
        fx.git(&["rev-parse", "main"]).trim()
    );
}

#[test]
fn restack_conflict_holds_and_touches_nothing() {
    let fx = Fixture::new();
    ident(&fx);
    fx.write("f.txt", "one\n");
    fx.commit("base");
    fx.git(&["switch", "-q", "-c", "feature"]);
    fx.write("f.txt", "two\n");
    let f1 = fx.commit("f1");
    fx.git(&["switch", "-q", "main"]);
    fx.write("f.txt", "three\n");
    fx.commit("m1");
    fx.git(&["switch", "-q", "feature"]);

    let feature_before = fx.git(&["rev-parse", "feature"]).trim().to_string();
    let main_before = fx.git(&["rev-parse", "main"]).trim().to_string();

    let (outcome, _ctx) = restack_call(&fx, None, None, NOW);
    let report = match outcome {
        RestackOutcome::Held(r) => r,
        other => panic!("editing the same line on both sides must hold, got {other:?}"),
    };

    assert_eq!(report.branch, "feature");
    assert_eq!(
        report.at,
        At::Commit {
            id: f1.clone(),
            subject: "f1".into()
        },
        "the report names the commit the replay stopped on"
    );
    assert_eq!(report.paths, vec!["f.txt".to_string()]);
    assert_eq!(fx.git(&["rev-parse", "feature"]).trim(), feature_before);
    assert_eq!(fx.git(&["rev-parse", "main"]).trim(), main_before);
    // `begin_verb` captures before every verb (legitimately writing objects
    // of its own), so the log tip moving proves nothing; what must be
    // there is the hold's own operation.
    let record = tip_record(&fx.repo());
    assert_eq!(record.verb, "hold");
    assert!(record.held.is_some_and(|t| t.new.is_some()));
}

#[test]
fn restack_onto_records_the_parent_and_undo_takes_it_back() {
    let fx = Fixture::new();
    ident(&fx);
    let (_c0, _c1, _f1, _f2, f3, _m2) = stack(&fx);

    fx.git(&["switch", "-q", "main"]);
    fx.git(&["switch", "-q", "-c", "other"]);
    fx.write("other.txt", "other\n");
    fx.commit("other1");
    fx.git(&["switch", "-q", "feature"]);

    let parent_before = ff_core::branchmeta::read(&fx.repo(), "feature")
        .unwrap()
        .parent;
    assert_eq!(parent_before, None);

    let (outcome, _ctx) = restack_call(&fx, Some("feature"), Some("other"), NOW);
    let report = match outcome {
        RestackOutcome::Restacked(r) => r,
        other => panic!("the restack must land, got {other:?}"),
    };
    assert!(report.reaimed);
    assert_eq!(
        ff_core::branchmeta::read(&fx.repo(), "feature")
            .unwrap()
            .parent,
        Some("other".to_string())
    );

    let repo = fx.repo();
    let opts = ff_core::RewindOptions {
        force: false,
        now: Some(NOW + 100),
        argv: vec!["ff".into(), "undo".into()],
    };
    ff_core::undo(&repo, &opts, &prov()).unwrap();

    assert_eq!(
        ff_core::branchmeta::read(&fx.repo(), "feature")
            .unwrap()
            .parent,
        parent_before,
        "undo must take the re-aim back"
    );
    assert_eq!(fx.git(&["rev-parse", "feature"]).trim(), f3);
}

#[test]
fn restack_onto_itself_is_a_usage_error() {
    let fx = Fixture::new();
    ident(&fx);
    stack(&fx);

    let repo = fx.repo();
    let err = ff_core::restack::restack(
        &repo,
        Some("feature".into()),
        Some("feature".into()),
        &prov(),
        Some(NOW),
        vec!["ff".into(), "restack".into()],
    )
    .expect_err("restacking a branch onto itself must refuse");

    assert_eq!(err.id(), "usage/restack-onto-self", "{err}");
}

#[test]
fn restack_without_a_base_refuses() {
    let fx = Fixture::new();
    ident(&fx);
    fx.write("root.txt", "root\n");
    fx.commit("root");

    let repo = fx.repo();
    let err = ff_core::restack::restack(
        &repo,
        None,
        None,
        &prov(),
        Some(NOW),
        vec!["ff".into(), "restack".into()],
    )
    .expect_err("standing on trunk with no recorded parent has nothing to restack onto");

    assert_eq!(err.id(), "restack/no-base", "{err}");
}

#[test]
fn onto_a_full_local_ref_matches_the_short_name() {
    let fx = Fixture::new();
    ident(&fx);
    stack(&fx);

    // A third local branch ahead of main, the base `--onto` will name in
    // full.
    fx.git(&["switch", "-q", "main"]);
    fx.git(&["switch", "-q", "-c", "release"]);
    fx.write("rel.txt", "rel\n");
    fx.commit("r1");
    fx.git(&["switch", "-q", "feature"]);

    let (outcome, _ctx) = restack_call(&fx, Some("feature"), Some("refs/heads/release"), NOW);
    let report = match outcome {
        RestackOutcome::Restacked(r) => r,
        other => panic!("a restack onto a full local ref must land, got {other:?}"),
    };
    // The display name, not the ref: a full local ref is still a local
    // branch and still re-aims.
    assert_eq!(report.base, "release");
    assert!(report.reaimed);
    assert_eq!(
        ff_core::branchmeta::read(&fx.repo(), "feature")
            .unwrap()
            .parent,
        Some("release".into())
    );
}

/// The landmine's setup: `feature` forks `main`'s first commit with one
/// commit of its own, and a tracking ref `origin/feature` points at `main`'s
/// tip — the shape `ff sync`'s remote axis will replay onto.
fn tracking_stack(fx: &Fixture) {
    fx.write("f.txt", "one\n");
    let c0 = fx.commit("root");
    fx.git(&["switch", "-q", "-c", "feature", &c0]);
    fx.write("a.txt", "a\n");
    fx.commit("f1");
    fx.git(&["switch", "-q", "main"]);
    fx.write("m.txt", "m\n");
    let c1 = fx.commit("m");
    // A fetch would create the tracking ref this way.
    fx.git(&["update-ref", "refs/remotes/origin/feature", &c1]);
    fx.git(&["switch", "-q", "feature"]);
}

#[test]
fn onto_a_tracking_ref_records_no_parent() {
    let fx = Fixture::new();
    ident(&fx);
    tracking_stack(&fx);

    let (outcome, _ctx) = restack_call(
        &fx,
        Some("feature"),
        Some("refs/remotes/origin/feature"),
        NOW,
    );
    let report = match outcome {
        RestackOutcome::Restacked(r) => r,
        other => panic!("a restack onto a tracking ref must land, got {other:?}"),
    };
    assert_eq!(report.base, "origin/feature");
    // A base that is not a local branch is never recorded as a parent.
    assert!(!report.reaimed);
    assert_eq!(report.previous_parent, None);
    assert_eq!(
        ff_core::branchmeta::read(&fx.repo(), "feature")
            .unwrap()
            .parent,
        None
    );
    assert_eq!(report.replayed, 1);
}

#[test]
fn a_tracking_ref_base_writes_no_parent_transition() {
    let fx = Fixture::new();
    ident(&fx);
    tracking_stack(&fx);

    let (outcome, _ctx) = restack_call(
        &fx,
        Some("feature"),
        Some("refs/remotes/origin/feature"),
        NOW,
    );
    assert!(
        matches!(outcome, RestackOutcome::Restacked(_)),
        "a restack onto a tracking ref must land, got {outcome:?}"
    );

    let record = tip_record(&fx.repo());
    assert_eq!(record.verb, "restack");
    assert!(record.parent.is_none());
    assert!(
        record.summary.contains("onto origin/feature"),
        "{:?}",
        record.summary
    );
    assert!(
        !record.summary.contains("refs/remotes"),
        "{:?}",
        record.summary
    );
}

#[test]
fn a_hold_onto_a_tracking_ref_records_the_full_ref_and_replans() {
    let fx = Fixture::new();
    ident(&fx);
    // Both sides edit the same line of the same file, so the replay holds.
    fx.write("f.txt", "one\n");
    let c0 = fx.commit("root");
    fx.git(&["switch", "-q", "-c", "feature", &c0]);
    fx.write("f.txt", "two\n");
    fx.commit("f1");
    fx.git(&["switch", "-q", "main"]);
    fx.write("f.txt", "three\n");
    let c1 = fx.commit("m");
    fx.git(&["update-ref", "refs/remotes/origin/feature", &c1]);
    fx.git(&["switch", "-q", "feature"]);

    let (outcome, _ctx) = restack_call(
        &fx,
        Some("feature"),
        Some("refs/remotes/origin/feature"),
        NOW,
    );
    let _report = match outcome {
        RestackOutcome::Held(r) => r,
        other => panic!("editing the same line on both sides must hold, got {other:?}"),
    };

    let repo = fx.repo();
    let held = ff_core::held::of(&repo, "feature").unwrap().unwrap();
    assert_eq!(
        held.intent,
        ff_core::held::Intent::Restack {
            branch: "feature".into(),
            onto: "refs/remotes/origin/feature".into()
        }
    );
    // The hold can still be re-asked: the full ref resolves at replan time.
    ff_core::held::replan(&repo, &held).unwrap();
}

#[test]
fn a_hold_that_recorded_a_bare_name_still_replans() {
    let fx = Fixture::new();
    ident(&fx);
    fx.write("root.txt", "root\n");
    fx.commit("root");
    fx.git(&["switch", "-q", "-c", "feature"]);
    fx.write("a.txt", "a\n");
    fx.commit("f1");

    let h = ff_core::held::Held {
        intent: ff_core::held::Intent::Restack {
            branch: "feature".into(),
            onto: "main".into(),
        },
        at: At::Commit {
            id: "a".repeat(40),
            subject: "f1".into(),
        },
        paths: vec![],
        time: NOW,
    };
    ff_core::held::set(&fx.repo(), "feature", Some(h.clone())).unwrap();
    ff_core::held::replan(&fx.repo(), &h).unwrap();
}

#[test]
fn onto_a_ref_that_is_not_there_is_not_found() {
    let fx = Fixture::new();
    ident(&fx);
    fx.write("root.txt", "root\n");
    fx.commit("root");
    fx.git(&["switch", "-q", "-c", "feature"]);

    let repo = fx.repo();
    let err = ff_core::restack::restack(
        &repo,
        Some("feature".into()),
        Some("refs/remotes/origin/nope".into()),
        &prov(),
        Some(NOW),
        vec!["ff".into(), "restack".into()],
    )
    .expect_err("a ref that is not there must refuse");

    assert_eq!(err.id(), "branch/not-found", "{err}");
}

/// A trunk that is remote-tracking only — `origin/HEAD` with no matching
/// local branch — is still restackable, and still displays as `main`.
///
/// Both halves are the point. `futures::base_for` measures against the ref
/// trunk resolution names, so a replay that quietly substituted
/// `refs/heads/main` would answer a different question than `ff status`
/// asked — and would simply fail when no local branch of that name exists.
/// But ref syntax must not reach the screen: the report and the operation
/// summary say `main`, not `origin/main`.
#[test]
fn a_remote_only_trunk_restacks_and_still_displays_short() {
    let fx = Fixture::new();
    ident(&fx);
    fx.write("root.txt", "root\n");
    let c0 = fx.commit("root");
    fx.git(&["switch", "-q", "-c", "feature"]);
    fx.write("f.txt", "f\n");
    fx.commit("f1");

    // Trunk lives only on the remote: origin/main is two commits past the
    // fork, and there is no local `main` at all.
    fx.git(&["switch", "-q", "--detach", &c0]);
    fx.write("m.txt", "m\n");
    let m1 = fx.commit("m1");
    fx.git(&["update-ref", "refs/remotes/origin/main", &m1]);
    fx.git(&[
        "symbolic-ref",
        "refs/remotes/origin/HEAD",
        "refs/remotes/origin/main",
    ]);
    fx.git(&["branch", "-D", "main"]);
    fx.git(&["switch", "-q", "feature"]);

    let (outcome, _ctx) = restack_call(&fx, None, None, NOW);
    let report = match outcome {
        RestackOutcome::Restacked(report) => report,
        other => panic!("a remote-only trunk is still a base, got {other:?}"),
    };
    assert_eq!(
        report.base, "main",
        "the base displays as a person says it, never as ref syntax"
    );
    assert_eq!(report.replayed, 1);
    assert!(
        !report.reaimed,
        "no parent is recorded for a base that is not local"
    );
    assert_eq!(
        ff_core::branchmeta::read(&fx.repo(), "feature")
            .unwrap()
            .parent,
        None
    );

    let repo = fx.repo();
    let log = ff_core::ops::OpLog::open(&repo).unwrap();
    let op = log.get(log.tip().unwrap().unwrap()).unwrap();
    let record = op
        .record()
        .unwrap()
        .cloned()
        .expect("a verb op has a record");
    assert_eq!(record.summary, "restack feature onto main");
    assert!(record.parent.is_none(), "no parent transition rides the op");
}
