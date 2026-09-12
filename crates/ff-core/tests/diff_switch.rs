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
            target: Some(target.into()),
            now: Some(NOW),
            argv: vec!["ff".into(), "switch".into(), target.into()],
            ..Default::default()
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
            target: Some("other".into()),
            now: Some(NOW),
            argv: Vec::new(),
            ..Default::default()
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

// --- the remote rung, and the mint under every spelling ---

fn rewind_opts(now: i64) -> ff_core::RewindOptions {
    ff_core::RewindOptions {
        force: false,
        now: Some(now),
        argv: Vec::new(),
    }
}

/// `branch.<name>.remote` and `branch.<name>.merge` as git reads them, or
/// `None` where the key is unset.
fn upstream_config(fx: &Fixture, name: &str) -> (Option<String>, Option<String>) {
    let read = |key: &str| {
        let out = fx.try_git(&["config", "--get", key]);
        out.status
            .success()
            .then(|| String::from_utf8(out.stdout).unwrap().trim().to_string())
    };
    (
        read(&format!("branch.{name}.remote")),
        read(&format!("branch.{name}.merge")),
    )
}

/// A clone whose remote holds `spike` and nothing here tracks it: the shape
/// `git fetch` leaves after a colleague pushes.
fn clone_with_remote_spike() -> (Fixture, String) {
    let fx = Fixture::new_cloned();
    fx.write("a.txt", "a\n");
    let sha = fx.commit("init");
    fx.git(&["push", "-q", "origin", "main"]);
    fx.remote_git(&["update-ref", "refs/heads/spike", &sha]);
    fx.git(&["fetch", "-q", "origin"]);
    (fx, sha)
}

/// The differential contract for the tracking mint: `ff switch spike` with
/// only `origin/spike` lands where `git switch spike` does — the branch at
/// the tracking tip, the upstream section written, HEAD there.
#[test]
fn remote_branch_by_name_matches_git_switch() {
    let (fx, sha) = clone_with_remote_spike();
    let repo = fx.repo();
    let before = ff_core::ops::OpLog::open(&repo).unwrap().tip().unwrap();

    let report = switch_to(&fx, "spike");
    assert_eq!(report.from, "main");
    assert_eq!(report.to, "spike");
    let minted = report.minted.as_ref().expect("minted");
    assert_eq!(minted.tracking.as_deref(), Some("origin/spike"));
    assert_eq!(
        minted.forked_from, None,
        "a tracking mint forked from nothing"
    );
    assert_eq!(minted.parent, None);
    assert_eq!(minted.carried, None);
    assert_eq!(fx.git(&["rev-parse", "refs/heads/spike"]).trim(), sha);
    assert_eq!(fx.git(&["symbolic-ref", "HEAD"]).trim(), "refs/heads/spike");
    assert_eq!(
        upstream_config(&fx, "spike"),
        (Some("origin".into()), Some("refs/heads/spike".into()))
    );
    let meta = ff_core::branchmeta::read(&repo, "spike").unwrap();
    assert_eq!(meta.parent, None);
    assert_eq!(meta.forked_from, None);
    let record = tip_record(&repo);
    assert_eq!(record.verb, "switch");
    let upstream = record.upstream.as_ref().expect("the upstream rides the op");
    assert_eq!(upstream.branch, "spike");
    assert_eq!(upstream.old, None);
    assert_eq!(upstream.new.as_deref(), Some("origin"));
    assert!(
        record.summary.contains("tracking origin/spike"),
        "{}",
        record.summary
    );
    let ops = ff_core::ops::OpLog::open(&repo).unwrap();
    let mut count = 0;
    let mut cursor = ops.tip().unwrap();
    while let Some(id) = cursor {
        if Some(id) == before {
            break;
        }
        if ops.get(id).unwrap().kind() == ff_core::ops::OpKind::Op {
            count += 1;
        }
        cursor = ops.get(id).unwrap().prev();
    }
    assert_eq!(count, 1, "one operation");
    let reflog = fx.git(&["reflog", "show", "--format=%gs", "refs/heads/spike"]);
    assert_eq!(reflog.trim(), "branch: created from origin/spike");

    // git's own answer to the same request, from the same starting state.
    fx.git(&["switch", "-q", "main"]);
    fx.git(&["branch", "-D", "spike"]);
    assert_eq!(upstream_config(&fx, "spike"), (None, None));
    fx.git(&["switch", "-q", "spike"]);
    assert_eq!(fx.git(&["rev-parse", "refs/heads/spike"]).trim(), sha);
    assert_eq!(fx.git(&["symbolic-ref", "HEAD"]).trim(), "refs/heads/spike");
    assert_eq!(
        upstream_config(&fx, "spike"),
        (Some("origin".into()), Some("refs/heads/spike".into()))
    );
}

