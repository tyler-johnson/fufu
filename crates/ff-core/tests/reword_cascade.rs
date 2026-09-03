//! `ff describe <rev>` and the branches stacked above the one it reworded:
//! the cascade rides the reword's own operation, and since a reword moves no
//! tree the replay above never conflicts. A branch another worktree holds is
//! skipped and named.

use ff_core::gix;
use ff_core::{Provenance, Verify};
use ff_testsupport::Fixture;

const NOW: i64 = 1_799_999_999;

/// `reword` reads the committer identity from the repo config, which the
/// fixture's hermetic env does not set; git itself gets its identity from
/// env vars, so this is only for the gix side.
fn ident(fx: &Fixture) {
    fx.set_config("user.name", "Fixture Committer");
    fx.set_config("user.email", "committer@fixture.test");
}

fn prov() -> Provenance {
    Provenance::new("pre", Some("ff describe".into()))
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

/// Record `parent` as the branch `branch` sits on, the way
/// `ff start <parent> -b <branch>` does.
fn stacked_on(fx: &Fixture, branch: &str, parent: &str) {
    let repo = fx.repo();
    let mut meta = ff_core::branchmeta::read(&repo, branch).unwrap();
    meta.parent = Some(parent.to_string());
    ff_core::branchmeta::write(&repo, branch, &meta).unwrap();
}

/// The verb operations in the log, captures and notes excluded, so the
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

fn reword_call(fx: &Fixture, target: &str, message: &str) -> ff_core::RewordReport {
    let repo = fx.repo();
    let (report, _ctx) = ff_core::describe::reword(
        &repo,
        oid(target),
        message.to_string(),
        Verify::Run,
        &prov(),
        Some(NOW),
        vec!["ff".into(), "describe".into()],
    )
    .unwrap();
    report
}

/// The stack `main` ← `feat` ← `top`, `top` recording `feat` as the branch
/// it sits on:
///
/// base ─ c1 ─ c2        (feat)
///              └─ x1    (top)
///
/// Returns (c1, c2, x1) and leaves the fixture standing on `feat`.
fn stack(fx: &Fixture) -> (String, String, String) {
    fx.write("a.txt", "base\n");
    fx.commit("base");
    fx.git(&["switch", "-q", "-c", "feat"]);
    fx.write("a.txt", "one\n");
    let c1 = fx.commit("c1");
    fx.write("c.txt", "c\n");
    let c2 = fx.commit("c2");
    fx.git(&["switch", "-q", "-c", "top"]);
    fx.write("a.txt", "two\n");
    let x1 = fx.commit("x1");
    stacked_on(fx, "top", "feat");
    fx.git(&["switch", "-q", "feat"]);
    (c1, c2, x1)
}

#[test]
fn reword_replays_the_branch_stacked_above() {
    let fx = Fixture::new();
    ident(&fx);
    let (c1, c2, x1) = stack(&fx);
    let top_tree_before = rev(&fx, "top^{tree}");

    let report = reword_call(&fx, &c1, "c1 reworded");

    assert_eq!(report.branch, "feat");
    assert_eq!(report.restacked, 1);
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
    // A reword moves no tree: top's commit holds the same tree it did, and
    // the reworded message sits beneath it.
    assert_eq!(rev(&fx, "top^{tree}"), top_tree_before);
    let subjects = fx.git(&["log", "--format=%s", "top"]);
    assert!(subjects.contains("c1 reworded"), "{subjects}");
    assert_eq!(fx.git(&["log", "--format=%s", "-1", "top"]).trim(), "x1");
}

#[test]
fn reword_cascade_rides_the_one_operation() {
    let fx = Fixture::new();
    ident(&fx);
    let (c1, c2, x1) = stack(&fx);
    let before = verb_ops(&fx);

    let report = reword_call(&fx, &c1, "c1 reworded");
    assert_eq!(report.cascade.moved.len(), 1);
    assert_eq!(
        verb_ops(&fx),
        before + 1,
        "one operation for the reword and its cascade"
    );

    let repo = fx.repo();
    let record = tip_record(&repo);
    assert!(
        record
            .refs
            .iter()
            .any(|t| t.name == "refs/heads/top" && t.old.as_deref() == Some(x1.as_str())),
        "top's move rides the reword's record: {:?}",
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
}

#[test]
fn a_reword_cascade_never_holds() {
    let fx = Fixture::new();
    ident(&fx);
    // x1 rewrites the line c1 wrote, the shape that holds under a lift or
    // an absorb of c1. A reword changes no tree, so the replay is clean.
    let (c1, _c2, x1) = stack(&fx);

    let report = reword_call(&fx, &c1, "c1 reworded");

    assert!(report.cascade.held.is_empty(), "{:?}", report.cascade.held);
    assert_eq!(report.cascade.moved.len(), 1, "{:?}", report.cascade);
    assert!(report.cascade.skipped.is_empty());
    assert_ne!(rev(&fx, "top"), x1, "top followed");

    let repo = fx.repo();
    assert!(ff_core::held::of(&repo, "top").unwrap().is_none());
    assert!(ff_core::held::of(&repo, "feat").unwrap().is_none());
    let record = tip_record(&repo);
    assert!(record.cascade_held.is_empty());
    assert!(record.held.is_none());
}

#[test]
fn a_reword_with_nothing_stacked_reports_an_empty_cascade() {
    let fx = Fixture::new();
    ident(&fx);
    let (c1, c2, _x1) = stack(&fx);
    fx.git(&["branch", "-D", "top"]);

    let report = reword_call(&fx, &c1, "c1 reworded");

    assert!(report.cascade.is_empty(), "{:?}", report.cascade);
    assert_ne!(rev(&fx, "feat"), c2, "the reword itself landed");
}

#[test]
fn a_stacked_branch_in_another_worktree_is_skipped() {
    let fx = Fixture::new();
    ident(&fx);
    let (c1, c2, x1) = stack(&fx);
    let bay = fx.root().join("bay");
    ff_core::linked::add::create(&fx.repo(), &bay, "top", 0).expect("create");

    let report = reword_call(&fx, &c1, "c1 reworded");

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
            assert!(path.contains("bay"), "names the worktree: {path}");
        }
        other => panic!("skipped for the worktree, got {other:?}"),
    }
    assert_ne!(rev(&fx, "feat"), c2, "the reword itself landed");
    assert_eq!(rev(&fx, "top"), x1, "the other worktree's branch stays put");
}
