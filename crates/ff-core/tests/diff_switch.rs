//! Differential contract for the switch: on a clean tree, `ff switch` must
//! land the worktree, index, and HEAD exactly where `git switch` does
//! (index compared semantically, worktree byte-for-byte via status). On a
//! dirty tree it must equal git's stash dance: stash push -u, switch,
//! and — when returning — stash pop --index.

use ff_core::{ArrivalReport, SwitchOptions};
use ff_testsupport::{Fixture, scenarios};

const NOW: i64 = 1_700_000_000;

fn ident(fx: &Fixture) {
    fx.set_config("user.name", "Switch User");
    fx.set_config("user.email", "switch@test");
}

fn switch_to(fx: &Fixture, target: &str) -> ff_core::SwitchReport {
    let repo = fx.repo();
    let (report, _ctx) = ff_core::switch(
        &repo,
        &SwitchOptions {
            target: target.into(),
            now: Some(NOW),
            argv: vec!["ff".into(), "switch".into(), target.into()],
        },
        &ff_core::Provenance::new("pre", Some(format!("ff switch {target}"))),
    )
    .unwrap();
    report
}

/// A two-branch fixture: `main` and `feature` diverge over shared files.
fn two_branch_fixture() -> Fixture {
    let fx = Fixture::new();
    fx.write("shared.txt", "base\n");
    fx.write("main-only.txt", "main\n");
    fx.write("dir/deep.txt", "deep\n");
    fx.commit("base");
    fx.git(&["checkout", "-q", "-b", "feature"]);
    fx.write("shared.txt", "feature version\n");
    fx.write("feature-only.txt", "feature\n");
    fx.remove("main-only.txt");
    fx.git(&["add", "-A"]);
    fx.git(&["commit", "-q", "-m", "feature work"]);
    fx.git(&["checkout", "-q", "main"]);
    ident(&fx);
    fx
}

#[test]
fn clean_switch_matches_git_switch() {
    let fx_ff = two_branch_fixture();
    let fx_git = two_branch_fixture();

    let report = switch_to(&fx_ff, "feature");
    assert_eq!(report.from, "main");
    assert_eq!(report.to, "feature");
    assert!(report.parked.is_none());
    fx_git.git(&["switch", "-q", "feature"]);

    assert_eq!(
        fx_ff.git(&["symbolic-ref", "HEAD"]).trim(),
        fx_git.git(&["symbolic-ref", "HEAD"]).trim()
    );
    assert_eq!(
        fx_ff.git(&["status", "--porcelain=v2"]),
        fx_git.git(&["status", "--porcelain=v2"]),
        "worktree agreement (both clean)"
    );
    assert_eq!(
        fx_ff.git(&["ls-files", "--stage"]),
        fx_git.git(&["ls-files", "--stage"]),
        "index agreement"
    );
    // Files really moved on disk.
    assert_eq!(
        std::fs::read_to_string(fx_ff.path().join("shared.txt")).unwrap(),
        "feature version\n"
    );
    assert!(!fx_ff.path().join("main-only.txt").exists());
    assert!(fx_ff.path().join("feature-only.txt").exists());
}

#[test]
fn dirty_switch_equals_git_stash_dance_round_trip() {
    let fx_ff = two_branch_fixture();
    let fx_git = two_branch_fixture();

    // Same dirt on both: staged + unstaged + untracked.
    for fx in [&fx_ff, &fx_git] {
        fx.write("shared.txt", "staged edit\n");
        fx.git(&["add", "shared.txt"]);
        fx.write("shared.txt", "staged then more\n");
        fx.write("untracked.txt", "loose\n");
    }

    // fufu: switch away and back.
    let away = switch_to(&fx_ff, "feature");
    assert!(away.parked.is_some(), "dirty tree parks");
    assert_eq!(
        fx_ff.git(&["status", "--porcelain=v2"]),
        "",
        "clean on arrival"
    );
    let back = switch_to(&fx_ff, "main");
    assert!(
        matches!(back.arrival, ArrivalReport::Restored { .. }),
        "{:?}",
        back.arrival
    );

    // git: the equivalent dance.
    fx_git.git(&["stash", "push", "-q", "-u", "-m", "fufu: wip on main"]);
    fx_git.git(&["switch", "-q", "feature"]);
    fx_git.git(&["switch", "-q", "main"]);
    fx_git.git(&["stash", "pop", "-q", "--index"]);

    assert_eq!(
        fx_ff.git(&["status", "--porcelain=v2"]),
        fx_git.git(&["status", "--porcelain=v2"]),
        "round-trip status equals git's stash dance"
    );
    assert_eq!(
        fx_ff.git(&["ls-files", "--stage"]),
        fx_git.git(&["ls-files", "--stage"]),
        "round-trip index equals git's stash dance"
    );
    assert_eq!(
        std::fs::read_to_string(fx_ff.path().join("shared.txt")).unwrap(),
        "staged then more\n"
    );
}

