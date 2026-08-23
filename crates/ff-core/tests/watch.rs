//! Watching the operation log: the seven motions, and the one that has to be
//! tested before any of the others.
//!
//! Every test here is synchronous. There is no process, no sleeping and no
//! timing anywhere in this file, because `classify` is pure logic over two
//! refs and a chain of trailers — the loop that drives it belongs to the
//! verb, and a test that spawned one would be testing the clock.

use ff_core::ops::{OpId, OpLog};
use ff_core::watch::{self, Filter, Motion, Rewrite};
use ff_core::{CloseOptions, Provenance, RewindOptions, TrimOptions};
use ff_testsupport::Fixture;

const T0: i64 = 1_700_000_000;
const DAY: i64 = 86_400;

fn prov() -> Provenance {
    Provenance::new("pre", None)
}

/// A repository with an initialized operation log and nothing else.
fn started() -> Fixture {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    fx.set_config("user.name", "Watch User");
    fx.set_config("user.email", "watch@test");
    let repo = fx.repo();
    ff_core::ops::reconcile(&repo, T0).unwrap();
    fx
}

/// One capture, guaranteed to write: the tree changes first.
fn capture(fx: &Fixture, body: &str, session: Option<&str>) -> OpId {
    fx.write("a.txt", body);
    let repo = fx.repo();
    let prov = prov().with_session(session.map(str::to_string));
    match ff_core::capture(&repo, &prov).unwrap() {
        ff_core::CaptureOutcome::Created { id, .. } => id,
        other => panic!("expected a capture, got {other:?}"),
    }
}

