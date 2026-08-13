//! `ff undo`: rollback-to-state must be a true inverse (close→undo =
//! identity on refs, tree, and index), redo = undo-of-undo, foreign entries
//! undo labeled, later ops roll back with an older target, and partial
//! application converges on re-run (declarative target).

use ff_core::{CloseOptions, CommitOutcome, SwitchOptions, UndoOptions};
use ff_testsupport::Fixture;

const NOW: i64 = 1_700_000_000;

fn ident(fx: &Fixture) {
    fx.set_config("user.name", "Undo User");
    fx.set_config("user.email", "undo@test");
}

fn prov() -> ff_core::Provenance {
    ff_core::Provenance::new("pre", Some("ff test".into()))
}

fn run_undo(fx: &Fixture, op: Option<String>) -> ff_core::UndoReport {
    let repo = fx.repo();
    let (report, _ctx) = ff_core::undo(
        &repo,
        &UndoOptions {
            op,
            force: false,
            now: Some(NOW + 100),
            argv: vec!["ff".into(), "undo".into()],
        },
        &prov(),
    )
    .unwrap();
    report
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

#[test]
fn close_then_undo_is_identity_and_undo_undo_is_redo() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    ident(&fx);
    fx.write("a.txt", "the change\n");
    fx.write("new.txt", "untracked\n");

    // Baseline BEFORE the close (with the journal bootstrapped so the undo
    // has a floor).
    let repo = fx.repo();
    ff_core::journal::reconcile(&repo, NOW - 10).unwrap();
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
    let CommitOutcome::Closed { id, .. } = outcome else {
        panic!("expected close");
    };
    let after_close = world_state(&fx);
    assert_ne!(before, after_close);

    // Undo: the world returns to the pre-close state exactly.
    let report = run_undo(&fx, None);
    assert_eq!(report.target_kind, "op");
    assert!(report.target_summary.contains("landed"));
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

    // Redo: undoing the undo brings the close back.
    let report = run_undo(&fx, None);
    assert!(report.target_summary.contains("undo"), "{report:?}");
    assert_eq!(world_state(&fx), after_close, "undo→undo = redo");
    assert_eq!(fx.git(&["rev-parse", "HEAD"]).trim(), id);

    // The journal reconciles clean after all of it.
    let repo = fx.repo();
    let after = ff_core::journal::reconcile(&repo, NOW + 500).unwrap();
    assert!(after.is_quiet(), "{after:?}");
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
    ff_core::journal::reconcile(&repo, NOW - 10).unwrap();
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

    let undo_report = run_undo(&fx, None);
    assert!(undo_report.target_summary.contains("switch"));
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
    ff_core::journal::reconcile(&repo, NOW - 10).unwrap();

    // A commit made with real git...
    fx.write("a.txt", "user's commit\n");
    fx.commit("user work");

    // ...absorbed, then undone.
    let report = run_undo(&fx, None);
    assert_eq!(report.target_kind, "foreign", "labeled foreign");
    assert_eq!(
        fx.git(&["rev-parse", "HEAD"]).trim(),
        first,
        "branch rolled back"
    );
    // The worktree keeps the committed content? No — rollback-to-state:
    // pre-state had a clean tree at `first`. The user's content lives on in
    // the pinned commit and the journal.
    assert_eq!(fx.git(&["status", "--porcelain=v2"]), "");
    assert_eq!(
        std::fs::read_to_string(fx.path().join("a.txt")).unwrap(),
        "a\n"
    );
}

#[test]
fn undoing_an_older_op_rolls_back_everything_after_it() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    ident(&fx);
    let repo = fx.repo();
    ff_core::journal::reconcile(&repo, NOW - 10).unwrap();
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

    // Find the FIRST close's op id and undo it.
    let repo = fx.repo();
    let ops = ff_core::journal::read_ops(&repo, 0).unwrap();
    let first_close = ops
        .iter()
        .find(|op| op.verb == "commit" && op.summary.contains("first close"))
        .unwrap();
    let report = run_undo(&fx, Some(first_close.id[..10].to_string()));
    assert_eq!(report.rolled_back, 2, "the second close rolled back too");
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
    ff_core::journal::reconcile(&repo, NOW - 10).unwrap();
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
    let CommitOutcome::Closed { id, .. } = outcome else {
        panic!()
    };

    // First undo succeeds; then simulate a crash that left main re-applied
    // (as if a partial redo died halfway): move main forward again by hand.
    run_undo(&fx, None);
    fx.git(&["update-ref", "refs/heads/main", &id]);
    fx.git(&["read-tree", &format!("{id}^{{tree}}")]);
    fx.git(&["checkout-index", "-a", "-f"]);

    // The next undo pass reconciles the divergence loudly and a re-issued
    // undo of the close converges to the same pre-close state.
    let repo = fx.repo();
    let ops = ff_core::journal::read_ops(&repo, 0).unwrap();
    let close_op = ops
        .iter()
        .find(|op| op.verb == "commit" && op.summary.contains("landed"))
        .unwrap();
    let report = run_undo(&fx, Some(close_op.id[..10].to_string()));
    assert!(report.rolled_back >= 1);
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

#[test]
fn undo_refuses_notes_and_the_floor() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    ident(&fx);
    let repo = fx.repo();
    ff_core::journal::reconcile(&repo, NOW).unwrap();

    // Only the init note exists: bare undo has nothing.
    let err = ff_core::undo(
        &repo,
        &UndoOptions {
            now: Some(NOW + 1),
            ..Default::default()
        },
        &prov(),
    );
    assert!(err.is_err(), "nothing undoable yet");

    // Targeting the note by id also refuses.
    let ops = ff_core::journal::read_ops(&repo, 0).unwrap();
    let note = ops.iter().find(|op| op.kind == "note").unwrap();
    let err = ff_core::undo(
        &repo,
        &UndoOptions {
            op: Some(note.id[..10].to_string()),
            now: Some(NOW + 2),
            ..Default::default()
        },
        &prov(),
    );
    assert!(err.is_err(), "notes are not undoable");
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
    run_undo(&fx, None);
    let meta = ff_core::branchmeta::read(&fx.repo(), "main").unwrap();
    assert_eq!(
        meta.pending_description.as_deref(),
        Some("v1"),
        "text rolled back"
    );
}