/// The qualified spelling is the same target: `origin/spike` with a local
/// `spike` continues the local branch and touches no upstream.
#[test]
fn qualified_name_means_the_local_branch() {
    let (fx, sha) = clone_with_remote_spike();
    fx.git(&["branch", "spike", &sha]);
    fx.write("a.txt", "dirty\n");

    let report = switch_to(&fx, "origin/spike");
    assert_eq!(report.to, "spike");
    assert_eq!(report.minted, None, "continued, not minted");
    assert!(report.parked.is_some(), "the dirty tree parked");
    assert_eq!(fx.git(&["symbolic-ref", "HEAD"]).trim(), "refs/heads/spike");
    assert_eq!(upstream_config(&fx, "spike"), (None, None));
}

/// A bare name two remotes hold names two branches, and picking one would
/// be a guess: the refusal lists the qualified spellings.
#[test]
fn two_remotes_holding_the_name_is_ambiguous() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    let sha = fx.commit("init");
    ident(&fx);
    for remote in ["origin", "upstream"] {
        fx.set_config(&format!("remote.{remote}.url"), "file:///nonexistent");
        fx.set_config(
            &format!("remote.{remote}.fetch"),
            &format!("+refs/heads/*:refs/remotes/{remote}/*"),
        );
        fx.git(&["update-ref", &format!("refs/remotes/{remote}/spike"), &sha]);
    }
    let repo = fx.repo();
    let err = ff_core::switch(
        &repo,
        &SwitchOptions {
            target: Some("spike".into()),
            now: Some(NOW),
            ..Default::default()
        },
        &ff_core::Provenance::new("pre", None),
    )
    .expect_err("must refuse");
    assert_eq!(err.id(), "branch/ambiguous");
    assert_eq!(
        err.to_string(),
        "ambiguous branch name spike: origin/spike, upstream/spike"
    );
    assert!(
        fx.git(&["for-each-ref", "refs/heads/spike"])
            .trim()
            .is_empty(),
        "nothing minted"
    );

    // Qualified, each is one branch.
    let report = switch_to(&fx, "upstream/spike");
    assert_eq!(report.to, "spike");
    assert_eq!(
        report.minted.unwrap().tracking.as_deref(),
        Some("upstream/spike")
    );
    assert_eq!(
        upstream_config(&fx, "spike"),
        (Some("upstream".into()), Some("refs/heads/spike".into()))
    );
}

