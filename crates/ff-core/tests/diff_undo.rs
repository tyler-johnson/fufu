//! Moving along the log. Undo-to-state must be a true inverse (close→undo =
//! identity on refs, tree, and index); a step is a *run*, not an operation;
//! the move is a pointer move, so redo walks forward along what it stepped
//! off; foreign entries are labeled; and partial application converges on
//! re-run, because the plan is a state and not a script.

use ff_core::{CloseOptions, CommitOutcome, Landing, RewindOptions, SwitchOptions};
use ff_testsupport::Fixture;

const NOW: i64 = 1_700_000_000;

fn ident(fx: &Fixture) {
    fx.set_config("user.name", "Undo User");
    fx.set_config("user.email", "undo@test");
}

fn prov() -> ff_core::Provenance {
    ff_core::Provenance::new("pre", Some("ff test".into()))
}

fn session_prov(name: &str) -> ff_core::Provenance {
    ff_core::Provenance::new("pre", Some("ff test".into())).with_session(Some(name.into()))
}

fn opts(now: i64) -> RewindOptions {
    RewindOptions {
        force: false,
        now: Some(now),
        argv: vec!["ff".into(), "undo".into()],
    }
}

fn run_undo(fx: &Fixture) -> ff_core::RewindReport {
    run_undo_at(fx, NOW + 100)
}

fn run_undo_at(fx: &Fixture, now: i64) -> ff_core::RewindReport {
    let repo = fx.repo();
    ff_core::undo(&repo, &opts(now), &prov()).unwrap().0
}

fn run_redo(fx: &Fixture, now: i64) -> ff_core::RewindReport {
    let repo = fx.repo();
    ff_core::redo(&repo, &opts(now), &prov()).unwrap().0
}

fn run_op_restore(fx: &Fixture, spec: &str, now: i64) -> ff_core::RewindReport {
    let repo = fx.repo();
    ff_core::rewind(&repo, &Landing::At(spec.into()), &opts(now), &prov())
        .unwrap()
        .0
}

fn world_state(fx: &Fixture) -> (String, String, String, String) {
    (
        fx.git(&[
            "for-each-ref",
            "refs/heads",
            "--format=%(refname) %(objectname)",
        ]),
        fx.git(&["symbolic-ref", "-q", "HEAD"]),
        fx.git(&["status", "--porcelain=v2"]),
        fx.git(&["ls-files", "--stage"]),
    )
}

/// The subtlety that needs pinning rather than avoiding: `ff undo` captures
/// first like every verb, so on a dirty tree its own pre-capture sits at the
/// tip and joins the run being undone. That is deliberate — the pre-undo
/// capture belongs at the head of the abandoned branch, so redo hands the
/// work you were holding back first.
#[test]
fn a_run_of_captures_is_one_step_and_a_session_boundary_ends_it() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    ident(&fx);
    let repo = fx.repo();
    ff_core::ops::reconcile(&repo, NOW - 10).unwrap();

    let capture = |fx: &Fixture, body: &str, now: i64, session: Option<&str>| {
        fx.write("a.txt", body);
        let repo = fx.repo();
        let prov = match session {
            Some(name) => session_prov(name),
            None => prov(),
        };
        ff_core::capture_with(
            &repo,
            &prov,
            &ff_core::TakeOptions {
                now: Some(now),
                max_file_size: None,
            },
        )
        .unwrap()
    };

    // Three untagged captures: one run.
    capture(&fx, "a1\n", NOW + 1, None);
    capture(&fx, "a2\n", NOW + 2, None);
    capture(&fx, "a3\n", NOW + 3, None);

    let report = run_undo_at(&fx, NOW + 10);
    assert!(
        report.collapsed >= 3,
        "three adjacent untagged captures are one run: {report:?}"
    );
    assert_eq!(report.stepped_ops, 0, "captures are not decisions");
    assert_eq!(
        std::fs::read_to_string(fx.path().join("a.txt")).unwrap(),
        "a\n",
        "all three went as one step"
    );

    // Redo brings all three back in one move, the same run in reverse.
    let report = run_redo(&fx, NOW + 11);
    assert!(report.forward);
    assert_eq!(
        std::fs::read_to_string(fx.path().join("a.txt")).unwrap(),
        "a3\n",
        "redo returns the whole run"
    );

    // A different session breaks the run: no tag never joins a tag.
    capture(&fx, "tagged\n", NOW + 20, Some("s"));
    capture(&fx, "untagged\n", NOW + 21, None);
    let report = run_undo_at(&fx, NOW + 30);
    assert_eq!(
        std::fs::read_to_string(fx.path().join("a.txt")).unwrap(),
        "tagged\n",
        "only the untagged capture went: {report:?}"
    );
}

