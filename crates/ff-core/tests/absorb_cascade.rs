//! Contract for the cascade `absorb::absorb` runs once its rewrite has
//! landed: every branch stacked above the rewritten one replays onto its
//! new tip inside absorb's own operation, a conflict above holds that
//! branch without holding the absorb, and a branch another worktree holds
//! is skipped and named. `diff_absorb.rs` is the differential suite against
//! git; this is the stack.

use ff_core::gix;
use ff_core::{AbsorbOutcome, Provenance};
use ff_testsupport::Fixture;

const NOW: i64 = 1_799_999_999;

/// `absorb` reads the committer identity from the repo config, which the
/// fixture's hermetic env does not set; git itself gets its identity from
/// env vars, so this is only for the gix side.
fn ident(fx: &Fixture) {
    fx.set_config("user.name", "Fixture Committer");
    fx.set_config("user.email", "committer@fixture.test");
}

fn prov() -> Provenance {
    Provenance::new("pre", Some("ff absorb".into()))
}

/// Record `parent` as the branch `branch` sits on, the way
/// `ff start <parent> -b <branch>` does.
fn stacked_on(fx: &Fixture, branch: &str, parent: &str) {
    let repo = fx.repo();
    let mut meta = ff_core::branchmeta::read(&repo, branch).unwrap();
    meta.parent = Some(parent.to_string());
    ff_core::branchmeta::write(&repo, branch, &meta).unwrap();
}

/// The stack, built by hand:
///
/// root ─ f1 ─ f2         (feat, on main)
///              └─ t1     (top, on feat)
///
/// `top` edits a file of its own by default; `top_file` lets a test aim it
/// at one the absorb is about to rewrite. Leaves the fixture standing on
/// `feat` with an open edit to `a.txt`, the file f1 introduced. Returns
/// (f1, f2, t1).
fn stack(fx: &Fixture, top_file: (&str, &str)) -> (String, String, String) {
    fx.write("root.txt", "root\n");
    fx.commit("root");

    fx.git(&["switch", "-q", "-c", "feat"]);
    fx.write("a.txt", "a\n");
    let f1 = fx.commit("f1");
    fx.write("b.txt", "b\n");
    let f2 = fx.commit("f2");
    stacked_on(fx, "feat", "main");

    fx.git(&["switch", "-q", "-c", "top"]);
    fx.write(top_file.0, top_file.1);
    let t1 = fx.commit("t1");
    stacked_on(fx, "top", "feat");

    fx.git(&["switch", "-q", "feat"]);
    fx.write("a.txt", "a\nmore\n");
    (f1, f2, t1)
}

fn absorb_into(fx: &Fixture, target: &str) -> ff_core::AbsorbReport {
    let repo = fx.repo();
    let into = gix::ObjectId::from_hex(target.as_bytes()).unwrap();
    let (outcome, _ctx) = ff_core::absorb::absorb(
        &repo,
        Some(into),
        Vec::new(),
        ff_core::Verify::Run,
        &prov(),
        Some(NOW),
        vec!["ff".into(), "absorb".into()],
    )
    .unwrap();
    match outcome {
        AbsorbOutcome::Absorbed(report) => report,
        other => panic!("the absorb must land, got {other:?}"),
    }
}

fn rev(fx: &Fixture, name: &str) -> String {
    fx.git(&["rev-parse", name]).trim().to_string()
}

fn is_ancestor(fx: &Fixture, ancestor: &str, of: &str) -> bool {
    fx.try_git(&["merge-base", "--is-ancestor", ancestor, of])
        .status
        .success()
}

/// The verb operations in the log — captures and notes excluded, so the count
/// says "one more thing the user asked for happened" and nothing about the
/// preamble's own writing.
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

#[test]
fn absorb_replays_the_branch_stacked_above() {
    let fx = Fixture::new();
    ident(&fx);
    let (f1, f2, t1) = stack(&fx, ("t.txt", "t\n"));

    let report = absorb_into(&fx, &f1);

    assert_eq!(report.restacked, 1, "f2 restacked above the target");
    assert_ne!(rev(&fx, "feat"), f2, "feat moved");
    assert_ne!(rev(&fx, "top"), t1, "top followed");
    assert!(
        is_ancestor(&fx, &rev(&fx, "feat"), &rev(&fx, "top")),
        "top's tip sits on feat's new tip"
    );
    assert_eq!(
        fx.git(&["show", "top:a.txt"]),
        "a\nmore\n",
        "top carries the absorbed content"
    );

    assert_eq!(report.cascade.moved.len(), 1, "{:?}", report.cascade);
    let moved = &report.cascade.moved[0];
    assert_eq!(moved.branch, "top");
    assert_eq!(moved.base, "feat");
    assert_eq!(moved.old_tip, t1);
    assert_eq!(moved.new_tip, rev(&fx, "top"));
    assert_eq!(moved.replayed, 1);
    assert!(report.cascade.held.is_empty());
    assert!(report.cascade.skipped.is_empty());
    assert!(
        report.moved.is_empty(),
        "top sits above the range, not inside it: {:?}",
        report.moved
    );
}

