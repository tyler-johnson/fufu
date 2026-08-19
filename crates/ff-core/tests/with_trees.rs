//! Contract for the `*_with` entry points: a landing whose rewritten
//! commits' trees were decided in advance. An empty decision is an ordinary
//! verb — the same landing, the same hold. A non-empty one lands where the
//! same verb would have held, and every rewritten commit carries exactly the
//! tree it was given.

use std::collections::HashMap;

use ff_core::gix;
use ff_core::rewrite::Decided;
use ff_core::{AbsorbOutcome, DoneOutcome, EditOutcome, LiftOutcome, Provenance, RestackOutcome};
use ff_testsupport::Fixture;

const NOW: i64 = 1_799_999_999;

/// The verbs read the committer identity from the repo config, which the
/// fixture's hermetic env does not set; git itself gets its identity from
/// env vars, so this is only for the gix side.
fn ident(fx: &Fixture) {
    fx.set_config("user.name", "Fixture Committer");
    fx.set_config("user.email", "committer@fixture.test");
}

fn prov() -> Provenance {
    Provenance::new("pre", Some("ff resolve".into()))
}

fn oid(hex: &str) -> gix::ObjectId {
    gix::ObjectId::from_hex(hex.trim().as_bytes()).unwrap()
}

/// A commit's tree, read back through the repository handle.
fn tree_of(fx: &Fixture, commit: &str) -> gix::ObjectId {
    let repo = fx.repo();
    repo.find_object(oid(commit))
        .unwrap()
        .into_commit()
        .tree_id()
        .unwrap()
        .detach()
}

/// The one hold per branch, if any.
fn held_of(fx: &Fixture, branch: &str) -> Option<ff_core::held::Held> {
    ff_core::held::of(&fx.repo(), branch).unwrap()
}

fn restack_call(fx: &Fixture, now: i64) -> (RestackOutcome, ff_core::ops::VerbContext) {
    let repo = fx.repo();
    ff_core::restack::restack(
        &repo,
        None,
        None,
        &prov(),
        Some(now),
        vec!["ff".into(), "restack".into()],
    )
    .unwrap()
}

fn restack_with_call(
    fx: &Fixture,
    now: i64,
    decided: &Decided,
) -> (RestackOutcome, ff_core::ops::VerbContext) {
    let repo = fx.repo();
    ff_core::restack::restack_with(
        &repo,
        None,
        None,
        &prov(),
        (Some(now), vec!["ff".into(), "restack".into()]),
        decided,
    )
    .unwrap()
}

fn absorb_with_call(
    fx: &Fixture,
    into: &str,
    decided: &Decided,
    now: i64,
) -> (AbsorbOutcome, ff_core::ops::VerbContext) {
    let repo = fx.repo();
    ff_core::absorb::absorb_with(
        &repo,
        Some(oid(into)),
        Vec::new(),
        &prov(),
        (Some(now), vec!["ff".into(), "absorb".into()]),
        decided,
    )
    .unwrap()
}

fn lift_with_call(
    fx: &Fixture,
    from: &str,
    decided: &Decided,
    now: i64,
) -> (LiftOutcome, ff_core::ops::VerbContext) {
    let repo = fx.repo();
    ff_core::absorb::lift_with(
        &repo,
        Some(oid(from)),
        vec!["lift.txt".into()],
        &prov(),
        (Some(now), vec!["ff".into(), "lift".into()]),
        decided,
    )
    .unwrap()
}

fn edit_call(fx: &Fixture, rev: &str, now: i64) -> (EditOutcome, ff_core::ops::VerbContext) {
    let repo = fx.repo();
    ff_core::edit::edit(
        &repo,
        rev,
        &prov(),
        Some(now),
        vec!["ff".into(), "edit".into()],
    )
    .unwrap()
}

