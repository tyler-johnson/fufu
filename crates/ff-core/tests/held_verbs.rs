//! Contract for the rewrite verbs when their replay conflicts — `restack`,
//! `absorb`, `lift` and `done` alike: each holds rather than refuses,
//! recording a pending intent on the branch and logging a slim operation
//! that `ff undo` can take back, with nothing written and no ref moved. One
//! hold per branch: a second conflicting rewrite refuses rather than guessing
//! an order, while a clean rewrite on another branch runs on by.

use ff_core::absorb::{Endpoint, MoveOptions, MoveVerb};
use ff_core::futures::At;
use ff_core::gix;
use ff_core::{DoneOutcome, EditOutcome, MoveOutcome, Provenance, RestackOutcome};
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
/// the namespace a restack is allowed to move.
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

fn oid(hex: &str) -> gix::ObjectId {
    gix::ObjectId::from_hex(hex.trim().as_bytes()).unwrap()
}

fn absorb_call(fx: &Fixture, into: &str, now: i64) -> (MoveOutcome, ff_core::ops::VerbContext) {
    let repo = fx.repo();
    ff_core::absorb::move_change(
        &repo,
        &MoveOptions {
            verb: MoveVerb::Absorb,
            from: None,
            into: Some(Endpoint::Commit(oid(into))),
            paths: Vec::new(),
            message: None,
            verify: ff_core::Verify::Run,
            now: Some(now),
            argv: vec!["ff".into(), "absorb".into()],
        },
        &prov(),
    )
    .unwrap()
}

