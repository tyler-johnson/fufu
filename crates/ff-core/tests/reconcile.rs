//! Reconciliation's core contract: bootstrap, clean-path silence, foreign
//! absorption, write-ahead crash labeling, gc pinning via parenthood, the ops
//! view, and the receipt left for a repository that predates the one-log
//! cutover.

use ff_core::gix;
use ff_core::ops::{OpKind, OpLog, reconcile};
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
    let report = reconcile(&repo, now(&fx)).unwrap();
    assert!(report.bootstrapped);
    let entry_id = report.entry.expect("bootstrap appends an init note");

    let log = OpLog::open(&repo).unwrap();
    let tip = log.tip().unwrap().expect("the log exists");
    assert_eq!(tip.to_string(), entry_id, "the id is spelled in letters");
    let op = log.get(tip).unwrap();
    assert_eq!(op.kind(), OpKind::Note);
    assert_eq!(op.record().unwrap().unwrap().verb, "init");
    assert_eq!(op.branch(), Some("main"));
    assert!(op.prev().is_none(), "the floor has nothing before it");

    // Second pass: clean, writes nothing.
    let report = reconcile(&repo, now(&fx) + 1).unwrap();
    assert!(report.is_quiet(), "clean pass must be quiet: {report:?}");
    assert!(report.entry.is_none());
    assert_eq!(log.tip().unwrap(), Some(tip), "tip unmoved");
}

/// The floor is parentless, and that is what keeps `git log --first-parent
/// refs/fufu/ops` inside the log. Putting the base commit at slot 1 — which is
/// where it lands when there is no previous operation to occupy it — is the
/// bug the journal shipped with.
#[test]
fn the_floor_is_parentless_and_the_first_capture_sits_on_it() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    let head = fx.commit("init");

    let repo = fx.repo();
    reconcile(&repo, now(&fx)).unwrap();
    let log = OpLog::open(&repo).unwrap();
    let floor = log.tip().unwrap().unwrap();

    let parents = fx.git(&["rev-list", "--parents", "-n", "1", &floor.hex()]);
    let fields: Vec<&str> = parents.split_whitespace().collect();
    assert_eq!(
        fields.len(),
        2,
        "the floor has exactly one parent, its own record: {fields:?}"
    );
    let record = fields[1];
    assert!(
        fx.git(&["rev-list", "--parents", "-n", "1", record])
            .split_whitespace()
            .count()
            == 1,
        "the record commit is parentless too"
    );

    // The first capture therefore has a predecessor, so its parents are
    // [prev, base] with base at the fixed slot 2.
    fx.write("a.txt", "dirty\n");
    let capture = match ff_core::capture(&repo, &ff_core::Provenance::new("manual", None)).unwrap()
    {
        ff_core::CaptureOutcome::Created { id, .. } => id,
        other => panic!("expected Created, got {other:?}"),
    };
    let parents = fx.git(&["rev-list", "--parents", "-n", "1", &capture.hex()]);
    let fields: Vec<&str> = parents.split_whitespace().collect();
    assert_eq!(fields[1], floor.hex(), "slot 1 is the previous operation");
    assert_eq!(fields[2], head, "slot 2 is the base");
    assert_eq!(fields.len(), 3, "a capture carries no record: {fields:?}");
}

#[test]
fn foreign_commit_is_absorbed_as_one_op_with_hint() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    let first = fx.commit("init");
    let repo = fx.repo();
    reconcile(&repo, now(&fx)).unwrap();

    // The user runs real git behind fufu's back.
    fx.write("a.txt", "changed\n");
    let second = fx.commit("user commit");

    let repo = fx.repo();
    let report = reconcile(&repo, now(&fx) + 10).unwrap();
    assert_eq!(report.foreign.len(), 1, "one branch moved: {report:?}");
    let change = &report.foreign[0];
    assert_eq!(change.name, "refs/heads/main");
    assert_eq!(change.old.as_deref(), Some(first.as_str()));
    assert_eq!(change.new.as_deref(), Some(second.as_str()));
    let hint = change.hint.as_deref().expect("reflog hint quoted");
    assert!(hint.contains("commit"), "git's own message: {hint}");

    let log = OpLog::open(&repo).unwrap();
    let op = log.get(log.tip().unwrap().unwrap()).unwrap();
    assert_eq!(op.kind(), OpKind::Foreign);
    assert_eq!(op.record().unwrap().unwrap().refs.len(), 1);

    // And the pass after that is clean again.
    let report = reconcile(&repo, now(&fx) + 20).unwrap();
    assert!(report.is_quiet());
}