#[test]
fn matrix_dirty_switch_round_trip_from_scenarios() {
    // Every stashable scenario shape must survive switch-away-and-back.
    for (name, setup) in scenarios() {
        let fx = Fixture::new();
        setup(&fx);
        ident(&fx);
        let repo = fx.repo();
        let head = ff_core::head_state(&repo).unwrap();
        if !matches!(head, ff_core::HeadState::Branch { .. }) {
            continue;
        }
        if ff_core::operation(&repo).is_some() {
            continue; // switch refuses mid-merge; covered below
        }
        let current = match &head {
            ff_core::HeadState::Branch { name, .. } => name.clone(),
            _ => unreachable!(),
        };
        // Park refusals (intent-to-add) surface as switch errors — skip.
        if ff_core::stash::plan_park(&repo, &head, NOW).is_err() {
            continue;
        }
        fx.git(&["branch", "elsewhere"]);
        let before = (
            fx.git(&["status", "--porcelain=v2"]),
            fx.git(&["ls-files", "--stage"]),
        );

        switch_to(&fx, "elsewhere");
        assert_eq!(
            fx.git(&["status", "--porcelain=v2"]),
            "",
            "scenario {name}: clean after switching away"
        );
        switch_to(&fx, &current);
        assert_eq!(
            (
                fx.git(&["status", "--porcelain=v2"]),
                fx.git(&["ls-files", "--stage"]),
            ),
            before,
            "scenario {name}: switch round trip is identity"
        );
    }
}

#[test]
fn switch_refuses_mid_operation_and_unknown_targets() {
    let fx = Fixture::new();
    fx.write("conflict.txt", "base\n");
    fx.commit("base");
    fx.git(&["checkout", "-q", "-b", "other"]);
    fx.write("conflict.txt", "theirs\n");
    fx.commit("theirs");
    fx.git(&["checkout", "-q", "main"]);
    fx.write("conflict.txt", "ours\n");
    fx.commit("ours");
    let out = fx.try_git(&["merge", "other"]);
    assert!(!out.status.success());
    ident(&fx);

    let repo = fx.repo();
    let err = ff_core::switch(
        &repo,
        &SwitchOptions {
            target: "other".into(),
            now: Some(NOW),
            argv: Vec::new(),
        },
        &ff_core::Provenance::new("pre", None),
    );
    assert!(err.is_err(), "mid-merge switch refuses");

    // Unknown and ambiguous targets.
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    fx.git(&["branch", "feat-one"]);
    fx.git(&["branch", "feat-two"]);
    ident(&fx);
    let repo = fx.repo();
    assert!(ff_core::resolve_branch(&repo, "nope").is_err());
    assert!(
        ff_core::resolve_branch(&repo, "feat-").is_err(),
        "ambiguous"
    );
    assert_eq!(
        ff_core::resolve_branch(&repo, "feat-o").unwrap(),
        "feat-one"
    );
    assert_eq!(ff_core::resolve_branch(&repo, "main").unwrap(), "main");
}

#[test]
fn switch_journals_one_entry_and_reconciles_clean() {
    let fx = two_branch_fixture();
    fx.write("shared.txt", "dirty\n");
    let report = switch_to(&fx, "feature");
    assert!(report.parked.is_some());

    let repo = fx.repo();
    let tip = ff_core::journal::tip(&repo).unwrap().unwrap();
    let entry = ff_core::journal::read_entry(&repo, tip).unwrap();
    assert_eq!(entry.record.verb, "switch");
    assert!(
        !entry.record.stash.is_empty(),
        "park journaled as stash effect"
    );
    assert_eq!(
        entry.record.head.as_ref().unwrap().1,
        "ref:refs/heads/feature"
    );

    let after = ff_core::journal::reconcile(&repo, NOW + 5).unwrap();
    assert!(after.is_quiet(), "plan matched reality: {after:?}");
}

#[test]
fn conflicted_arrival_reports_and_stays_parked_through_switch() {
    let fx = two_branch_fixture();
    // Park a change on main that will conflict after main advances.
    fx.write("shared.txt", "parked edit\n");
    switch_to(&fx, "feature");
    // Advance main underneath the parked change.
    fx.git(&["switch", "-q", "main"]);
    fx.write("shared.txt", "advanced\n");
    fx.git(&["add", "-A"]);
    fx.git(&["commit", "-q", "-m", "advance main"]);
    fx.git(&["switch", "-q", "feature"]);

    let report = switch_to(&fx, "main");
    match &report.arrival {
        ArrivalReport::StillParked { paths, .. } => {
            assert_eq!(paths, &vec!["shared.txt".to_string()]);
        }
        other => panic!("expected StillParked, got {other:?}"),
    }
    // The tree is the clean advanced main; the change is still in the stash.
    assert_eq!(
        std::fs::read_to_string(fx.path().join("shared.txt")).unwrap(),
        "advanced\n"
    );
    assert_eq!(fx.git(&["stash", "list"]).lines().count(), 1);
}