/// A verb's operation is a decision somebody made, so it is always its own
/// step and always ends a run — which is what keeps undo from stepping past a
/// commit by accident.
#[test]
fn a_verb_operation_is_always_its_own_step() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    ident(&fx);
    let repo = fx.repo();
    ff_core::ops::reconcile(&repo, NOW - 10).unwrap();

    fx.write("a.txt", "first\n");
    ff_core::close(
        &repo,
        &CloseOptions {
            message: Some("one".into()),
            now: Some(NOW),
            argv: Vec::new(),
            ..Default::default()
        },
        &prov(),
    )
    .unwrap();
    fx.write("a.txt", "second\n");
    let repo = fx.repo();
    ff_core::close(
        &repo,
        &CloseOptions {
            message: Some("two".into()),
            now: Some(NOW + 1),
            argv: Vec::new(),
            ..Default::default()
        },
        &prov(),
    )
    .unwrap();

    let report = run_undo_at(&fx, NOW + 10);
    assert_eq!(report.stepped_ops, 1, "one close, not two: {report:?}");
    assert!(
        report.stepped_summary.as_deref().unwrap().contains("two"),
        "{report:?}"
    );
    assert_eq!(fx.git(&["log", "--oneline"]).lines().count(), 2);
}

#[test]
fn close_then_undo_is_identity_and_redo_puts_it_back() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    ident(&fx);
    fx.write("a.txt", "the change\n");
    fx.write("new.txt", "untracked\n");

    // Baseline BEFORE the close (with the log bootstrapped so the undo has a
    // floor).
    let repo = fx.repo();
    ff_core::ops::reconcile(&repo, NOW - 10).unwrap();
    let before = world_state(&fx);

    let (outcome, _) = ff_core::close(
        &repo,
        &CloseOptions {
            message: Some("landed".into()),
            now: Some(NOW),
            argv: Vec::new(),
            ..Default::default()
        },
        &prov(),
    )
    .unwrap();
    let CommitOutcome::Closed { id, .. } = outcome;
    let after_close = world_state(&fx);
    assert_ne!(before, after_close);

    // Undo: the world returns to the pre-close state exactly.
    let report = run_undo(&fx);
    assert_eq!(report.stepped_kind.as_deref(), Some("op"));
    assert!(
        report
            .stepped_summary
            .as_deref()
            .unwrap()
            .contains("landed")
    );
    assert!(!report.forward);
    assert_eq!(world_state(&fx), before, "close→undo = identity");
    assert_eq!(
        std::fs::read_to_string(fx.path().join("a.txt")).unwrap(),
        "the change\n",
        "dirty worktree restored"
    );
    assert_eq!(
        std::fs::read_to_string(fx.path().join("new.txt")).unwrap(),
        "untracked\n",
        "untracked file restored"
    );

    // Redo is its own verb now: undo-of-undo is not it, because undo appended
    // nothing to undo.
    let report = run_redo(&fx, NOW + 101);
    assert!(report.forward, "{report:?}");
    assert_eq!(world_state(&fx), after_close, "undo→redo = identity");
    assert_eq!(fx.git(&["rev-parse", "HEAD"]).trim(), id);

    // The log reconciles clean after all of it.
    let repo = fx.repo();
    let after = ff_core::ops::reconcile(&repo, NOW + 500).unwrap();
    assert!(after.is_quiet(), "{after:?}");
}

