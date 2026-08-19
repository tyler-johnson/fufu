//! Contract for `ff resolve` — the verb that deals with a held rewrite. It
//! materializes every surviving conflict region into the working tree at once,
//! as labeled markers, in one session; releases the hold when the world moved
//! out of the conflict; and, under `--abandon`, drops the hold and the session
//! together. Opening is a success, so no outcome here moves a branch ref.

use ff_core::gix;
use ff_core::{Provenance, ResolveOutcome};
use ff_testsupport::Fixture;

const NOW: i64 = 1_799_999_999;

/// `restack` and the replay read the committer identity from the repo config,
/// which the fixture's hermetic env does not set; git itself gets its identity
/// from env vars, so this is only for the gix side.
fn ident(fx: &Fixture) {
    fx.set_config("user.name", "Fixture Committer");
    fx.set_config("user.email", "committer@fixture.test");
}

fn prov() -> Provenance {
    Provenance::new("pre", Some("ff restack".into()))
}

fn resolve_call(
    fx: &Fixture,
    abandon: bool,
    now: i64,
) -> (ResolveOutcome, ff_core::ops::VerbContext) {
    let repo = fx.repo();
    ff_core::resolve::resolve(
        &repo,
        abandon,
        &prov(),
        Some(now),
        vec!["ff".into(), "resolve".into()],
    )
    .unwrap()
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

/// Every branch ref as (full ref name, sha), sorted by name — the whole of
/// the namespace a resolve is allowed to move.
fn head_refs(fx: &Fixture) -> Vec<(String, String)> {
    let repo = fx.repo();
    let platform = repo.references().unwrap();
    let mut out = Vec::new();
    for reference in platform.prefixed("refs/heads/").unwrap() {
        let reference = reference.unwrap();
        if let Some(id) = reference.target().try_id() {
            out.push((reference.name().as_bstr().to_string(), id.to_string()));
        }
    }
    out.sort();
    out
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

/// `feature` and `main` edit the same line of the same file from the same
/// base, so replaying f1 onto main's tip conflicts. Returns the base commit
/// (so a test can move main back past the conflict) and leaves the fixture
/// standing on `feature`.
fn conflict_stack(fx: &Fixture) -> String {
    fx.write("f.txt", "one\n");
    let base = fx.commit("base");
    fx.git(&["switch", "-q", "-c", "feature"]);
    fx.write("f.txt", "two\n");
    let _f1 = fx.commit("f1");
    fx.git(&["switch", "-q", "main"]);
    fx.write("f.txt", "three\n");
    let _m1 = fx.commit("m1");
    fx.git(&["switch", "-q", "feature"]);
    base
}

/// Hold a conflicting restack so there is something to resolve.
fn hold_a_restack(fx: &Fixture) {
    let (outcome, _ctx) = {
        let repo = fx.repo();
        ff_core::restack::restack(
            &repo,
            None,
            None,
            &prov(),
            Some(NOW),
            vec!["ff".into(), "restack".into()],
        )
        .unwrap()
    };
    assert!(
        matches!(outcome, ff_core::RestackOutcome::Held(_)),
        "the precondition is a held restack, got {outcome:?}"
    );
}

#[test]
fn resolving_a_held_restack_puts_the_markers_in_the_working_tree() {
    let fx = Fixture::new();
    ident(&fx);
    conflict_stack(&fx);
    hold_a_restack(&fx);
    let before = head_refs(&fx);

    let (outcome, _ctx) = resolve_call(&fx, false, NOW + 100);
    let report = match outcome {
        ResolveOutcome::Opened(r) => r,
        other => panic!("a conflicting resolve must open, got {other:?}"),
    };

    assert_eq!(report.branch, "feature");
    assert_eq!(report.verb, "restack");
    assert!(
        report.files.iter().any(|f| f == "f.txt"),
        "the report names the conflicted file: {:?}",
        report.files
    );
    assert!(report.regions >= 1, "at least one region waits");

    // The markers are on disk, in the working tree.
    let on_disk = std::fs::read_to_string(fx.path().join("f.txt")).unwrap();
    assert!(
        on_disk.contains("<<<<<<< the rewrite so far"),
        "the opener must be in the file: {on_disk}"
    );
    assert!(
        on_disk.contains(">>>>>>> rebasing \"f1\""),
        "the closer must name the commit: {on_disk}"
    );

    // A resolve opens a session; it does not move a branch ref.
    assert_eq!(head_refs(&fx), before, "a resolve moves no branch ref");
}

#[test]
fn resolving_records_the_session() {
    let fx = Fixture::new();
    ident(&fx);
    conflict_stack(&fx);
    hold_a_restack(&fx);
    let held_before = ff_core::held::of(&fx.repo(), "feature").unwrap().unwrap();

    let (outcome, _ctx) = resolve_call(&fx, false, NOW + 100);
    assert!(matches!(outcome, ResolveOutcome::Opened(_)));

    let session = ff_core::held::resolving(&fx.repo(), "feature")
        .unwrap()
        .expect("the session must be recorded");

    // Its `from` is the tree the report was computed from — replay the plan
    // and compare trees.
    let replan = ff_core::held::replan(&fx.repo(), &session.hold).unwrap();
    let chain = ff_core::rewrite::chain(&fx.repo(), replan.target, replan.tip, &replan.change, &[])
        .unwrap();
    assert_eq!(
        session.from,
        chain.tree.to_string(),
        "the session's `from` is the tree the markers landed in"
    );

    // The steps name the commits oldest-first.
    assert_eq!(session.steps, vec!["f1".to_string()]);

    // The hold stays: it is what the session is resolving.
    assert_eq!(
        ff_core::held::of(&fx.repo(), "feature").unwrap(),
        Some(held_before),
        "the hold must stand through the resolve"
    );
}

#[test]
fn a_second_resolve_refuses() {
    let fx = Fixture::new();
    ident(&fx);
    conflict_stack(&fx);
    hold_a_restack(&fx);

    let (outcome, _ctx) = resolve_call(&fx, false, NOW + 100);
    assert!(matches!(outcome, ResolveOutcome::Opened(_)));

    let repo = fx.repo();
    let err = ff_core::resolve::resolve(
        &repo,
        false,
        &prov(),
        Some(NOW + 200),
        vec!["ff".into(), "resolve".into()],
    )
    .expect_err("a second resolve must refuse while a session is open");

    assert_eq!(err.id(), "held/resolving", "{err}");
}

#[test]
fn resolving_with_nothing_held_refuses() {
    let fx = Fixture::new();
    ident(&fx);
    fx.write("f.txt", "one\n");
    fx.commit("base");
    fx.git(&["switch", "-q", "-c", "feature"]);
    fx.write("f.txt", "two\n");
    fx.commit("f1");
    fx.git(&["switch", "-q", "feature"]);

    let repo = fx.repo();
    let err = ff_core::resolve::resolve(
        &repo,
        false,
        &prov(),
        Some(NOW),
        vec!["ff".into(), "resolve".into()],
    )
    .expect_err("a resolve with nothing held must refuse");

    assert_eq!(err.id(), "held/none", "{err}");
}

#[test]
fn a_rewrite_that_stopped_conflicting_is_released() {
    let fx = Fixture::new();
    ident(&fx);
    let base = conflict_stack(&fx);
    hold_a_restack(&fx);
    let before = verb_ops(&fx);

    // The world moved: main goes back past the conflicting commit, so f1 now
    // replays cleanly onto it.
    fx.git(&["branch", "-f", "main", &base]);

    let (outcome, _ctx) = resolve_call(&fx, false, NOW + 100);
    let report = match outcome {
        ResolveOutcome::Released(r) => r,
        other => panic!("a now-clean resolve must release, got {other:?}"),
    };

    assert_eq!(report.branch, "feature");
    assert_eq!(report.verb, "restack");
    assert_eq!(
        ff_core::held::of(&fx.repo(), "feature").unwrap(),
        None,
        "the released hold is cleared"
    );
    assert_eq!(
        verb_ops(&fx),
        before,
        "releasing a stale cache entry appends no verb op"
    );
}

#[test]
fn abandoning_drops_the_hold() {
    let fx = Fixture::new();
    ident(&fx);
    conflict_stack(&fx);
    hold_a_restack(&fx);
    let before = verb_ops(&fx);

    let (outcome, _ctx) = resolve_call(&fx, true, NOW + 100);
    let report = match outcome {
        ResolveOutcome::Abandoned(r) => r,
        other => panic!("an abandon must drop the hold, got {other:?}"),
    };

    assert_eq!(report.branch, "feature");
    assert!(!report.was_resolving, "no session was open");
    assert_eq!(
        ff_core::held::of(&fx.repo(), "feature").unwrap(),
        None,
        "the abandoned hold is cleared"
    );
    assert_eq!(
        verb_ops(&fx),
        before + 1,
        "an abandon is exactly one verb operation"
    );
    let record = tip_record(&fx.repo());
    assert_eq!(record.verb, "resolve");
    let transition = record.held.expect("the record carries the hold transition");
    assert_eq!(transition.branch, "feature");
    assert!(transition.old.is_some(), "there was a hold to drop");
    assert_eq!(transition.new, None, "it is gone");
}

#[test]
fn abandoning_an_open_resolution_puts_the_tree_back() {
    let fx = Fixture::new();
    ident(&fx);
    conflict_stack(&fx);
    hold_a_restack(&fx);

    let (outcome, _ctx) = resolve_call(&fx, false, NOW + 100);
    assert!(matches!(outcome, ResolveOutcome::Opened(_)));
    let with_markers = std::fs::read_to_string(fx.path().join("f.txt")).unwrap();
    assert!(with_markers.contains("<<<<<<< the rewrite so far"));

    let (outcome, _ctx) = resolve_call(&fx, true, NOW + 200);
    let report = match outcome {
        ResolveOutcome::Abandoned(r) => r,
        other => panic!("an abandon must drop the session, got {other:?}"),
    };

    assert!(report.was_resolving, "a session was open");
    let restored = std::fs::read_to_string(fx.path().join("f.txt")).unwrap();
    assert_eq!(
        restored, "two\n",
        "the file is back to the tip's content: {restored}"
    );
    assert!(
        !restored.contains("<<<<<<< the rewrite so far"),
        "the markers are gone"
    );
    assert_eq!(
        ff_core::held::of(&fx.repo(), "feature").unwrap(),
        None,
        "the hold is cleared"
    );
    assert_eq!(
        ff_core::held::resolving(&fx.repo(), "feature").unwrap(),
        None,
        "the session is closed"
    );
}

#[test]
fn undoing_a_resolution_closes_it() {
    let fx = Fixture::new();
    ident(&fx);
    conflict_stack(&fx);
    hold_a_restack(&fx);
    let held_before = ff_core::held::of(&fx.repo(), "feature").unwrap().unwrap();

    let (outcome, _ctx) = resolve_call(&fx, false, NOW + 100);
    assert!(matches!(outcome, ResolveOutcome::Opened(_)));
    assert!(
        ff_core::held::resolving(&fx.repo(), "feature")
            .unwrap()
            .is_some(),
        "the session is open before the undo"
    );

    let opts = ff_core::RewindOptions {
        force: false,
        now: Some(NOW + 200),
        argv: vec!["ff".into(), "undo".into()],
    };
    ff_core::undo(&fx.repo(), &opts, &prov()).unwrap();

    assert_eq!(
        ff_core::held::resolving(&fx.repo(), "feature").unwrap(),
        None,
        "the undo closes the session"
    );
    assert_eq!(
        ff_core::held::of(&fx.repo(), "feature").unwrap(),
        Some(held_before),
        "the undo restores the hold"
    );
    let restored = std::fs::read_to_string(fx.path().join("f.txt")).unwrap();
    assert!(
        !restored.contains("<<<<<<< the rewrite so far"),
        "the markers are gone after the undo: {restored}"
    );
}

#[test]
fn a_dirty_tree_is_parked_before_the_markers_go_in() {
    let fx = Fixture::new();
    ident(&fx);
    fx.write("f.txt", "one\n");
    fx.write("other.txt", "orig\n");
    let _base = fx.commit("base");
    fx.git(&["switch", "-q", "-c", "feature"]);
    fx.write("f.txt", "two\n");
    let _f1 = fx.commit("f1");
    fx.git(&["switch", "-q", "main"]);
    fx.write("f.txt", "three\n");
    let _m1 = fx.commit("m1");
    fx.git(&["switch", "-q", "feature"]);
    hold_a_restack(&fx);

    // An open change in an unrelated file.
    fx.write("other.txt", "dirty\n");

    let (outcome, _ctx) = resolve_call(&fx, false, NOW + 100);
    let report = match outcome {
        ResolveOutcome::Opened(r) => r,
        other => panic!("a conflicting resolve must open, got {other:?}"),
    };

    assert!(
        report.parked.is_some(),
        "the dirty change must be parked to make room"
    );
    let restored = std::fs::read_to_string(fx.path().join("other.txt")).unwrap();
    assert_eq!(
        restored, "orig\n",
        "the dirty file is back to its committed content: {restored}"
    );
}

#[test]
fn a_tangled_chain_resolves_what_it_can() {
    let fx = Fixture::new();
    ident(&fx);
    // Two commits on `feature` both rewrite the same line of `f.txt`, and
    // `main` rewrites it a third way: the first replay conflicts, and the
    // second lands on the same region — a tangle, so the chain stops there.
    fx.write("f.txt", "one\nrest\n");
    let _base = fx.commit("base");
    fx.git(&["switch", "-q", "-c", "feature"]);
    fx.write("f.txt", "F1\nrest\n");
    let _f1 = fx.commit("f1");
    fx.write("f.txt", "F2\nrest\n");
    let _f2 = fx.commit("f2");
    fx.git(&["switch", "-q", "main"]);
    fx.write("f.txt", "M\nrest\n");
    let _m1 = fx.commit("m1");
    fx.git(&["switch", "-q", "feature"]);
    hold_a_restack(&fx);

    let (outcome, _ctx) = resolve_call(&fx, false, NOW + 100);
    let report = match outcome {
        ResolveOutcome::Opened(r) => r,
        other => panic!("a tangled resolve must still open, got {other:?}"),
    };

    assert_eq!(
        report.tangled.as_deref(),
        Some("f2"),
        "the report names the commit the chain stopped before"
    );
    assert!(
        report.steps < report.of,
        "the chain stopped short of the whole stack: steps={:?} of={:?}",
        report.steps,
        report.of
    );
    assert_eq!(report.of, 2, "both commits are part of the stack");

    // Only the first conflict's markers made it into the tree.
    let on_disk = std::fs::read_to_string(fx.path().join("f.txt")).unwrap();
    assert!(
        on_disk.contains("<<<<<<< the rewrite so far"),
        "the first conflict's opener is present: {on_disk}"
    );
}
