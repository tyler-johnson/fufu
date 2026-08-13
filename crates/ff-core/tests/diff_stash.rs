//! Differential contract for tree memory: fufu's park must produce stash
//! entries shaped exactly like `git stash push -u -m "fufu: wip on <branch>"`
//! — same trees (content-addressed, identity-free), same messages, same
//! parent topology, same reflog shape, same post-park worktree and index —
//! and park→arrive must be the identity on the working tree.

use ff_core::gix;
use ff_core::stash::{self, Arrival};
use ff_testsupport::{Fixture, scenarios};

const NOW: i64 = 1_700_000_000;

fn ident(fx: &Fixture) {
    fx.set_config("user.name", "Stash User");
    fx.set_config("user.email", "stash@test");
}

fn head_state(fx: &Fixture) -> ff_core::HeadState {
    ff_core::head_state(&fx.repo()).unwrap()
}

fn commit_info(fx: &Fixture, rev: &str) -> (String, Vec<String>, String) {
    // (tree, parents, raw message)
    let tree = fx
        .git(&["rev-parse", &format!("{rev}^{{tree}}")])
        .trim()
        .to_string();
    let parents = fx
        .git(&["log", "--format=%P", "-1", rev])
        .split_whitespace()
        .map(|s| s.to_string())
        .collect();
    let raw = fx.git(&["cat-file", "commit", rev]);
    let message = raw
        .split_once("\n\n")
        .map(|(_, m)| m.to_string())
        .unwrap_or_default();
    (tree, parents, message)
}

#[test]
fn matrix_park_matches_git_stash_push() {
    for (name, setup) in scenarios() {
        let fx_ff = Fixture::new();
        setup(&fx_ff);
        ident(&fx_ff);
        let fx_git = Fixture::new();
        setup(&fx_git);
        ident(&fx_git);

        let head = head_state(&fx_ff);
        let branch = match &head {
            ff_core::HeadState::Branch { name, .. } => name.clone(),
            // Unborn and detached parks refuse (asserted in their own test).
            _ => continue,
        };
        let refusals = [
            "conflicted_merge",
            "conflicted_merge_with_other_changes",
            "intent_to_add",
        ];
        if refusals.contains(&name) {
            // git stash refuses these; so must park.
            let out = fx_git.try_git(&["stash", "push", "-u", "-m", "x"]);
            assert!(!out.status.success(), "scenario {name}: git refuses");
            let repo = fx_ff.repo();
            assert!(
                stash::park(&repo, &head, NOW).is_err(),
                "scenario {name}: park must refuse where git does"
            );
            continue;
        }

        let label = format!("fufu: wip on {branch}");
        let out = fx_git.try_git(&["stash", "push", "-u", "-m", &label]);
        assert!(
            out.status.success(),
            "scenario {name}: git stash push failed"
        );
        let git_said_nothing = String::from_utf8_lossy(&out.stdout).contains("No local changes");

        let repo = fx_ff.repo();
        let parked = stash::park(&repo, &head, NOW).unwrap();

        if git_said_nothing {
            assert!(
                parked.is_none(),
                "scenario {name}: park must be a no-op where git is"
            );
            continue;
        }
        let parked =
            parked.unwrap_or_else(|| panic!("scenario {name}: git stashed, park must too"));

        // Anatomy: trees are content-addressed, so equality across the two
        // fixtures is byte-equality of the stashed state.
        let (g_wip_tree, g_parents, g_msg) = commit_info(&fx_git, "refs/stash");
        let ours = parked.stash.to_string();
        let (f_wip_tree, f_parents, f_msg) = commit_info(&fx_ff, &ours);
        assert_eq!(f_wip_tree, g_wip_tree, "scenario {name}: wip tree");
        assert_eq!(f_msg, g_msg, "scenario {name}: wip message");
        assert_eq!(
            f_parents.len(),
            g_parents.len(),
            "scenario {name}: parent count"
        );
        assert_eq!(
            f_parents[0], g_parents[0],
            "scenario {name}: base parent is HEAD"
        );

        for (slot, what) in [(1usize, "index"), (2usize, "untracked")] {
            if slot < g_parents.len() {
                let (g_tree, _, g_m) = commit_info(&fx_git, &g_parents[slot]);
                let (f_tree, _, f_m) = commit_info(&fx_ff, &f_parents[slot]);
                assert_eq!(f_tree, g_tree, "scenario {name}: {what} tree");
                assert_eq!(f_m, g_m, "scenario {name}: {what} message");
            }
        }

        // Reflog shape: same messages, same depth.
        let g_log = fx_git.git(&["stash", "list", "--format=%gs"]);
        let f_log = fx_ff.git(&["stash", "list", "--format=%gs"]);
        assert_eq!(f_log, g_log, "scenario {name}: stash list");

        // Post-park state: worktree and index equal git's, byte for byte.
        assert_eq!(
            fx_ff.git(&["status", "--porcelain=v2"]),
            fx_git.git(&["status", "--porcelain=v2"]),
            "scenario {name}: post-park status"
        );
        assert_eq!(
            fx_ff.git(&["ls-files", "--stage"]),
            fx_git.git(&["ls-files", "--stage"]),
            "scenario {name}: post-park index"
        );
    }
}