/// The log records work and never navigation: a whole undo/redo round trip
/// appends nothing, and the pointer's travels live in the ref's own reflog.
#[test]
fn moving_appends_nothing_and_the_reflog_records_where_it_stood() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    ident(&fx);
    let repo = fx.repo();
    ff_core::ops::reconcile(&repo, NOW - 10).unwrap();
    fx.write("a.txt", "work\n");
    ff_core::close(
        &repo,
        &CloseOptions {
            message: Some("landed".into()),
            now: Some(NOW),
            argv: Vec::new(),
            ..Default::default()
        },
        &prov(),
    )
    .unwrap();

    let before = ff_core::ops::read_ops(&fx.repo(), 0).unwrap().len();
    let tip_before = fx.git(&["rev-parse", "refs/fufu/ops"]);
    run_undo(&fx);
    run_redo(&fx, NOW + 101);
    let after = ff_core::ops::read_ops(&fx.repo(), 0).unwrap().len();
    assert_eq!(before, after, "a round trip wrote no operation");
    assert_eq!(
        tip_before,
        fx.git(&["rev-parse", "refs/fufu/ops"]),
        "and landed exactly back where it started"
    );

    let reflog = fx.git(&["reflog", "show", "refs/fufu/ops"]);
    assert!(reflog.contains("fufu: undo to"), "{reflog}");
    assert!(reflog.contains("fufu: redo to"), "{reflog}");
}

/// Work after an undo forks the log rather than truncating it, so redo stops
/// offering a path it can no longer take.
#[test]
fn redo_refuses_once_work_has_landed() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    ident(&fx);
    let repo = fx.repo();
    ff_core::ops::reconcile(&repo, NOW - 10).unwrap();
    fx.write("a.txt", "work\n");
    ff_core::close(
        &repo,
        &CloseOptions {
            message: Some("landed".into()),
            now: Some(NOW),
            argv: Vec::new(),
            ..Default::default()
        },
        &prov(),
    )
    .unwrap();
    run_undo(&fx);

    // New work: a capture of something the post-undo state does not hold.
    fx.write("a.txt", "a different direction\n");
    let repo = fx.repo();
    ff_core::capture_with(
        &repo,
        &prov(),
        &ff_core::TakeOptions {
            now: Some(NOW + 200),
            max_file_size: None,
        },
    )
    .unwrap();

    let repo = fx.repo();
    let err = ff_core::redo(&repo, &opts(NOW + 201), &prov()).unwrap_err();
    assert_eq!(err.id(), "op/nothing-to-redo", "{err}");
    assert_eq!(
        std::fs::read_to_string(fx.path().join("a.txt")).unwrap(),
        "a different direction\n",
        "and nothing was stepped over"
    );
}

/// Nothing is discarded: an abandoned operation still resolves by id, which
/// is what `ff op restore` promises until trim ages it out.
#[test]
fn an_abandoned_operation_still_resolves_by_id() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    ident(&fx);
    let repo = fx.repo();
    ff_core::ops::reconcile(&repo, NOW - 10).unwrap();
    fx.write("a.txt", "work\n");
    ff_core::close(
        &repo,
        &CloseOptions {
            message: Some("landed".into()),
            now: Some(NOW),
            argv: Vec::new(),
            ..Default::default()
        },
        &prov(),
    )
    .unwrap();
    let repo = fx.repo();
    let abandoned = ff_core::ops::OpLog::open(&repo)
        .unwrap()
        .tip()
        .unwrap()
        .unwrap();

    run_undo(&fx);
    fx.write("a.txt", "elsewhere\n");
    let repo = fx.repo();
    ff_core::capture_with(
        &repo,
        &prov(),
        &ff_core::TakeOptions {
            now: Some(NOW + 200),
            max_file_size: None,
        },
    )
    .unwrap();

    // The forward path is gone, but the id is not.
    let repo = fx.repo();
    let letters = abandoned.to_string();
    let report = run_op_restore(&fx, &letters[..8], NOW + 300);
    assert_eq!(report.landed, letters, "{report:?}");
    assert_eq!(
        std::fs::read_to_string(fx.path().join("a.txt")).unwrap(),
        "work\n"
    );
    drop(repo);
}