/// One commit through the verb path, which appends a real `op`.
fn close_at(fx: &Fixture, msg: &str, when: i64) {
    fx.write("a.txt", &format!("{msg}\n"));
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

fn tip(fx: &Fixture) -> Option<OpId> {
    OpLog::open(&fx.repo()).unwrap().tip().unwrap()
}

fn trash(fx: &Fixture) -> Option<OpId> {
    OpLog::open(&fx.repo()).unwrap().trash_tip().unwrap()
}

fn classify(
    fx: &Fixture,
    last_seen: Option<OpId>,
    last_trash: Option<OpId>,
    filter: &Filter,
) -> watch::Watched {
    watch::classify(&fx.repo(), last_seen, last_trash, filter).unwrap()
}

fn landed_ids(motion: &[Motion]) -> Vec<String> {
    motion
        .iter()
        .filter_map(|m| match m {
            Motion::Landed { op } => Some(op.id.clone()),
            _ => None,
        })
        .collect()
}

#[test]
fn a_plain_append_lands_oldest_first() {
    let fx = started();
    let anchor = tip(&fx);
    let first = capture(&fx, "one\n", None);
    let second = capture(&fx, "two\n", None);

    let seen = classify(&fx, anchor, trash(&fx), &Filter::default());

    assert_eq!(
        landed_ids(&seen.motion),
        vec![first.to_string(), second.to_string()],
        "appends arrive oldest first"
    );
    assert_eq!(seen.tip, Some(second));
}

#[test]
fn a_still_log_reports_no_motion() {
    let fx = started();
    capture(&fx, "one\n", None);
    let anchor = tip(&fx);

    let seen = classify(&fx, anchor, trash(&fx), &Filter::default());

    assert!(seen.motion.is_empty(), "got {:?}", seen.motion);
    assert_eq!(seen.tip, anchor);
}

#[test]
fn an_undo_steps_back_over_what_it_left() {
    let fx = started();
    close_at(&fx, "one", T0 + DAY);
    close_at(&fx, "two", T0 + 2 * DAY);
    let anchor = tip(&fx).expect("a tip");

    let repo = fx.repo();
    ff_core::undo(
        &repo,
        &RewindOptions {
            now: Some(T0 + 3 * DAY),
            ..Default::default()
        },
        &prov(),
    )
    .unwrap();

    let landing = tip(&fx).expect("a tip after the undo");
    assert_ne!(landing, anchor, "the undo moved the pointer");

    // Derived through the public walk rather than hardcoded: undo appends one
    // or two operations onto the side it abandons before it moves, so the
    // exact number behind the anchor is not something this test should claim.
    let log = OpLog::open(&repo).unwrap();
    let expected = log
        .iter_from(anchor)
        .map(|op| op.unwrap().id())
        .take_while(|id| *id != landing)
        .count();
    assert!(expected > 0, "the landing must lie behind the anchor");

    let seen = classify(&fx, Some(anchor), trash(&fx), &Filter::default());

    assert_eq!(
        seen.motion,
        vec![Motion::SteppedBack {
            tip: Some(landing),
            over: expected,
        }]
    );
}

#[test]
fn work_after_an_undo_forks() {
    let fx = started();
    close_at(&fx, "one", T0 + DAY);
    close_at(&fx, "two", T0 + 2 * DAY);
    let anchor = tip(&fx).expect("a tip");

    let repo = fx.repo();
    ff_core::undo(
        &repo,
        &RewindOptions {
            now: Some(T0 + 3 * DAY),
            ..Default::default()
        },
        &prov(),
    )
    .unwrap();
    let fresh = capture(&fx, "after the undo\n", None);

    let seen = classify(&fx, Some(anchor), trash(&fx), &Filter::default());

    let Some(Motion::Forked { from, op }) = seen.motion.first() else {
        panic!("expected a fork, got {:?}", seen.motion);
    };
    assert_eq!(op.id, fresh.to_string(), "the fork carries its first op");

    // `from` is a real shared ancestor: it is on the chain behind the anchor.
    let log = OpLog::open(&repo).unwrap();
    let behind: Vec<OpId> = log.iter_from(anchor).map(|op| op.unwrap().id()).collect();
    assert!(
        behind.contains(from),
        "{from} must lie behind the anchor, which holds {behind:?}"
    );
}

#[test]
fn a_trim_is_a_rewrite_and_nothing_else() {
    let fx = started();
    close_at(&fx, "one", T0);
    close_at(&fx, "two", T0 + 10 * DAY);
    close_at(&fx, "three", T0 + 20 * DAY);
    let anchor = tip(&fx);
    let parked = trash(&fx);

    let repo = fx.repo();
    let report = ff_core::trim(
        &repo,
        &TrimOptions {
            now: Some(T0 + 21 * DAY),
            keep_secs: Some(6 * DAY),
            ..Default::default()
        },
    )
    .unwrap();
    assert!(
        report.log.as_ref().is_some_and(|row| row.dropped > 0),
        "the trim must have dropped something: {report:?}"
    );

    let seen = classify(&fx, anchor, parked, &Filter::default());

    // Exactly one: the operations trim replayed carry rewritten ids, and a
    // watcher that reported them as appends would be handing a subscriber
    // rows it can never resolve back to what it already holds.
    assert_eq!(seen.motion.len(), 1, "got {:?}", seen.motion);
    assert!(
        matches!(
            seen.motion[0],
            Motion::Rewritten {
                reason: Rewrite::Trim,
                ..
            }
        ),
        "got {:?}",
        seen.motion[0]
    );
}

#[test]
fn a_deleted_ops_ref_is_a_reset() {
    let fx = started();
    capture(&fx, "one\n", None);
    let anchor = tip(&fx);
    let parked = trash(&fx);

    fx.git(&["update-ref", "-d", "refs/fufu/wt/main/ops"]);

    let seen = classify(&fx, anchor, parked, &Filter::default());

    assert_eq!(
        seen.motion,
        vec![Motion::Rewritten {
            reason: Rewrite::Reset,
            tip: None,
        }]
    );
}

#[test]
fn the_kind_filter_keeps_only_its_kind() {
    let fx = started();
    let anchor = tip(&fx);
    capture(&fx, "loose work\n", None);
    close_at(&fx, "a commit", T0 + DAY);

    let only_ops = classify(
        &fx,
        anchor,
        trash(&fx),
        &Filter::new(Some("op"), None).unwrap(),
    );
    let ops = rows(&only_ops.motion);
    assert!(!ops.is_empty(), "the commit must land");
    assert!(ops.iter().all(|row| row.kind == "op"), "got {ops:?}");

    let only_captures = classify(
        &fx,
        anchor,
        trash(&fx),
        &Filter::new(Some("capture"), None).unwrap(),
    );
    let captures = rows(&only_captures.motion);
    assert!(!captures.is_empty(), "the capture must land");
    assert!(
        captures.iter().all(|row| row.kind == "capture"),
        "got {captures:?}"
    );

    assert!(
        Filter::new(Some("nonsense"), None).is_err(),
        "an unknown kind is refused, not silently matched against nothing"
    );
}

#[test]
fn the_session_filter_keeps_only_its_session() {
    let fx = started();
    let anchor = tip(&fx);
    capture(&fx, "alpha work\n", Some("alpha"));
    capture(&fx, "beta work\n", Some("beta"));

    let seen = classify(
        &fx,
        anchor,
        trash(&fx),
        &Filter::new(None, Some("alpha".into())).unwrap(),
    );

    let rows = rows(&seen.motion);
    assert_eq!(rows.len(), 1, "got {rows:?}");
    assert_eq!(rows[0].session.as_deref(), Some("alpha"));
}

fn rows(motion: &[Motion]) -> Vec<ff_core::OpEntry> {
    motion
        .iter()
        .filter_map(|m| match m {
            Motion::Landed { op } => Some((**op).clone()),
            _ => None,
        })
        .collect()
}
