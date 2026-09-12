//! Differential contract for the switch: on a clean tree, `ff switch` must
//! land the worktree, index, and HEAD exactly where `git switch` does
//! (index compared semantically, worktree byte-for-byte via status). On a
//! dirty tree it must equal git's stash dance on the worktree: stash push
//! -u, switch, and — when returning — stash pop. The index is not carried:
//! the park is the open commit, a tree over the tip, so a staged hunk comes
//! back as an unstaged edit.

use ff_core::{ArrivalReport, SwitchOptions};
use ff_testsupport::{Fixture, scenarios};

/// The newest operation's record, read through the public reader.
fn tip_record(repo: &gix::Repository) -> ff_core::ops::OpRecord {
    let log = ff_core::ops::OpLog::open(repo).unwrap();
    let op = log.get(log.tip().unwrap().unwrap()).unwrap();
    op.record()
        .unwrap()
        .cloned()
        .expect("a verb op has a record")
}

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

    // git: the equivalent dance, without `--index`: the park carries the
    // tree, not the index.
    fx_git.git(&["stash", "push", "-q", "-u", "-m", "fufu: wip on main"]);
    fx_git.git(&["switch", "-q", "feature"]);
    fx_git.git(&["switch", "-q", "main"]);
    fx_git.git(&["stash", "pop", "-q"]);

    let paths = |status: String| -> Vec<String> {
        status
            .lines()
            .map(|l| l.rsplit(' ').next().unwrap().to_string())
            .collect()
    };
    assert_eq!(
        paths(fx_ff.git(&["status", "--porcelain=v2"])),
        paths(fx_git.git(&["status", "--porcelain=v2"])),
        "round-trip touches the same paths as git's stash dance"
    );
    for path in ["shared.txt", "untracked.txt"] {
        assert_eq!(
            std::fs::read(fx_ff.path().join(path)).unwrap(),
            std::fs::read(fx_git.path().join(path)).unwrap(),
            "{path} is byte-identical to git's"
        );
    }
    assert_eq!(
        std::fs::read_to_string(fx_ff.path().join("shared.txt")).unwrap(),
        "staged then more\n"
    );
    assert!(
        fx_ff.git(&["stash", "list"]).is_empty(),
        "nothing on refs/stash"
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
        fx.git(&["branch", "elsewhere"]);
        // The worktree as a tree — `add -A`'s selection — is what the park
        // carries; the index is not. So the round trip is identity on that
        // tree, and afterwards status names exactly the paths that tree
        // changes against HEAD: a staged hunk comes back unstaged, a staged
        // rename as a deletion beside an untracked file, and a staged-only
        // mode change, which no worktree tree can carry, is gone.
        let before = ff_testsupport::capture::git_capture_tree(&fx, &fx.path(), &[]);
        let head_tree = fx.git(&["rev-parse", "HEAD^{tree}"]).trim().to_string();

        switch_to(&fx, "elsewhere");
        assert_eq!(
            fx.git(&["status", "--porcelain=v2"]),
            "",
            "scenario {name}: clean after switching away"
        );
        switch_to(&fx, &current);
        let after = ff_testsupport::capture::git_capture_tree(&fx, &fx.path(), &[]);
        assert_eq!(
            after, before,
            "scenario {name}: switch round trip is identity on the worktree tree"
        );
        let mut status_paths: Vec<String> = fx
            .git(&["status", "--porcelain=v2", "--untracked-files=all"])
            .lines()
            .map(|l| l.rsplit(' ').next().unwrap().to_string())
            .collect();
        status_paths.sort();
        let mut tree_paths: Vec<String> = fx
            .git(&["diff-tree", "--name-only", "-r", &head_tree, &after])
            .lines()
            .map(str::to_string)
            .collect();
        tree_paths.sort();
        tree_paths.dedup();
        assert_eq!(
            status_paths, tree_paths,
            "scenario {name}: status names what the worktree tree changes, nothing staged"
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
    let record = tip_record(&repo);
    assert_eq!(record.verb, "switch");
    assert!(
        record.stash.is_empty(),
        "the park writes nothing to refs/stash"
    );
    assert_eq!(record.head.as_ref().unwrap().1, "ref:refs/heads/feature");

    let after = ff_core::ops::reconcile(&repo, NOW + 5).unwrap();
    assert!(after.is_quiet(), "plan matched reality: {after:?}");
}

#[test]
fn conflicted_arrival_holds_the_branch_through_switch() {
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
        ArrivalReport::Held { paths, .. } => {
            assert_eq!(paths, &vec!["shared.txt".to_string()]);
        }
        other => panic!("expected Held, got {other:?}"),
    }
    // The tree is the clean advanced main; the change is held, not stashed.
    assert_eq!(
        std::fs::read_to_string(fx.path().join("shared.txt")).unwrap(),
        "advanced\n"
    );
    assert!(fx.git(&["stash", "list"]).is_empty());
    assert!(ff_core::held::of(&fx.repo(), "main").unwrap().is_some());
}