#[test]
fn undo_of_a_hook_precaptured_close_restores_the_dirty_worktree() {
    // The common real-world shape: an agent/shell hook snapshots the dirty
    // tree moments before `ff commit`, so the close's own capture-first
    // snapshot no-ops. The operation must still carry the pre-verb state
    // (the tip that already holds it), and undo must reopen the change —
    // modified AND untracked files — not check out a clean tree at the parent.
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    ident(&fx);
    fx.write("a.txt", "the change\n");
    fx.write("new.txt", "untracked\n");

    let repo = fx.repo();
    ff_core::ops::reconcile(&repo, NOW - 10).unwrap();
    // The hook's snapshot captures the dirty state onto the log first.
    let snap = ff_core::capture_with(
        &repo,
        &ff_core::Provenance::new("claude", Some("hook".into())),
        &ff_core::TakeOptions {
            now: Some(NOW - 5),
            max_file_size: None,
        },
    )
    .unwrap();
    assert!(matches!(snap, ff_core::CaptureOutcome::Created { .. }));
    let before = world_state(&fx);

    let (outcome, ctx) = ff_core::close(
        &repo,
        &CloseOptions {
            message: Some("landed".into()),
            now: Some(NOW),
            argv: Vec::new(),
            ..Default::default()
        },
        &prov(),
    )
    .unwrap();
    let CommitOutcome::Closed { .. } = outcome;
    assert!(
        ctx.pre_op.is_some(),
        "a no-op capture must still surface the tip as the pre-verb operation"
    );

    let report = run_undo(&fx);
    assert!(
        report
            .stepped_summary
            .as_deref()
            .unwrap()
            .contains("landed")
    );
    assert_eq!(world_state(&fx), before, "close→undo = identity");
    assert_eq!(
        std::fs::read_to_string(fx.path().join("a.txt")).unwrap(),
        "the change\n",
        "dirty worktree restored"
    );
    assert_eq!(
        std::fs::read_to_string(fx.path().join("new.txt")).unwrap(),
        "untracked\n",
        "untracked file restored"
    );
}

#[test]
fn switch_then_undo_returns_with_the_parked_change_reopened() {
    let fx = Fixture::new();
    fx.write("shared.txt", "base\n");
    fx.commit("base");
    fx.git(&["branch", "feature"]);
    ident(&fx);
    fx.write("shared.txt", "wip on main\n");

    let repo = fx.repo();
    ff_core::ops::reconcile(&repo, NOW - 10).unwrap();
    let before = world_state(&fx);

    let (report, _) = ff_core::switch(
        &repo,
        &SwitchOptions {
            target: "feature".into(),
            now: Some(NOW),
            argv: Vec::new(),
        },
        &prov(),
    )
    .unwrap();
    assert!(report.parked.is_some());

    let undo_report = run_undo(&fx);
    assert!(
        undo_report
            .stepped_summary
            .as_deref()
            .unwrap()
            .contains("switch")
    );
    assert_eq!(world_state(&fx), before, "switch→undo = identity");
    assert_eq!(
        std::fs::read_to_string(fx.path().join("shared.txt")).unwrap(),
        "wip on main\n",
        "the parked change is open again"
    );
    // The park's stash entry was dropped by the rollback.
    assert!(
        fx.git(&["stash", "list"]).is_empty(),
        "stash effect inverted"
    );
    assert!(
        ff_core::stash::parked_entry(&fx.repo(), "main")
            .unwrap()
            .is_none()
    );
}