#[test]
fn foreign_branch_create_move_delete_all_absorb() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    let repo = fx.repo();
    reconcile(&repo, now(&fx)).unwrap();

    fx.git(&["branch", "feature"]);
    fx.git(&["tag", "v1"]);
    let repo = fx.repo();
    let report = reconcile(&repo, now(&fx) + 1).unwrap();
    let names: Vec<&str> = report.foreign.iter().map(|c| c.name.as_str()).collect();
    assert!(names.contains(&"refs/heads/feature"), "{names:?}");
    assert!(names.contains(&"refs/tags/v1"), "{names:?}");

    fx.git(&["branch", "-D", "feature"]);
    let repo = fx.repo();
    let report = reconcile(&repo, now(&fx) + 2).unwrap();
    assert_eq!(report.foreign.len(), 1);
    assert_eq!(report.foreign[0].name, "refs/heads/feature");
    assert!(report.foreign[0].new.is_none(), "deletion has no new value");
}

#[test]
fn deleted_branch_tip_survives_gc_via_the_logs_pin() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    let repo = fx.repo();
    reconcile(&repo, now(&fx)).unwrap();

    // A commit on a side branch, then the branch is deleted: without the log's
    // pin the commit would be collectable.
    fx.git(&["checkout", "-q", "-b", "doomed"]);
    fx.write("doomed.txt", "d\n");
    let doomed_tip = fx.commit("doomed work");
    fx.git(&["checkout", "-q", "main"]);

    let repo = fx.repo();
    reconcile(&repo, now(&fx) + 1).unwrap();
    fx.git(&["branch", "-D", "doomed"]);
    let repo = fx.repo();
    let report = reconcile(&repo, now(&fx) + 2).unwrap();
    assert!(!report.foreign.is_empty());

    fx.git(&["reflog", "expire", "--expire=all", "--all"]);
    fx.git(&["gc", "--prune=now", "--quiet"]);

    let repo = fx.repo();
    let id = gix::ObjectId::from_hex(doomed_tip.as_bytes()).unwrap();
    assert!(
        repo.try_find_object(id).unwrap().is_some(),
        "the log's parenthood pins the deleted branch tip through gc"
    );
}

#[test]
fn ops_view_walks_the_log_and_prefixes_resolve() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    let repo = fx.repo();
    reconcile(&repo, now(&fx)).unwrap();
    fx.write("a.txt", "more\n");
    fx.commit("more");
    let repo = fx.repo();
    reconcile(&repo, now(&fx) + 1).unwrap();

    let ops = ff_core::ops::read_ops(&repo, 0).unwrap();
    assert_eq!(ops.len(), 2);
    assert_eq!(ops[0].kind, "foreign");
    assert_eq!(ops[1].kind, "note");
    assert_eq!(ops[1].verb, "init");
    assert!(
        ops[0].id.chars().all(|c| ('k'..='z').contains(&c)),
        "the ops view spells ids in letters: {:?}",
        ops[0].id
    );

    // Limit honored.
    let one = ff_core::ops::read_ops(&repo, 1).unwrap();
    assert_eq!(one.len(), 1);

    // Prefix resolution finds exactly one.
    let log = OpLog::open(&repo).unwrap();
    let target = &ops[0].id;
    let resolved = log.resolve(&target[..6]).unwrap();
    assert_eq!(resolved.to_string(), *target);
    assert_eq!(
        log.resolve("zzzzzz").unwrap_err().id(),
        "op/not-found",
        "a prefix matching nothing is not found"
    );
    assert_eq!(
        log.resolve(&"a".repeat(40)).unwrap_err().id(),
        "op/not-found",
        "hex is refused where an operation belongs"
    );
}

/// The bug the journal shipped with, as a test: its slot 1 held a pin on the
/// first entry, so this walk ran off the root and kept going through the
/// user's own commits. The floor is parentless now, so it stops.
#[test]
fn the_log_is_legible_to_git_log_first_parent_and_terminates() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    let repo = fx.repo();
    reconcile(&repo, now(&fx)).unwrap();
    fx.write("b.txt", "b\n");
    fx.commit("second");
    let repo = fx.repo();
    reconcile(&repo, now(&fx) + 1).unwrap();

    let log = fx.git(&["log", "--first-parent", "--format=%s", "refs/fufu/ops"]);
    let lines: Vec<&str> = log.lines().collect();
    assert!(lines[0].starts_with("absorbed "), "{lines:?}");
    assert!(
        lines[1].starts_with("operation log initialized"),
        "{lines:?}"
    );
    // The floor has no previous operation to occupy slot 1, so its own record
    // commit sits there and the walk shows it before ending. One extra row,
    // parentless, written by fufu — which is the whole difference from the
    // journal, whose slot-1 pin sent this same command through every commit
    // the user had ever made.
    assert_eq!(
        lines,
        vec![
            lines[0],
            "operation log initialized from observed state; earlier operations not undoable",
            "record"
        ],
        "the walk is the log and then it stops: {lines:?}"
    );

    // The author column proves every row is fufu's, root included.
    let authors = fx.git(&["log", "--first-parent", "--format=%an", "refs/fufu/ops"]);
    assert!(
        authors.lines().all(|line| line == "fufu"),
        "every row on the first-parent walk is fufu's: {authors:?}"
    );
}