fn done_with_call(
    fx: &Fixture,
    decided: &Decided,
    now: i64,
) -> (DoneOutcome, ff_core::ops::VerbContext) {
    let repo = fx.repo();
    ff_core::done::done_with(
        &repo,
        false,
        &prov(),
        (Some(now), vec!["ff".into(), "done".into()]),
        decided,
    )
    .unwrap()
}

/// The shared clean stack:
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

/// A restack that conflicts: `feature` and `main` edit the same line of the
/// same file from the same base, so replaying f1 onto main's tip conflicts.
/// Leaves the fixture standing on `feature`.
fn conflict_stack(fx: &Fixture) -> (String, String) {
    fx.write("f.txt", "one\n");
    let _base = fx.commit("base");
    fx.git(&["switch", "-q", "-c", "feature"]);
    fx.write("f.txt", "two\n");
    let f1 = fx.commit("f1");
    fx.git(&["switch", "-q", "main"]);
    fx.write("f.txt", "three\n");
    let m1 = fx.commit("m1");
    fx.git(&["switch", "-q", "feature"]);
    (f1, m1)
}

/// A tree of one's own making on a side branch: `path` set to `content` on
/// top of `at` — the commit the decided tree stands in for — so it carries
/// nothing any later commit of the range also introduces: a supplied tree
/// equal to a descendant's own content would drop that descendant as empty,
/// which is correct but not what these tests are proving.
fn supplied_tree(fx: &Fixture, at: &str, branch: &str, path: &str, content: &str) -> gix::ObjectId {
    let mut args = vec!["switch", "-q", "-c", "side"];
    args.push(at);
    fx.git(&args);
    fx.write(path, content);
    let side = fx.commit("side");
    let tree = tree_of(fx, &side);
    fx.git(&["switch", "-q", branch]);
    tree
}

#[test]
fn an_empty_decision_is_an_ordinary_restack() {
    // Two identical stacks, built with the same git-call sequence, so their
    // commits — and everything a report derives from them — are identical.
    let fx_a = Fixture::new();
    ident(&fx_a);
    let _ = stack(&fx_a);
    let fx_b = Fixture::new();
    ident(&fx_b);
    let _ = stack(&fx_b);

    let (outcome_a, _ctx) = restack_call(&fx_a, NOW);
    let (outcome_b, _ctx) = restack_with_call(&fx_b, NOW, &Decided::none());

    let a = match outcome_a {
        RestackOutcome::Restacked(r) => *r,
        other => panic!("a clean restack must land, got {other:?}"),
    };
    let b = match outcome_b {
        RestackOutcome::Restacked(r) => *r,
        other => panic!("a clean restack must land, got {other:?}"),
    };

    assert_eq!(a.new_tip, b.new_tip, "same new tip");
    assert_eq!(a.moved, b.moved, "same carried branches");
    assert_eq!(a.replayed, b.replayed, "same replayed count");
    assert_eq!(a, b, "the reports must be field for field identical");
}

#[test]
fn a_decided_tree_is_what_the_commit_carries() {
    let fx = Fixture::new();
    ident(&fx);
    let (_c0, _c1, f1, _f2, _f3, _m2) = stack(&fx);

    let supplied = supplied_tree(&fx, &f1, "feature", "s.txt", "supplied\n");

    // f1 is the target: the oldest commit of the range.
    let decided = Decided {
        trees: HashMap::from([(oid(&f1), supplied)]),
        clearing: None,
    };
    let (outcome, _ctx) = restack_with_call(&fx, NOW, &decided);
    let report = match outcome {
        RestackOutcome::Restacked(r) => *r,
        other => panic!("a decided restack must land, got {other:?}"),
    };
    assert!(
        held_of(&fx, "feature").is_none(),
        "a landing records no hold"
    );

    // The rewritten f1 sits two below the new tip: its tree is exactly the
    // one supplied, not a merge.
    let repo = fx.repo();
    let new_tip = repo.find_commit(oid(&report.new_tip)).unwrap();
    let new_f2 = repo
        .find_commit(new_tip.parent_ids().next().unwrap())
        .unwrap();
    let new_f1 = repo
        .find_commit(new_f2.parent_ids().next().unwrap())
        .unwrap();
    assert_eq!(
        new_f1.tree_id().unwrap().detach(),
        supplied,
        "the rewritten commit carries exactly the supplied tree"
    );
}

