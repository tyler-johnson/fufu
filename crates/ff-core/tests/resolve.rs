//! Contract for `ff resolve` — the verb that deals with a held rewrite. It
//! materializes every surviving conflict region into the working tree at once,
//! as labeled markers, in one session; releases the hold when the world moved
//! out of the conflict; and, under `--abandon`, drops the hold and the session
//! together. Opening is a success, so no outcome here moves a branch ref, except
//! a restack held at the open change, which lands in place.

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
fn resolving_a_held_restack_opens_a_session_branch_carrying_the_markers() {
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
    let session = report.session.clone();
    assert!(
        ff_core::branch::is_anonymous(&session),
        "the session is an anonymous branch: {session}"
    );
    assert_eq!(
        fx.git(&["symbolic-ref", "--short", "HEAD"]).trim(),
        session,
        "HEAD moved onto it"
    );
    // Its tip is one commit over the branch's tip, carrying the marker tree.
    let marker_commit = fx.git(&["rev-parse", &session]).trim().to_string();
    assert_eq!(
        fx.git(&["rev-parse", &format!("{session}~1")]).trim(),
        fx.git(&["rev-parse", "feature"]).trim()
    );
    let open = ff_core::held::resolving(&fx.repo(), "feature")
        .unwrap()
        .expect("the resolution is recorded on the held branch");
    assert_eq!(open.session, session, "and it names the session");
    assert_eq!(
        fx.git(&["rev-parse", &format!("{marker_commit}^{{tree}}")])
            .trim(),
        open.from,
        "the commit's tree is the marker tree"
    );
    assert_eq!(
        ff_core::branchmeta::read(&fx.repo(), &session)
            .unwrap()
            .session,
        Some(ff_core::branchmeta::Session {
            onto: "feature".into(),
            at: marker_commit,
        }),
        "the session's own metadata points back at the held branch"
    );
    assert_eq!(
        ff_core::held::session_of(&fx.repo(), &session)
            .unwrap()
            .map(|(onto, _)| onto),
        Some("feature".to_string())
    );
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

    // A resolve mints a session; it moves no branch ref that existed.
    let after: Vec<(String, String)> = head_refs(&fx)
        .into_iter()
        .filter(|(name, _)| !name.ends_with(&session))
        .collect();
    assert_eq!(after, before, "a resolve moves no existing branch ref");
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
    assert!(report.returned, "HEAD came back from the session");
    let session = report
        .session
        .clone()
        .expect("the session branch was deleted");
    assert_eq!(
        fx.git(&["symbolic-ref", "--short", "HEAD"]).trim(),
        "feature"
    );
    assert!(
        !head_refs(&fx)
            .iter()
            .any(|(name, _)| name.ends_with(&session)),
        "the session branch is gone"
    );
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
fn abandoning_from_the_held_branch_deletes_the_session_elsewhere() {
    let fx = Fixture::new();
    ident(&fx);
    conflict_stack(&fx);
    hold_a_restack(&fx);

    let session = match resolve_call(&fx, false, NOW + 100).0 {
        ResolveOutcome::Opened(r) => r.session,
        other => panic!("a held restack opens, got {other:?}"),
    };
    let repo = fx.repo();
    ff_core::switch(
        &repo,
        &ff_core::SwitchOptions {
            target: Some("feature".into()),
            now: Some(NOW + 150),
            argv: vec!["ff".into(), "switch".into(), "feature".into()],
            ..Default::default()
        },
        &prov(),
    )
    .unwrap();
    drop(repo);

    let (outcome, _ctx) = resolve_call(&fx, true, NOW + 200);
    let report = match outcome {
        ResolveOutcome::Abandoned(r) => r,
        other => panic!("an abandon must drop the session, got {other:?}"),
    };

    assert!(report.was_resolving);
    assert!(!report.returned, "HEAD was already on feature");
    assert_eq!(report.session.as_deref(), Some(session.as_str()));
    assert_eq!(
        fx.git(&["symbolic-ref", "--short", "HEAD"]).trim(),
        "feature"
    );
    assert!(
        !head_refs(&fx)
            .iter()
            .any(|(name, _)| name.ends_with(&session)),
        "the session branch is gone"
    );
    let repo = fx.repo();
    assert_eq!(ff_core::held::of(&repo, "feature").unwrap(), None);
    assert_eq!(ff_core::held::resolving(&repo, "feature").unwrap(), None);
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

    // Two operations opened it — the mint and the switch — so two undos take
    // it back: the first returns to the branch, the second unmints.
    let session = fx
        .git(&["symbolic-ref", "--short", "HEAD"])
        .trim()
        .to_string();
    let undo = |now: i64| {
        let opts = ff_core::RewindOptions {
            force: false,
            now: Some(now),
            argv: vec!["ff".into(), "undo".into()],
        };
        ff_core::undo(&fx.repo(), &opts, &prov()).unwrap();
    };
    undo(NOW + 200);
    assert_eq!(
        fx.git(&["symbolic-ref", "--short", "HEAD"]).trim(),
        "feature"
    );
    assert!(
        ff_core::held::resolving(&fx.repo(), "feature")
            .unwrap()
            .is_some(),
        "one undo only takes the switch back; the session still stands"
    );
    undo(NOW + 300);

    assert_eq!(
        ff_core::held::resolving(&fx.repo(), "feature").unwrap(),
        None,
        "the second undo closes the session"
    );
    assert!(
        !head_refs(&fx)
            .iter()
            .any(|(name, _)| name.ends_with(&session)),
        "and unmints its branch"
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

    let parked = report
        .parked
        .clone()
        .expect("the dirty change must be parked to make room");
    assert_eq!(
        fx.git(&["rev-parse", "refs/fufu/open/feature"]).trim(),
        parked,
        "the park is feature's open commit"
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

// ---- A restack held at the open change: landed in place, no session ----

/// `feature` has no commits of its own, and its uncommitted edit conflicts
/// with main's new work. Leaves the fixture standing on `feature`.
fn open_change_behind(fx: &Fixture) {
    fx.write("f.txt", "one\n");
    fx.commit("base");
    fx.git(&["switch", "-q", "-c", "feature"]);
    fx.git(&["switch", "-q", "main"]);
    fx.write("f.txt", "three\n");
    fx.commit("m1");
    fx.git(&["switch", "-q", "feature"]);
    fx.write("f.txt", "mine\n");
}

/// `open_change_behind`, with one commit of the branch's own that replays
/// cleanly.
fn open_change_over_a_commit(fx: &Fixture) {
    fx.write("f.txt", "one\n");
    fx.commit("base");
    fx.git(&["switch", "-q", "-c", "feature"]);
    fx.write("own.txt", "own\n");
    fx.commit("f1");
    fx.git(&["switch", "-q", "main"]);
    fx.write("f.txt", "three\n");
    fx.commit("m1");
    fx.git(&["switch", "-q", "feature"]);
    fx.write("f.txt", "mine\n");
}

fn rev(fx: &Fixture, name: &str) -> String {
    fx.git(&["rev-parse", name]).trim().to_string()
}

fn read(fx: &Fixture, path: &str) -> String {
    std::fs::read_to_string(fx.path().join(path)).unwrap()
}

/// Both sides of `f.txt`'s conflict stand in one labeled marker block.
fn assert_marked(fx: &Fixture) {
    let text = read(fx, "f.txt");
    assert!(text.contains("<<<<<<< the rewrite so far (1/1)"), "{text}");
    assert!(text.contains("three\n=======\nmine\n"), "{text}");
    assert!(
        text.contains(">>>>>>> rebasing \"your open change\" (1/1)"),
        "{text}"
    );
}

fn carried(outcome: ResolveOutcome) -> ff_core::CarriedReport {
    match outcome {
        ResolveOutcome::Carried(report) => report,
        other => panic!("expected the landing in place, got {other:?}"),
    }
}

#[test]
fn a_fast_forward_held_at_the_open_change_lands_with_markers() {
    let fx = Fixture::new();
    ident(&fx);
    open_change_behind(&fx);
    hold_a_restack(&fx);
    let ops = verb_ops(&fx);

    let (outcome, _ctx) = resolve_call(&fx, false, NOW + 100);
    let report = carried(outcome);

    assert_eq!(report.paths, vec!["f.txt".to_string()]);
    assert!(report.restack.fast_forward);
    assert!(report.restack.still_open);
    assert_eq!(report.restack.new_tip, rev(&fx, "main"));
    assert_eq!(rev(&fx, "feature"), rev(&fx, "main"), "the branch moved");
    assert_marked(&fx);

    assert_eq!(ff_core::held::of(&fx.repo(), "feature").unwrap(), None);
    assert_eq!(
        ff_core::held::resolving(&fx.repo(), "feature").unwrap(),
        None
    );
    assert_eq!(
        fx.git(&["symbolic-ref", "--short", "HEAD"]).trim(),
        "feature",
        "HEAD stays on the branch"
    );
    let names: Vec<String> = head_refs(&fx).into_iter().map(|(name, _)| name).collect();
    assert_eq!(
        names,
        vec![
            "refs/heads/feature".to_string(),
            "refs/heads/main".to_string()
        ],
        "no session branch"
    );
    assert_eq!(verb_ops(&fx), ops + 1, "one operation");
    assert_eq!(
        fx.git(&["status", "--porcelain"]).trim_end(),
        " M f.txt",
        "the change is open against the new tip, with nothing staged"
    );
}

/// The hold aimed at the base the branch already has, so nothing re-aims it,
/// though the landing names that base outright.
#[test]
fn landing_on_the_standing_base_records_no_parent() {
    let fx = Fixture::new();
    ident(&fx);
    open_change_behind(&fx);
    hold_a_restack(&fx);

    let (outcome, _ctx) = resolve_call(&fx, false, NOW + 100);
    let report = carried(outcome);

    assert!(!report.restack.reaimed);
    assert_eq!(
        ff_core::branchmeta::read(&fx.repo(), "feature")
            .unwrap()
            .parent,
        None
    );
}

/// `ff restack --onto` asked for a new base, and the hold stood in for that
/// request: landing it records the base it named.
#[test]
fn a_held_re_aim_lands_as_a_re_aim() {
    let fx = Fixture::new();
    ident(&fx);
    fx.write("f.txt", "one\n");
    fx.commit("base");
    fx.git(&["branch", "other"]);
    fx.git(&["switch", "-q", "-c", "feature"]);
    fx.git(&["switch", "-q", "other"]);
    fx.write("f.txt", "three\n");
    fx.commit("o1");
    fx.git(&["switch", "-q", "feature"]);
    fx.write("f.txt", "mine\n");
    let (outcome, _ctx) = ff_core::restack::restack(
        &fx.repo(),
        None,
        Some("other".into()),
        &prov(),
        Some(NOW),
        vec!["ff".into(), "restack".into()],
    )
    .unwrap();
    assert!(matches!(outcome, ff_core::RestackOutcome::Held(_)));

    let (outcome, _ctx) = resolve_call(&fx, false, NOW + 100);
    let report = carried(outcome);

    assert!(report.restack.reaimed);
    assert_eq!(rev(&fx, "feature"), rev(&fx, "other"));
    assert_eq!(
        ff_core::branchmeta::read(&fx.repo(), "feature")
            .unwrap()
            .parent
            .as_deref(),
        Some("other")
    );
}

#[test]
fn commits_replay_under_a_hold_at_the_open_change() {
    let fx = Fixture::new();
    ident(&fx);
    open_change_over_a_commit(&fx);
    hold_a_restack(&fx);
    let old_tip = rev(&fx, "feature");

    let (outcome, _ctx) = resolve_call(&fx, false, NOW + 100);
    let report = carried(outcome);

    assert_eq!(report.paths, vec!["f.txt".to_string()]);
    assert_eq!(report.restack.replayed, 1);
    assert!(!report.restack.fast_forward);
    assert_ne!(rev(&fx, "feature"), old_tip, "the commit was replayed");
    assert_eq!(rev(&fx, "feature~1"), rev(&fx, "main"));
    assert_eq!(read(&fx, "own.txt"), "own\n");
    assert_marked(&fx);
    assert_eq!(ff_core::held::of(&fx.repo(), "feature").unwrap(), None);
}

/// The open change was committed after the hold was recorded, so a commit
/// conflicts now, and a commit is what a session is for.
#[test]
fn commits_that_conflict_since_the_hold_open_a_session() {
    let fx = Fixture::new();
    ident(&fx);
    open_change_behind(&fx);
    hold_a_restack(&fx);
    fx.commit("the change, committed");

    let (outcome, _ctx) = resolve_call(&fx, false, NOW + 100);

    assert!(
        matches!(outcome, ResolveOutcome::Opened(_)),
        "got {outcome:?}"
    );
    assert!(
        ff_core::held::resolving(&fx.repo(), "feature")
            .unwrap()
            .is_some()
    );
}

#[test]
fn a_branch_really_on_its_base_is_still_stale() {
    let fx = Fixture::new();
    ident(&fx);
    open_change_behind(&fx);
    hold_a_restack(&fx);
    fx.git(&["reset", "-q", "--hard", "main"]);

    let err = ff_core::resolve::resolve(&fx.repo(), false, &prov(), Some(NOW + 100), vec![])
        .expect_err("the hold has outlived its meaning");

    assert_eq!(err.id(), "held/expired");
    assert!(err.to_string().contains("already sits on main"), "{err}");
}

#[test]
fn a_base_that_is_gone_is_still_stale() {
    let fx = Fixture::new();
    ident(&fx);
    fx.write("f.txt", "one\n");
    fx.commit("base");
    fx.git(&["switch", "-q", "-c", "other"]);
    fx.write("f.txt", "three\n");
    fx.commit("o1");
    fx.git(&["switch", "-q", "-c", "feature", "main"]);
    fx.write("f.txt", "mine\n");
    let (outcome, _ctx) = ff_core::restack::restack(
        &fx.repo(),
        None,
        Some("other".into()),
        &prov(),
        Some(NOW),
        vec!["ff".into(), "restack".into()],
    )
    .unwrap();
    assert!(matches!(outcome, ff_core::RestackOutcome::Held(_)));
    fx.git(&["branch", "-q", "-D", "other"]);

    let err = ff_core::resolve::resolve(&fx.repo(), false, &prov(), Some(NOW + 100), vec![])
        .expect_err("the base it aimed at is gone");

    assert_eq!(err.id(), "held/expired");
}

/// The conflict went away, and the branch still has to move: it lands, with
/// no re-run of a verb to wait for.
#[test]
fn an_open_change_that_stopped_conflicting_lands_clean() {
    let fx = Fixture::new();
    ident(&fx);
    open_change_behind(&fx);
    hold_a_restack(&fx);
    fx.write("f.txt", "one\n");
    fx.write("new.txt", "new\n");

    let (outcome, _ctx) = resolve_call(&fx, false, NOW + 100);
    let report = carried(outcome);

    assert!(report.paths.is_empty());
    assert!(report.restack.still_open);
    assert_eq!(rev(&fx, "feature"), rev(&fx, "main"));
    assert_eq!(read(&fx, "f.txt"), "three\n");
    assert_eq!(read(&fx, "new.txt"), "new\n");
    assert_eq!(ff_core::held::of(&fx.repo(), "feature").unwrap(), None);
}

#[test]
fn one_undo_takes_the_landing_back() {
    let fx = Fixture::new();
    ident(&fx);
    open_change_behind(&fx);
    hold_a_restack(&fx);
    let tip = rev(&fx, "feature");
    let held = ff_core::held::of(&fx.repo(), "feature").unwrap();
    assert!(held.is_some());

    let (outcome, _ctx) = resolve_call(&fx, false, NOW + 100);
    carried(outcome);
    let opts = ff_core::RewindOptions {
        force: false,
        now: Some(NOW + 200),
        argv: vec!["ff".into(), "undo".into()],
    };
    ff_core::undo(&fx.repo(), &opts, &prov()).unwrap();

    assert_eq!(rev(&fx, "feature"), tip);
    assert_eq!(read(&fx, "f.txt"), "mine\n");
    assert_eq!(ff_core::held::of(&fx.repo(), "feature").unwrap(), held);
}

/// The landing's record names what one undo reads: the hold it cleared.
#[test]
fn the_landing_is_a_restack_that_cleared_the_hold() {
    let fx = Fixture::new();
    ident(&fx);
    open_change_behind(&fx);
    hold_a_restack(&fx);
    let held = ff_core::held::of(&fx.repo(), "feature").unwrap();

    let (outcome, _ctx) = resolve_call(&fx, false, NOW + 100);
    carried(outcome);

    let record = tip_record(&fx.repo());
    assert_eq!(record.verb, "restack");
    let transition = record.held.expect("the op carries the hold");
    assert_eq!(transition.old, held);
    assert_eq!(transition.new, None);
}

/// A cascade held the branch you stand on: the landing moves that branch and
/// not the one below, which has already landed.
#[test]
fn a_hold_on_a_cascade_child_lands_on_that_branch() {
    let fx = Fixture::new();
    ident(&fx);
    fx.write("f.txt", "one\n");
    fx.commit("base");
    fx.git(&["switch", "-q", "-c", "feature"]);
    fx.write("a.txt", "a\n");
    fx.commit("f1");
    fx.git(&["switch", "-q", "-c", "child"]);
    fx.write("x.txt", "x\n");
    fx.commit("x1");
    {
        let repo = fx.repo();
        let mut meta = ff_core::branchmeta::read(&repo, "child").unwrap();
        meta.parent = Some("feature".into());
        ff_core::branchmeta::write(&repo, "child", &meta).unwrap();
    }
    fx.git(&["switch", "-q", "main"]);
    fx.write("f.txt", "three\n");
    fx.commit("m1");
    fx.git(&["switch", "-q", "child"]);
    fx.write("f.txt", "mine\n");
    let (outcome, _ctx) = ff_core::restack::restack(
        &fx.repo(),
        Some("feature".into()),
        None,
        &prov(),
        Some(NOW),
        vec!["ff".into(), "restack".into()],
    )
    .unwrap();
    let ff_core::RestackOutcome::Restacked(landed) = outcome else {
        panic!("feature lands: {outcome:?}");
    };
    assert_eq!(landed.cascade.held.len(), 1, "{:?}", landed.cascade);
    let feature = rev(&fx, "feature");

    let (outcome, _ctx) = resolve_call(&fx, false, NOW + 100);
    let report = carried(outcome);

    assert_eq!(report.restack.branch, "child");
    assert_eq!(report.restack.replayed, 1);
    assert_eq!(report.paths, vec!["f.txt".to_string()]);
    assert_eq!(rev(&fx, "feature"), feature, "the branch below stays put");
    assert_eq!(rev(&fx, "child~1"), feature, "child sits on it now");
    assert_marked(&fx);
    assert_eq!(ff_core::held::of(&fx.repo(), "child").unwrap(), None);
}
