//! Operation-log retention: expiry on the `fufu.keep` knob, trash-first, the
//! log rebuilt with its three stated links rewritten, reflogs replayed — and
//! expiry releasing pins, so the floor refuses undo afterwards.

use ff_core::{CloseOptions, RewindOptions, TrimOptions};
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
            sign: Default::default(),
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
    ff_core::ops::reconcile(&repo, T0).unwrap();
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
    let log = report.log.expect("the log was trimmed");
    assert!(log.dropped >= 3, "{log:?}");
    assert!(log.kept >= 1, "{log:?}");
    assert_eq!(
        log.trash_ref.as_deref(),
        Some("refs/fufu/wt/main/trash/@ops")
    );

    // The surviving log is well-formed: the newest close, its prev rewritten
    // to nothing, plus the trim note appended on top.
    let repo = fx.repo();
    let ops = ff_core::ops::read_ops(&repo, 0).unwrap();
    let kinds: Vec<(&str, &str)> = ops
        .iter()
        .map(|o| (o.kind.as_str(), o.verb.as_str()))
        .collect();
    assert_eq!(kinds, vec![("note", "trim"), ("op", "commit")], "{ops:?}");
    assert!(ops[1].summary.contains("close 3"));

    // Reconcile stays clean over the rebuilt chain.
    let after = ff_core::ops::reconcile(&repo, T0 + 22 * DAY).unwrap();
    assert!(after.is_quiet(), "{after:?}");
}

#[test]
fn trim_dry_run_writes_nothing_to_the_log() {
    let fx = aged_fixture();
    let tip_before = fx.git(&["rev-parse", "refs/fufu/wt/main/ops"]);
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
    let log = report.log.expect("reported");
    assert!(log.dropped >= 3, "{log:?}");
    assert_eq!(fx.git(&["rev-parse", "refs/fufu/wt/main/ops"]), tip_before);
    assert!(
        !fx.try_git(&[
            "rev-parse",
            "--verify",
            "--quiet",
            "refs/fufu/wt/main/trash/@ops"
        ])
        .status
        .success(),
        "a dry run must not even park a trash tip"
    );
}

#[test]
fn whole_log_expiry_deletes_the_ref_and_rebootstraps() {
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
    let log = report.log.unwrap();
    assert!(log.deleted);
    for gone in ["refs/fufu/wt/main/ops", "refs/fufu/snap/main"] {
        assert!(
            !fx.try_git(&["rev-parse", "--verify", "--quiet", gone])
                .status
                .success(),
            "{gone} must go with the log it pointed into"
        );
    }
    // Next invocation bootstraps a fresh floor.
    let repo = fx.repo();
    let report = ff_core::ops::reconcile(&repo, T0 + 101 * DAY).unwrap();
    assert!(report.bootstrapped);
}

#[test]
fn expiry_releases_pins_and_the_floor_refuses_undo() {
    // One operation is left standing and it is the oldest on the log: what it
    // would roll back to expired with everything else, so undoing it refuses.
    //
    // The describe is what makes this constructible. A verb's preamble
    // captures first, and a capture shares its verb's timestamp — so any
    // cutoff that keeps a close keeps the capture underneath it too, and the
    // close is never the floor. On a clean tree the capture no-ops and writes
    // nothing, which leaves the describe alone at the cutoff.
    let fx = aged_fixture();
    let repo = fx.repo();
    ff_core::describe::set_pending(
        &repo,
        Some("alone above the cutoff".into()),
        &prov(),
        Some(T0 + 30 * DAY),
        Vec::new(),
    )
    .unwrap();

    let repo = fx.repo();
    let report = ff_core::trim(
        &repo,
        &TrimOptions {
            now: Some(T0 + 31 * DAY),
            keep_secs: Some(6 * DAY),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(report.log.as_ref().unwrap().kept, 1, "{report:?}");

    let repo = fx.repo();
    let err = ff_core::undo(
        &repo,
        &RewindOptions {
            now: Some(T0 + 32 * DAY),
            ..Default::default()
        },
        &prov(),
    );
    match err {
        Err(e) => assert_eq!(e.id(), "op/floor", "{e}"),
        Ok(_) => panic!("the floor must not be undoable"),
    }
}

#[test]
fn ops_above_the_floor_stay_undoable_after_a_trim() {
    // Keep the last two closes: the newer one still undoes cleanly over the
    // rebuilt log.
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
        &RewindOptions {
            now: Some(T0 + 22 * DAY),
            ..Default::default()
        },
        &prov(),
    )
    .unwrap();
    assert!(
        report
            .stepped_summary
            .as_deref()
            .unwrap()
            .contains("close 3"),
        "{report:?}"
    );
    let subject = fx.git(&["log", "-1", "--format=%s", "HEAD"]);
    assert_eq!(subject.trim(), "close 2");
}

/// A survivor keeps what it stated about the open commit: the rebuild
/// rewrites the links and nothing else, so `fufu-open` and the parent slot
/// that pins it come through, and the ref still names the same sha.
#[test]
fn a_survivor_keeps_its_open_commit() {
    let fx = aged_fixture();
    // A dirty capture above every close: the newest op on the log, and the
    // one the trim keeps.
    fx.write("a.txt", "open\n");
    let repo = fx.repo();
    let outcome = ff_core::capture_with(
        &repo,
        &ff_core::Provenance::new("manual", None),
        &ff_core::TakeOptions {
            now: Some(T0 + 20 * DAY + 60),
            max_file_size: None,
        },
    )
    .unwrap();
    let ff_core::CaptureOutcome::Created { id, .. } = outcome else {
        panic!("{outcome:?}");
    };
    let open = fx
        .git(&["rev-parse", "refs/fufu/open/main"])
        .trim()
        .to_string();
    let log = ff_core::ops::OpLog::open(&repo).unwrap();
    assert_eq!(
        log.get(id).unwrap().open_commit().map(|c| c.to_string()),
        Some(open.clone())
    );
    drop(log);

    ff_core::trim(
        &repo,
        &TrimOptions {
            now: Some(T0 + 21 * DAY),
            keep_secs: Some(2 * DAY),
            ..Default::default()
        },
    )
    .unwrap();

    let repo = fx.repo();
    let log = ff_core::ops::OpLog::open(&repo).unwrap();
    // The trim's own note sits on top; it left the tree alone, so it states
    // the same open commit. The survivor is the op beneath it.
    let note = log.get(log.tip().unwrap().expect("the trim note")).unwrap();
    assert_eq!(note.kind(), ff_core::ops::OpKind::Note, "{note:?}");
    assert_eq!(
        note.open_commit().map(|c| c.to_string()),
        Some(open.clone()),
        "the note reuses the sha rather than minting one"
    );
    let tip = note.prev().expect("a survivor");
    let survivor = log.get(tip).unwrap();
    assert!(survivor.is_capture(), "{survivor:?}");
    assert_eq!(
        survivor.open_commit().map(|c| c.to_string()),
        Some(open.clone()),
        "the rebuilt op still states its open commit"
    );
    let parents = fx.git(&["rev-list", "--parents", "-n", "1", &tip.to_string()]);
    assert_eq!(
        parents.split_whitespace().last(),
        Some(open.as_str()),
        "and still pins it: {parents}"
    );
    assert_eq!(
        fx.git(&["rev-parse", "refs/fufu/open/main"]).trim(),
        open,
        "the ref is not the trim's to move"
    );
    assert_eq!(
        ff_core::open_change(&repo).unwrap().pending.as_deref(),
        Some(open.as_str())
    );
}