#[test]
fn a_decided_landing_skips_the_pre_flight() {
    let fx = Fixture::new();
    ident(&fx);
    let (f1, _m1) = conflict_stack(&fx);

    // A resolved tree for the one affected commit: without it the probe
    // above the plan holds the restack, and the verb never reaches the plan.
    let supplied = supplied_tree(&fx, &f1, "feature", "f.txt", "resolved\n");
    let decided = Decided {
        trees: HashMap::from([(oid(&f1), supplied)]),
        clearing: None,
    };

    let (outcome, _ctx) = restack_with_call(&fx, NOW, &decided);
    let report = match outcome {
        RestackOutcome::Restacked(r) => *r,
        other => panic!("a decided landing must land rather than hold, got {other:?}"),
    };
    assert!(
        held_of(&fx, "feature").is_none(),
        "a decided landing records no hold"
    );

    // f1 was the whole range, so the new tip is the rewritten f1 itself.
    let repo = fx.repo();
    let new_tip = repo.find_commit(oid(&report.new_tip)).unwrap();
    assert_eq!(
        new_tip.tree_id().unwrap().detach(),
        supplied,
        "the rewritten commit carries exactly the supplied tree"
    );
}

#[test]
fn a_decided_absorb_skips_the_fold() {
    let fx = Fixture::new();
    ident(&fx);

    fx.write("f.txt", "base\n");
    let _c0 = fx.commit("c0");
    let supplied = supplied_tree(&fx, &_c0, "main", "f.txt", "supplied\n");

    fx.write("f.txt", "c1\n");
    let c1 = fx.commit("c1"); // the target
    fx.write("f.txt", "c2\n");
    let c2 = fx.commit("c2"); // the descendant
    let c2_tree = tree_of(&fx, &c2);

    // The open change edits the same path the target moved from the tip, so
    // the fold itself would conflict: without a decision the absorb holds.
    fx.write("f.txt", "open\n");

    let decided = Decided {
        trees: HashMap::from([(oid(&c1), supplied), (oid(&c2), c2_tree)]),
        clearing: None,
    };
    let (outcome, _ctx) = absorb_with_call(&fx, &c1, &decided, NOW);
    let report = match outcome {
        AbsorbOutcome::Absorbed(r) => r,
        other => panic!("a decided absorb must land rather than hold, got {other:?}"),
    };
    assert!(
        held_of(&fx, "main").is_none(),
        "a decided landing records no hold"
    );

    let new_c1 = report.new.clone().expect("the target survives the rewrite");
    assert_eq!(
        tree_of(&fx, &new_c1),
        supplied,
        "the target carries the supplied tree, fold skipped"
    );
}

#[test]
fn a_decided_lift_lands() {
    let fx = Fixture::new();
    ident(&fx);

    fx.write("lift.txt", "zero\n");
    let _c0 = fx.commit("c0");
    fx.write("lift.txt", "one\n");
    let c1 = fx.commit("c1"); // the lift target
    fx.write("lift.txt", "two\n");
    let c2 = fx.commit("c2"); // the descendant the replay would conflict on

    // A resolved tree for the descendant.
    let supplied = supplied_tree(&fx, &c2, "main", "lift.txt", "resolved\n");
    let decided = Decided {
        trees: HashMap::from([(oid(&c2), supplied)]),
        clearing: None,
    };

    let (outcome, _ctx) = lift_with_call(&fx, &c1, &decided, NOW);
    // The fully-lifted c1 drops as an empty commit — its new tree equals
    // c0's — so `main` lands on the rewritten c2, which the assertions below
    // read as the new tip.
    match outcome {
        LiftOutcome::Lifted(_) => {}
        other => panic!("a decided lift must land rather than hold, got {other:?}"),
    };
    assert!(
        held_of(&fx, "main").is_none(),
        "a decided landing records no hold"
    );

    // c2 is the tip, and it carries exactly the supplied tree.
    let new_tip = oid(fx.git(&["rev-parse", "main"]).trim());
    let repo = fx.repo();
    assert_eq!(
        repo.find_commit(new_tip)
            .unwrap()
            .tree_id()
            .unwrap()
            .detach(),
        supplied,
        "the replayed descendant carries exactly the supplied tree"
    );
}