#[test]
fn foreign_ops_undo_with_a_label() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    let first = fx.commit("init");
    ident(&fx);
    let repo = fx.repo();
    ff_core::ops::reconcile(&repo, NOW - 10).unwrap();

    // A commit made with real git...
    fx.write("a.txt", "user's commit\n");
    fx.commit("user work");

    // ...absorbed, then undone.
    let report = run_undo(&fx);
    assert_eq!(
        report.stepped_kind.as_deref(),
        Some("foreign"),
        "labeled foreign: {report:?}"
    );
    assert_eq!(
        fx.git(&["rev-parse", "HEAD"]).trim(),
        first,
        "branch rolled back"
    );
    // Rollback-to-state: the pre-state had a clean tree at `first`. The
    // user's content lives on in the pinned commit and on the log.
    assert_eq!(fx.git(&["status", "--porcelain=v2"]), "");
    assert_eq!(
        std::fs::read_to_string(fx.path().join("a.txt")).unwrap(),
        "a\n"
    );
}

#[test]
fn landing_on_an_older_operation_steps_over_everything_after_it() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    ident(&fx);
    let repo = fx.repo();
    ff_core::ops::reconcile(&repo, NOW - 10).unwrap();
    let before_all = world_state(&fx);

    // Two closes.
    for (n, msg) in [(1, "first close"), (2, "second close")] {
        fx.write("a.txt", &format!("change {n}\n"));
        let repo = fx.repo();
        ff_core::close(
            &repo,
            &CloseOptions {
                message: Some(msg.into()),
                now: Some(NOW + n),
                argv: Vec::new(),
                ..Default::default()
            },
            &prov(),
        )
        .unwrap();
    }

    // Land on the operation BEFORE the first close: `ff op restore` names an
    // operation, so it lands on that one rather than before it.
    let repo = fx.repo();
    let ops = ff_core::ops::read_ops(&repo, 0).unwrap();
    let first_close = ops
        .iter()
        .find(|op| op.verb == "commit" && op.summary.contains("first close"))
        .unwrap();
    let target = ff_core::ops::OpLog::open(&repo)
        .unwrap()
        .resolve(&first_close.id[..10])
        .unwrap();
    let before_first = ff_core::ops::OpLog::open(&repo)
        .unwrap()
        .get(target)
        .unwrap()
        .prev()
        .unwrap();
    drop(repo);

    let report = run_op_restore(&fx, &before_first.to_string()[..10], NOW + 100);
    assert_eq!(
        report.stepped_ops, 2,
        "both closes stepped over: {report:?}"
    );
    // a.txt returns to its pre-first-close (dirty) state.
    assert_eq!(
        std::fs::read_to_string(fx.path().join("a.txt")).unwrap(),
        "change 1\n"
    );
    let (refs, head, _, _) = world_state(&fx);
    let (b_refs, b_head, _, _) = before_all;
    assert_eq!(refs, b_refs);
    assert_eq!(head, b_head);
}

