//! Journal core contract: bootstrap, clean-path silence, foreign absorption,
//! write-ahead crash labeling, gc pinning via parenthood, and the ops view.

use ff_core::gix;
use ff_core::journal::{self, OpKind, OpRecord, RefTransition};
use ff_testsupport::Fixture;

fn now(fx: &Fixture) -> i64 {
    // Any monotone value works; the fixture clock is opaque here, so use a
    // fixed late timestamp.
    let _ = fx;
    1_700_000_000
}

#[test]
fn bootstrap_then_clean_pass_writes_nothing() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");

    let repo = fx.repo();
    let report = journal::reconcile(&repo, now(&fx)).unwrap();
    assert!(report.bootstrapped);
    let entry_id = report.entry.expect("bootstrap appends an init note");

    let tip = journal::tip(&repo).unwrap().expect("journal exists");
    assert_eq!(tip.to_string(), entry_id);
    let entry = journal::read_entry(&repo, tip).unwrap();
    assert_eq!(entry.record.kind, OpKind::Note);
    assert_eq!(entry.record.verb, "init");
    assert_eq!(entry.record.branch.as_deref(), Some("main"));
    assert!(entry.prev.is_none());

    // Second pass: clean, writes nothing.
    let report = journal::reconcile(&repo, now(&fx) + 1).unwrap();
    assert!(report.is_quiet(), "clean pass must be quiet: {report:?}");
    assert!(report.entry.is_none());
    assert_eq!(journal::tip(&repo).unwrap(), Some(tip), "tip unmoved");
}

#[test]
fn foreign_commit_is_absorbed_as_one_entry_with_hint() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    let first = fx.commit("init");
    let repo = fx.repo();
    journal::reconcile(&repo, now(&fx)).unwrap();

    // The user runs real git behind fufu's back.
    fx.write("a.txt", "changed\n");
    let second = fx.commit("user commit");

    let repo = fx.repo();
    let report = journal::reconcile(&repo, now(&fx) + 10).unwrap();
    assert_eq!(report.foreign.len(), 1, "one branch moved: {report:?}");
    let change = &report.foreign[0];
    assert_eq!(change.name, "refs/heads/main");
    assert_eq!(change.old.as_deref(), Some(first.as_str()));
    assert_eq!(change.new.as_deref(), Some(second.as_str()));
    let hint = change.hint.as_deref().expect("reflog hint quoted");
    assert!(hint.contains("commit"), "git's own message: {hint}");

    let tip = journal::tip(&repo).unwrap().unwrap();
    let entry = journal::read_entry(&repo, tip).unwrap();
    assert_eq!(entry.record.kind, OpKind::Foreign);
    assert_eq!(entry.record.refs.len(), 1);

    // And the pass after that is clean again.
    let report = journal::reconcile(&repo, now(&fx) + 20).unwrap();
    assert!(report.is_quiet());
}

#[test]
fn foreign_branch_create_move_delete_all_absorb() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    let repo = fx.repo();
    journal::reconcile(&repo, now(&fx)).unwrap();

    fx.git(&["branch", "feature"]);
    fx.git(&["tag", "v1"]);
    let repo = fx.repo();
    let report = journal::reconcile(&repo, now(&fx) + 1).unwrap();
    let names: Vec<&str> = report.foreign.iter().map(|c| c.name.as_str()).collect();
    assert!(names.contains(&"refs/heads/feature"), "{names:?}");
    assert!(names.contains(&"refs/tags/v1"), "{names:?}");

    fx.git(&["branch", "-D", "feature"]);
    let repo = fx.repo();
    let report = journal::reconcile(&repo, now(&fx) + 2).unwrap();
    assert_eq!(report.foreign.len(), 1);
    assert_eq!(report.foreign[0].name, "refs/heads/feature");
    assert!(report.foreign[0].new.is_none(), "deletion has no new value");
}

#[test]
fn deleted_branch_tip_survives_gc_via_journal_pin() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    let repo = fx.repo();
    journal::reconcile(&repo, now(&fx)).unwrap();

    // A commit on a side branch, then the branch is deleted: without the
    // journal pin the commit would be collectable.
    fx.git(&["checkout", "-q", "-b", "doomed"]);
    fx.write("doomed.txt", "d\n");
    let doomed_tip = fx.commit("doomed work");
    fx.git(&["checkout", "-q", "main"]);

    let repo = fx.repo();
    journal::reconcile(&repo, now(&fx) + 1).unwrap();
    fx.git(&["branch", "-D", "doomed"]);
    let repo = fx.repo();
    let report = journal::reconcile(&repo, now(&fx) + 2).unwrap();
    assert!(!report.foreign.is_empty());

    fx.git(&["reflog", "expire", "--expire=all", "--all"]);
    fx.git(&["gc", "--prune=now", "--quiet"]);

    let repo = fx.repo();
    let id = gix::ObjectId::from_hex(doomed_tip.as_bytes()).unwrap();
    assert!(
        repo.try_find_object(id).unwrap().is_some(),
        "journal parenthood pins the deleted branch tip through gc"
    );
}

