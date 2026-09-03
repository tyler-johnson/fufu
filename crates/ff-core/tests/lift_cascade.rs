//! `ff lift` and the branches stacked above the one it rewrote: the cascade
//! rides lift's own operation, a conflict above holds that branch and not
//! the lift, and a branch another worktree holds is skipped and named.

use ff_core::futures::At;
use ff_core::gix;
use ff_core::{LiftOutcome, Provenance};
use ff_testsupport::Fixture;

const NOW: i64 = 1_799_999_999;

/// `lift` reads the committer identity from the repo config, which the
/// fixture's hermetic env does not set; git itself gets its identity from
/// env vars, so this is only for the gix side.
fn ident(fx: &Fixture) {
    fx.set_config("user.name", "Fixture Committer");
    fx.set_config("user.email", "committer@fixture.test");
}

fn prov() -> Provenance {
    Provenance::new("pre", Some("ff lift".into()))
}

fn oid(hex: &str) -> gix::ObjectId {
    gix::ObjectId::from_hex(hex.trim().as_bytes()).unwrap()
}

fn rev(fx: &Fixture, name: &str) -> String {
    fx.git(&["rev-parse", name]).trim().to_string()
}

fn is_ancestor(fx: &Fixture, ancestor: &str, of: &str) -> bool {
    fx.try_git(&["merge-base", "--is-ancestor", ancestor, of])
        .status
        .success()
}

/// The paths a commit's tree holds, one per line.
fn tree_paths(fx: &Fixture, rev: &str) -> String {
    fx.git(&["ls-tree", "--name-only", "-r", rev])
}

/// Record `parent` as the branch `branch` sits on, the way
/// `ff start <parent> -b <branch>` does.
fn stacked_on(fx: &Fixture, branch: &str, parent: &str) {
    let repo = fx.repo();
    let mut meta = ff_core::branchmeta::read(&repo, branch).unwrap();
    meta.parent = Some(parent.to_string());
    ff_core::branchmeta::write(&repo, branch, &meta).unwrap();
}

