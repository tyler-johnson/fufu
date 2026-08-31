//! Contract for `edit::edit`: a session mints an anonymous branch at the
//! target commit and switches to it, with the branch you came from staying
//! exactly where it stands.

use ff_core::gix;
use ff_core::{EditOutcome, Provenance};
use ff_testsupport::Fixture;

const NOW: i64 = 1_799_999_999;

/// `edit` reads the committer identity from the repo config, which the
/// fixture's hermetic env does not set; git itself gets its identity from
/// env vars, so this is only for the gix side.
fn ident(fx: &Fixture) {
    fx.set_config("user.name", "Fixture Committer");
    fx.set_config("user.email", "committer@fixture.test");
}

fn prov() -> Provenance {
    Provenance::new("pre", Some("ff edit".into()))
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

fn edit_err(fx: &Fixture, rev: &str, now: i64) -> ff_core::Error {
    let repo = fx.repo();
    ff_core::edit::edit(
        &repo,
        rev,
        &prov(),
        Some(now),
        vec!["ff".into(), "edit".into()],
    )
    .unwrap_err()
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

/// The newest op whose record names the verb, walking back through the
/// captures that sit between the verb ops.
fn record_of_verb(repo: &gix::Repository, verb: &str) -> ff_core::ops::OpRecord {
    let log = ff_core::ops::OpLog::open(repo).unwrap();
    let mut id = log.tip().unwrap().unwrap();
    loop {
        let op = log.get(id).unwrap();
        if let Some(record) = op.record().unwrap()
            && record.verb == verb
        {
            return record.clone();
        }
        id = op
            .prev()
            .expect("the op is in the log; the walk cannot run past its root");
    }
}

/// How many anonymous branches exist.
fn anon_count(fx: &Fixture) -> usize {
    fx.git(&["for-each-ref", "refs/heads/ff/", "--format=%(refname)"])
        .lines()
        .count()
}

/// The shared stack: `main` with four commits `c0..c3`, distinct files per
/// commit so nothing overlaps, and `mid` at `c1`.
fn base(fx: &Fixture) -> (String, String, String, String) {
    fx.write("c0.txt", "c0\n");
    let c0 = fx.commit("c0");
    fx.write("c1.txt", "c1\n");
    let c1 = fx.commit("c1");
    fx.write("c2.txt", "c2\n");
    let c2 = fx.commit("c2");
    fx.write("c3.txt", "c3\n");
    let c3 = fx.commit("c3");
    fx.git(&["branch", "mid", &c1]);
    (c0, c1, c2, c3)
}

fn opened(outcome: EditOutcome) -> ff_core::EditReport {
    match outcome {
        EditOutcome::Opened(report) => report,
        other => panic!("a session must open, got {other:?}"),
    }
}

#[test]
fn edit_mints_a_session_and_switches_to_it() {
    let fx = Fixture::new();
    ident(&fx);
    let (_c0, c1, _c2, c3) = base(&fx);

    let (outcome, _ctx) = edit_call(&fx, &c1, NOW);
    let report = opened(outcome);

    assert!(
        report.session.starts_with("ff/"),
        "session: {}",
        report.session
    );
    assert_eq!(
        fx.git(&["rev-parse", "--abbrev-ref", "HEAD"]).trim(),
        report.session
    );
    assert_eq!(
        fx.git(&["rev-parse", &format!("refs/heads/{}", report.session)])
            .trim(),
        c1
    );
    assert_eq!(report.editing, c1);
    assert_eq!(report.onto, "main");
    assert_eq!(report.ahead, 2);
    assert_eq!(
        fx.git(&["rev-parse", "main"]).trim(),
        c3,
        "the branch you came from stays exactly where it stands"
    );
}

#[test]
fn the_worktree_holds_the_edited_commits_tree() {
    let fx = Fixture::new();
    ident(&fx);
    let (_c0, c1, _c2, _c3) = base(&fx);

    let (outcome, _ctx) = edit_call(&fx, &c1, NOW);
    assert!(
        matches!(outcome, EditOutcome::Opened(_)),
        "the session must open"
    );

    assert!(!fx.path().join("c2.txt").exists(), "c2's file must be gone");
    assert!(!fx.path().join("c3.txt").exists(), "c3's file must be gone");
    assert_eq!(
        std::fs::read_to_string(fx.path().join("c1.txt")).unwrap(),
        "c1\n"
    );
    assert!(
        fx.git(&["status", "--porcelain"]).trim().is_empty(),
        "HEAD and the working tree must agree: plain git sees an ordinary branch"
    );
}

#[test]
fn the_session_is_recorded_in_metadata() {
    let fx = Fixture::new();
    ident(&fx);
    let (_c0, c1, _c2, _c3) = base(&fx);

    let (outcome, _ctx) = edit_call(&fx, &c1, NOW);
    let report = opened(outcome);

    assert_eq!(
        ff_core::branchmeta::read(&fx.repo(), &report.session)
            .unwrap()
            .session,
        Some(ff_core::branchmeta::Session {
            onto: "main".into(),
            at: c1,
        })
    );
    assert_eq!(
        ff_core::branchmeta::read(&fx.repo(), "main")
            .unwrap()
            .session,
        None,
        "the session is on the session branch, not on the branch it will replay onto"
    );
}

#[test]
fn the_op_records_the_session_transition() {
    let fx = Fixture::new();
    ident(&fx);
    let (_c0, c1, _c2, _c3) = base(&fx);

    let (outcome, _ctx) = edit_call(&fx, &c1, NOW);
    let report = opened(outcome);

    // The switch appends a later op, so the tip is the switch's; the mint
    // is the one that carries the session.
    assert_eq!(
        tip_record(&fx.repo()).verb,
        "switch",
        "the switch appends after the mint"
    );
    let record = record_of_verb(&fx.repo(), "edit");
    let transition = record
        .edit_session
        .as_ref()
        .expect("the mint op must record the session it opens");
    assert_eq!(transition.branch, report.session);
    assert_eq!(transition.old, None);
    assert_eq!(
        transition.new,
        Some(ff_core::branchmeta::Session {
            onto: "main".into(),
            at: c1,
        })
    );
}

#[test]
fn a_dirty_tree_parks() {
    let fx = Fixture::new();
    ident(&fx);
    let (_c0, c1, _c2, _c3) = base(&fx);

    fx.write("wip.txt", "wip\n");
    let (outcome, _ctx) = edit_call(&fx, &c1, NOW);
    let report = opened(outcome);

    assert!(report.parked.is_some(), "the open change must park");
    // `fx.git` panics on failure, so this line only runs when the ref exists.
    fx.git(&["rev-parse", "--verify", "refs/fufu/parked/main"]);
    assert!(
        !fx.path().join("wip.txt").exists(),
        "the parked change must not sit in the session's worktree"
    );
}

#[test]
fn a_branch_name_is_a_switch() {
    let fx = Fixture::new();
    ident(&fx);
    let (_c0, _c1, _c2, _c3) = base(&fx);

    let anon_before = anon_count(&fx);
    let (outcome, _ctx) = edit_call(&fx, "mid", NOW);
    assert!(
        matches!(outcome, EditOutcome::Switched(_)),
        "a branch name must redirect to a switch"
    );
    assert_eq!(fx.git(&["rev-parse", "--abbrev-ref", "HEAD"]).trim(), "mid");
    assert_eq!(anon_count(&fx), anon_before, "no session may be minted");
}

#[test]
fn the_open_change_is_never_an_edit_target() {
    let fx = Fixture::new();
    ident(&fx);
    let _ = base(&fx);

    // However the open change is spelled: the refusal is on the resolved
    // revision, so a function wrapper does not smuggle it past.
    for rev in ["@", "latest(@)", "heads(@)"] {
        let err = edit_err(&fx, rev, NOW);
        assert_eq!(err.id(), "target/unresolvable", "{rev}");
        assert!(
            err.exits().iter().any(|e| e.contains("ff edit HEAD")),
            "{rev} must name the exit: {err}"
        );
    }
}

#[test]
fn a_commit_outside_the_branch_is_refused() {
    let fx = Fixture::new();
    ident(&fx);
    let (c0, _c1, _c2, c3) = base(&fx);

    // A branch off `c0` with a commit of its own: `o1` is reachable from
    // `other`, not from `main`.
    fx.git(&["switch", "-q", "-c", "other", &c0]);
    fx.write("o.txt", "o\n");
    let o1 = fx.commit("o1");
    fx.git(&["switch", "-q", "main"]);

    let anon_before = anon_count(&fx);
    let err = edit_err(&fx, &o1, NOW);
    assert_eq!(err.id(), "edit/not-in-history", "{err}");
    assert_eq!(anon_count(&fx), anon_before, "no session may be minted");
    assert_eq!(
        fx.git(&["rev-parse", "--abbrev-ref", "HEAD"]).trim(),
        "main",
        "HEAD must not have moved"
    );
    assert_eq!(
        fx.git(&["rev-parse", "main"]).trim(),
        c3,
        "main's tip must be unchanged"
    );
}

#[test]
fn a_session_cannot_nest() {
    let fx = Fixture::new();
    ident(&fx);
    let (c0, c1, _c2, _c3) = base(&fx);

    let (outcome, _ctx) = edit_call(&fx, &c1, NOW);
    assert!(
        matches!(outcome, EditOutcome::Opened(_)),
        "the first session must open"
    );

    let err = edit_err(&fx, &c0, NOW + 100);
    assert_eq!(err.id(), "session/open", "{err}");
    assert!(
        err.exits().iter().any(|e| e.contains("ff done")),
        "the refusal must name the exit: {err}"
    );
    assert_eq!(anon_count(&fx), 1, "no second session may be minted");
}

#[test]
fn editing_the_tip_is_allowed() {
    let fx = Fixture::new();
    ident(&fx);
    let (_c0, _c1, _c2, c3) = base(&fx);

    let (outcome, _ctx) = edit_call(&fx, &c3, NOW);
    let report = opened(outcome);
    assert_eq!(
        report.ahead, 0,
        "a session on the tip is a legitimate degenerate case"
    );
}

#[test]
fn undo_takes_the_session_back() {
    let fx = Fixture::new();
    ident(&fx);
    let (_c0, c1, _c2, c3) = base(&fx);

    // Bootstrap the log so the second step has a floor to land on: the
    // fixture's commits are plain git, so without it the mint is the oldest
    // operation and `op/floor` stops the walk short of the pre-edit state.
    let repo = fx.repo();
    ff_core::ops::reconcile(&repo, NOW - 10).unwrap();

    let (outcome, _ctx) = edit_call(&fx, &c1, NOW);
    assert!(
        matches!(outcome, EditOutcome::Opened(_)),
        "the session must open"
    );

    // The mint and the switch are two operations, exactly as `ff start` is,
    // and a step is a run, not a verb: the first undo lands ON the mint,
    // whose recorded state still holds the branch it created, so the
    // session only comes back on the second step.
    for step in [100, 101] {
        let opts = ff_core::RewindOptions {
            force: false,
            now: Some(NOW + step),
            argv: vec!["ff".into(), "undo".into()],
        };
        ff_core::undo(&fx.repo(), &opts, &prov()).unwrap();
    }

    assert_eq!(
        fx.git(&["rev-parse", "--abbrev-ref", "HEAD"]).trim(),
        "main"
    );
    assert_eq!(anon_count(&fx), 0, "the session branch must be gone");
    assert_eq!(
        fx.git(&["rev-parse", "main"]).trim(),
        c3,
        "main's tip must be unchanged"
    );
}

/// `ff commit` is the close; inside a session the close IS the `ff done` the
/// guard refuses, so it must refuse and name the exit — and write nothing.
#[test]
fn committing_inside_a_session_refuses() {
    let fx = Fixture::new();
    ident(&fx);
    let (_c0, c1, _c2, c3) = base(&fx);

    let (outcome, _ctx) = edit_call(&fx, &c1, NOW);
    let report = opened(outcome);

    // The amendment a session would fold in: close would have committed it.
    fx.write("amend.txt", "amend\n");

    let repo = fx.repo();
    let err = ff_core::close(
        &repo,
        &ff_core::CloseOptions {
            message: Some("close message".into()),
            no_verify: false,
            branch: None,
            sign: Default::default(),
            paths: Vec::new(),
            now: Some(NOW + 100),
            argv: vec!["ff".into(), "commit".into()],
        },
        &ff_core::Provenance::new("pre", Some("ff commit".into())),
    )
    .unwrap_err();

    assert_eq!(err.id(), "session/open", "{err}");
    assert!(
        err.exits().iter().any(|e| e.contains("ff done")),
        "the refusal must name the exit: {err}"
    );

    // Nothing moved: the session branch's tip, main's tip, and the open
    // change all stand exactly where they did.
    assert_eq!(
        fx.git(&["rev-parse", &format!("refs/heads/{}", report.session)])
            .trim(),
        c1,
        "the session branch's tip must be unchanged"
    );
    assert_eq!(
        fx.git(&["rev-parse", "main"]).trim(),
        c3,
        "main's tip must be unchanged"
    );
    assert!(
        fx.path().join("amend.txt").exists(),
        "the open change must still be open, not committed"
    );
}

/// A session branch sits below its landing branch by construction, so
/// "behind, fast-forwards" is a permanent condition of the session, not
/// pending work: the base axis is silenced there and nowhere else.
#[test]
fn a_session_branch_has_no_base_axis() {
    let fx = Fixture::new();
    ident(&fx);
    let (_c0, c1, _c2, _c3) = base(&fx);

    let (outcome, _ctx) = edit_call(&fx, &c1, NOW);
    let report = opened(outcome);

    let repo = fx.repo();
    assert_eq!(
        ff_core::futures::base_for(&repo, &report.session).unwrap(),
        None,
        "a session branch must have no base axis"
    );
    let base = ff_core::futures::base_for(&repo, "mid")
        .unwrap()
        .expect("a plain branch's base axis must be unaffected");
    assert_eq!(
        base.name, "main",
        "the silencing must reach the session branch alone"
    );
}

/// Restacking moves a session off the commit it edits, so both spellings
/// refuse — and write nothing.
#[test]
fn restacking_a_session_refuses() {
    let fx = Fixture::new();
    ident(&fx);
    let (_c0, c1, _c2, c3) = base(&fx);

    let (outcome, _ctx) = edit_call(&fx, &c1, NOW);
    let report = opened(outcome);
    let session = report.session.clone();

    // Spelling one: the bare verb from inside the session.
    let repo = fx.repo();
    let err = ff_core::restack::restack(
        &repo,
        None,
        None,
        &prov(),
        Some(NOW + 100),
        vec!["ff".into(), "restack".into()],
    )
    .unwrap_err();
    assert_eq!(err.id(), "session/open", "{err}");

    // Spelling two: the named branch from elsewhere.
    fx.git(&["switch", "-q", "main"]);
    let err = ff_core::restack::restack(
        &repo,
        Some(session.clone()),
        None,
        &prov(),
        Some(NOW + 200),
        vec!["ff".into(), "restack".into(), session.clone()],
    )
    .unwrap_err();
    assert_eq!(err.id(), "session/open", "{err}");

    assert_eq!(
        fx.git(&["rev-parse", &format!("refs/heads/{session}")])
            .trim(),
        c1,
        "the session branch's tip must be unchanged"
    );
    assert_eq!(
        fx.git(&["rev-parse", "main"]).trim(),
        c3,
        "main's tip must be unchanged"
    );
}

/// The control: on a plain branch with a dirty tree the guard must not fire,
/// or `ff commit` would refuse every commit in the repository.
#[test]
fn committing_outside_a_session_still_works() {
    let fx = Fixture::new();
    ident(&fx);
    let _ = base(&fx);

    fx.write("plain.txt", "plain\n");

    let repo = fx.repo();
    let (outcome, _ctx) = ff_core::close(
        &repo,
        &ff_core::CloseOptions {
            message: Some("plain close".into()),
            no_verify: false,
            branch: None,
            sign: Default::default(),
            paths: Vec::new(),
            now: Some(NOW + 100),
            argv: vec!["ff".into(), "commit".into()],
        },
        &ff_core::Provenance::new("pre", Some("ff commit".into())),
    )
    .unwrap();

    assert!(
        matches!(outcome, ff_core::CommitOutcome::Closed { .. }),
        "a plain close on a non-session branch must succeed"
    );
}
