//! `ff start` — always mints a fresh branch, forking either at trunk (bare)
//! or at a resolved revision, and never carries the open change across the
//! fork. Plus `ff describe` pending-description round trips, unrelated to
//! start but hosted here alongside the rest of the composition tests.

use ff_core::{StartOptions, SwitchOptions};
use ff_testsupport::Fixture;

const NOW: i64 = 1_700_000_000;

fn ident(fx: &Fixture) {
    fx.set_config("user.name", "New User");
    fx.set_config("user.email", "new@test");
}

fn prov() -> ff_core::Provenance {
    ff_core::Provenance::new("pre", Some("ff start".into()))
}

fn run_start(fx: &Fixture, opts: StartOptions) -> ff_core::StartReport {
    let repo = fx.repo();
    let (report, _ctx) = ff_core::start(&repo, &opts, &prov()).unwrap();
    report
}

fn commit_count(fx: &Fixture, rev: &str) -> String {
    fx.git(&["rev-list", "--count", rev]).trim().to_string()
}

#[test]
fn bare_forks_trunk_and_opens_clean() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    fx.git(&["branch", "feature"]);
    fx.write("a.txt", "b\n");
    fx.commit("second"); // main advances past feature
    fx.git(&["checkout", "-q", "feature"]);
    ident(&fx);
    let main_tip = fx.git(&["rev-parse", "main"]).trim().to_string();
    let feature_tip = fx.git(&["rev-parse", "feature"]).trim().to_string();
    assert_ne!(
        main_tip, feature_tip,
        "test fixture: trunk must have moved on"
    );

    fx.write("a.txt", "dirty on feature\n");
    let report = run_start(
        &fx,
        StartOptions {
            now: Some(NOW),
            ..Default::default()
        },
    );

    assert!(report.minted.starts_with("ff/"), "{}", report.minted);
    assert_eq!(report.forked_from, "main");
    assert_eq!(
        fx.git(&["rev-parse", &format!("refs/heads/{}", report.minted)])
            .trim(),
        main_tip,
        "forked at trunk's tip, not the branch it was standing on"
    );
    assert_eq!(fx.git(&["status", "--porcelain=v2"]), "", "opens clean");
    assert!(report.parked.is_some());
}

#[test]
fn bare_parks_the_open_change_retrievably() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    fx.git(&["branch", "feature"]);
    fx.git(&["checkout", "-q", "feature"]);
    ident(&fx);
    fx.write("a.txt", "parked work\n");

    let report = run_start(
        &fx,
        StartOptions {
            now: Some(NOW),
            ..Default::default()
        },
    );
    assert!(report.parked.is_some());

    let repo = fx.repo();
    let (switch_report, _ctx) = ff_core::switch(
        &repo,
        &SwitchOptions {
            target: "feature".into(),
            now: Some(NOW + 10),
            argv: Vec::new(),
        },
        &prov(),
    )
    .unwrap();
    assert!(
        matches!(
            switch_report.arrival,
            ff_core::ArrivalReport::Restored { .. }
        ),
        "{:?}",
        switch_report.arrival
    );
    assert_eq!(
        std::fs::read_to_string(fx.path().join("a.txt")).unwrap(),
        "parked work\n"
    );
}

#[test]
fn bare_works_with_remote_only_trunk() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    fx.git(&["branch", "other"]);
    fx.git(&["checkout", "-q", "other"]);
    fx.git(&["branch", "-D", "main"]);
    let sha = fx.git(&["rev-parse", "other"]).trim().to_string();
    fx.git(&["update-ref", "refs/remotes/origin/main", &sha]);
    fx.git(&[
        "symbolic-ref",
        "refs/remotes/origin/HEAD",
        "refs/remotes/origin/main",
    ]);
    ident(&fx);
    fx.write("a.txt", "dirty on other\n");

    let report = run_start(
        &fx,
        StartOptions {
            now: Some(NOW),
            ..Default::default()
        },
    );
    assert_eq!(report.forked_from, "main");
    assert_eq!(
        fx.git(&["rev-parse", &format!("refs/heads/{}", report.minted)])
            .trim(),
        sha
    );
}

