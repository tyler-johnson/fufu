//! Contract for `held::replan`: a hold is a question, not an answer, so
//! turning it back into a plan re-reads the repository as it stands now — the
//! base as it stands, the branch's tip as it stands, the working tree as it
//! stands — and, when the thing the hold named has since gone, says the hold
//! is stale rather than resolving something nobody asked for. Expiring is an
//! operation with a verb attached, so replanning never performs one.

use ff_core::gix;
use ff_core::{AbsorbOutcome, DoneOutcome, EditOutcome, LiftOutcome, Provenance, RestackOutcome};
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

fn oid(hex: impl AsRef<str>) -> gix::ObjectId {
    gix::ObjectId::from_hex(hex.as_ref().trim().as_bytes()).unwrap()
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

fn absorb_call(fx: &Fixture, into: &str, now: i64) -> (AbsorbOutcome, ff_core::ops::VerbContext) {
    let repo = fx.repo();
    ff_core::absorb::absorb(
        &repo,
        Some(oid(into)),
        Vec::new(),
        ff_core::Verify::Run,
        &prov(),
        Some(now),
        vec!["ff".into(), "absorb".into()],
    )
    .unwrap()
}

fn lift_call(
    fx: &Fixture,
    from: &str,
    paths: Vec<String>,
    now: i64,
) -> (LiftOutcome, ff_core::ops::VerbContext) {
    let repo = fx.repo();
    ff_core::absorb::lift(
        &repo,
        Some(oid(from)),
        paths,
        &prov(),
        Some(now),
        vec!["ff".into(), "lift".into()],
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

fn done_call(fx: &Fixture, now: i64) -> (DoneOutcome, ff_core::ops::VerbContext) {
    let repo = fx.repo();
    ff_core::done::done(
        &repo,
        false,
        ff_core::Verify::Run,
        &prov(),
        Some(now),
        vec!["ff".into(), "done".into()],
    )
    .unwrap()
}

/// `feature` and `main` edit the same line of the same file from the same
/// base, so replaying f1 onto main's tip conflicts. Leaves the fixture
/// standing on `feature`.
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

/// A stack whose open change cannot be folded into the target `c1`: all three
/// sides rewrite line 1 of `f.txt` differently. Leaves the fixture on `main`.
fn fold_conflict_stack(fx: &Fixture) -> String {
    fx.write("f.txt", "x\nrest\n");
    let _c0 = fx.commit("base");
    fx.write("f.txt", "A\nrest\n");
    let c1 = fx.commit("c1");
    fx.write("f.txt", "A2\nrest\n");
    let _c2 = fx.commit("c2");
    // The open change rewrites line 1 a third way, conflicting with the fold.
    fx.write("f.txt", "C\nrest\n");
    c1
}

/// `c1` introduces `doc.txt`, and the descendant `c2` edits it — lifting the
/// file out of `c1` makes `c2`'s edit a modification of nothing.
fn lift_conflict_stack(fx: &Fixture) -> (String, String) {
    fx.write("f0.txt", "base\n");
    let _c0 = fx.commit("base");
    fx.write("doc.txt", "v1\n");
    let c1 = fx.commit("c1");
    fx.write("doc.txt", "v2\n");
    let c2 = fx.commit("c2");
    (c1, c2)
}

/// A session on `c1` whose amend the commits ahead cannot replay over. Returns
/// the session branch, the anchor, and `main`'s tip.
fn done_conflict_stack(fx: &Fixture) -> (String, String, String) {
    fx.write("shared.txt", "line1\nbase\nline3\n");
    fx.write("c0.txt", "c0\n");
    let _c0 = fx.commit("c0");
    fx.write("shared.txt", "line1\nc1-edit\nline3\n");
    let c1 = fx.commit("c1");
    fx.write("shared.txt", "line1\nc2-edit\nline3\n");
    let _c2 = fx.commit("c2");
    fx.write("c3.txt", "c3\n");
    let c3 = fx.commit("c3");

    let (edit_outcome, _ctx) = edit_call(fx, &c1, NOW);
    let session = match edit_outcome {
        EditOutcome::Opened(r) => r.session,
        other => panic!("a session must open, got {other:?}"),
    };
    fx.write("shared.txt", "line1\nsession-edit\nline3\n");
    (session, c1, c3)
}

#[test]
fn a_held_restack_replans_against_the_base_as_it_stands_now() {
    let fx = Fixture::new();
    ident(&fx);
    conflict_stack(&fx);

    let (outcome, _ctx) = restack_call(&fx, None, None, NOW);
    assert!(matches!(outcome, RestackOutcome::Held(_)));
    let held = ff_core::held::of(&fx.repo(), "feature")
        .unwrap()
        .expect("the hold must stand on the restacked branch");

    // The base moves: add a commit to it. The hold still names the base by
    // name, so re-planning must read the new tip, not the one it stood on.
    fx.git(&["switch", "-q", "main"]);
    fx.write("base2.txt", "base2\n");
    let new_base = fx.commit("m2");
    fx.git(&["switch", "-q", "feature"]);

    let replan = ff_core::held::replan(&fx.repo(), &held).unwrap();
    assert_eq!(
        replan.change,
        ff_core::rewrite::Change::Onto(oid(new_base)),
        "a hold is a question: the base is read as it stands now, not as it stood when held"
    );
}

#[test]
fn a_held_restack_replans_over_commits_added_since() {
    let fx = Fixture::new();
    ident(&fx);
    let (f1, _m1) = conflict_stack(&fx);

    let (outcome, _ctx) = restack_call(&fx, None, None, NOW);
    assert!(matches!(outcome, RestackOutcome::Held(_)));
    let held = ff_core::held::of(&fx.repo(), "feature")
        .unwrap()
        .expect("the hold must stand on the restacked branch");

    // The feature branch grows: the pending rewrite replays over whatever is
    // added, so the tip it names is the new one.
    fx.write("f2.txt", "f2\n");
    let f2 = fx.commit("f2");

    let replan = ff_core::held::replan(&fx.repo(), &held).unwrap();
    assert_eq!(
        replan.tip,
        oid(f2),
        "the tip is the branch's tip as it stands now"
    );
    assert_eq!(
        replan.target,
        oid(f1),
        "the target is still the oldest commit not already on the base"
    );
}

#[test]
fn a_held_absorb_replans_from_the_working_tree_as_it_stands_now() {
    let fx = Fixture::new();
    ident(&fx);
    let c1 = fold_conflict_stack(&fx);

    let (outcome, _ctx) = absorb_call(&fx, &c1, NOW);
    assert!(matches!(outcome, AbsorbOutcome::Held(_)));
    let held = ff_core::held::of(&fx.repo(), "main")
        .unwrap()
        .expect("the hold must stand on the branch underfoot");

    // The tree the verb's fold would take, read now — the working tree as it
    // stands. Then the working tree moves, and the replan must see that.
    let before = ff_core::held::replan(&fx.repo(), &held).unwrap();
    fx.write("f.txt", "D\nrest\n");
    let after = ff_core::held::replan(&fx.repo(), &held).unwrap();

    assert_ne!(
        before.change, after.change,
        "the fold is never frozen: the replan sees the working tree as it stands now"
    );
}

#[test]
fn a_held_lift_replans_the_same_paths() {
    let fx = Fixture::new();
    ident(&fx);
    let (c1, _c2) = lift_conflict_stack(&fx);

    let (outcome, _ctx) = lift_call(&fx, &c1, vec!["doc.txt".into()], NOW);
    assert!(matches!(outcome, LiftOutcome::Held(_)));
    let held = ff_core::held::of(&fx.repo(), "main")
        .unwrap()
        .expect("the hold must stand on the branch underfoot");

    // The same hold, asked twice: the target's tree with those paths reverted
    // is a pure function of the repository, so it answers identically.
    let first = ff_core::held::replan(&fx.repo(), &held).unwrap();
    let second = ff_core::held::replan(&fx.repo(), &held).unwrap();
    assert_eq!(
        first.target,
        oid(c1),
        "the target is the commit lifted from"
    );
    assert_eq!(
        first.change, second.change,
        "the same paths lifted from the same target replay the same tree"
    );
}

#[test]
fn a_held_done_replans_the_session() {
    let fx = Fixture::new();
    ident(&fx);
    let (session, c1, c3) = done_conflict_stack(&fx);

    let (outcome, _ctx) = done_call(&fx, NOW + 100);
    assert!(matches!(outcome, DoneOutcome::Held(_)));
    let held = ff_core::held::of(&fx.repo(), &session)
        .unwrap()
        .expect("the hold must stand on the session branch");

    let replan = ff_core::held::replan(&fx.repo(), &held).unwrap();
    assert_eq!(
        replan.target,
        oid(c1),
        "the target is the anchor the session opened at"
    );
    assert_eq!(
        replan.tip,
        oid(c3),
        "the tip is onto's tip as it stands now"
    );
}

#[test]
fn a_hold_whose_base_is_gone_expires() {
    let fx = Fixture::new();
    ident(&fx);
    conflict_stack(&fx);

    let (outcome, _ctx) = restack_call(&fx, None, None, NOW);
    assert!(matches!(outcome, RestackOutcome::Held(_)));
    let held = ff_core::held::of(&fx.repo(), "feature")
        .unwrap()
        .expect("the hold must stand on the restacked branch");

    // The base branch is gone: the question no longer has an answer.
    fx.git(&["branch", "-D", "main"]);

    let err =
        ff_core::held::replan(&fx.repo(), &held).expect_err("a hold whose base is gone expires");
    assert_eq!(err.id(), "held/expired", "{err}");
    assert_eq!(
        err.exit_code(),
        3,
        "the held/ namespace is a decision, exit 3"
    );
}

#[test]
fn a_hold_whose_target_is_gone_expires() {
    let fx = Fixture::new();
    ident(&fx);
    let c1 = fold_conflict_stack(&fx);

    let (outcome, _ctx) = absorb_call(&fx, &c1, NOW);
    assert!(matches!(outcome, AbsorbOutcome::Held(_)));
    let held = ff_core::held::of(&fx.repo(), "main")
        .unwrap()
        .expect("the hold must stand on the branch underfoot");

    // The branch is reset back past the target: the target is no longer in
    // the branch's history, so the fold has nothing to fold into.
    fx.git(&["reset", "--hard", &format!("{c1}^")]);

    let err =
        ff_core::held::replan(&fx.repo(), &held).expect_err("a hold whose target is gone expires");
    assert_eq!(err.id(), "held/expired", "{err}");
}

#[test]
fn replanning_leaves_the_hold_alone() {
    let fx = Fixture::new();
    ident(&fx);
    conflict_stack(&fx);

    let (outcome, _ctx) = restack_call(&fx, None, None, NOW);
    assert!(matches!(outcome, RestackOutcome::Held(_)));
    let held = ff_core::held::of(&fx.repo(), "feature")
        .unwrap()
        .expect("the hold must stand on the restacked branch");

    // Make the hold expire, and confirm replanning reports it without acting.
    fx.git(&["branch", "-D", "main"]);
    let err = ff_core::held::replan(&fx.repo(), &held).expect_err("expires");
    assert_eq!(err.id(), "held/expired", "{err}");

    assert_eq!(
        ff_core::held::of(&fx.repo(), "feature").unwrap(),
        Some(held.clone()),
        "replanning answers a question; expiring is an operation it does not perform"
    );
}