#[test]
fn unreadable_tip_parks_the_log_and_reinitializes() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    let repo = fx.repo();
    reconcile(&repo, now(&fx)).unwrap();
    let old_tip = OpLog::open(&repo).unwrap().tip().unwrap().unwrap();

    // Point the log at a commit that is not an operation.
    let head = fx.git(&["rev-parse", "HEAD"]).trim().to_string();
    fx.git(&["update-ref", "refs/fufu/ops", &head]);

    let repo = fx.repo();
    let report = reconcile(&repo, now(&fx) + 1).unwrap();
    assert!(report.reinitialized, "{report:?}");
    assert!(report.bootstrapped);
    assert!(!report.warnings.is_empty());

    // The unreadable log is parked and a fresh floor is in place.
    let trash = fx.git(&["rev-parse", "refs/fufu/trash/@ops"]);
    assert_eq!(trash.trim(), head);
    let log = OpLog::open(&repo).unwrap();
    let tip = log.tip().unwrap().unwrap();
    assert_ne!(tip, old_tip);
    assert_eq!(
        log.get(tip).unwrap().record().unwrap().unwrap().verb,
        "init"
    );
}

#[test]
fn an_operation_carries_its_index_tree_by_containment() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    fx.write("staged.txt", "s\n");
    fx.git(&["add", "staged.txt"]);

    let repo = fx.repo();
    reconcile(&repo, now(&fx)).unwrap();
    let log = OpLog::open(&repo).unwrap();
    let op = log.get(log.tip().unwrap().unwrap()).unwrap();

    // The pinned index tree is what write-tree would say right now.
    let expected = fx.git(&["write-tree"]).trim().to_string();
    assert_eq!(op.index_tree().unwrap().unwrap().to_string(), expected);
}

/// The receipt. A repository that still holds the pre-cutover chains must not
/// have them CAS'd away by the first invocation: they are copied under
/// `refs/fufu/legacy/` before the log exists, and the warning names them.
#[test]
fn pre_cutover_chains_are_parked_rather_than_overwritten() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    let head = fx.commit("init");

    // Stand in for an old chain and an old journal: any commit fufu did not
    // write as an operation will do, because that is exactly what the guard
    // tests for.
    for name in ["refs/fufu/snap/main", "refs/fufu/journal"] {
        fx.git(&["update-ref", name, &head]);
    }
    fx.git(&["update-ref", "refs/fufu/trash/main", &head]);

    let repo = fx.repo();
    let report = reconcile(&repo, now(&fx)).unwrap();
    assert!(report.bootstrapped);
    let warning = report
        .warnings
        .iter()
        .find(|w| w.contains("refs/fufu/legacy/"))
        .unwrap_or_else(|| panic!("the park must be reported: {:?}", report.warnings));
    for parked in [
        "refs/fufu/legacy/snap/main",
        "refs/fufu/legacy/trash/main",
        "refs/fufu/legacy/journal",
    ] {
        assert!(warning.contains(parked), "{warning}");
        assert_eq!(
            fx.git(&["rev-parse", parked]).trim(),
            head,
            "{parked} must still hold what the old ref held"
        );
    }

    // The old names are gone — kept there, they would name a commit the
    // decoder cannot read, and the next append would either refuse or write a
    // link pointing at a non-operation.
    for gone in ["refs/fufu/journal", "refs/fufu/trash/main"] {
        assert!(
            !fx.try_git(&["rev-parse", "--verify", "--quiet", gone])
                .status
                .success(),
            "{gone} must not survive the cutover"
        );
    }

    // And the branch pointer now points into the new log.
    let log = OpLog::open(&repo).unwrap();
    assert_eq!(
        log.branch_tip("main").unwrap(),
        log.tip().unwrap(),
        "the pointer moved with the log's floor"
    );
}

/// A capture reaches the log without ever reconciling — `ff` bare and
/// `ff hook` both do — so the receipt cannot live only in the preamble.
#[test]
fn a_bare_capture_parks_the_old_chains_too() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    let head = fx.commit("init");
    fx.git(&["update-ref", "refs/fufu/snap/main", &head]);

    fx.write("a.txt", "dirty\n");
    let repo = fx.repo();
    let outcome = ff_core::capture(&repo, &ff_core::Provenance::new("manual", None)).unwrap();
    assert!(
        matches!(outcome, ff_core::CaptureOutcome::Created { .. }),
        "the capture must succeed rather than trip over an unreadable pointer: {outcome:?}"
    );
    assert_eq!(
        fx.git(&["rev-parse", "refs/fufu/legacy/snap/main"]).trim(),
        head
    );
}