#[test]
fn at_is_rejected() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    ident(&fx);
    fx.write("a.txt", "dirty\n");

    let head_before = fx.git(&["symbolic-ref", "HEAD"]);
    let status_before = fx.git(&["status", "--porcelain=v2"]);
    let refs_before = fx.git(&["for-each-ref", "--format=%(refname) %(objectname)"]);

    let repo = fx.repo();
    let Err(err) = ff_core::start(
        &repo,
        &StartOptions {
            target: Some("@".into()),
            now: Some(NOW),
            ..Default::default()
        },
        &prov(),
    ) else {
        panic!("expected an error");
    };
    assert_eq!(
        err.to_string(),
        "@ is not a start target — ff start always opens a clean branch; \
         to move the open change onto its own branch, use ff commit -b <name>"
    );

    assert_eq!(
        fx.git(&["symbolic-ref", "HEAD"]),
        head_before,
        "HEAD unmoved"
    );
    assert_eq!(
        fx.git(&["status", "--porcelain=v2"]),
        status_before,
        "working tree untouched"
    );
    assert_eq!(
        fx.git(&["for-each-ref", "--format=%(refname) %(objectname)"]),
        refs_before,
        "no branch minted"
    );
    assert!(
        ff_core::stash::parked_entry(&repo, "main")
            .unwrap()
            .is_none(),
        "no parked entry created"
    );
}

#[test]
fn rev_target_forks_and_parks() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    let first = fx.commit("first");
    fx.write("a.txt", "b\n");
    fx.commit("second");
    ident(&fx);
    fx.write("a.txt", "dirty on main\n");

    let report = run_start(
        &fx,
        StartOptions {
            target: Some(first.clone()),
            now: Some(NOW),
            ..Default::default()
        },
    );

    assert!(report.minted.starts_with("ff/"), "{}", report.minted);
    assert_eq!(
        fx.git(&["rev-parse", &format!("refs/heads/{}", report.minted)])
            .trim(),
        first
    );
    assert!(report.parked.is_some());
    assert_eq!(fx.git(&["status", "--porcelain=v2"]), "", "opens clean");
    assert_eq!(
        std::fs::read_to_string(fx.path().join("a.txt")).unwrap(),
        "a\n",
        "worktree moved to the fork point"
    );
    assert!(
        first.starts_with(&report.forked_from),
        "forked_from ({}) should be a short sha of {first}",
        report.forked_from
    );
    let meta = ff_core::branchmeta::read(&fx.repo(), &report.minted).unwrap();
    assert_eq!(
        meta.forked_from.as_deref(),
        Some(report.forked_from.as_str())
    );
}

#[test]
fn branch_target_forks_instead_of_continuing() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    ident(&fx);
    let main_tip = fx.git(&["rev-parse", "main"]).trim().to_string();

    let report = run_start(
        &fx,
        StartOptions {
            target: Some("main".into()),
            now: Some(NOW),
            ..Default::default()
        },
    );

    assert_ne!(
        report.minted, "main",
        "a new branch is minted, not main itself"
    );
    assert_eq!(report.forked_from, "main");
    assert_eq!(
        fx.git(&["rev-parse", &format!("refs/heads/{}", report.minted)])
            .trim(),
        main_tip
    );
    assert_eq!(
        fx.git(&["rev-parse", "main"]).trim(),
        main_tip,
        "main itself is untouched"
    );
    assert_eq!(
        fx.git(&["symbolic-ref", "HEAD"]).trim(),
        format!("refs/heads/{}", report.minted)
    );
}

#[test]
fn dash_b_names_the_mint() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    ident(&fx);

    let report = run_start(
        &fx,
        StartOptions {
            branch: Some("hotfix".into()),
            now: Some(NOW),
            ..Default::default()
        },
    );
    assert_eq!(report.minted, "hotfix");
}

#[test]
fn dash_b_on_an_existing_name_errors() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    fx.git(&["branch", "existing"]);
    ident(&fx);

    let repo = fx.repo();
    let Err(err) = ff_core::start(
        &repo,
        &StartOptions {
            branch: Some("existing".into()),
            now: Some(NOW),
            ..Default::default()
        },
        &prov(),
    ) else {
        panic!("expected an error");
    };
    assert_eq!(err.to_string(), "a branch named existing already exists");
}