#[test]
fn a_decided_done_lands() {
    let fx = Fixture::new();
    ident(&fx);

    // `shared.txt` is edited three ways at the same line: by c1, by c2, and
    // in the session — so replaying c2 over the session's amend of c1
    // conflicts, and an ordinary `ff done` holds.
    fx.write("shared.txt", "line1\nbase\nline3\n");
    fx.write("c0.txt", "c0\n");
    let _c0 = fx.commit("c0");
    fx.write("shared.txt", "line1\nc1-edit\nline3\n");
    let c1 = fx.commit("c1"); // the anchor
    fx.write("shared.txt", "line1\nc2-edit\nline3\n");
    let c2 = fx.commit("c2"); // the conflicting replay
    fx.write("c3.txt", "c3\n");
    let _c3 = fx.commit("c3");

    // A resolved tree for the conflicting replay.
    let supplied = supplied_tree(&fx, &c2, "main", "shared.txt", "line1\nresolved\nline3\n");

    let (edit_outcome, _ctx) = edit_call(&fx, &c1, NOW);
    let session = match edit_outcome {
        EditOutcome::Opened(r) => r.session,
        other => panic!("a session must open, got {other:?}"),
    };
    fx.write("shared.txt", "line1\nsession-edit\nline3\n");

    let decided = Decided {
        trees: HashMap::from([(oid(&c2), supplied)]),
        clearing: None,
    };
    let (outcome, _ctx) = done_with_call(&fx, &decided, NOW + 100);
    let report = match outcome {
        DoneOutcome::Done(r) => r,
        other => panic!("a decided done must land rather than hold, got {other:?}"),
    };
    assert!(
        held_of(&fx, &session).is_none(),
        "a decided landing records no hold"
    );
    assert_eq!(
        report.replayed, 2,
        "c2 and c3 replay ahead of the amended c1"
    );

    // c2 sits one below the new tip, and it carries exactly the supplied
    // tree.
    let repo = fx.repo();
    let new_tip = repo.find_commit(oid(&report.new_tip)).unwrap();
    let new_c2 = repo
        .find_commit(new_tip.parent_ids().next().unwrap())
        .unwrap();
    assert_eq!(
        new_c2.tree_id().unwrap().detach(),
        supplied,
        "the replayed commit carries exactly the supplied tree"
    );
}

#[test]
fn an_empty_decision_still_holds_on_a_conflict() {
    let fx = Fixture::new();
    ident(&fx);
    let (_f1, _m1) = conflict_stack(&fx);

    // The same stack that a decided landing lands: through an empty decision
    // it must hold exactly as `restack` does — the guard is not a switch that
    // turns the pre-flight off for everyone.
    let (outcome, _ctx) = restack_with_call(&fx, NOW, &Decided::none());
    let report = match outcome {
        RestackOutcome::Held(r) => r,
        other => panic!("an empty decision must hold, got {other:?}"),
    };
    assert_eq!(report.verb, "restack");
    let held = held_of(&fx, "feature").expect("the hold must be recorded");
    assert!(
        held.paths.iter().any(|p| p == "f.txt"),
        "the hold must name the conflicting path: {:?}",
        held.paths
    );
}