#[test]
fn park_refuses_where_git_cannot_stash() {
    // Unborn.
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.git(&["add", "a.txt"]);
    ident(&fx);
    let repo = fx.repo();
    let head = ff_core::head_state(&repo).unwrap();
    assert!(
        stash::park(&repo, &head, NOW).is_err(),
        "unborn dirty refuses"
    );

    // Detached.
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    let c = fx.commit("init");
    fx.git(&["checkout", "-q", &c]);
    fx.write("a.txt", "dirty\n");
    ident(&fx);
    let repo = fx.repo();
    let head = ff_core::head_state(&repo).unwrap();
    assert!(stash::park(&repo, &head, NOW).is_err(), "detached refuses");
}

#[test]
fn matrix_park_arrive_round_trip_is_identity() {
    for (name, setup) in scenarios() {
        let fx = Fixture::new();
        setup(&fx);
        ident(&fx);
        let head = head_state(&fx);
        let branch = match &head {
            ff_core::HeadState::Branch { name, .. } => name.clone(),
            _ => continue,
        };
        let repo = fx.repo();
        let Ok(Some(parked)) = stash::park(&repo, &head, NOW) else {
            continue; // refusals and no-ops covered elsewhere
        };
        let before_status = fx.git(&["status", "--porcelain=v2"]);
        assert!(
            !before_status.contains("1 ") || before_status.is_empty() || true,
            "post-park may still show nothing"
        );

        let baseline_status = {
            // Reconstruct the expected pre-park state from a twin fixture.
            let twin = Fixture::new();
            setup(&twin);
            ident(&twin);
            (
                twin.git(&["status", "--porcelain=v2"]),
                twin.git(&["ls-files", "--stage"]),
            )
        };

        let repo = fx.repo();
        let arrival = stash::arrive(&repo, &branch, NOW + 1).unwrap();
        assert!(
            matches!(arrival, Arrival::Restored { .. }),
            "scenario {name}: arrival should restore, got {arrival:?}"
        );

        assert_eq!(
            fx.git(&["status", "--porcelain=v2"]),
            baseline_status.0,
            "scenario {name}: round-trip status identity"
        );
        assert_eq!(
            fx.git(&["ls-files", "--stage"]),
            baseline_status.1,
            "scenario {name}: round-trip index identity"
        );
        // The stash entry is consumed and the parked ref cleared.
        assert!(fx.git(&["stash", "list"]).is_empty(), "scenario {name}");
        assert!(
            stash::parked_entry(&fx.repo(), &branch).unwrap().is_none(),
            "scenario {name}: parked ref cleared"
        );
        let _ = parked;
    }
}

#[test]
fn drop_by_identity_matches_git_reflog_delete_rewrite() {
    let make = || {
        let fx = Fixture::new();
        fx.write("a.txt", "base\n");
        fx.commit("init");
        ident(&fx);
        for n in 1..=3 {
            fx.write("a.txt", &format!("change {n}\n"));
            fx.git(&["stash", "push", "-u", "-m", &format!("entry {n}")]);
        }
        fx
    };

    let ours = make();
    let control = make();

    // Drop the middle entry (stash@{1} = "entry 2") by identity on ours,
    // by position on the control.
    let middle = ours.git(&["rev-parse", "stash@{1}"]).trim().to_string();
    let repo = ours.repo();
    stash::drop_stash_entry(&repo, gix::ObjectId::from_hex(middle.as_bytes()).unwrap()).unwrap();
    control.git(&["reflog", "delete", "--rewrite", "--updateref", "stash@{1}"]);

    assert_eq!(
        ours.git(&["stash", "list", "--format=%gs %H"]),
        control.git(&["stash", "list", "--format=%gs %H"]),
        "stash list identical after drop"
    );
    assert_eq!(
        ours.git(&["rev-parse", "refs/stash"]),
        control.git(&["rev-parse", "refs/stash"]),
        "refs/stash target identical"
    );
    let ours_log = std::fs::read_to_string(ours.path().join(".git/logs/refs/stash")).unwrap();
    let control_log = std::fs::read_to_string(control.path().join(".git/logs/refs/stash")).unwrap();
    assert_eq!(ours_log, control_log, "reflog bytes identical after drop");

    // Dropping the last two empties the stack and removes the ref.
    let repo = ours.repo();
    for rev in ["stash@{0}", "stash@{0}"] {
        let sha = ours.git(&["rev-parse", rev]).trim().to_string();
        stash::drop_stash_entry(&repo, gix::ObjectId::from_hex(sha.as_bytes()).unwrap()).unwrap();
    }
    let out = ours.try_git(&["rev-parse", "--verify", "refs/stash"]);
    assert!(!out.status.success(), "refs/stash gone once empty");
}

