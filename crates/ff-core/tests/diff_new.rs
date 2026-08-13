//! `ff new` composition and never-guess resolution, plus `ff describe`
//! pending-description round trips.

use ff_core::{CommitOutcome, NewOptions, NewTarget};
use ff_testsupport::Fixture;

const NOW: i64 = 1_700_000_000;

fn ident(fx: &Fixture) {
    fx.set_config("user.name", "New User");
    fx.set_config("user.email", "new@test");
}

fn prov() -> ff_core::Provenance {
    ff_core::Provenance::new("pre", Some("ff new".into()))
}

fn run_new(fx: &Fixture, opts: NewOptions) -> ff_core::NewReport {
    let repo = fx.repo();
    let (report, _ctx) = ff_core::new(&repo, &opts, &prov()).unwrap();
    report
}

#[test]
fn bare_new_is_exactly_a_close() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    ident(&fx);
    fx.write("a.txt", "work\n");
    let report = run_new(
        &fx,
        NewOptions {
            now: Some(NOW),
            ..Default::default()
        },
    );
    assert!(report.switch.is_none());
    assert!(matches!(report.commit, CommitOutcome::Closed { .. }));
    assert_eq!(report.opened, "main");
    assert_eq!(fx.git(&["status", "--porcelain=v2"]), "", "fresh slate");
}

#[test]
fn new_with_message_sets_the_pending_description_of_the_opened_change() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    ident(&fx);
    fx.write("a.txt", "work\n");
    run_new(
        &fx,
        NewOptions {
            message: Some("next: the plan".into()),
            now: Some(NOW),
            ..Default::default()
        },
    );
    let meta = ff_core::branchmeta::read(&fx.repo(), "main").unwrap();
    assert_eq!(meta.pending_description.as_deref(), Some("next: the plan"));

    // The next close consumes it as its message.
    fx.write("a.txt", "more work\n");
    let repo = fx.repo();
    let (outcome, _) = ff_core::close(
        &repo,
        &ff_core::CloseOptions {
            now: Some(NOW + 10),
            ..Default::default()
        },
        &prov(),
    )
    .unwrap();
    let CommitOutcome::Closed { subject, .. } = outcome else {
        panic!("expected close");
    };
    assert_eq!(subject, "next: the plan");
}

#[test]
fn new_to_branch_parks_switches_and_closes_what_was_open_there() {
    let fx = Fixture::new();
    fx.write("shared.txt", "base\n");
    fx.commit("base");
    fx.git(&["branch", "feature"]);
    ident(&fx);

    // Dirty main; new to feature.
    fx.write("shared.txt", "main wip\n");
    let report = run_new(
        &fx,
        NewOptions {
            target: Some("feature".into()),
            now: Some(NOW),
            ..Default::default()
        },
    );
    let switch = report.switch.as_ref().unwrap();
    assert_eq!(switch.from, "main");
    assert_eq!(switch.to, "feature");
    assert!(switch.parked.is_some(), "main's open change parked");
    // Nothing was open on feature: close no-ops.
    assert!(matches!(
        report.commit,
        CommitOutcome::NothingToClose { .. }
    ));
    assert_eq!(report.opened, "feature");
    assert_eq!(fx.git(&["status", "--porcelain=v2"]), "");

    // Returning to main resumes and closes the parked change.
    fx.write("noise.txt", "irrelevant dirt on feature\n");
    let report = run_new(
        &fx,
        NewOptions {
            target: Some("main".into()),
            now: Some(NOW + 10),
            ..Default::default()
        },
    );
    let CommitOutcome::Closed { branch, .. } = &report.commit else {
        panic!("the resumed parked change closes: {report:?}");
    };
    assert_eq!(branch, "main");
    let head_file = fx.git(&["show", "HEAD:shared.txt"]);
    assert_eq!(head_file, "main wip\n", "parked work landed in the close");
}

