//! Contract for the held-rewrite record: what persists, what an empty meta
//! does with it, and that a metadata file written before holds existed still
//! reads.

use ff_core::branchmeta::{self, BranchMeta, Session};
use ff_core::futures::At;
use ff_core::held::{self, Held, Intent, Resolve};
use ff_testsupport::Fixture;

/// A concrete hold: a `Restack` that stopped on a commit.
fn a_hold() -> Held {
    Held {
        intent: Intent::Restack {
            branch: "feature".into(),
            onto: "main".into(),
        },
        at: At::Commit {
            id: "a".repeat(40),
            subject: "the stopped commit".into(),
        },
        paths: vec!["a.txt".into()],
        time: 1_799_999_999,
    }
}

/// A concrete resolution session: a hold, a marker-tree sha, three steps, and
/// the working tree it took possession of.
fn a_resolve() -> Resolve {
    Resolve {
        hold: a_hold(),
        from: "b".repeat(40),
        steps: vec!["first".into(), "second".into(), "third".into()],
        open: Some("c".repeat(40)),
    }
}

fn fixture() -> Fixture {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    fx
}

#[test]
fn a_hold_round_trips_through_the_metadata() {
    let fx = fixture();
    let repo = fx.repo();
    let h = a_hold();

    held::set(&repo, "base", Some(h.clone())).unwrap();
    assert_eq!(held::of(&repo, "base").unwrap(), Some(h));

    held::set(&repo, "base", None).unwrap();
    assert_eq!(held::of(&repo, "base").unwrap(), None);
}

#[test]
fn every_intent_round_trips() {
    let fx = fixture();
    let repo = fx.repo();
    let intents = [
        Intent::Restack {
            branch: "feature".into(),
            onto: "main".into(),
        },
        Intent::Done {
            session: "main".into(),
        },
        Intent::Absorb {
            into: "main".into(),
            paths: vec!["a.txt".into()],
        },
        Intent::Lift {
            from: "main".into(),
            paths: vec!["a.txt".into()],
        },
    ];

    for intent in intents {
        let h = Held {
            intent,
            at: At::OpenChange,
            paths: vec!["a.txt".into()],
            time: 1_799_999_999,
        };
        held::set(&repo, "base", Some(h.clone())).unwrap();
        assert_eq!(held::of(&repo, "base").unwrap(), Some(h));
    }
}

#[test]
fn a_hold_alone_keeps_the_file() {
    let fx = fixture();
    let repo = fx.repo();
    let path = repo.common_dir().join("fufu/branch").join("base");

    let meta = BranchMeta {
        held: Some(a_hold()),
        ..Default::default()
    };
    assert!(!meta.is_empty(), "a held rewrite alone is not empty");
    branchmeta::write(&repo, "base", &meta).unwrap();
    assert!(path.exists(), "hold metadata lives at {path:?}");
    assert_eq!(held::of(&repo, "base").unwrap(), meta.held);

    branchmeta::write(&repo, "base", &BranchMeta::default()).unwrap();
    assert!(!path.exists(), "clearing the hold deleted {path:?}");

    let rmeta = BranchMeta {
        resolving: Some(a_resolve()),
        ..Default::default()
    };
    assert!(!rmeta.is_empty(), "a resolution session alone is not empty");
    branchmeta::write(&repo, "base", &rmeta).unwrap();
    assert!(path.exists(), "resolution metadata lives at {path:?}");
    assert_eq!(held::resolving(&repo, "base").unwrap(), rmeta.resolving);

    branchmeta::write(&repo, "base", &BranchMeta::default()).unwrap();
    assert!(!path.exists(), "clearing the resolution deleted {path:?}");
}

#[test]
fn a_resolution_session_round_trips() {
    let fx = fixture();
    let repo = fx.repo();
    let r = a_resolve();

    held::set_resolving(&repo, "base", Some(r.clone())).unwrap();
    assert_eq!(held::resolving(&repo, "base").unwrap(), Some(r));
}

#[test]
fn a_hold_and_a_session_coexist() {
    let fx = fixture();
    let repo = fx.repo();
    let meta = BranchMeta {
        pending_description: Some("wip".into()),
        session: Some(Session {
            onto: "main".into(),
            at: "c".repeat(40),
        }),
        held: Some(a_hold()),
        resolving: Some(a_resolve()),
        ..Default::default()
    };
    branchmeta::write(&repo, "base", &meta).unwrap();

    let back = branchmeta::read(&repo, "base").unwrap();
    assert_eq!(back.pending_description, meta.pending_description);
    assert_eq!(back.session, meta.session);
    assert_eq!(back.held, meta.held);
    assert_eq!(back.resolving, meta.resolving);
}

#[test]
fn metadata_written_before_holds_existed_still_reads() {
    let fx = fixture();
    let repo = fx.repo();

    let path = repo.common_dir().join("fufu/branch").join("base");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, r#"{"pending_description":"hi"}"#).unwrap();

    let meta = branchmeta::read(&repo, "base").unwrap();
    assert_eq!(meta.pending_description.as_deref(), Some("hi"));
    assert_eq!(meta.held, None);
    assert_eq!(meta.resolving, None);
}

#[test]
fn a_held_record_serializes_without_its_absent_fields() {
    let meta = BranchMeta {
        pending_description: Some("hi".into()),
        ..Default::default()
    };
    let json = serde_json::to_string(&meta).unwrap();
    assert!(!json.contains("\"held\""), "{json}");
    assert!(!json.contains("\"resolving\""), "{json}");
}