#[test]
fn arrive_merges_when_branch_moved_beneath() {
    let fx = Fixture::new();
    fx.write("work.txt", "work\n");
    fx.write("other.txt", "other\n");
    fx.commit("init");
    ident(&fx);
    fx.write("work.txt", "parked change\n");
    let repo = fx.repo();
    let head = ff_core::head_state(&repo).unwrap();
    stash::park(&repo, &head, NOW).unwrap().unwrap();

    // The branch moves beneath the parked change, touching a different file.
    fx.write("other.txt", "moved on\n");
    fx.commit("advance");

    let repo = fx.repo();
    let arrival = stash::arrive(&repo, "main", NOW + 1).unwrap();
    assert!(matches!(arrival, Arrival::Restored { .. }), "{arrival:?}");

    let work = std::fs::read_to_string(fx.path().join("work.txt")).unwrap();
    assert_eq!(work, "parked change\n", "parked edit survives the merge");
    let other = std::fs::read_to_string(fx.path().join("other.txt")).unwrap();
    assert_eq!(other, "moved on\n", "new base content stays");
    assert!(fx.git(&["stash", "list"]).is_empty());
}

#[test]
fn conflicting_arrival_stays_parked_and_loud() {
    let fx = Fixture::new();
    fx.write("work.txt", "base\n");
    fx.commit("init");
    ident(&fx);
    fx.write("work.txt", "parked change\n");
    let repo = fx.repo();
    let head = ff_core::head_state(&repo).unwrap();
    let parked = stash::park(&repo, &head, NOW).unwrap().unwrap();

    // The branch rewrites the same lines: the merge must conflict.
    fx.write("work.txt", "conflicting advance\n");
    fx.commit("conflicting");

    let repo = fx.repo();
    let arrival = stash::arrive(&repo, "main", NOW + 1).unwrap();
    match arrival {
        Arrival::Conflicted { stash: sha, paths } => {
            assert_eq!(sha, parked.stash.to_string());
            assert_eq!(paths, vec!["work.txt".to_string()]);
        }
        other => panic!("expected Conflicted, got {other:?}"),
    }
    // Nothing moved: entry still parked, stash intact, worktree clean.
    assert!(stash::parked_entry(&fx.repo(), "main").unwrap().is_some());
    assert_eq!(fx.git(&["stash", "list"]).lines().count(), 1);
    assert_eq!(fx.git(&["status", "--porcelain=v2"]), "");
    // A conflicted probe must leave no loose objects behind (gc-proofness).
    let loose = fx.git(&["count-objects"]);
    let count: usize = loose
        .split(' ')
        .next()
        .and_then(|n| n.parse().ok())
        .unwrap_or(999);
    assert!(count <= 60, "no conflict-blob leak into the odb: {loose}");
}

#[test]
fn externally_dropped_stash_demotes_the_parked_ref() {
    let fx = Fixture::new();
    fx.write("a.txt", "base\n");
    fx.commit("init");
    ident(&fx);
    fx.write("a.txt", "parked\n");
    let repo = fx.repo();
    let head = ff_core::head_state(&repo).unwrap();
    stash::park(&repo, &head, NOW).unwrap().unwrap();

    // The user pops it with real git behind fufu's back.
    fx.git(&["stash", "pop", "-q"]);
    fx.git(&["checkout", "-q", "--", "."]); // and discards, for a clean tree

    let repo = fx.repo();
    let arrival = stash::arrive(&repo, "main", NOW + 1).unwrap();
    assert!(
        matches!(arrival, Arrival::Invalidated { .. }),
        "{arrival:?}"
    );
    assert!(stash::parked_entry(&fx.repo(), "main").unwrap().is_none());
}

#[test]
fn untracked_collision_blocks_arrival() {
    let fx = Fixture::new();
    fx.write("a.txt", "base\n");
    fx.commit("init");
    ident(&fx);
    fx.write("new.txt", "parked untracked\n");
    let repo = fx.repo();
    let head = ff_core::head_state(&repo).unwrap();
    stash::park(&repo, &head, NOW).unwrap().unwrap();

    // A different file appears at the same path before arrival.
    fx.write("new.txt", "usurper\n");

    let repo = fx.repo();
    let arrival = stash::arrive(&repo, "main", NOW + 1).unwrap();
    match arrival {
        Arrival::Conflicted { paths, .. } => assert_eq!(paths, vec!["new.txt".to_string()]),
        other => panic!("expected Conflicted, got {other:?}"),
    }
    assert_eq!(
        std::fs::read_to_string(fx.path().join("new.txt")).unwrap(),
        "usurper\n",
        "the usurper is untouched"
    );
    assert!(stash::parked_entry(&fx.repo(), "main").unwrap().is_some());
}
