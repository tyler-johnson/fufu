//! The op log as a third party sees it: a complete reader, and no way in to
//! the writer. Everything reachable from here is what an extension, a
//! completion source, or `ff watch` gets — capture drives the floor, and the
//! rest is reads.

use ff_core::ops::{CaptureOutcome, OpKind, OpLog, capture};
use ff_core::{Provenance, TakeOptions};
use ff_testsupport::Fixture;

const NOW: i64 = 1_700_000_000;

fn snap(fx: &Fixture, now: i64) -> String {
    let repo = fx.repo();
    match capture(
        &repo,
        &Provenance::new("manual", None).with_session(Some("agent-7".into())),
        &TakeOptions {
            now: Some(now),
            max_file_size: None,
        },
    )
    .expect("capture")
    {
        CaptureOutcome::Created { id, .. } => id.to_string(),
        other => panic!("expected Created, got {other:?}"),
    }
}

#[test]
fn the_public_reader_walks_tags_and_resolves() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");

    let mut spelled = Vec::new();
    for i in 0..5 {
        fx.write("a.txt", &format!("v{i}\n"));
        spelled.push(snap(&fx, NOW + i));
    }

    let repo = fx.repo();
    let log = OpLog::open(&repo).expect("open");

    // Ids are letters wherever they surface, and never anything else.
    for id in &spelled {
        assert!(
            id.chars().all(|c| ('k'..='z').contains(&c)),
            "an op id must share no character with hex: {id}"
        );
    }

    // Laziness is the default: taking two decodes two.
    let newest: Vec<String> = log
        .iter()
        .take(2)
        .map(|op| op.expect("decode").id().to_string())
        .collect();
    assert_eq!(newest, [spelled[4].clone(), spelled[3].clone()]);

    // Every row carries its kind, its session tag and its base without
    // anyone fetching a record — captures have none.
    for op in log.iter() {
        let op = op.expect("decode");
        assert_eq!(op.kind(), OpKind::Capture);
        assert_eq!(op.session(), Some("agent-7"));
        assert!(op.base().is_some());
        assert!(op.record().expect("no record to read").is_none());
    }

    // Filtering is the caller's business, not a parameter.
    assert_eq!(
        log.iter()
            .filter(|op| op.as_ref().is_ok_and(|op| !op.is_capture()))
            .count(),
        0
    );

    assert_eq!(log.resolve("@").unwrap().to_string(), spelled[4]);
    assert_eq!(log.resolve("@~4").unwrap().to_string(), spelled[0]);
    assert_eq!(log.resolve(&spelled[2]).unwrap().to_string(), spelled[2]);
    let on_main: Vec<String> = log
        .iter_branch("main")
        .map(|op| op.expect("decode").id().to_string())
        .collect();
    assert_eq!(on_main.len(), 5, "the branch pointer reaches all five");
}

#[test]
fn every_stop_names_a_coded_id() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    fx.write("a.txt", "one\n");
    snap(&fx, NOW);

    let repo = fx.repo();
    let log = OpLog::open(&repo).expect("open");
    for (spec, id) in [
        ("zzzzzzzzzzzz", "op/not-found"),
        ("deadbeef", "op/not-found"),
        ("@~9", "op/floor"),
        ("@-", "op/not-found"),
        ("@^2", "usage/rev-in-op-position"),
    ] {
        let err = log.resolve(spec).unwrap_err();
        assert_eq!(err.id(), id, "{spec} reported {}: {err}", err.id());
        assert!(!err.exits().is_empty(), "{spec} named no way out");
    }
}