/// `-b` on a remote's branch is a fork like any other branch target: a new
/// branch at its tip with the remote's branch as parent, and no upstream.
#[test]
fn dash_b_on_a_remote_branch_forks_without_an_upstream() {
    let (fx, sha) = clone_with_remote_spike();
    let repo = fx.repo();
    let (report, _) = ff_core::switch(
        &repo,
        &SwitchOptions {
            target: Some("spike".into()),
            branch: Some(Some("mine".into())),
            now: Some(NOW),
            ..Default::default()
        },
        &ff_core::Provenance::new("pre", None),
    )
    .unwrap();
    assert_eq!(report.to, "mine");
    let minted = report.minted.as_ref().expect("minted");
    assert_eq!(minted.tracking, None);
    assert_eq!(minted.forked_from.as_deref(), Some("origin/spike"));
    assert_eq!(minted.parent.as_deref(), Some("origin/spike"));
    assert_eq!(fx.git(&["rev-parse", "refs/heads/mine"]).trim(), sha);
    assert!(
        fx.git(&["for-each-ref", "refs/heads/spike"])
            .trim()
            .is_empty(),
        "no local spike"
    );
    assert_eq!(upstream_config(&fx, "mine"), (None, None));
    let meta = ff_core::branchmeta::read(&repo, "mine").unwrap();
    assert_eq!(meta.parent.as_deref(), Some("origin/spike"));
    assert!(tip_record(&repo).upstream.is_none());
    let reflog = fx.git(&["reflog", "show", "--format=%gs", "refs/heads/mine"]);
    assert_eq!(reflog.trim(), "branch: forked from origin/spike");

    // Bare `-b` is the same fork under a petname.
    let (report, _) = ff_core::switch(
        &repo,
        &SwitchOptions {
            target: Some("origin/spike".into()),
            branch: Some(None),
            now: Some(NOW + 10),
            ..Default::default()
        },
        &ff_core::Provenance::new("pre", None),
    )
    .unwrap();
    assert!(report.to.starts_with("ff/"), "{}", report.to);
    assert_eq!(
        report.minted.unwrap().parent.as_deref(),
        Some("origin/spike")
    );
}

/// The upstream rides the operation: undo takes the section away with the
/// branch, redo writes it back. A stale section a plain delete left is what
/// the mint replaced, and undo puts that one back rather than none.
#[test]
fn undo_of_a_tracking_mint_removes_the_section() {
    let (fx, sha) = clone_with_remote_spike();
    let repo = fx.repo();

    switch_to(&fx, "spike");
    ff_core::undo(
        &repo,
        &rewind_opts(NOW + 10),
        &ff_core::Provenance::new("pre", None),
    )
    .unwrap();
    assert_eq!(fx.git(&["symbolic-ref", "HEAD"]).trim(), "refs/heads/main");
    assert!(
        fx.git(&["for-each-ref", "refs/heads/spike"])
            .trim()
            .is_empty(),
        "the mint is gone"
    );
    assert_eq!(upstream_config(&fx, "spike"), (None, None));

    ff_core::redo(
        &repo,
        &rewind_opts(NOW + 20),
        &ff_core::Provenance::new("pre", None),
    )
    .unwrap();
    assert_eq!(fx.git(&["symbolic-ref", "HEAD"]).trim(), "refs/heads/spike");
    assert_eq!(fx.git(&["rev-parse", "refs/heads/spike"]).trim(), sha);
    assert_eq!(
        upstream_config(&fx, "spike"),
        (Some("origin".into()), Some("refs/heads/spike".into()))
    );

    // A stale section: what a plain `git branch -D` leaves behind.
    fx.git(&["switch", "-q", "main"]);
    fx.git(&["branch", "-D", "spike"]);
    fx.set_config("branch.spike.remote", "elsewhere");
    fx.set_config("branch.spike.merge", "refs/heads/spike");
    let (report, _) = ff_core::switch(
        &repo,
        &SwitchOptions {
            target: Some("spike".into()),
            now: Some(NOW + 30),
            ..Default::default()
        },
        &ff_core::Provenance::new("pre", None),
    )
    .unwrap();
    assert_eq!(
        report.minted.unwrap().tracking.as_deref(),
        Some("origin/spike")
    );
    let upstream = tip_record(&repo).upstream.unwrap();
    assert_eq!(upstream.old.as_deref(), Some("elsewhere"));
    assert_eq!(upstream.new.as_deref(), Some("origin"));
    assert_eq!(
        upstream_config(&fx, "spike"),
        (Some("origin".into()), Some("refs/heads/spike".into()))
    );
    ff_core::undo(
        &repo,
        &rewind_opts(NOW + 40),
        &ff_core::Provenance::new("pre", None),
    )
    .unwrap();
    assert_eq!(
        upstream_config(&fx, "spike"),
        (Some("elsewhere".into()), Some("refs/heads/spike".into())),
        "the stale section is back"
    );
}

