//! Branch metadata round trips through the durable `jsonfile` writer, the
//! parent-carrying rename, and `ff start`'s parent recording.

use ff_core::branchmeta::{self, BranchMeta, Session};
use ff_testsupport::Fixture;

const NOW: i64 = 1_700_000_000;

fn ident(fx: &Fixture) {
    fx.set_config("user.name", "New User");
    fx.set_config("user.email", "new@test");
}

fn prov() -> ff_core::Provenance {
    ff_core::Provenance::new("pre", Some("ff start".into()))
}

fn run_start(fx: &Fixture, opts: ff_core::StartOptions) -> ff_core::StartReport {
    let repo = fx.repo();
    let (report, _ctx) = ff_core::start(&repo, &opts, &prov()).unwrap();
    report
}

fn populated() -> BranchMeta {
    BranchMeta {
        pending_description: Some("wip".into()),
        forked_from: Some("abc1234".into()),
        parent: Some("main".into()),
        session: None,
        held: None,
        resolving: None,
    }
}

#[test]
fn read_of_absent_branch_is_empty() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");

    let meta = branchmeta::read(&fx.repo(), "nobody").unwrap();
    assert_eq!(meta, BranchMeta::default());
    assert!(meta.is_empty());
}

#[test]
fn write_then_read_round_trips_every_field() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    let meta = populated();

    branchmeta::write(&fx.repo(), "base", &meta).unwrap();
    assert_eq!(branchmeta::read(&fx.repo(), "base").unwrap(), meta);
}

#[test]
fn a_session_round_trips() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    let mut meta = populated();
    meta.session = Some(Session {
        onto: "feature".into(),
        at: "a".repeat(40),
    });

    branchmeta::write(&fx.repo(), "base", &meta).unwrap();
    assert_eq!(branchmeta::read(&fx.repo(), "base").unwrap(), meta);
}

#[test]
fn a_session_alone_is_not_empty() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    let repo = fx.repo();
    let mut meta = BranchMeta {
        session: Some(Session {
            onto: "feature".into(),
            at: "a".repeat(40),
        }),
        ..Default::default()
    };
    assert!(!meta.is_empty());

    branchmeta::write(&repo, "base", &meta).unwrap();
    let path = repo.common_dir().join("fufu/branch").join("base");
    assert!(path.exists(), "session metadata lives at {path:?}");
    assert_eq!(branchmeta::read(&repo, "base").unwrap(), meta);

    meta.session = None;
    branchmeta::write(&repo, "base", &meta).unwrap();
    assert!(!path.exists(), "clearing the session deleted {path:?}");
}

#[test]
fn empty_metadata_deletes_the_file() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    let repo = fx.repo();
    branchmeta::write(&repo, "base", &populated()).unwrap();

    let path = repo.common_dir().join("fufu/branch").join("base");
    assert!(path.exists(), "populated metadata lives at {path:?}");

    branchmeta::write(&repo, "base", &BranchMeta::default()).unwrap();
    assert!(!path.exists(), "empty metadata deleted {path:?}");
}

#[test]
fn write_leaves_no_temp_file_behind() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    let repo = fx.repo();
    branchmeta::write(&repo, "base", &populated()).unwrap();

    let dir = repo.common_dir().join("fufu/branch");
    let leftovers: Vec<String> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|name| name.starts_with(".tmp-"))
        .collect();
    assert!(
        leftovers.is_empty(),
        "temp files left behind: {leftovers:?}"
    );
}

#[test]
fn corrupt_metadata_names_the_branch() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    let repo = fx.repo();

    let dir = repo.common_dir().join("fufu/branch");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("broken"), b"{not json").unwrap();

    let msg = branchmeta::read(&repo, "broken").unwrap_err().to_string();
    assert!(msg.contains("corrupt branch metadata for"), "{msg}");
    assert!(msg.contains("broken"), "{msg}");
}

#[test]
fn rename_carries_the_parent() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    let repo = fx.repo();
    branchmeta::write(&repo, "old", &populated()).unwrap();

    branchmeta::rename(&repo, "old", "new").unwrap();

    let moved = branchmeta::read(&repo, "new").unwrap();
    assert_eq!(moved.parent.as_deref(), Some("main"));
    let gone = branchmeta::read(&repo, "old").unwrap();
    assert!(gone.is_empty());
}

#[test]
fn a_slash_in_the_branch_name_round_trips() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    let meta = populated();

    branchmeta::write(&fx.repo(), "ff/witty-otter", &meta).unwrap();
    assert_eq!(
        branchmeta::read(&fx.repo(), "ff/witty-otter").unwrap(),
        meta
    );
}

#[test]
fn bare_start_records_no_parent() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    ident(&fx);

    let report = run_start(
        &fx,
        ff_core::StartOptions {
            target: None,
            branch: Some("feature".into()),
            now: Some(NOW),
            ..Default::default()
        },
    );
    assert_eq!(report.minted, "feature");

    let meta = branchmeta::read(&fx.repo(), "feature").unwrap();
    assert_eq!(meta.parent, None);
    assert!(meta.forked_from.is_some());
}

#[test]
fn start_from_a_named_branch_records_it_as_parent() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    fx.git(&["branch", "base"]);
    ident(&fx);

    let report = run_start(
        &fx,
        ff_core::StartOptions {
            target: Some("base".into()),
            branch: Some("stacked".into()),
            now: Some(NOW),
            ..Default::default()
        },
    );
    assert_eq!(report.minted, "stacked");

    let meta = branchmeta::read(&fx.repo(), "stacked").unwrap();
    assert_eq!(meta.parent.as_deref(), Some("base"));
}

#[test]
fn start_from_a_sha_records_no_parent() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    fx.git(&["branch", "base"]);
    ident(&fx);
    let sha = fx.git(&["rev-parse", "HEAD"]).trim().to_string();

    let report = run_start(
        &fx,
        ff_core::StartOptions {
            target: Some(sha.clone()),
            branch: Some("stacked".into()),
            now: Some(NOW),
            ..Default::default()
        },
    );
    assert_eq!(report.minted, "stacked");

    let meta = branchmeta::read(&fx.repo(), "stacked").unwrap();
    assert_eq!(meta.parent, None);
    let forked_from = meta.forked_from.as_deref().expect("forked_from recorded");
    assert!(
        sha.starts_with(forked_from),
        "forked_from ({forked_from}) should be a short sha of {sha}"
    );
}
