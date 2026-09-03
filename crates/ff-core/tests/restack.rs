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

/// `restack_call` for the refusal path: the error, panicking with the
/// outcome if the restack unexpectedly lands.
fn restack_err(fx: &Fixture, branch: Option<&str>, onto: Option<&str>, now: i64) -> ff_core::Error {
    let repo = fx.repo();
    match ff_core::restack::restack(
        &repo,
        branch.map(String::from),
        onto.map(String::from),
        &prov(),
        Some(now),
        vec!["ff".into(), "restack".into()],
    ) {
        Ok(outcome) => panic!("the restack must refuse, but it landed: {outcome:?}"),
        Err(err) => err,
    }
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
fn restack_leaves_an_inner_branch_diverged() {
    let fx = Fixture::new();
    ident(&fx);
    let (_c0, _c1, _f1, f2, _f3, _m2) = stack(&fx);

    let (outcome, _ctx) = restack_call(&fx, None, None, NOW);
    let report = match outcome {
        RestackOutcome::Restacked(r) => r,
        other => panic!("the restack must land, got {other:?}"),
    };

    assert!(
        !report.moved.contains(&"mid".to_string()),
        "mid is not the branch restack was asked to move, so it must not be carried: {:?}",
        report.moved
    );
    assert!(
        report.diverged.contains(&"mid".to_string()),
        "mid sits inside the rewritten range and was left where it stood: {:?}",
        report.diverged
    );
    assert_eq!(
        fx.git(&["rev-parse", "mid"]).trim(),
        f2,
        "mid's ref must not move: it was left exactly where it stood"
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
    // `mid` is not the branch restack was asked to move: it is left exactly
    // where it stood, reported as diverged rather than carried.
    assert_eq!(
        fx.git(&["rev-parse", "mid"]).trim(),
        mid_before,
        "mid must not move"
    );
    assert!(report.diverged.contains(&"mid".to_string()));
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

    let ops_tip_before = fx
        .git(&["rev-parse", "refs/fufu/wt/main/ops"])
        .trim()
        .to_string();
    let (outcome, _ctx) = restack_call(&fx, None, None, NOW + 100);
    match outcome {
        RestackOutcome::NothingToRestack { branch, base } => {
            assert_eq!(branch, "feature");
            assert_eq!(base, "main");
        }
        other => panic!("the second restack must be a no-op, got {other:?}"),
    }
    let ops_tip_after = fx
        .git(&["rev-parse", "refs/fufu/wt/main/ops"])
        .trim()
        .to_string();
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

/// The setup: `feature` forks `main`'s first commit with one commit of its
/// own, and a tracking ref `origin/feature` points at `main`'s tip. No
/// upstream is configured, so this is not the branch's own copy — as far as
/// fufu can tell, someone else's branch.
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
fn onto_someone_elses_tracking_ref_records_it_as_the_parent() {
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
    // No upstream config: as far as fufu can tell this is someone else's
    // branch, so `--onto` records it as the parent.
    assert!(report.reaimed);
    assert_eq!(report.previous_parent, None);
    assert_eq!(
        ff_core::branchmeta::read(&fx.repo(), "feature")
            .unwrap()
            .parent,
        Some("origin/feature".into())
    );
    assert_eq!(report.replayed, 1);
}

#[test]
fn a_tracking_ref_base_records_the_parent_transition() {
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
    let parent = record
        .parent
        .expect("a tracking-ref base records the parent transition");
    assert_eq!(parent.branch, "feature");
    assert_eq!(parent.old, None);
    assert_eq!(parent.new.as_deref(), Some("origin/feature"));
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

/// `tracking_stack`'s shape, but the remote holds `main`: a branch that
/// exists only as a tracking ref.
fn remote_main_stack(fx: &Fixture) {
    fx.write("f.txt", "one\n");
    let c0 = fx.commit("root");
    fx.git(&["switch", "-q", "-c", "feature", &c0]);
    fx.write("a.txt", "a\n");
    fx.commit("f1");
    fx.git(&["switch", "-q", "main"]);
    fx.write("m.txt", "m\n");
    let c1 = fx.commit("m");
    // A fetch would create the tracking ref this way.
    fx.git(&["update-ref", "refs/remotes/origin/main", &c1]);
    fx.git(&["switch", "-q", "feature"]);
}

#[test]
fn onto_a_remote_branch_by_its_short_name_replays_and_records_it() {
    let fx = Fixture::new();
    ident(&fx);
    remote_main_stack(&fx);

    let (outcome, _ctx) = restack_call(&fx, Some("feature"), Some("origin/main"), NOW);
    let report = match outcome {
        RestackOutcome::Restacked(r) => r,
        other => panic!("a restack onto a remote branch must land, got {other:?}"),
    };
    assert_eq!(report.base, "origin/main");
    assert!(report.reaimed);
    assert_eq!(report.replayed, 1);
    assert_eq!(
        ff_core::branchmeta::read(&fx.repo(), "feature")
            .unwrap()
            .parent,
        Some("origin/main".into())
    );

    // Move the remote forward one more commit: the bare verb must resolve
    // the recorded parent through `base_for` and land on the new tip.
    fx.git(&["switch", "-q", "main"]);
    fx.write("m2.txt", "m2\n");
    let c2 = fx.commit("m2");
    fx.git(&["update-ref", "refs/remotes/origin/main", &c2]);
    fx.git(&["switch", "-q", "feature"]);

    let (outcome, _ctx) = restack_call(&fx, Some("feature"), None, NOW);
    let report = match outcome {
        RestackOutcome::Restacked(r) => r,
        other => panic!("the bare verb must replay onto the recorded parent, got {other:?}"),
    };
    assert_eq!(report.base, "origin/main");
    assert!(
        fx.try_git(&["merge-base", "--is-ancestor", &c2, "feature"])
            .status
            .success(),
        "feature must sit on the new origin/main tip"
    );
}

#[test]
fn both_spellings_of_a_remote_base_do_the_same_thing() {
    let short = Fixture::new();
    ident(&short);
    remote_main_stack(&short);
    let (outcome, _ctx) = restack_call(&short, Some("feature"), Some("origin/main"), NOW);
    let a = match outcome {
        RestackOutcome::Restacked(r) => r,
        other => panic!("the short spelling must land, got {other:?}"),
    };

    let long = Fixture::new();
    ident(&long);
    remote_main_stack(&long);
    let (outcome, _ctx) = restack_call(
        &long,
        Some("feature"),
        Some("refs/remotes/origin/main"),
        NOW,
    );
    let b = match outcome {
        RestackOutcome::Restacked(r) => r,
        other => panic!("the long spelling must land, got {other:?}"),
    };

    assert_eq!(a.base, b.base);
    assert_eq!(a.reaimed, b.reaimed);
    assert_eq!(a.replayed, b.replayed);
    // Both arms name it the way a person types it.
    assert_eq!(a.base, "origin/main");
    for fx in [&short, &long] {
        assert_eq!(
            ff_core::branchmeta::read(&fx.repo(), "feature")
                .unwrap()
                .parent,
            Some("origin/main".into())
        );
    }
}

#[test]
fn onto_this_branchs_own_shared_copy_is_refused() {
    let fx = Fixture::new();
    ident(&fx);
    fx.write("f.txt", "one\n");
    let c0 = fx.commit("root");
    fx.git(&["switch", "-q", "-c", "feature", &c0]);
    fx.write("a.txt", "a\n");
    let f1 = fx.commit("f1");
    // The branch's own shared copy, with the upstream config that says so.
    fx.git(&["update-ref", "refs/remotes/origin/feature", &f1]);
    fx.git(&["config", "remote.origin.url", "file:///nonexistent"]);
    fx.git(&[
        "config",
        "remote.origin.fetch",
        "+refs/heads/*:refs/remotes/origin/*",
    ]);
    fx.git(&["config", "branch.feature.remote", "origin"]);
    fx.git(&["config", "branch.feature.merge", "refs/heads/feature"]);

    let err = restack_err(&fx, Some("feature"), Some("origin/feature"), NOW);
    assert_eq!(err.id(), "restack/own-remote", "{err}");

    // The long spelling of the same ref is refused the same way: the two
    // spellings of a base must mean the same thing.
    let err = restack_err(
        &fx,
        Some("feature"),
        Some("refs/remotes/origin/feature"),
        NOW,
    );
    assert_eq!(err.id(), "restack/own-remote", "{err}");

    // A refusal writes nothing.
    assert_eq!(
        ff_core::branchmeta::read(&fx.repo(), "feature")
            .unwrap()
            .parent,
        None
    );
}

#[test]
fn a_local_prefix_still_resolves_and_does_not_reach_across_namespaces() {
    let fx = Fixture::new();
    ident(&fx);
    fx.write("root.txt", "root\n");
    let c0 = fx.commit("root");
    fx.git(&["switch", "-q", "-c", "feature", &c0]);
    fx.write("a.txt", "a\n");
    fx.commit("f1");
    fx.git(&["switch", "-q", "main"]);
    fx.write("m.txt", "m\n");
    let c1 = fx.commit("m");
    fx.git(&["switch", "-q", "-c", "release", &c1]);
    // A tracking ref with no local branch of the name `or` would reach.
    fx.git(&["update-ref", "refs/remotes/origin/oak", &c1]);
    fx.git(&["switch", "-q", "feature"]);

    let (outcome, _ctx) = restack_call(&fx, Some("feature"), Some("ma"), NOW);
    let report = match outcome {
        RestackOutcome::Restacked(r) => r,
        other => panic!("a restack onto a local prefix must land, got {other:?}"),
    };
    assert_eq!(report.base, "main");

    // A local not-found must not fall through to `origin/oak`.
    let err = restack_err(&fx, Some("feature"), Some("or"), NOW);
    assert_eq!(err.id(), "branch/not-found", "{err}");
}

#[test]
fn onto_a_remote_head_is_not_found_not_a_crash() {
    let fx = Fixture::new();
    ident(&fx);
    fx.write("m.txt", "m\n");
    let c0 = fx.commit("root");
    // The shape every clone leaves behind: one remote branch and its
    // symbolic HEAD.
    fx.git(&["update-ref", "refs/remotes/origin/main", &c0]);
    fx.git(&[
        "symbolic-ref",
        "refs/remotes/origin/HEAD",
        "refs/remotes/origin/main",
    ]);

    let err = restack_err(&fx, Some("main"), Some("origin/HEAD"), NOW);
    assert_eq!(err.id(), "branch/not-found", "{err}");
    // The not-found, not the symbolic-ref error a direct lookup would raise.
    assert!(!err.to_string().contains("symbolic"), "{err}");
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

// ---- The cascade: the branches stacked above the one that moved ----

/// Record `parent` as the branch `branch` sits on, the way
/// `ff start <parent> -b <branch>` does.
fn stacked_on(fx: &Fixture, branch: &str, parent: &str) {
    let repo = fx.repo();
    let mut meta = ff_core::branchmeta::read(&repo, branch).unwrap();
    meta.parent = Some(parent.to_string());
    ff_core::branchmeta::write(&repo, branch, &meta).unwrap();
}

/// Two branches stacked above `feature`, one commit each, each recording
/// the branch beneath it:
///
/// f3 ─ x1        (child, on feature)
///       └─ y1    (grandchild, on child)
///
/// Distinct files, so the replays are clean. Leaves the fixture standing on
/// `feature`.
fn stack_above(fx: &Fixture) -> (String, String) {
    fx.git(&["switch", "-q", "-c", "child"]);
    fx.write("x.txt", "x\n");
    let x1 = fx.commit("x1");
    stacked_on(fx, "child", "feature");
    fx.git(&["switch", "-q", "-c", "grandchild"]);
    fx.write("y.txt", "y\n");
    let y1 = fx.commit("y1");
    stacked_on(fx, "grandchild", "child");
    fx.git(&["switch", "-q", "feature"]);
    (x1, y1)
}

fn rev(fx: &Fixture, name: &str) -> String {
    fx.git(&["rev-parse", name]).trim().to_string()
}

fn is_ancestor(fx: &Fixture, ancestor: &str, of: &str) -> bool {
    fx.try_git(&["merge-base", "--is-ancestor", ancestor, of])
        .status
        .success()
}

fn undo(fx: &Fixture) {
    let repo = fx.repo();
    let opts = ff_core::RewindOptions {
        force: false,
        now: Some(NOW + 100),
        argv: vec!["ff".into(), "undo".into()],
    };
    ff_core::undo(&repo, &opts, &prov()).unwrap();
}

fn landed(outcome: RestackOutcome) -> ff_core::RestackReport {
    match outcome {
        RestackOutcome::Restacked(r) => *r,
        other => panic!("the restack must land, got {other:?}"),
    }
}

#[test]
fn restack_cascades_onto_the_branches_above() {
    let fx = Fixture::new();
    ident(&fx);
    let (_c0, _c1, _f1, _f2, _f3, m2) = stack(&fx);
    let (x1, y1) = stack_above(&fx);

    let (outcome, _ctx) = restack_call(&fx, None, None, NOW);
    let report = landed(outcome);

    let moved: Vec<&str> = report
        .cascade
        .moved
        .iter()
        .map(|m| m.branch.as_str())
        .collect();
    assert_eq!(moved, ["child", "grandchild"], "parent before child");
    assert_eq!(report.cascade.moved[0].base, "feature");
    assert_eq!(report.cascade.moved[1].base, "child");
    assert_eq!(report.cascade.moved[0].replayed, 1);
    assert_eq!(report.cascade.moved[1].replayed, 1);
    assert!(report.cascade.held.is_empty(), "{:?}", report.cascade.held);
    assert!(
        report.cascade.skipped.is_empty(),
        "{:?}",
        report.cascade.skipped
    );

    let feature = rev(&fx, "feature");
    let child = rev(&fx, "child");
    let grandchild = rev(&fx, "grandchild");
    assert_ne!(child, x1, "child moved");
    assert_ne!(grandchild, y1, "grandchild moved");
    assert_eq!(report.cascade.moved[0].old_tip, x1);
    assert_eq!(report.cascade.moved[0].new_tip, child);
    assert_eq!(report.cascade.moved[1].new_tip, grandchild);
    assert!(
        is_ancestor(&fx, &m2, &feature),
        "feature sits on the moved main"
    );
    assert!(
        is_ancestor(&fx, &feature, &child),
        "child sits on the moved feature"
    );
    assert!(
        is_ancestor(&fx, &child, &grandchild),
        "grandchild sits on the moved child"
    );

    // `mid` sits inside feature's range with nothing of its own: it did
    // not record a parent, so it is not a child, and it stays diverged.
    assert!(
        report.diverged.contains(&"mid".to_string()),
        "{:?}",
        report.diverged
    );

    // One operation carries all three moves and every rewrite.
    let record = tip_record(&fx.repo());
    assert_eq!(record.verb, "restack");
    assert!(
        record.summary.contains("and 2 above it"),
        "{}",
        record.summary
    );
    let names: Vec<&str> = record.refs.iter().map(|t| t.name.as_str()).collect();
    assert!(names.contains(&"refs/heads/feature"), "{names:?}");
    assert!(names.contains(&"refs/heads/child"), "{names:?}");
    assert!(names.contains(&"refs/heads/grandchild"), "{names:?}");
    assert_eq!(
        record.rewrites.len(),
        5,
        "three of feature's, one each above"
    );
}

#[test]
fn one_undo_takes_the_whole_cascade_back() {
    let fx = Fixture::new();
    ident(&fx);
    let (_c0, _c1, _f1, _f2, f3, _m2) = stack(&fx);
    let (x1, y1) = stack_above(&fx);

    let (outcome, _ctx) = restack_call(&fx, None, None, NOW);
    let report = landed(outcome);
    assert_eq!(report.cascade.moved.len(), 2);

    undo(&fx);
    assert_eq!(rev(&fx, "feature"), f3, "undo puts feature back");
    assert_eq!(rev(&fx, "child"), x1, "undo puts child back");
    assert_eq!(rev(&fx, "grandchild"), y1, "undo puts grandchild back");
}

#[test]
fn a_conflicting_replay_holds_that_branch_and_leaves_its_subtree_alone() {
    let fx = Fixture::new();
    ident(&fx);
    let (_c0, _c1, _f1, _f2, f3, _m2) = stack(&fx);
    // main's m2 added d.txt; child adds its own d.txt, so its replay onto
    // the moved feature is an add/add conflict.
    fx.git(&["switch", "-q", "-c", "child"]);
    fx.write("d.txt", "child\n");
    let x1 = fx.commit("x1");
    stacked_on(&fx, "child", "feature");
    fx.git(&["switch", "-q", "-c", "grandchild"]);
    fx.write("y.txt", "y\n");
    let y1 = fx.commit("y1");
    stacked_on(&fx, "grandchild", "child");
    fx.git(&["switch", "-q", "feature"]);

    let (outcome, _ctx) = restack_call(&fx, None, None, NOW);
    let report = landed(outcome);

    assert!(
        report.cascade.moved.is_empty(),
        "{:?}",
        report.cascade.moved
    );
    assert_eq!(report.cascade.held.len(), 1);
    let hold = &report.cascade.held[0];
    assert_eq!(hold.branch, "child");
    assert_eq!(hold.base, "feature");
    assert_eq!(hold.left_alone, vec!["grandchild".to_string()]);
    assert_eq!(hold.report.paths, vec!["d.txt".to_string()]);
    assert_eq!(hold.report.of, 1);
    match &hold.report.at {
        At::Commit { id, subject } => {
            assert_eq!(id, &x1);
            assert_eq!(subject, "x1");
        }
        other => panic!("the hold is at the conflicting commit, got {other:?}"),
    }

    assert_ne!(rev(&fx, "feature"), f3, "feature itself moved");
    assert_eq!(rev(&fx, "child"), x1, "the held branch stays put");
    assert_eq!(rev(&fx, "grandchild"), y1, "its subtree stays put");

    let repo = fx.repo();
    let held = ff_core::held::of(&repo, "child")
        .unwrap()
        .expect("child holds");
    match held.intent {
        ff_core::held::Intent::Restack { branch, onto } => {
            assert_eq!(branch, "child");
            assert_eq!(onto, "refs/heads/feature");
        }
        other => panic!("a restack hold, got {other:?}"),
    }
    let record = tip_record(&repo);
    assert_eq!(
        record.cascade_held.len(),
        1,
        "the hold rides the restack's op"
    );
    assert!(record.held.is_none());

    // One undo clears the hold with the restack.
    undo(&fx);
    assert_eq!(rev(&fx, "feature"), f3);
    assert!(ff_core::held::of(&fx.repo(), "child").unwrap().is_none());
}

#[test]
fn a_branch_with_nothing_of_its_own_stays_put() {
    let fx = Fixture::new();
    ident(&fx);
    let (_c0, _c1, _f1, f2, f3, _m2) = stack(&fx);
    // `child` sits exactly at feature's tip and `mid` partway up it; both
    // record feature as their base and neither holds a commit of its own.
    fx.git(&["branch", "child", "feature"]);
    stacked_on(&fx, "child", "feature");
    stacked_on(&fx, "mid", "feature");

    let (outcome, _ctx) = restack_call(&fx, None, None, NOW);
    let report = landed(outcome);

    assert!(
        report.cascade.moved.is_empty(),
        "{:?}",
        report.cascade.moved
    );
    assert_eq!(
        report.cascade.unchanged,
        vec!["child".to_string(), "mid".to_string()]
    );
    assert_eq!(
        rev(&fx, "child"),
        f3,
        "not replayed to nothing and landed on the tip"
    );
    assert_eq!(rev(&fx, "mid"), f2);
    assert!(
        report.diverged.contains(&"child".to_string()),
        "{:?}",
        report.diverged
    );
    assert!(
        report.diverged.contains(&"mid".to_string()),
        "{:?}",
        report.diverged
    );
}

#[test]
fn a_branch_held_by_another_worktree_is_skipped_and_named() {
    let fx = Fixture::new();
    ident(&fx);
    let (_c0, _c1, _f1, _f2, f3, _m2) = stack(&fx);
    let (x1, y1) = stack_above(&fx);
    let wt = fx.root().join("linked-wt");
    fx.git(&["worktree", "add", "-q", wt.to_str().unwrap(), "child"]);

    let (outcome, _ctx) = restack_call(&fx, None, None, NOW);
    let report = landed(outcome);

    assert!(
        report.cascade.moved.is_empty(),
        "{:?}",
        report.cascade.moved
    );
    assert_eq!(report.cascade.skipped.len(), 1);
    let skip = &report.cascade.skipped[0];
    assert_eq!(skip.branch, "child");
    assert_eq!(skip.left_alone, vec!["grandchild".to_string()]);
    match &skip.reason {
        ff_core::SkipReason::Worktree { path } => {
            assert!(path.contains("linked-wt"), "names the worktree: {path}");
        }
        other => panic!("skipped for the worktree, got {other:?}"),
    }
    assert_ne!(rev(&fx, "feature"), f3);
    assert_eq!(rev(&fx, "child"), x1);
    assert_eq!(rev(&fx, "grandchild"), y1);
}

#[test]
fn a_branch_already_holding_a_rewrite_is_skipped() {
    let fx = Fixture::new();
    ident(&fx);
    let (_c0, _c1, _f1, _f2, _f3, _m2) = stack(&fx);
    let (x1, y1) = stack_above(&fx);
    let standing = ff_core::held::Held {
        intent: ff_core::held::Intent::Restack {
            branch: "child".into(),
            onto: "refs/heads/feature".into(),
        },
        at: At::OpenChange,
        paths: vec!["x.txt".into()],
        time: NOW - 10,
    };
    ff_core::held::set(&fx.repo(), "child", Some(standing.clone())).unwrap();

    let (outcome, _ctx) = restack_call(&fx, None, None, NOW);
    let report = landed(outcome);

    assert_eq!(report.cascade.skipped.len(), 1);
    assert_eq!(report.cascade.skipped[0].branch, "child");
    assert_eq!(
        report.cascade.skipped[0].reason,
        ff_core::SkipReason::AlreadyHeld
    );
    assert_eq!(
        report.cascade.skipped[0].left_alone,
        vec!["grandchild".to_string()]
    );
    assert_eq!(rev(&fx, "child"), x1);
    assert_eq!(rev(&fx, "grandchild"), y1);
    assert_eq!(
        ff_core::held::of(&fx.repo(), "child").unwrap(),
        Some(standing),
        "the standing hold is left exactly as it was"
    );
}

#[test]
fn a_parent_loop_ends_the_cascade() {
    let fx = Fixture::new();
    ident(&fx);
    let (_c0, _c1, _f1, _f2, _f3, _m2) = stack(&fx);
    fx.git(&["switch", "-q", "-c", "child"]);
    fx.write("x.txt", "x\n");
    let x1 = fx.commit("x1");
    stacked_on(&fx, "child", "feature");
    // Aimed in a loop: feature says it sits on child, child on feature.
    stacked_on(&fx, "feature", "child");
    fx.git(&["switch", "-q", "feature"]);

    let (outcome, _ctx) = restack_call(&fx, Some("feature"), Some("main"), NOW);
    let report = landed(outcome);

    assert!(report.reaimed);
    let moved: Vec<&str> = report
        .cascade
        .moved
        .iter()
        .map(|m| m.branch.as_str())
        .collect();
    assert_eq!(
        moved,
        ["child"],
        "child follows once, and feature is not visited again"
    );
    assert_ne!(rev(&fx, "child"), x1);
    assert_eq!(
        ff_core::branchmeta::read(&fx.repo(), "feature")
            .unwrap()
            .parent,
        Some("main".to_string())
    );
}

#[test]
fn head_on_a_branch_above_carries_the_open_change() {
    let fx = Fixture::new();
    ident(&fx);
    let (_c0, _c1, _f1, _f2, _f3, _m2) = stack(&fx);
    let (x1, _y1) = stack_above(&fx);
    fx.git(&["switch", "-q", "child"]);
    fx.write("z.txt", "z\n");

    let (outcome, _ctx) = restack_call(&fx, Some("feature"), None, NOW);
    let report = landed(outcome);

    assert_eq!(report.cascade.moved[0].branch, "child");
    assert_ne!(rev(&fx, "child"), x1);
    assert_eq!(rev(&fx, "HEAD"), rev(&fx, "child"), "HEAD followed child");
    assert!(report.files > 0, "main's file arrived in the worktree");
    assert!(report.still_open, "the open change is still open");
    let files = worktree_files(&fx);
    assert!(
        files.contains(&("d.txt".to_string(), b"d\n".to_vec())),
        "{files:?}"
    );
    assert!(
        files.contains(&("z.txt".to_string(), b"z\n".to_vec())),
        "{files:?}"
    );
    assert_eq!(fx.git(&["status", "--porcelain"]).trim(), "?? z.txt");
}

#[test]
fn an_open_change_above_that_conflicts_holds_that_branch() {
    let fx = Fixture::new();
    ident(&fx);
    let (_c0, _c1, _f1, _f2, f3, _m2) = stack(&fx);
    let (x1, y1) = stack_above(&fx);
    fx.git(&["switch", "-q", "child"]);
    // Uncommitted, and the same path main's m2 added.
    fx.write("d.txt", "mine\n");

    let (outcome, _ctx) = restack_call(&fx, Some("feature"), None, NOW);
    let report = landed(outcome);

    assert_ne!(rev(&fx, "feature"), f3, "feature itself moved");
    assert_eq!(report.cascade.held.len(), 1);
    let hold = &report.cascade.held[0];
    assert_eq!(hold.branch, "child");
    assert_eq!(hold.report.at, At::OpenChange);
    assert_eq!(hold.report.paths, vec!["d.txt".to_string()]);
    assert_eq!(hold.left_alone, vec!["grandchild".to_string()]);
    assert_eq!(rev(&fx, "child"), x1);
    assert_eq!(rev(&fx, "grandchild"), y1);
    assert_eq!(report.files, 0, "no file moved");
    let files = worktree_files(&fx);
    assert!(
        files.contains(&("d.txt".to_string(), b"mine\n".to_vec())),
        "{files:?}"
    );
}