#[test]
fn every_start_mints() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    ident(&fx);

    let one = run_start(
        &fx,
        StartOptions {
            now: Some(NOW),
            ..Default::default()
        },
    );
    let two = run_start(
        &fx,
        StartOptions {
            now: Some(NOW + 10),
            ..Default::default()
        },
    );
    let three = run_start(
        &fx,
        StartOptions {
            now: Some(NOW + 20),
            ..Default::default()
        },
    );

    assert_ne!(one.minted, two.minted);
    assert_ne!(two.minted, three.minted);
    assert_ne!(one.minted, three.minted);
}

#[test]
fn start_never_commits() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    let first = fx.commit("first");
    fx.write("a.txt", "b\n");
    fx.commit("second");
    fx.git(&["branch", "other"]); // same tip as main
    ident(&fx);
    let main_before = commit_count(&fx, "main");
    let other_before = commit_count(&fx, "other");

    // Case 1: bare, forks trunk.
    fx.write("a.txt", "dirty1\n");
    run_start(
        &fx,
        StartOptions {
            now: Some(NOW),
            ..Default::default()
        },
    );
    assert_eq!(commit_count(&fx, "main"), main_before);
    assert_eq!(commit_count(&fx, "other"), other_before);

    // Case 5: rev target.
    fx.write("some.txt", "dirty2\n");
    let rev_report = run_start(
        &fx,
        StartOptions {
            target: Some(first.clone()),
            now: Some(NOW + 10),
            ..Default::default()
        },
    );
    assert_eq!(
        commit_count(&fx, &rev_report.minted),
        "1",
        "the mint carries exactly the fork commit, nothing more"
    );
    assert_eq!(commit_count(&fx, "main"), main_before);
    assert_eq!(commit_count(&fx, "other"), other_before);

    // Case 6: branch target.
    fx.write("more.txt", "dirty3\n");
    run_start(
        &fx,
        StartOptions {
            target: Some("other".into()),
            now: Some(NOW + 20),
            ..Default::default()
        },
    );
    assert_eq!(commit_count(&fx, "main"), main_before);
    assert_eq!(commit_count(&fx, "other"), other_before);
}

#[test]
fn message_describes_the_opened_change() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    ident(&fx);

    let report = run_start(
        &fx,
        StartOptions {
            message: Some("next: the plan".into()),
            now: Some(NOW),
            ..Default::default()
        },
    );
    let meta = ff_core::branchmeta::read(&fx.repo(), &report.minted).unwrap();
    assert_eq!(meta.pending_description.as_deref(), Some("next: the plan"));
    assert_eq!(
        fx.git(&[
            "log",
            "-1",
            "--format=%s",
            &format!("refs/heads/{}", report.minted)
        ])
        .trim(),
        "init",
        "the message lands as a pending description, not a commit"
    );
}

#[test]
fn describe_round_trips_and_journals() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    ident(&fx);
    let repo = fx.repo();
    let (report, _ctx) = ff_core::describe::set_pending(
        &repo,
        Some("the plan".into()),
        &prov(),
        Some(NOW),
        Vec::new(),
    )
    .unwrap();
    assert_eq!(report.new.as_deref(), Some("the plan"));
    assert_eq!(report.old, None);

    let tip = ff_core::journal::tip(&repo).unwrap().unwrap();
    let entry = ff_core::journal::read_entry(&repo, tip).unwrap();
    assert_eq!(entry.record.verb, "describe");
    let transition = entry.record.description.as_ref().unwrap();
    assert_eq!(transition.new.as_deref(), Some("the plan"));

    // Clearing round-trips; indexes untouched throughout.
    let index_before = fx.index_bytes();
    let (report, _ctx) =
        ff_core::describe::set_pending(&repo, None, &prov(), Some(NOW + 1), Vec::new()).unwrap();
    assert_eq!(report.old.as_deref(), Some("the plan"));
    assert_eq!(report.new, None);
    assert_eq!(
        fx.index_bytes(),
        index_before,
        "describe never touches the index"
    );
    let meta = ff_core::branchmeta::read(&repo, "main").unwrap();
    assert!(meta.is_empty());
}