#[test]
fn absorb_cascade_rides_the_one_operation() {
    let fx = Fixture::new();
    ident(&fx);
    let (f1, f2, t1) = stack(&fx, ("t.txt", "t\n"));
    let before = verb_ops(&fx);

    let report = absorb_into(&fx, &f1);
    assert_eq!(report.cascade.moved.len(), 1);

    assert_eq!(
        verb_ops(&fx),
        before + 1,
        "the absorb and its cascade are one operation"
    );
    let repo = fx.repo();
    let record = tip_record(&repo);
    assert!(
        record.summary.ends_with(", and 1 above it"),
        "{}",
        record.summary
    );
    assert!(
        record.refs.iter().any(|t| t.name == "refs/heads/top"),
        "top's move is on the record: {:?}",
        record.refs
    );
    assert!(
        record.rewrites.iter().any(|r| r.old == t1),
        "top's rewrite is on the record: {:?}",
        record.rewrites
    );

    undo(&fx);
    assert_eq!(rev(&fx, "feat"), f2, "undo puts feat back");
    assert_eq!(rev(&fx, "top"), t1, "and top with it");
}

#[test]
fn a_conflicting_stacked_branch_holds_and_absorb_still_lands() {
    let fx = Fixture::new();
    ident(&fx);
    // top edits the line the absorb is about to change in f1, so its replay
    // onto the rewritten feat conflicts.
    let (f1, f2, t1) = stack(&fx, ("a.txt", "a\ntop\n"));

    let report = absorb_into(&fx, &f1);

    assert_ne!(rev(&fx, "feat"), f2, "feat moved");
    assert_eq!(rev(&fx, "top"), t1, "the held branch stays put");
    assert!(
        report.cascade.moved.is_empty(),
        "{:?}",
        report.cascade.moved
    );
    assert_eq!(report.cascade.held.len(), 1);
    let hold = &report.cascade.held[0];
    assert_eq!(hold.branch, "top");
    assert_eq!(hold.base, "feat");
    assert_eq!(hold.report.paths, vec!["a.txt".to_string()]);
    assert_eq!(hold.report.of, 1);

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
        "the absorb itself is not held"
    );
    let record = tip_record(&repo);
    assert_eq!(record.cascade_held.len(), 1, "the hold rides absorb's op");
    assert!(record.held.is_none());

    // One undo takes the absorb and the hold back together.
    undo(&fx);
    assert_eq!(rev(&fx, "feat"), f2);
    assert!(ff_core::held::of(&fx.repo(), "top").unwrap().is_none());
}

#[test]
fn a_stacked_branch_in_another_worktree_is_skipped() {
    let fx = Fixture::new();
    ident(&fx);
    let (f1, f2, t1) = stack(&fx, ("t.txt", "t\n"));
    let bay = fx.root().join("bay");
    ff_core::linked::add::create(&fx.repo(), &bay, "top", 0).expect("create");

    let report = absorb_into(&fx, &f1);

    assert_ne!(rev(&fx, "feat"), f2, "feat moved");
    assert_eq!(rev(&fx, "top"), t1, "top stays where its worktree holds it");
    assert!(
        report.cascade.moved.is_empty(),
        "{:?}",
        report.cascade.moved
    );
    assert_eq!(report.cascade.skipped.len(), 1);
    let skip = &report.cascade.skipped[0];
    assert_eq!(skip.branch, "top");
    match &skip.reason {
        ff_core::SkipReason::Worktree { path } => {
            assert!(path.contains("bay"), "names the worktree: {path}");
        }
        other => panic!("skipped for the worktree, got {other:?}"),
    }
}

#[test]
fn a_branch_inside_the_rewritten_range_moves_once() {
    let fx = Fixture::new();
    ident(&fx);
    let (f1, f2, _t1) = stack(&fx, ("t.txt", "t\n"));
    // A branch at f2 recording feat as its base: absorb's own plan carries
    // it, and the cascade must not replay it a second time.
    fx.git(&["branch", "mid", &f2]);
    stacked_on(&fx, "mid", "feat");

    let report = absorb_into(&fx, &f1);

    assert_eq!(report.moved, vec!["mid".to_string()]);
    assert_eq!(rev(&fx, "mid"), rev(&fx, "feat"), "mid rode the rewrite");
    assert!(report.cascade.moved.iter().all(|m| m.branch != "mid"));
    assert_eq!(report.cascade.unchanged, vec!["mid".to_string()]);
    let record = tip_record(&fx.repo());
    assert_eq!(
        record
            .refs
            .iter()
            .filter(|t| t.name == "refs/heads/mid")
            .count(),
        1,
        "one transition for mid: {:?}",
        record.refs
    );
}