/// The verb operations in the log — captures and notes excluded, so the
/// count says "one more thing the user asked for happened" and nothing
/// about the preamble's own writing.
fn verb_ops(fx: &Fixture) -> usize {
    let repo = fx.repo();
    let log = ff_core::ops::OpLog::open(&repo).unwrap();
    log.iter()
        .flatten()
        .filter(|op| op.kind() == ff_core::ops::OpKind::Op)
        .count()
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

fn undo(fx: &Fixture) {
    let repo = fx.repo();
    let opts = ff_core::RewindOptions {
        force: false,
        now: Some(NOW + 100),
        argv: vec!["ff".into(), "undo".into()],
    };
    ff_core::undo(&repo, &opts, &prov()).unwrap();
}

fn lift_call(fx: &Fixture, from: &str, paths: &[&str]) -> ff_core::LiftReport {
    let repo = fx.repo();
    let (outcome, _ctx) = ff_core::absorb::lift(
        &repo,
        Some(oid(from)),
        paths.iter().map(|p| p.to_string()).collect(),
        &prov(),
        Some(NOW),
        vec!["ff".into(), "lift".into()],
    )
    .unwrap();
    match outcome {
        LiftOutcome::Lifted(report) => report,
        other => panic!("the lift must land, got {other:?}"),
    }
}

/// The stack `main` ← `feat` ← `top`, `top` recording `feat` as the branch
/// it sits on:
///
/// base ─ c1 ─ c2        (feat: c1 edits a.txt and adds b.txt, c2 adds c.txt)
///              └─ x1    (top: `top_a` says what x1 does to a.txt)
///
/// `main` seeds a.txt so a lift of it out of c1 restores the seed rather
/// than deleting the file. Returns (c1, c2, x1) and leaves the fixture
/// standing on `feat`.
fn stack(fx: &Fixture, top_a: Option<&str>) -> (String, String, String) {
    fx.write("a.txt", "base\n");
    fx.commit("base");
    fx.git(&["switch", "-q", "-c", "feat"]);
    fx.write("a.txt", "one\n");
    fx.write("b.txt", "b\n");
    let c1 = fx.commit("c1");
    fx.write("c.txt", "c\n");
    let c2 = fx.commit("c2");
    fx.git(&["switch", "-q", "-c", "top"]);
    match top_a {
        Some(content) => fx.write("a.txt", content),
        None => fx.write("x.txt", "x\n"),
    }
    let x1 = fx.commit("x1");
    stacked_on(fx, "top", "feat");
    fx.git(&["switch", "-q", "feat"]);
    (c1, c2, x1)
}

#[test]
fn lift_replays_the_branch_stacked_above() {
    let fx = Fixture::new();
    ident(&fx);
    let (c1, c2, x1) = stack(&fx, None);

    let report = lift_call(&fx, &c1, &["a.txt"]);

    assert_eq!(report.branch, "feat");
    assert_eq!(report.cascade.moved.len(), 1, "{:?}", report.cascade);
    let moved = &report.cascade.moved[0];
    assert_eq!(moved.branch, "top");
    assert_eq!(moved.base, "feat");
    assert_eq!(moved.old_tip, x1);
    assert_eq!(moved.replayed, 1);
    assert!(report.cascade.held.is_empty(), "{:?}", report.cascade.held);
    assert!(
        report.cascade.skipped.is_empty(),
        "{:?}",
        report.cascade.skipped
    );

    assert_ne!(rev(&fx, "feat"), c2, "feat moved");
    assert_ne!(rev(&fx, "top"), x1, "top followed");
    assert_eq!(rev(&fx, "top"), moved.new_tip);
    assert!(
        is_ancestor(&fx, "feat", "top"),
        "top sits on feat's new tip"
    );
    // What the lift took out of c1 is out of top too: top's tree holds the
    // seed a.txt, its own x.txt, and nothing the lift removed.
    assert_eq!(
        fx.git(&["show", "top:a.txt"]),
        "base\n",
        "the lifted edit is gone from top"
    );
    let paths = tree_paths(&fx, "top");
    assert!(paths.contains("x.txt"), "{paths}");
    assert!(paths.contains("c.txt"), "{paths}");
    // And the lift moved no file: the edit is open in the worktree.
    assert_eq!(
        std::fs::read_to_string(fx.path().join("a.txt")).unwrap(),
        "one\n"
    );
}

#[test]
fn lift_cascade_rides_the_one_operation() {
    let fx = Fixture::new();
    ident(&fx);
    let (c1, c2, x1) = stack(&fx, None);
    let before = verb_ops(&fx);

    let report = lift_call(&fx, &c1, &["a.txt"]);
    assert_eq!(report.cascade.moved.len(), 1);
    assert_eq!(
        verb_ops(&fx),
        before + 1,
        "one operation for lift and its cascade"
    );

    let repo = fx.repo();
    let record = tip_record(&repo);
    assert!(
        record
            .refs
            .iter()
            .any(|t| t.name == "refs/heads/top" && t.old.as_deref() == Some(x1.as_str())),
        "top's move rides lift's record: {:?}",
        record.refs
    );
    assert!(
        record.summary.contains("and 1 above it"),
        "{}",
        record.summary
    );

    undo(&fx);
    assert_eq!(rev(&fx, "feat"), c2, "one undo puts feat back");
    assert_eq!(rev(&fx, "top"), x1, "and top");
    assert_eq!(
        fx.git(&["show", "feat:a.txt"]),
        "one\n",
        "the lifted edit is back in the commit"
    );
}

#[test]
fn a_conflicting_stacked_branch_holds_and_lift_still_lands() {
    let fx = Fixture::new();
    ident(&fx);
    // x1 rewrites the line c1 wrote; the lift puts the seed back in its
    // place, so replaying x1 onto the rewritten feat conflicts on it.
    let (c1, c2, x1) = stack(&fx, Some("two\n"));

    let report = lift_call(&fx, &c1, &["a.txt"]);

    assert!(
        report.cascade.moved.is_empty(),
        "{:?}",
        report.cascade.moved
    );
    assert_eq!(report.cascade.held.len(), 1, "{:?}", report.cascade);
    let hold = &report.cascade.held[0];
    assert_eq!(hold.branch, "top");
    assert_eq!(hold.base, "feat");
    assert!(hold.left_alone.is_empty());
    assert_eq!(hold.report.paths, vec!["a.txt".to_string()]);
    assert_eq!(hold.report.of, 1);
    match &hold.report.at {
        At::Commit { id, subject } => {
            assert_eq!(id, &x1);
            assert_eq!(subject, "x1");
        }
        other => panic!("the hold is at the conflicting commit, got {other:?}"),
    }

    assert_ne!(rev(&fx, "feat"), c2, "the lift itself landed");
    assert_eq!(rev(&fx, "top"), x1, "the held branch stays put");

    let repo = fx.repo();
    let held = ff_core::held::of(&repo, "top").unwrap().expect("top holds");
    match held.intent {
        ff_core::held::Intent::Restack { branch, onto } => {
            assert_eq!(branch, "top");
            assert_eq!(onto, "refs/heads/feat");
        }
        other => panic!("a restack hold, got {other:?}"),
    }
    assert!(
        ff_core::held::of(&repo, "feat").unwrap().is_none(),
        "the lift's own branch does not hold"
    );
    let record = tip_record(&repo);
    assert_eq!(record.cascade_held.len(), 1, "the hold rides lift's op");
    assert!(record.held.is_none());

    // One undo clears the hold with the lift.
    undo(&fx);
    assert_eq!(rev(&fx, "feat"), c2);
    assert!(ff_core::held::of(&fx.repo(), "top").unwrap().is_none());
}

#[test]
fn a_stacked_branch_in_another_worktree_is_skipped() {
    let fx = Fixture::new();
    ident(&fx);
    let (c1, c2, x1) = stack(&fx, None);
    let wt = fx.root().join("linked-wt");
    fx.git(&["worktree", "add", "-q", wt.to_str().unwrap(), "top"]);

    let report = lift_call(&fx, &c1, &["a.txt"]);

    assert!(
        report.cascade.moved.is_empty(),
        "{:?}",
        report.cascade.moved
    );
    assert!(report.cascade.held.is_empty(), "{:?}", report.cascade.held);
    assert_eq!(report.cascade.skipped.len(), 1);
    let skip = &report.cascade.skipped[0];
    assert_eq!(skip.branch, "top");
    assert_eq!(skip.base, "feat");
    match &skip.reason {
        ff_core::SkipReason::Worktree { path } => {
            assert!(path.contains("linked-wt"), "names the worktree: {path}");
        }
        other => panic!("skipped for the worktree, got {other:?}"),
    }
    assert_ne!(rev(&fx, "feat"), c2, "the lift itself landed");
    assert_eq!(rev(&fx, "top"), x1, "the other worktree's branch stays put");
}