/// `-m` describes the change a switch opens, and a continue opens nothing.
#[test]
fn dash_m_on_a_continue_is_refused() {
    let fx = two_branch_fixture();
    fx.write("shared.txt", "dirty\n");
    let repo = fx.repo();
    let before = ff_core::ops::OpLog::open(&repo).unwrap().tip().unwrap();
    let err = ff_core::switch(
        &repo,
        &SwitchOptions {
            target: Some("feature".into()),
            message: Some("the plan".into()),
            now: Some(NOW),
            ..Default::default()
        },
        &ff_core::Provenance::new("pre", None),
    )
    .expect_err("must refuse");
    assert_eq!(err.id(), "switch/nothing-opened");
    assert_eq!(
        err.to_string(),
        "-m describes the change a switch opens, and switching to feature opens nothing: it \
         resumes what is there"
    );
    assert_eq!(err.exits(), &["ff describe -m <msg>"]);
    assert_eq!(fx.git(&["symbolic-ref", "HEAD"]).trim(), "refs/heads/main");
    assert_eq!(
        ff_core::ops::OpLog::open(&repo).unwrap().tip().unwrap(),
        before,
        "refused before the preamble: nothing written"
    );
}

/// A bare word that names nothing — no branch here, none on a remote, no
/// revision — is the branch rung's refusal, and the revset's other refusals
/// stay their own.
#[test]
fn an_unknown_name_is_branch_not_found() {
    let fx = two_branch_fixture();
    let repo = fx.repo();
    let refuse = |target: &str| {
        ff_core::switch(
            &repo,
            &SwitchOptions {
                target: Some(target.into()),
                now: Some(NOW),
                ..Default::default()
            },
            &ff_core::Provenance::new("pre", None),
        )
        .expect_err("must refuse")
    };
    let err = refuse("nosuch");
    assert_eq!(err.id(), "branch/not-found");
    assert_eq!(err.to_string(), "no branch named nosuch");
    assert_eq!(refuse("::feature").id(), "usage/revset-not-a-point");
    assert_eq!(fx.git(&["symbolic-ref", "HEAD"]).trim(), "refs/heads/main");
}

/// Whatever the spelling, a switch that mints is one operation: a mint at
/// trunk from a dirty tree, and one undo lands back on the branch it left
/// with its tree dirty.
#[test]
fn switch_is_one_operation_under_every_spelling() {
    let fx = two_branch_fixture();
    fx.write("shared.txt", "dirty\n");
    let repo = fx.repo();
    let before = ff_core::ops::OpLog::open(&repo).unwrap().tip().unwrap();
    let (report, _) = ff_core::switch(
        &repo,
        &SwitchOptions {
            now: Some(NOW),
            argv: vec!["ff".into(), "start".into()],
            ..Default::default()
        },
        &ff_core::Provenance::new("pre", Some("ff start".into())),
    )
    .unwrap();
    assert!(report.to.starts_with("ff/"), "{}", report.to);
    assert!(report.parked.is_some());
    assert_eq!(
        report.minted.as_ref().unwrap().forked_from.as_deref(),
        Some("main")
    );
    let record = tip_record(&repo);
    assert_eq!(record.verb, "switch", "the verb under every spelling");
    assert_eq!(record.argv, vec!["ff".to_string(), "start".to_string()]);
    assert_eq!(fx.git(&["status", "--porcelain=v2"]), "", "opens clean");

    ff_core::undo(
        &repo,
        &rewind_opts(NOW + 10),
        &ff_core::Provenance::new("pre", None),
    )
    .unwrap();
    assert_eq!(fx.git(&["symbolic-ref", "HEAD"]).trim(), "refs/heads/main");
    assert_eq!(
        std::fs::read_to_string(fx.path().join("shared.txt")).unwrap(),
        "dirty\n"
    );
    assert!(
        fx.git(&["for-each-ref", &format!("refs/heads/{}", report.to)])
            .trim()
            .is_empty(),
        "the mint is gone"
    );
    let after = ff_core::ops::OpLog::open(&repo).unwrap().tip().unwrap();
    assert_ne!(after, before, "the undo is its own operation");
}