fn lift_call(
    fx: &Fixture,
    from: &str,
    paths: Vec<String>,
    now: i64,
) -> (MoveOutcome, ff_core::ops::VerbContext) {
    let repo = fx.repo();
    ff_core::absorb::move_change(
        &repo,
        &MoveOptions {
            verb: MoveVerb::Lift,
            from: Some(vec![Endpoint::Commit(oid(from))]),
            into: None,
            paths,
            message: None,
            verify: ff_core::Verify::Run,
            now: Some(now),
            argv: vec!["ff".into(), "lift".into()],
        },
        &prov(),
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

/// A stack whose open change folds cleanly into `c1`, but whose descendant
/// `c2` cannot replay over the fold: `c1` sets line 1 to `A`, `c2` to `B`,
/// `c3` reverts it to `A` (so the fold's base still reads `A`), and the open
/// change sets it to `C` — `c2`'s `B` collides with the folded `C`.
fn descendant_conflict_stack(fx: &Fixture) -> (String, String) {
    fx.write("f.txt", "start\n");
    let _c0 = fx.commit("base");
    fx.write("f.txt", "A\n");
    let c1 = fx.commit("c1");
    fx.write("f.txt", "B\n");
    let c2 = fx.commit("c2");
    fx.write("f.txt", "A\n");
    let _c3 = fx.commit("c3");
    // The open change folds cleanly into c1 (base and c1 both read A) but
    // lands C, which c2's B cannot replay over.
    fx.write("f.txt", "C\n");
    (c1, c2)
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

/// A session on `c1` whose amend the commits ahead cannot replay over:
/// `shared.txt` is rewritten by `c1`, then `c2`, then again inside the
/// session. Returns the session branch, the anchor, the conflicting
/// descendant, and `main`'s tip.
fn done_conflict_stack(fx: &Fixture) -> (String, String, String, String) {
    fx.write("shared.txt", "line1\nbase\nline3\n");
    fx.write("c0.txt", "c0\n");
    let _c0 = fx.commit("c0");
    fx.write("shared.txt", "line1\nc1-edit\nline3\n");
    let c1 = fx.commit("c1");
    fx.write("shared.txt", "line1\nc2-edit\nline3\n");
    let c2 = fx.commit("c2");
    fx.write("c3.txt", "c3\n");
    let c3 = fx.commit("c3");
    fx.git(&["branch", "mid", &c1]);

    let (edit_outcome, _ctx) = edit_call(fx, &c1, NOW);
    let session = match edit_outcome {
        EditOutcome::Opened(r) => r.session,
        other => panic!("a session must open, got {other:?}"),
    };
    fx.write("shared.txt", "line1\nsession-edit\nline3\n");
    (session, c1, c2, c3)
}

#[test]
fn a_conflicting_restack_holds_instead_of_refusing() {
    let fx = Fixture::new();
    ident(&fx);
    let (f1, _m1) = conflict_stack(&fx);
    let before = head_refs(&fx);

    let (outcome, _ctx) = restack_call(&fx, None, None, NOW);
    let report = match outcome {
        RestackOutcome::Held(r) => r,
        other => panic!("a conflicting restack must hold, got {other:?}"),
    };

    assert_eq!(report.verb, "restack");
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

    assert_eq!(head_refs(&fx), before, "a hold moves no ref");
}

#[test]
fn a_hold_is_written_where_status_will_find_it() {
    let fx = Fixture::new();
    ident(&fx);
    conflict_stack(&fx);

    let (outcome, _ctx) = restack_call(&fx, None, None, NOW);
    let report = match outcome {
        RestackOutcome::Held(r) => r,
        other => panic!("a conflicting restack must hold, got {other:?}"),
    };

    let held = ff_core::held::of(&fx.repo(), "feature")
        .unwrap()
        .expect("the hold must stand on the restacked branch");
    match &held.intent {
        ff_core::held::Intent::Restack { branch, onto } => {
            assert_eq!(branch, "feature");
            assert_eq!(
                onto, "refs/heads/main",
                "onto records the full ref, not the sha it stood on when held"
            );
        }
        other => panic!("the intent must be Restack, got {other:?}"),
    }
    assert_eq!(held.paths, report.paths);
}

#[test]
fn a_hold_is_an_operation() {
    let fx = Fixture::new();
    ident(&fx);
    conflict_stack(&fx);

    // Prime the log so the bootstrap op cannot be mistaken for the hold's.
    ff_core::ops::reconcile(&fx.repo(), NOW).unwrap();
    let before = verb_ops(&fx);

    let (outcome, _ctx) = restack_call(&fx, None, None, NOW);
    assert!(matches!(outcome, RestackOutcome::Held(_)));

    assert_eq!(
        verb_ops(&fx),
        before + 1,
        "a hold is exactly one verb operation, preamble and all"
    );
    let record = tip_record(&fx.repo());
    assert_eq!(record.verb, "hold");
    let transition = record.held.expect("the record carries the hold transition");
    assert_eq!(transition.branch, "feature");
    assert_eq!(transition.old, None, "no hold stood before this one");
    assert_eq!(
        transition.new,
        ff_core::held::of(&fx.repo(), "feature").unwrap(),
        "the operation and the metadata agree on the hold"
    );
}

#[test]
fn undoing_a_hold_removes_it() {
    let fx = Fixture::new();
    ident(&fx);
    conflict_stack(&fx);

    let (outcome, _ctx) = restack_call(&fx, None, None, NOW);
    assert!(matches!(outcome, RestackOutcome::Held(_)));
    assert!(
        ff_core::held::of(&fx.repo(), "feature").unwrap().is_some(),
        "the hold stands before the undo"
    );

    let opts = ff_core::RewindOptions {
        force: false,
        now: Some(NOW + 100),
        argv: vec!["ff".into(), "undo".into()],
    };
    ff_core::undo(&fx.repo(), &opts, &prov()).unwrap();

    assert_eq!(
        ff_core::held::of(&fx.repo(), "feature").unwrap(),
        None,
        "undoing the hold's operation restores its absence"
    );
}

#[test]
fn a_second_conflicting_rewrite_refuses_while_one_is_held() {
    let fx = Fixture::new();
    ident(&fx);
    conflict_stack(&fx);

    let (outcome, _ctx) = restack_call(&fx, None, None, NOW);
    assert!(matches!(outcome, RestackOutcome::Held(_)));
    let first = ff_core::held::of(&fx.repo(), "feature").unwrap();

    let repo = fx.repo();
    let err = ff_core::restack::restack(
        &repo,
        None,
        None,
        &prov(),
        Some(NOW + 100),
        vec!["ff".into(), "restack".into()],
    )
    .expect_err("a second conflicting rewrite must refuse while one is held");

    assert_eq!(err.id(), "held/already-held", "{err}");
    assert_eq!(
        ff_core::held::of(&repo, "feature").unwrap(),
        first,
        "the refusal leaves the first hold untouched"
    );
}

#[test]
fn a_clean_rewrite_runs_while_one_is_held() {
    let fx = Fixture::new();
    ident(&fx);
    // feature and other both fork from the root commit and touch different
    // files; main then adds f.txt, so feature's replay of it conflicts and
    // other's — a file of its own — cannot.
    fx.write("root.txt", "root\n");
    let _c0 = fx.commit("root");
    fx.git(&["switch", "-q", "-c", "feature"]);
    fx.write("f.txt", "two\n");
    let _f1 = fx.commit("f1");
    fx.git(&["switch", "-q", "main"]);
    fx.git(&["switch", "-q", "-c", "other"]);
    fx.write("o.txt", "o\n");
    let _o1 = fx.commit("o1");
    fx.git(&["switch", "-q", "main"]);
    fx.write("f.txt", "three\n");
    let _m1 = fx.commit("m1");
    fx.git(&["switch", "-q", "feature"]);

    let (outcome, _ctx) = restack_call(&fx, None, None, NOW);
    assert!(matches!(outcome, RestackOutcome::Held(_)));
    let first = ff_core::held::of(&fx.repo(), "feature").unwrap();

    let (outcome, _ctx) = restack_call(&fx, Some("other"), None, NOW + 100);
    let report = match outcome {
        RestackOutcome::Restacked(r) => r,
        other => {
            panic!("a clean restack must land while one holds on another branch, got {other:?}")
        }
    };
    assert_eq!(report.branch, "other");
    assert_eq!(report.replayed, 1);

    assert_eq!(
        ff_core::held::of(&fx.repo(), "feature").unwrap(),
        first,
        "the hold stands untouched"
    );
}

#[test]
fn a_clean_restack_still_lands() {
    let fx = Fixture::new();
    ident(&fx);
    fx.write("root.txt", "root\n");
    let _c0 = fx.commit("root");
    fx.write("m.txt", "m\n");
    let _c1 = fx.commit("m");
    fx.git(&["switch", "-q", "-c", "feature"]);
    fx.write("a.txt", "a\n");
    let _f1 = fx.commit("f1");
    fx.git(&["switch", "-q", "main"]);
    fx.write("d.txt", "d\n");
    let _m2 = fx.commit("m2");
    fx.git(&["switch", "-q", "feature"]);

    let (outcome, _ctx) = restack_call(&fx, None, None, NOW);
    let report = match outcome {
        RestackOutcome::Restacked(r) => r,
        other => panic!("a clean restack must land, got {other:?}"),
    };
    assert_eq!(report.branch, "feature");
    assert_eq!(report.replayed, 1);
}

#[test]
fn a_conflicting_absorb_holds() {
    let fx = Fixture::new();
    ident(&fx);
    let c1 = fold_conflict_stack(&fx);
    let before = head_refs(&fx);

    let (outcome, _ctx) = absorb_call(&fx, &c1, NOW);
    let report = match outcome {
        MoveOutcome::Held(r) => r,
        other => panic!("a conflicting absorb must hold, got {other:?}"),
    };

    assert_eq!(report.verb, "absorb");
    assert_eq!(report.branch, "main");
    assert_eq!(
        report.at,
        At::OpenChange,
        "the fold cannot apply the open change to the target"
    );
    assert!(
        report.paths.iter().any(|p| p == "f.txt"),
        "the report must name the conflicting path: {:?}",
        report.paths
    );

    assert_eq!(head_refs(&fx), before, "a hold moves no ref");

    let held = ff_core::held::of(&fx.repo(), "main")
        .unwrap()
        .expect("the hold must stand on the branch underfoot");
    match &held.intent {
        ff_core::held::Intent::Absorb { into, .. } => {
            assert_eq!(into, &c1, "the intent names the target");
        }
        other => panic!("the intent must be Absorb, got {other:?}"),
    }
}

#[test]
fn a_conflicting_absorb_of_descendants_holds() {
    let fx = Fixture::new();
    ident(&fx);
    let (c1, c2) = descendant_conflict_stack(&fx);
    let before = head_refs(&fx);

    let (outcome, _ctx) = absorb_call(&fx, &c1, NOW);
    let report = match outcome {
        MoveOutcome::Held(r) => r,
        other => panic!("a conflicting absorb must hold, got {other:?}"),
    };

    assert_eq!(report.verb, "absorb");
    assert_eq!(
        report.at,
        At::Commit {
            id: c2.clone(),
            subject: "c2".into()
        },
        "the report names the descendant that cannot replay"
    );

    assert_eq!(head_refs(&fx), before, "a hold moves no ref");

    let held = ff_core::held::of(&fx.repo(), "main")
        .unwrap()
        .expect("the hold must stand on the branch underfoot");
    match &held.intent {
        ff_core::held::Intent::Absorb { into, .. } => {
            assert_eq!(into, &c1, "the intent names the target");
        }
        other => panic!("the intent must be Absorb, got {other:?}"),
    }
}

#[test]
fn a_conflicting_lift_holds() {
    let fx = Fixture::new();
    ident(&fx);
    let (c1, _c2) = lift_conflict_stack(&fx);
    let before = head_refs(&fx);

    let (outcome, _ctx) = lift_call(&fx, &c1, vec!["doc.txt".into()], NOW);
    let report = match outcome {
        MoveOutcome::Held(r) => r,
        other => panic!("a conflicting lift must hold, got {other:?}"),
    };

    assert_eq!(report.verb, "lift");
    assert_eq!(report.branch, "main");

    assert_eq!(head_refs(&fx), before, "a hold moves no ref");

    let held = ff_core::held::of(&fx.repo(), "main")
        .unwrap()
        .expect("the hold must stand on the branch underfoot");
    match &held.intent {
        ff_core::held::Intent::Lift { from, paths, .. } => {
            assert_eq!(from, &vec![c1.clone()], "the intent names the source");
            assert_eq!(
                paths,
                &vec!["doc.txt".to_string()],
                "the paths recorded are the ones lifted"
            );
        }
        other => panic!("the intent must be Lift, got {other:?}"),
    }
}

#[test]
fn a_conflicting_done_holds() {
    let fx = Fixture::new();
    ident(&fx);
    let (session, _c1, c2, _c3) = done_conflict_stack(&fx);
    let before = head_refs(&fx);

    let (outcome, _ctx) = done_call(&fx, NOW + 100);
    let report = match outcome {
        DoneOutcome::Held(r) => r,
        other => panic!("a conflicting done must hold, got {other:?}"),
    };

    assert_eq!(report.verb, "done");
    assert_eq!(report.branch, session);
    assert_eq!(
        report.at,
        At::Commit {
            id: c2.clone(),
            subject: "c2".into()
        },
        "the report names the commit the replay stopped on"
    );

    assert_eq!(head_refs(&fx), before, "a hold moves no ref");

    // The hold stands on the session branch, and the session is still open.
    let held = ff_core::held::of(&fx.repo(), &session)
        .unwrap()
        .expect("the hold must stand on the session branch");
    match &held.intent {
        ff_core::held::Intent::Done { session: s } => {
            assert_eq!(s, &session, "the intent names the session branch");
        }
        other => panic!("the intent must be Done, got {other:?}"),
    }
    assert!(
        ff_core::branchmeta::read(&fx.repo(), &session)
            .unwrap()
            .session
            .is_some(),
        "the session must still be open for ff resolve or a clean retry"
    );
}

#[test]
fn undoing_an_absorb_hold_removes_it() {
    let fx = Fixture::new();
    ident(&fx);
    let c1 = fold_conflict_stack(&fx);

    let (outcome, _ctx) = absorb_call(&fx, &c1, NOW);
    assert!(matches!(outcome, MoveOutcome::Held(_)));
    assert!(
        ff_core::held::of(&fx.repo(), "main").unwrap().is_some(),
        "the hold stands before the undo"
    );

    let opts = ff_core::RewindOptions {
        force: false,
        now: Some(NOW + 100),
        argv: vec!["ff".into(), "undo".into()],
    };
    ff_core::undo(&fx.repo(), &opts, &prov()).unwrap();

    assert_eq!(
        ff_core::held::of(&fx.repo(), "main").unwrap(),
        None,
        "undoing the hold's operation restores its absence"
    );
}

// What a rewrite does to the hold standing on the branch it rewrites, by
// the hold's kind: a held restack is dropped by a re-aim, cleared by a
// replay onto its own base, and kept and remapped otherwise; a hold
// carrying content refuses every rewrite; an open resolution refuses too.

/// `other`, forked from the base commit `feature` and `main` share, with a
/// file of its own — a base a re-aim of `feature` lands on cleanly. Leaves
/// the fixture standing on `feature`.
fn other_off_base(fx: &Fixture) {
    fx.git(&["branch", "other", "main~1"]);
    fx.git(&["switch", "-q", "other"]);
    fx.write("o.txt", "o\n");
    fx.commit("o1");
    fx.git(&["switch", "-q", "feature"]);
}

/// The hold a conflicting restack of `feature` onto `main` records.
fn hold_feature(fx: &Fixture) -> ff_core::held::Held {
    let (outcome, _ctx) = restack_call(fx, None, None, NOW);
    assert!(matches!(outcome, RestackOutcome::Held(_)));
    ff_core::held::of(&fx.repo(), "feature")
        .unwrap()
        .expect("the restack holds")
}

fn undo_call(fx: &Fixture, now: i64) {
    let opts = ff_core::RewindOptions {
        force: false,
        now: Some(now),
        argv: vec!["ff".into(), "undo".into()],
    };
    ff_core::undo(&fx.repo(), &opts, &prov()).unwrap();
}

fn rev(fx: &Fixture, name: &str) -> String {
    fx.git(&["rev-parse", name]).trim().to_string()
}

fn at_id(held: &ff_core::held::Held) -> String {
    match &held.at {
        At::Commit { id, .. } => id.clone(),
        At::OpenChange => panic!("the hold stopped at a commit"),
    }
}

#[test]
fn a_reaim_drops_a_base_following_hold() {
    let fx = Fixture::new();
    ident(&fx);
    let (f1, _m1) = conflict_stack(&fx);
    other_off_base(&fx);
    let first = hold_feature(&fx);

    let (outcome, _ctx) = restack_call(&fx, Some("feature"), Some("other"), NOW + 100);
    let report = match outcome {
        RestackOutcome::Restacked(r) => r,
        other => panic!("a clean re-aim must land, got {other:?}"),
    };
    assert!(report.reaimed);
    assert_eq!(
        report.dropped_hold,
        Some(ff_core::DroppedHold {
            verb: "restack".into(),
            onto: "main".into(),
        }),
        "the report names the hold the re-aim dropped"
    );
    assert_eq!(
        ff_core::held::of(&fx.repo(), "feature").unwrap(),
        None,
        "the hold's question is moot once the branch sits elsewhere"
    );
    assert_eq!(
        tip_record(&fx.repo()).held,
        Some(ff_core::ops::record::HeldTransition {
            branch: "feature".into(),
            old: Some(first.clone()),
            new: None,
        }),
        "the drop rides the restack's own record"
    );

    undo_call(&fx, NOW + 200);
    assert_eq!(rev(&fx, "feature"), f1, "undo puts the tip back");
    assert_eq!(
        ff_core::held::of(&fx.repo(), "feature").unwrap(),
        Some(first),
        "and the hold with it, byte-identical"
    );
}

#[test]
fn an_absorb_under_a_base_following_hold_remaps_it() {
    let fx = Fixture::new();
    ident(&fx);
    let (f1, _m1) = conflict_stack(&fx);
    let first = hold_feature(&fx);
    assert_eq!(at_id(&first), f1);

    fx.write("g.txt", "g\n");
    let (outcome, _ctx) = absorb_call(&fx, &f1, NOW + 100);
    assert!(
        matches!(outcome, MoveOutcome::Moved(_)),
        "a clean absorb lands under a held restack, got {outcome:?}"
    );
    let new_f1 = rev(&fx, "feature");
    assert_ne!(new_f1, f1);

    let held = ff_core::held::of(&fx.repo(), "feature")
        .unwrap()
        .expect("the hold stays");
    assert_eq!(
        held.at,
        At::Commit {
            id: new_f1,
            subject: "f1".into()
        },
        "the hold points at the rewritten commit"
    );
    assert_eq!(held.intent, first.intent, "the intent is untouched");
    assert_eq!(held.paths, first.paths);
    assert_eq!(held.time, first.time);

    // Resolve replans against main from the rewritten tip.
    let repo = fx.repo();
    let (outcome, _ctx) = ff_core::resolve::resolve(
        &repo,
        false,
        &prov(),
        Some(NOW + 200),
        vec!["ff".into(), "resolve".into()],
    )
    .unwrap();
    match outcome {
        ff_core::ResolveOutcome::Opened(r) => assert_eq!(r.branch, "feature"),
        other => panic!("the held restack still conflicts, got {other:?}"),
    }
}

#[test]
fn a_describe_under_a_base_following_hold_remaps_it() {
    let fx = Fixture::new();
    ident(&fx);
    let (f1, _m1) = conflict_stack(&fx);
    hold_feature(&fx);

    let repo = fx.repo();
    let (report, _ctx) = ff_core::describe::reword(
        &repo,
        oid(&f1),
        "f1 reworded".into(),
        ff_core::Verify::Run,
        &prov(),
        Some(NOW + 100),
        vec!["ff".into(), "describe".into()],
    )
    .unwrap();
    assert_ne!(report.new, f1);

    let held = ff_core::held::of(&repo, "feature")
        .unwrap()
        .expect("the hold stays");
    assert_eq!(at_id(&held), report.new, "the hold follows the reword");
}

#[test]
fn a_restack_onto_the_holds_own_base_lands_and_clears_it() {
    let fx = Fixture::new();
    ident(&fx);
    let (f1, _m1) = conflict_stack(&fx);
    let first = hold_feature(&fx);

    // main moves off the commit that conflicted: back to the base, then a
    // commit touching another file, so the replay is clean.
    fx.git(&["switch", "-q", "main"]);
    fx.git(&["reset", "-q", "--hard", "main~1"]);
    fx.write("n.txt", "n\n");
    fx.commit("m2");
    fx.git(&["switch", "-q", "feature"]);

    let (outcome, _ctx) = restack_call(&fx, None, None, NOW + 100);
    let report = match outcome {
        RestackOutcome::Restacked(r) => r,
        other => panic!("the replay is clean now, got {other:?}"),
    };
    assert_eq!(report.replayed, 1);
    assert_eq!(report.dropped_hold, None, "a landing is not a drop");
    assert_eq!(
        ff_core::held::of(&fx.repo(), "feature").unwrap(),
        None,
        "the replay the hold recorded has landed"
    );
    assert_eq!(
        tip_record(&fx.repo()).held,
        Some(ff_core::ops::record::HeldTransition {
            branch: "feature".into(),
            old: Some(first.clone()),
            new: None,
        })
    );

    undo_call(&fx, NOW + 200);
    assert_eq!(rev(&fx, "feature"), f1);
    assert_eq!(
        ff_core::held::of(&fx.repo(), "feature").unwrap(),
        Some(first),
        "undo restores the hold"
    );
}

#[test]
fn a_content_hold_refuses_every_rewrite() {
    let fx = Fixture::new();
    ident(&fx);
    fx.git(&["switch", "-q", "-c", "feature"]);
    let (c1, c2) = lift_conflict_stack(&fx);
    fx.git(&["branch", "other", "feature~2"]);
    fx.git(&["switch", "-q", "other"]);
    fx.write("o.txt", "o\n");
    fx.commit("o1");
    fx.git(&["switch", "-q", "feature"]);

    let (outcome, _ctx) = lift_call(&fx, &c1, vec!["doc.txt".into()], NOW);
    assert!(matches!(outcome, MoveOutcome::Held(_)));
    let repo = fx.repo();
    let first = ff_core::held::of(&repo, "feature")
        .unwrap()
        .expect("the lift holds");
    let before = head_refs(&fx);

    let err = ff_core::restack::restack(
        &repo,
        Some("feature".into()),
        Some("other".into()),
        &prov(),
        Some(NOW + 100),
        vec!["ff".into(), "restack".into()],
    )
    .expect_err("a re-aim under a held lift must refuse");
    assert_eq!(err.id(), "held/already-held", "{err}");
    assert!(err.to_string().contains("nothing was restacked"), "{err}");

    let err = ff_core::describe::reword(
        &repo,
        oid(&c2),
        "c2 reworded".into(),
        ff_core::Verify::Run,
        &prov(),
        Some(NOW + 200),
        vec!["ff".into(), "describe".into()],
    )
    .expect_err("a reword under a held lift must refuse");
    assert_eq!(err.id(), "held/already-held", "{err}");
    assert!(err.to_string().contains("nothing was reworded"), "{err}");

    fx.write("z.txt", "z\n");
    let err = ff_core::absorb::move_change(
        &repo,
        &MoveOptions {
            verb: MoveVerb::Absorb,
            from: None,
            into: Some(Endpoint::Commit(oid(&c2))),
            paths: Vec::new(),
            message: None,
            verify: ff_core::Verify::Run,
            now: Some(NOW + 300),
            argv: vec!["ff".into(), "absorb".into()],
        },
        &prov(),
    )
    .expect_err("an absorb under a held lift must refuse");
    assert_eq!(err.id(), "held/already-held", "{err}");
    assert!(err.to_string().contains("nothing was absorbed"), "{err}");

    assert_eq!(head_refs(&fx), before, "a refusal moves no ref");
    assert_eq!(
        ff_core::held::of(&repo, "feature").unwrap(),
        Some(first),
        "the hold stands byte-identical"
    );
}

#[test]
fn an_open_resolution_refuses_a_reaim() {
    let fx = Fixture::new();
    ident(&fx);
    conflict_stack(&fx);
    other_off_base(&fx);
    hold_feature(&fx);

    let repo = fx.repo();
    let (outcome, _ctx) = ff_core::resolve::resolve(
        &repo,
        false,
        &prov(),
        Some(NOW + 100),
        vec!["ff".into(), "resolve".into()],
    )
    .unwrap();
    let session = match outcome {
        ff_core::ResolveOutcome::Opened(r) => r.session,
        other => panic!("the resolution must open, got {other:?}"),
    };
    // Parked: HEAD leaves the session for the held branch.
    ff_core::switch(
        &repo,
        &ff_core::SwitchOptions {
            target: Some("feature".into()),
            now: Some(NOW + 200),
            argv: vec!["ff".into(), "switch".into(), "feature".into()],
            ..Default::default()
        },
        &prov(),
    )
    .unwrap();

    let err = ff_core::restack::restack(
        &repo,
        Some("feature".into()),
        Some("other".into()),
        &prov(),
        Some(NOW + 300),
        vec!["ff".into(), "restack".into()],
    )
    .expect_err("a re-aim under an open resolution must refuse");
    assert_eq!(err.id(), "held/resolving", "{err}");
    assert!(
        err.to_string().contains(&session),
        "the refusal names the session: {err}"
    );
}

#[test]
fn a_reaim_that_conflicts_replaces_the_hold() {
    let fx = Fixture::new();
    ident(&fx);
    let (f1, _m1) = conflict_stack(&fx);
    // other edits the same line too, so a re-aim onto it conflicts as well.
    fx.git(&["branch", "other", "main~1"]);
    fx.git(&["switch", "-q", "other"]);
    fx.write("f.txt", "four\n");
    fx.commit("o1");
    fx.git(&["switch", "-q", "feature"]);
    let first = hold_feature(&fx);

    let (outcome, _ctx) = restack_call(&fx, Some("feature"), Some("other"), NOW + 100);
    assert!(
        matches!(outcome, RestackOutcome::Held(_)),
        "the re-aim conflicts and holds, got {outcome:?}"
    );
    let repo = fx.repo();
    let held = ff_core::held::of(&repo, "feature")
        .unwrap()
        .expect("the re-aim holds");
    assert_eq!(
        held.intent,
        ff_core::held::Intent::Restack {
            branch: "feature".into(),
            onto: "refs/heads/other".into(),
        },
        "the new hold stands in the old one's place"
    );
    let record = tip_record(&repo);
    assert_eq!(record.verb, "hold");
    assert_eq!(
        record.held.as_ref().and_then(|t| t.old.clone()),
        Some(first.clone()),
        "the hold op records the hold it replaced"
    );

    undo_call(&fx, NOW + 200);
    assert_eq!(rev(&fx, "feature"), f1);
    assert_eq!(
        ff_core::held::of(&repo, "feature").unwrap(),
        Some(first),
        "undo restores the hold onto main"
    );
}

#[test]
fn a_fold_of_a_held_source_drops_the_hold() {
    let fx = Fixture::new();
    ident(&fx);
    let (f1, _m1) = conflict_stack(&fx);
    other_off_base(&fx);
    let first = hold_feature(&fx);
    let other_tip = rev(&fx, "other");

    let repo = fx.repo();
    let (report, _ctx, _other) = ff_core::fold::fold(
        &repo,
        Some("other".into()),
        false,
        &prov(),
        Some(NOW + 100),
        vec!["ff".into(), "fold".into(), "other".into()],
    )
    .unwrap();
    assert_eq!(
        report.dropped_hold,
        Some(ff_core::DroppedHold {
            verb: "restack".into(),
            onto: "main".into(),
        })
    );
    assert_eq!(ff_core::held::of(&repo, "feature").unwrap(), None);
    assert_eq!(
        tip_record(&repo).held,
        Some(ff_core::ops::record::HeldTransition {
            branch: "feature".into(),
            old: Some(first.clone()),
            new: None,
        })
    );

    undo_call(&fx, NOW + 200);
    assert_eq!(rev(&fx, "feature"), f1, "the source is back at its tip");
    assert_eq!(rev(&fx, "other"), other_tip);
    assert_eq!(
        ff_core::held::of(&repo, "feature").unwrap(),
        Some(first),
        "and its hold is back with it"
    );
}

#[test]
fn a_done_landing_under_a_base_following_hold_remaps_it() {
    let fx = Fixture::new();
    ident(&fx);
    let (f1, _m1) = conflict_stack(&fx);
    hold_feature(&fx);

    let (outcome, _ctx) = edit_call(&fx, &f1, NOW + 100);
    assert!(matches!(outcome, EditOutcome::Opened(_)));
    fx.write("g.txt", "g\n");
    let (outcome, _ctx) = done_call(&fx, NOW + 200);
    assert!(
        matches!(outcome, DoneOutcome::Done(_)),
        "the landing is clean, got {outcome:?}"
    );
    let amended = rev(&fx, "feature");
    assert_ne!(amended, f1);

    let held = ff_core::held::of(&fx.repo(), "feature")
        .unwrap()
        .expect("the hold stays on the landing branch");
    assert_eq!(at_id(&held), amended, "the hold follows the amend");
}