#[test]
fn write_ahead_crash_is_labeled_incomplete() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    let head = fx.commit("init");
    let repo = fx.repo();
    journal::reconcile(&repo, now(&fx)).unwrap();

    // Simulate a verb that journaled its plan and died before mutating:
    // planned table says main moved to a new sha; reality still has `head`.
    fx.write("a.txt", "planned\n");
    fx.git(&["add", "-A"]);
    let planned_tree = fx.git(&["write-tree"]).trim().to_string();
    let planned = fx
        .git(&["commit-tree", &planned_tree, "-p", &head, "-m", "planned"])
        .trim()
        .to_string();
    fx.git(&["reset", "-q", "--mixed", &head]); // leave reality at `head`

    let repo = fx.repo();
    let mut table = journal::observe_refs(&repo).unwrap();
    table.refs.insert("refs/heads/main".into(), planned.clone());
    let mut record = OpRecord::new(OpKind::Op, "commit", "close on main (test)", now(&fx) + 1);
    record.refs = vec![RefTransition {
        name: "refs/heads/main".into(),
        old: Some(head.clone()),
        new: Some(planned.clone()),
    }];
    let index_tree = ff_core::index::tree_from_index(&repo).unwrap();
    let planned_id = gix::ObjectId::from_hex(planned.as_bytes()).unwrap();
    journal::append(
        &repo,
        &record,
        &table,
        index_tree,
        &[planned_id],
        now(&fx) + 1,
    )
    .unwrap();

    let report = journal::reconcile(&repo, now(&fx) + 2).unwrap();
    assert_eq!(report.foreign.len(), 1);
    let tip = journal::tip(&repo).unwrap().unwrap();
    let entry = journal::read_entry(&repo, tip).unwrap();
    assert!(
        entry.record.summary.contains("may not have completed"),
        "crash between append and mutation is loud: {}",
        entry.record.summary
    );
}

#[test]
fn ops_view_walks_first_parent_and_prefixes_resolve() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    let repo = fx.repo();
    journal::reconcile(&repo, now(&fx)).unwrap();
    fx.write("a.txt", "more\n");
    fx.commit("more");
    let repo = fx.repo();
    journal::reconcile(&repo, now(&fx) + 1).unwrap();

    let ops = journal::read_ops(&repo, 0).unwrap();
    assert_eq!(ops.len(), 2);
    assert_eq!(ops[0].kind, "foreign");
    assert_eq!(ops[1].kind, "note");
    assert_eq!(ops[1].verb, "init");

    // Limit honored.
    let one = journal::read_ops(&repo, 1).unwrap();
    assert_eq!(one.len(), 1);

    // Prefix resolution finds exactly one.
    let target = &ops[0].id;
    let resolved = journal::resolve_op_prefix(&repo, &target[..8]).unwrap();
    assert_eq!(resolved.to_string(), *target);
    assert!(journal::resolve_op_prefix(&repo, "zzzzzz").is_err());
}

#[test]
fn journal_is_legible_to_git_log_first_parent() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    let repo = fx.repo();
    journal::reconcile(&repo, now(&fx)).unwrap();
    fx.write("b.txt", "b\n");
    fx.commit("second");
    let repo = fx.repo();
    journal::reconcile(&repo, now(&fx) + 1).unwrap();

    let log = fx.git(&["log", "--first-parent", "--format=%s", "refs/fufu/journal"]);
    let lines: Vec<&str> = log.lines().collect();
    // Entries first; the bootstrap's parent 1 is a pin, so the walk then
    // continues into the pinned repository history — legible, by design.
    assert!(lines.len() >= 2, "{lines:?}");
    assert!(lines[0].starts_with("foreign: "), "{lines:?}");
    assert!(lines[1].starts_with("note: "), "{lines:?}");
}

#[test]
fn corrupt_tip_parks_chain_and_reinitializes() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    let repo = fx.repo();
    journal::reconcile(&repo, now(&fx)).unwrap();
    let old_tip = journal::tip(&repo).unwrap().unwrap();

    // Point the journal at a non-entry commit (no op.json).
    let head = fx.git(&["rev-parse", "HEAD"]).trim().to_string();
    fx.git(&["update-ref", "refs/fufu/journal", &head]);

    let repo = fx.repo();
    let report = journal::reconcile(&repo, now(&fx) + 1).unwrap();
    assert!(report.reinitialized, "{report:?}");
    assert!(report.bootstrapped);
    assert!(!report.warnings.is_empty());

    // The corrupt chain is parked, a fresh init note is in place.
    let trash = fx.git(&["rev-parse", "refs/fufu/trash/@journal"]);
    assert_eq!(trash.trim(), head);
    let tip = journal::tip(&repo).unwrap().unwrap();
    assert_ne!(tip, old_tip);
    let entry = journal::read_entry(&repo, tip).unwrap();
    assert_eq!(entry.record.verb, "init");
}

#[test]
fn entry_carries_index_tree_by_containment() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    fx.write("staged.txt", "s\n");
    fx.git(&["add", "staged.txt"]);

    let repo = fx.repo();
    journal::reconcile(&repo, now(&fx)).unwrap();
    let tip = journal::tip(&repo).unwrap().unwrap();
    let entry = journal::read_entry(&repo, tip).unwrap();

    // The pinned index tree is what write-tree would say right now.
    let expected = fx.git(&["write-tree"]).trim().to_string();
    assert_eq!(entry.index_tree.to_string(), expected);
}