#[test]
fn resolution_never_guesses() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    let first = fx.commit("first");
    fx.write("a.txt", "b\n");
    let second = fx.commit("second");
    fx.git(&["branch", "twin-one"]); // same tip as main
    fx.git(&["branch", "solo", &first]);
    ident(&fx);
    let repo = fx.repo();

    // Branch name → that branch.
    assert_eq!(
        ff_core::resolve_target(&repo, "solo", None).unwrap(),
        NewTarget::Existing("solo".into())
    );
    // Tip of exactly one branch → continue it.
    assert_eq!(
        ff_core::resolve_target(&repo, &first, None).unwrap(),
        NewTarget::Existing("solo".into())
    );
    // Tip shared by several → error listing them.
    let err = ff_core::resolve_target(&repo, &second, None).unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("twin-one") && msg.contains("main"), "{msg}");
    // Mid-stack commit → mint an anonymous branch there.
    fx.git(&["branch", "-D", "solo"]);
    let repo = fx.repo();
    match ff_core::resolve_target(&repo, &first, None).unwrap() {
        NewTarget::Mint { at, name, .. } => {
            assert_eq!(at.to_string(), first);
            assert!(name.starts_with("ff/"), "{name}");
        }
        other => panic!("expected Mint, got {other:?}"),
    }
    // -b names the minted branch.
    match ff_core::resolve_target(&repo, &first, Some("named-fork")).unwrap() {
        NewTarget::Mint { name, .. } => assert_eq!(name, "named-fork"),
        other => panic!("expected Mint, got {other:?}"),
    }
    // -b with an existing-branch target is contradictory.
    assert!(ff_core::resolve_target(&repo, "main", Some("x")).is_err());
    // Nonsense is an error.
    assert!(ff_core::resolve_target(&repo, "not-a-thing", None).is_err());
}

#[test]
fn new_at_rev_mints_anonymous_branch_with_fork_base() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    let first = fx.commit("first");
    fx.write("a.txt", "b\n");
    fx.commit("second");
    ident(&fx);

    let report = run_new(
        &fx,
        NewOptions {
            target: Some(first.clone()),
            now: Some(NOW),
            ..Default::default()
        },
    );
    let minted = report.minted.as_ref().expect("minted a branch");
    assert!(minted.starts_with("ff/"));
    assert_eq!(&report.opened, minted);
    assert_eq!(
        fx.git(&["symbolic-ref", "HEAD"]).trim(),
        format!("refs/heads/{minted}")
    );
    assert_eq!(fx.git(&["rev-parse", "HEAD"]).trim(), first);
    assert_eq!(
        std::fs::read_to_string(fx.path().join("a.txt")).unwrap(),
        "a\n",
        "worktree moved to the fork point"
    );
    let meta = ff_core::branchmeta::read(&fx.repo(), minted).unwrap();
    assert_eq!(meta.forked_from.as_deref(), Some(first.as_str()));
}

#[test]
fn bare_new_dash_b_advances_current_then_forks_at_the_tip() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    ident(&fx);
    fx.write("a.txt", "work to land on main\n");
    let report = run_new(
        &fx,
        NewOptions {
            branch: Some("spinoff".into()),
            now: Some(NOW),
            ..Default::default()
        },
    );
    let CommitOutcome::Closed { id, branch, .. } = &report.commit else {
        panic!("expected close");
    };
    assert_eq!(branch, "main", "close advanced main");
    assert_eq!(report.opened, "spinoff");
    assert_eq!(fx.git(&["rev-parse", "refs/heads/main"]).trim(), id);
    assert_eq!(
        fx.git(&["rev-parse", "refs/heads/spinoff"]).trim(),
        id,
        "fork at resulting tip"
    );
    assert_eq!(
        fx.git(&["symbolic-ref", "HEAD"]).trim(),
        "refs/heads/spinoff"
    );
}

#[test]
fn bare_new_dash_b_on_placeholder_claims_it() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    ident(&fx);
    fx.git(&["checkout", "-q", "-b", "ff/fleet-heron"]);
    fx.write("a.txt", "anon work\n");
    let report = run_new(
        &fx,
        NewOptions {
            branch: Some("proper".into()),
            now: Some(NOW),
            ..Default::default()
        },
    );
    let CommitOutcome::Closed { branch, .. } = &report.commit else {
        panic!("expected close");
    };
    assert_eq!(branch, "proper", "close landed on the claimed name");
    assert!(report.minted.is_none(), "a claim, not a fork");
    assert!(
        !fx.try_git(&["rev-parse", "--verify", "refs/heads/ff/fleet-heron"])
            .status
            .success()
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