#[test]
fn partial_application_converges_on_rerun() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    fx.git(&["branch", "other"]);
    ident(&fx);
    let repo = fx.repo();
    ff_core::ops::reconcile(&repo, NOW - 10).unwrap();
    let before = world_state(&fx);

    fx.write("a.txt", "work\n");
    let pre_close_status = fx.git(&["status", "--porcelain=v2"]);
    let repo = fx.repo();
    let (outcome, _) = ff_core::close(
        &repo,
        &CloseOptions {
            message: Some("landed".into()),
            now: Some(NOW),
            argv: Vec::new(),
            ..Default::default()
        },
        &prov(),
    )
    .unwrap();
    let CommitOutcome::Closed { id, .. } = outcome;
    let repo = fx.repo();
    let landing = ff_core::ops::OpLog::open(&repo)
        .unwrap()
        .tip()
        .unwrap()
        .unwrap();
    let landing = ff_core::ops::OpLog::open(&repo)
        .unwrap()
        .get(landing)
        .unwrap()
        .prev()
        .unwrap();
    drop(repo);

    // First undo succeeds; then simulate a crash that left main re-applied
    // (as if a partial redo died halfway): move main forward again by hand.
    run_undo(&fx);
    fx.git(&["update-ref", "refs/heads/main", &id]);
    fx.git(&["read-tree", &format!("{id}^{{tree}}")]);
    fx.git(&["checkout-index", "-a", "-f"]);

    // The next pass reconciles the divergence loudly, and a re-issued landing
    // on the same operation converges to the same pre-close state.
    let report = run_op_restore(&fx, &landing.to_string()[..10], NOW + 200);
    assert!(report.stepped >= 1, "{report:?}");
    let (refs, head, status, _stage) = world_state(&fx);
    let (b_refs, b_head, _b_status, _b_stage) = before;
    assert_eq!(refs, b_refs, "refs converge");
    assert_eq!(head, b_head);
    // The pre-close tree was dirty; convergence restores that dirt.
    assert_eq!(
        status, pre_close_status,
        "worktree converges to pre-close state"
    );
    assert_eq!(
        std::fs::read_to_string(fx.path().join("a.txt")).unwrap(),
        "work\n"
    );
}

/// A note marks something that happened rather than something that was done,
/// so a run of them is skipped and a log holding only notes has nothing to
/// step back over.
#[test]
fn undo_skips_notes_and_stops_at_the_floor() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    ident(&fx);
    let repo = fx.repo();
    ff_core::ops::reconcile(&repo, NOW).unwrap();

    // Only the init note exists: bare undo has nothing.
    let err = ff_core::undo(&repo, &opts(NOW + 1), &prov()).unwrap_err();
    assert!(
        matches!(err.id(), "undo/nothing" | "op/floor"),
        "nothing to step back over: {err}"
    );
}

/// Reverting a capture or a note is refused: neither moved a ref, so there is
/// nothing in either to invert. Undo does not land here, because runs of
/// captures are exactly what it steps over.
#[test]
fn revert_refuses_the_kinds_that_moved_no_ref() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    ident(&fx);
    let repo = fx.repo();
    ff_core::ops::reconcile(&repo, NOW).unwrap();
    let ops = ff_core::ops::read_ops(&repo, 0).unwrap();
    let note = ops.iter().find(|op| op.kind == "note").unwrap();
    let hex = ff_core::ops::OpLog::open(&repo)
        .unwrap()
        .resolve(&note.id[..10])
        .unwrap();
    drop(repo);

    let repo = fx.repo();
    let err = ff_core::revert(
        &repo,
        &hex.to_string()[..10],
        &ff_core::OpVerbOptions {
            now: Some(NOW + 2),
            argv: Vec::new(),
        },
        &prov(),
    )
    .unwrap_err();
    assert_eq!(err.id(), "undo/not-undoable", "{err}");
}

#[test]
fn describe_undo_restores_the_pending_text() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    ident(&fx);
    let repo = fx.repo();
    ff_core::describe::set_pending(&repo, Some("v1".into()), &prov(), Some(NOW), Vec::new())
        .unwrap();
    ff_core::describe::set_pending(&repo, Some("v2".into()), &prov(), Some(NOW + 1), Vec::new())
        .unwrap();
    run_undo(&fx);
    let meta = ff_core::branchmeta::read(&fx.repo(), "main").unwrap();
    assert_eq!(
        meta.pending_description.as_deref(),
        Some("v1"),
        "text rolled back"
    );

    // And forward again: the same replay, in the other direction.
    run_redo(&fx, NOW + 101);
    let meta = ff_core::branchmeta::read(&fx.repo(), "main").unwrap();
    assert_eq!(meta.pending_description.as_deref(), Some("v2"));
}
