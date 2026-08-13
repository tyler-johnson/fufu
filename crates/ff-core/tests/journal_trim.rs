//! Journal retention: expiry on the same `fufu.keep` knob, trash-first,
//! chain rebuilt with prev links rewritten, reflog replayed — and expiry
//! releases pins (the undo refuses with "trimmed" afterwards).

use ff_core::{CloseOptions, TrimOptions, UndoOptions};
use ff_testsupport::Fixture;

const T0: i64 = 1_700_000_000;
const DAY: i64 = 86_400;

fn ident(fx: &Fixture) {
    fx.set_config("user.name", "Trim User");
    fx.set_config("user.email", "trim@test");
}

fn prov() -> ff_core::Provenance {
    ff_core::Provenance::new("pre", None)
}

fn close_at(fx: &Fixture, msg: &str, when: i64) {
    let repo = fx.repo();
    ff_core::close(
        &repo,
        &CloseOptions {
            message: Some(msg.into()),
            now: Some(when),
            argv: Vec::new(),
            ..Default::default()
        },
        &prov(),
    )
    .unwrap();
}

/// Three ops spread over time: day 0, day 10, day 20.
fn aged_fixture() -> Fixture {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    ident(&fx);
    let repo = fx.repo();
    ff_core::journal::reconcile(&repo, T0).unwrap();
    for (n, when) in [(1, T0), (2, T0 + 10 * DAY), (3, T0 + 20 * DAY)] {
        fx.write("a.txt", &format!("change {n}\n"));
        close_at(&fx, &format!("close {n}"), when);
    }
    fx
}

#[test]
fn journal_expires_on_the_same_keep_cutoff() {
    let fx = aged_fixture();
    let repo = fx.repo();
    // Cutoff at day 15: init note + close 1 + close 2 expire, close 3 stays.
    let report = ff_core::trim(
        &repo,
        &TrimOptions {
            now: Some(T0 + 21 * DAY),
            keep_secs: Some(6 * DAY),
            ..Default::default()
        },
    )
    .unwrap();
    let journal = report.journal.expect("journal was trimmed");
    assert_eq!(journal.dropped, 3, "{journal:?}");
    assert_eq!(journal.kept, 1);
    assert_eq!(
        journal.trash_ref.as_deref(),
        Some("refs/fufu/trash/@journal")
    );

    // The surviving chain is well-formed: one entry, prev rewritten to None
    // (plus the trim note appended on top).
    let repo = fx.repo();
    let ops = ff_core::journal::read_ops(&repo, 0).unwrap();
    let kinds: Vec<(&str, &str)> = ops
        .iter()
        .map(|o| (o.kind.as_str(), o.verb.as_str()))
        .collect();
    assert_eq!(kinds, vec![("note", "trim"), ("op", "commit")], "{ops:?}");
    assert!(ops[1].summary.contains("close 3"));

    // Reconcile stays clean over the rebuilt chain.
    let after = ff_core::journal::reconcile(&repo, T0 + 22 * DAY).unwrap();
    assert!(after.is_quiet(), "{after:?}");
}

#[test]
fn trim_dry_run_writes_nothing_to_the_journal() {
    let fx = aged_fixture();
    let tip_before = fx.git(&["rev-parse", "refs/fufu/journal"]);
    let repo = fx.repo();
    let report = ff_core::trim(
        &repo,
        &TrimOptions {
            now: Some(T0 + 21 * DAY),
            keep_secs: Some(6 * DAY),
            dry_run: true,
            ..Default::default()
        },
    )
    .unwrap();
    let journal = report.journal.expect("reported");
    assert_eq!(journal.dropped, 3);
    assert_eq!(fx.git(&["rev-parse", "refs/fufu/journal"]), tip_before);
}

#[test]
fn whole_journal_expiry_deletes_the_ref_and_rebootstraps() {
    let fx = aged_fixture();
    let repo = fx.repo();
    let report = ff_core::trim(
        &repo,
        &TrimOptions {
            now: Some(T0 + 100 * DAY),
            keep_secs: Some(DAY),
            ..Default::default()
        },
    )
    .unwrap();
    let journal = report.journal.unwrap();
    assert!(journal.deleted);
    assert!(
        !fx.try_git(&["rev-parse", "--verify", "refs/fufu/journal"])
            .status
            .success()
    );
    // Next invocation bootstraps a fresh floor.
    let repo = fx.repo();
    let report = ff_core::journal::reconcile(&repo, T0 + 101 * DAY).unwrap();
    assert!(report.bootstrapped);
}

#[test]
fn expiry_releases_pins_and_the_floor_refuses_undo() {
    // Keep only the newest close: it becomes the journal floor — its
    // pre-state expired with the chain, so undoing it refuses.
    let fx = aged_fixture();
    let repo = fx.repo();
    ff_core::trim(
        &repo,
        &TrimOptions {
            now: Some(T0 + 21 * DAY),
            keep_secs: Some(6 * DAY),
            ..Default::default()
        },
    )
    .unwrap();
    let repo = fx.repo();
    let err = ff_core::undo(
        &repo,
        &UndoOptions {
            now: Some(T0 + 22 * DAY),
            ..Default::default()
        },
        &prov(),
    );
    match err {
        Err(e) => assert!(e.to_string().contains("floor"), "{e}"),
        Ok(_) => panic!("the floor must not be undoable"),
    }
}

#[test]
fn ops_above_the_floor_stay_undoable_after_a_trim() {
    // Keep the last two closes: the newer one still undoes cleanly over
    // the rebuilt chain.
    let fx = aged_fixture();
    let repo = fx.repo();
    ff_core::trim(
        &repo,
        &TrimOptions {
            now: Some(T0 + 21 * DAY),
            keep_secs: Some(12 * DAY),
            ..Default::default()
        },
    )
    .unwrap();
    let repo = fx.repo();
    let (report, _) = ff_core::undo(
        &repo,
        &UndoOptions {
            now: Some(T0 + 22 * DAY),
            ..Default::default()
        },
        &prov(),
    )
    .unwrap();
    assert!(report.target_summary.contains("close 3"), "{report:?}");
    let subject = fx.git(&["log", "-1", "--format=%s", "HEAD"]);
    assert_eq!(subject.trim(), "close 2");
}
