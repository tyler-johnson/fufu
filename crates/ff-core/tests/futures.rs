//! Futures: the verdict matrix, the invariant that a probe writes nothing to
//! the object database, the base ladder, and the cache.

use ff_core::futures::{self, At, Role, UnknownReason, Verdict};
use ff_core::gix;
use ff_testsupport::Fixture;

fn oid(hex: &str) -> gix::ObjectId {
    gix::ObjectId::from_hex(hex.trim().as_bytes()).unwrap()
}

fn tip(fx: &Fixture, rev: &str) -> gix::ObjectId {
    oid(&fx.git(&["rev-parse", rev]))
}

/// Loose objects on disk, counted as files by a stack walk over the odb.
fn loose_count(fx: &Fixture) -> usize {
    let mut count = 0;
    let objects = fx.path().join(".git/objects");
    let mut stack = vec![objects];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(dir).unwrap() {
            let entry = entry.unwrap();
            if entry.file_type().unwrap().is_dir() {
                stack.push(entry.path());
            } else {
                count += 1;
            }
        }
    }
    count
}

/// A 3-commit feature that replays cleanly onto a moved main.
fn linear_clean() -> Fixture {
    let fx = Fixture::new();
    fx.write("shared.txt", "line1\nline2\nline3\n");
    fx.commit("base");
    fx.git(&["branch", "feature"]);
    fx.write("m.txt", "main side\n");
    fx.commit("main moves");
    fx.git(&["switch", "feature"]);
    fx.write("f1.txt", "one\n");
    fx.commit("feat one");
    fx.write("f2.txt", "two\n");
    fx.commit("feat two");
    fx.write("f3.txt", "three\n");
    fx.commit("feat three");
    fx
}

/// The middle of three feature commits collides with main; both ends are clean.
fn conflict_in_the_middle() -> Fixture {
    let fx = Fixture::new();
    fx.write("shared.txt", "line1\nline2\nline3\n");
    fx.commit("base");
    fx.git(&["branch", "feature"]);
    fx.write("shared.txt", "MAIN\nline2\nline3\n");
    fx.commit("main edits line1");
    fx.git(&["switch", "feature"]);
    fx.write("other.txt", "ok\n");
    fx.commit("feat one clean");
    fx.write("shared.txt", "FEATURE\nline2\nline3\n");
    fx.commit("feat two conflicts");
    fx.write("third.txt", "ok\n");
    fx.commit("feat three clean");
    fx
}

// --- The verdict matrix ---

#[test]
fn clean_replay_counts_every_commit() {
    let fx = linear_clean();
    let repo = fx.repo();
    let v = futures::probe(&repo, tip(&fx, "main"), tip(&fx, "feature"), None).unwrap();
    assert_eq!(
        v,
        Verdict::Clean {
            replayed: 3,
            dropped: 0
        }
    );
}

#[test]
fn conflict_names_the_middle_commit_not_the_tip() {
    let fx = conflict_in_the_middle();
    let repo = fx.repo();
    // feature~1 is the colliding commit; the tip (feature) is clean.
    let id = tip(&fx, "feature~1").to_string();
    let v = futures::probe(&repo, tip(&fx, "main"), tip(&fx, "feature"), None).unwrap();
    assert_eq!(
        v,
        Verdict::Conflict {
            at: At::Commit {
                id,
                subject: "feat two conflicts".into(),
            },
            paths: vec!["shared.txt".into()],
        }
    );
}

#[test]
fn conflict_introduced_only_by_the_open_change() {
    let fx = Fixture::new();
    fx.write("shared.txt", "line1\nline2\nline3\n");
    fx.commit("base");
    fx.git(&["branch", "feature"]);
    fx.write("shared.txt", "MAIN\nline2\nline3\n");
    fx.commit("main edits line1");
    fx.git(&["switch", "feature"]);
    fx.write("other.txt", "ok\n");
    fx.commit("clean change");
    // Uncommitted work that collides with main's edit, captured as a tree.
    fx.write("shared.txt", "FEATURE\nline2\nline3\n");
    fx.git(&["add", "-A"]);
    let open_tree = oid(&fx.git(&["write-tree"]));

    let repo = fx.repo();
    let onto = tip(&fx, "main");
    let branch_tip = tip(&fx, "feature");
    assert_eq!(
        futures::probe(&repo, onto, branch_tip, Some(open_tree)).unwrap(),
        Verdict::Conflict {
            at: At::OpenChange,
            paths: vec!["shared.txt".into()],
        }
    );
    // Without the open change the same replay is clean: the contrast is the
    // assertion.
    assert_eq!(
        futures::probe(&repo, onto, branch_tip, None).unwrap(),
        Verdict::Clean {
            replayed: 1,
            dropped: 0
        }
    );
}

#[test]
fn open_change_equal_to_the_tip_tree_is_skipped() {
    let fx = linear_clean();
    let open_tree = oid(&fx.git(&["rev-parse", "feature^{tree}"]));
    let repo = fx.repo();
    let v = futures::probe(
        &repo,
        tip(&fx, "main"),
        tip(&fx, "feature"),
        Some(open_tree),
    )
    .unwrap();
    assert_eq!(
        v,
        Verdict::Clean {
            replayed: 3,
            dropped: 0
        }
    );
}

#[test]
fn fast_forward_reports_how_far_behind() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("base");
    fx.git(&["branch", "feature"]);
    fx.write("b.txt", "b\n");
    fx.commit("main one");
    fx.write("c.txt", "c\n");
    fx.commit("main two");
    let repo = fx.repo();
    let v = futures::probe(&repo, tip(&fx, "main"), tip(&fx, "feature"), None).unwrap();
    assert_eq!(v, Verdict::FastForward { behind: 2 });
}

#[test]
fn up_to_date_reports_how_far_ahead() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("base");
    fx.git(&["branch", "feature"]);
    fx.git(&["switch", "feature"]);
    fx.write("b.txt", "b\n");
    fx.commit("feat one");
    fx.write("c.txt", "c\n");
    fx.commit("feat two");
    let repo = fx.repo();
    let v = futures::probe(&repo, tip(&fx, "main"), tip(&fx, "feature"), None).unwrap();
    assert_eq!(v, Verdict::UpToDate { ahead: 2 });
}

// Regression guard for an ordering bug: with equal tips both the up-to-date
// and the fast-forward conditions hold, and answering FastForward { behind: 0 }
// would make `ff status` announce a fast-forward when nothing moved.
#[test]
fn identical_tips_are_up_to_date_not_a_fast_forward() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("base");
    fx.git(&["branch", "feature"]);
    let repo = fx.repo();
    let v = futures::probe(&repo, tip(&fx, "main"), tip(&fx, "feature"), None).unwrap();
    assert_eq!(v, Verdict::UpToDate { ahead: 0 });
}

#[test]
fn unrelated_histories_are_unknown() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("main commit");
    fx.git(&["switch", "--orphan", "island"]);
    fx.write("b.txt", "b\n");
    fx.commit("island commit");
    let repo = fx.repo();
    let v = futures::probe(&repo, tip(&fx, "main"), tip(&fx, "island"), None).unwrap();
    assert_eq!(
        v,
        Verdict::Unknown {
            reason: UnknownReason::UnrelatedHistories,
        }
    );
}

#[test]
fn a_merge_commit_in_the_range_is_unknown() {
    // The merge has to be between two *feature-side* branches. Merging the
    // base in instead would make the base an ancestor, and the probe would
    // answer "nothing to replay" before the walk ever saw the merge — which
    // is the case the next test pins.
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("base");
    fx.git(&["branch", "feature"]);
    fx.git(&["switch", "feature"]);
    fx.write("f1.txt", "one\n");
    fx.commit("feat one");
    fx.git(&["branch", "sidetrack"]);
    fx.write("f2.txt", "two\n");
    fx.commit("feat two");
    fx.git(&["switch", "sidetrack"]);
    fx.write("s1.txt", "side\n");
    fx.commit("side work");
    fx.git(&["switch", "feature"]);
    fx.git(&["merge", "--no-edit", "sidetrack"]);
    fx.git(&["switch", "main"]);
    fx.write("m.txt", "m\n");
    fx.commit("main moves");

    let repo = fx.repo();
    let v = futures::probe(&repo, tip(&fx, "main"), tip(&fx, "feature"), None).unwrap();
    assert_eq!(
        v,
        Verdict::Unknown {
            reason: UnknownReason::MergeCommits,
        }
    );
}

/// A branch that has already merged its base is up to date with it: there is
/// nothing of the base's left to integrate, so no replay is simulated and no
/// conflict is possible. Real `git rebase` still does work here — it drops
/// the merge and linearizes — but that is a rewrite of the branch's own
/// history, not a cost the base imposes, and Phase 3 only reports the latter.
#[test]
fn a_branch_that_merged_its_base_is_up_to_date_not_unknown() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("base");
    fx.git(&["branch", "side"]);
    fx.write("b.txt", "b\n");
    fx.commit("main moves");
    fx.git(&["switch", "side"]);
    fx.write("c.txt", "c\n");
    fx.commit("side moves");
    fx.git(&["merge", "--no-edit", "main"]);

    let repo = fx.repo();
    let v = futures::probe(&repo, tip(&fx, "main"), tip(&fx, "side"), None).unwrap();
    assert_eq!(v, Verdict::UpToDate { ahead: 2 });
}

#[test]
fn past_the_depth_cap_the_verdict_is_unknown() {
    let fx = Fixture::new();
    fx.set_config("fufu.futuresDepth", "3");
    fx.write("a.txt", "a\n");
    fx.commit("base");
    fx.git(&["branch", "feature"]);
    fx.write("m.txt", "main\n");
    fx.commit("main moves");
    fx.git(&["switch", "feature"]);
    for name in ["f1", "f2", "f3", "f4"] {
        fx.write(&format!("{name}.txt"), name);
        fx.commit(&format!("feat {name}"));
    }
    let repo = fx.repo();
    let onto = tip(&fx, "main");
    let branch_tip = tip(&fx, "feature");
    assert_eq!(
        futures::probe(&repo, onto, branch_tip, None).unwrap(),
        Verdict::Unknown {
            reason: UnknownReason::TooManyCommits,
        }
    );
    // Admit the range and the same history replays: the cap was what refused,
    // not the history.
    fx.set_config("fufu.futuresDepth", "10");
    // A fresh handle: a gix Repository serves the config it had at open
    // time, so the stale handle would still refuse at the old cap of 3.
    let repo = fx.repo();
    assert_eq!(
        futures::probe(&repo, onto, branch_tip, None).unwrap(),
        Verdict::Clean {
            replayed: 4,
            dropped: 0
        }
    );
}

// --- The invariant: a probe writes nothing ---

#[test]
fn a_probe_writes_nothing_to_the_object_database() {
    let fx = linear_clean();
    let before = loose_count(&fx);
    let repo = fx.repo();
    let v = futures::probe(&repo, tip(&fx, "main"), tip(&fx, "feature"), None).unwrap();
    assert_eq!(
        v,
        Verdict::Clean {
            replayed: 3,
            dropped: 0
        }
    );
    let after = loose_count(&fx);
    assert_eq!(
        before, after,
        "clean probe moved the loose object count {before} -> {after}"
    );

    // A conflicted merge is the case most likely to leak a blob.
    let fx = conflict_in_the_middle();
    let before = loose_count(&fx);
    let repo = fx.repo();
    let v = futures::probe(&repo, tip(&fx, "main"), tip(&fx, "feature"), None).unwrap();
    assert!(
        matches!(v, Verdict::Conflict { .. }),
        "fixture should conflict: {v:?}"
    );
    let after = loose_count(&fx);
    assert_eq!(
        before, after,
        "conflict probe moved the loose object count {before} -> {after}"
    );
}

/// The first numeric field of `git count-objects`: the loose object count.
fn count_objects_files(fx: &Fixture) -> usize {
    fx.git(&["count-objects"])
        .split_whitespace()
        .find_map(|t| t.trim_end_matches(',').parse::<usize>().ok())
        .expect("count-objects reports a count")
}

// --- The base ladder ---

#[test]
fn parent_metadata_wins_the_ladder() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("base");
    fx.git(&["branch", "base"]);
    fx.git(&["branch", "stacked"]);
    ff_core::branchmeta::write(
        &fx.repo(),
        "stacked",
        &ff_core::branchmeta::BranchMeta {
            parent: Some("base".into()),
            ..Default::default()
        },
    )
    .expect("write metadata");
    let got = futures::base_for(&fx.repo(), "stacked")
        .expect("base_for")
        .expect("a base");
    assert_eq!(
        got,
        futures::SyncRef {
            name: "base".into(),
            r#ref: "refs/heads/base".into(),
            tip: tip(&fx, "base").to_string(),
            role: Role::Parent,
        }
    );
}

#[test]
fn a_parent_that_no_longer_resolves_falls_through_to_trunk() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("base");
    fx.git(&["branch", "stacked"]);
    ff_core::branchmeta::write(
        &fx.repo(),
        "stacked",
        &ff_core::branchmeta::BranchMeta {
            parent: Some("ghost".into()),
            ..Default::default()
        },
    )
    .expect("write metadata");
    let got = futures::base_for(&fx.repo(), "stacked")
        .expect("base_for")
        .expect("a base");
    assert_eq!(
        got,
        futures::SyncRef {
            name: "main".into(),
            r#ref: "refs/heads/main".into(),
            tip: tip(&fx, "main").to_string(),
            role: Role::Trunk,
        }
    );
}

#[test]
fn a_branch_measures_against_trunk() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("base");
    fx.git(&["branch", "feature"]);
    let got = futures::base_for(&fx.repo(), "feature")
        .expect("base_for")
        .expect("a base");
    assert_eq!(
        got,
        futures::SyncRef {
            name: "main".into(),
            r#ref: "refs/heads/main".into(),
            tip: tip(&fx, "main").to_string(),
            role: Role::Trunk,
        }
    );
}

/// Configure `origin/main` as the upstream of `main`, pointing at `target`.
fn set_upstream(fx: &Fixture, target: &str) {
    fx.git(&["config", "remote.origin.url", "file:///nonexistent"]);
    fx.git(&[
        "config",
        "remote.origin.fetch",
        "+refs/heads/*:refs/remotes/origin/*",
    ]);
    fx.git(&["config", "branch.main.remote", "origin"]);
    fx.git(&["config", "branch.main.merge", "refs/heads/main"]);
    if !target.is_empty() {
        fx.git(&["update-ref", "refs/remotes/origin/main", target]);
    }
}

#[test]
fn standing_on_trunk_has_no_base() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("base");
    set_upstream(&fx, fx.git(&["rev-parse", "main"]).trim());
    // Trunk sits on nothing, so the upstream is the remote axis's business,
    // not the base's.
    assert!(
        futures::base_for(&fx.repo(), "main")
            .expect("base_for")
            .is_none()
    );
}

#[test]
fn standing_on_trunk_with_no_upstream_has_no_base() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("base");
    assert!(
        futures::base_for(&fx.repo(), "main")
            .expect("base_for")
            .is_none()
    );
}

#[test]
fn an_ambiguous_trunk_is_swallowed_to_none() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("base");
    fx.git(&["branch", "master"]);
    fx.git(&["branch", "feature"]);
    // main and master both exist and no fufu.trunk is set, so the trunk
    // cannot be named; base_for must swallow that to Ok(None), not Err.
    assert!(
        futures::base_for(&fx.repo(), "feature")
            .expect("no error")
            .is_none()
    );
}

// --- The remote ladder ---

#[test]
fn the_remote_is_this_branchs_own_copy() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("base");
    set_upstream(&fx, fx.git(&["rev-parse", "main"]).trim());
    let got = futures::remote_for(&fx.repo(), "main")
        .expect("remote_for")
        .expect("a remote");
    assert_eq!(
        got,
        futures::SyncRef {
            name: "origin/main".into(),
            r#ref: "refs/remotes/origin/main".into(),
            tip: tip(&fx, "main").to_string(),
            role: Role::Remote,
        }
    );
}

#[test]
fn a_tracking_ref_wearing_another_name_is_an_alias() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("base");
    let main_sha = fx.git(&["rev-parse", "main"]).trim().to_string();
    fx.git(&["branch", "feature"]);
    fx.git(&["config", "remote.origin.url", "file:///nonexistent"]);
    fx.git(&[
        "config",
        "remote.origin.fetch",
        "+refs/heads/*:refs/remotes/origin/*",
    ]);
    // feature tracks origin/main: a tracking ref wearing another branch's name.
    fx.git(&["config", "branch.feature.remote", "origin"]);
    fx.git(&["config", "branch.feature.merge", "refs/heads/main"]);
    fx.git(&["update-ref", "refs/remotes/origin/main", &main_sha]);
    let got = futures::remote_for(&fx.repo(), "feature")
        .expect("remote_for")
        .expect("a remote");
    assert_eq!(got.role, Role::RemoteAlias);
    assert_eq!(got.name, "origin/main");
}

#[test]
fn no_upstream_means_no_remote() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("base");
    assert!(
        futures::remote_for(&fx.repo(), "main")
            .expect("remote_for")
            .is_none()
    );
}

#[test]
fn a_configured_but_absent_remote_is_gone() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("base");
    // The empty target skips the update-ref: configured, but the ref is
    // absent — a gone remote.
    set_upstream(&fx, "");
    let got = futures::remote_for(&fx.repo(), "main")
        .expect("remote_for")
        .expect("a remote");
    assert_eq!(got.tip, "");
    assert_eq!(got.role, Role::Remote);
    let f = futures::remote_future(&fx.repo(), "main", Some(tip(&fx, "main")), None)
        .expect("remote_future")
        .expect("a future");
    assert_eq!(f.verdict, Verdict::Gone);
}

// --- The cache ---

/// The cache file for `branch` under the fixture's common dir.
fn cache_file(fx: &Fixture, branch: &str) -> std::path::PathBuf {
    fx.path().join(".git/fufu/futures").join(branch)
}

/// Overwrite the stored base verdict with a lie no probe could produce.
fn poison(path: &std::path::Path) {
    let text = std::fs::read_to_string(path).expect("the cache file exists");
    let mut v: serde_json::Value = serde_json::from_str(&text).expect("the cache is JSON");
    v["base"]["verdict"] = serde_json::json!({ "kind": "clean", "replayed": 999 });
    std::fs::write(path, serde_json::to_string(&v).unwrap()).expect("write the poisoned cache");
}

/// Overwrite the stored remote verdict with a lie no probe could produce.
fn poison_remote(path: &std::path::Path) {
    let text = std::fs::read_to_string(path).expect("the cache file exists");
    let mut v: serde_json::Value = serde_json::from_str(&text).expect("the cache is JSON");
    v["remote"]["verdict"] = serde_json::json!({ "kind": "clean", "replayed": 999 });
    std::fs::write(path, serde_json::to_string(&v).unwrap()).expect("write the poisoned cache");
}

#[test]
fn a_warm_call_serves_the_stored_verdict() {
    let fx = linear_clean();
    let repo = fx.repo();
    let t = tip(&fx, "feature");
    let f = futures::base_future(&repo, "feature", Some(t), None)
        .unwrap()
        .expect("a future");
    assert_eq!(
        f.verdict,
        Verdict::Clean {
            replayed: 3,
            dropped: 0
        }
    );
    // A second call returning the poisoned 999 could not have re-merged.
    poison(&cache_file(&fx, "feature"));
    let f = futures::base_future(&repo, "feature", Some(t), None)
        .unwrap()
        .expect("a future");
    assert_eq!(
        f.verdict,
        Verdict::Clean {
            replayed: 999,
            dropped: 0
        }
    );
}

#[test]
fn cache_invalidates_when_the_base_tip_changes() {
    let fx = linear_clean();
    let repo = fx.repo();
    let t = tip(&fx, "feature");
    futures::base_future(&repo, "feature", Some(t), None)
        .unwrap()
        .expect("a future");
    poison(&cache_file(&fx, "feature"));
    // Main moves again: a different base tip must miss the cache.
    fx.git(&["switch", "main"]);
    fx.write("m2.txt", "more\n");
    fx.commit("main moves again");
    let f = futures::base_future(&repo, "feature", Some(t), None)
        .unwrap()
        .expect("a future");
    assert_eq!(
        f.verdict,
        Verdict::Clean {
            replayed: 3,
            dropped: 0
        }
    );
}

#[test]
fn cache_invalidates_when_the_branch_tip_changes() {
    let fx = linear_clean();
    let repo = fx.repo();
    let t = tip(&fx, "feature");
    futures::base_future(&repo, "feature", Some(t), None)
        .unwrap()
        .expect("a future");
    poison(&cache_file(&fx, "feature"));
    // Feature moves: a different branch tip must miss the cache.
    fx.write("f4.txt", "four\n");
    fx.commit("feat four");
    let f = futures::base_future(&repo, "feature", Some(tip(&fx, "feature")), None)
        .unwrap()
        .expect("a future");
    assert_eq!(
        f.verdict,
        Verdict::Clean {
            replayed: 4,
            dropped: 0
        }
    );
}

#[test]
fn cache_invalidates_when_the_open_tree_changes() {
    let fx = linear_clean();
    let repo = fx.repo();
    let t = tip(&fx, "feature");
    futures::base_future(&repo, "feature", Some(t), None)
        .unwrap()
        .expect("a future");
    poison(&cache_file(&fx, "feature"));
    // A different open_tree argument must miss the cache; the tip's own tree
    // is admitted and skipped, so the truth is still the clean replay.
    let open = oid(&fx.git(&["rev-parse", "feature^{tree}"]));
    let f = futures::base_future(&repo, "feature", Some(t), Some(open))
        .unwrap()
        .expect("a future");
    assert_eq!(
        f.verdict,
        Verdict::Clean {
            replayed: 3,
            dropped: 0
        }
    );
}

#[test]
fn cache_invalidates_when_the_base_ref_changes() {
    let fx = linear_clean();
    let repo = fx.repo();
    let t = tip(&fx, "feature");
    futures::base_future(&repo, "feature", Some(t), None)
        .unwrap()
        .expect("a future");
    poison(&cache_file(&fx, "feature"));
    // Point the ladder at a different base: a different base ref must miss
    // the cache.
    fx.git(&["branch", "alt", "main"]);
    fx.set_config("fufu.trunk", "alt");
    // A fresh handle: a gix Repository serves the config it had at open
    // time, so the stale handle would still measure against main.
    let repo = fx.repo();
    let f = futures::base_future(&repo, "feature", Some(t), None)
        .unwrap()
        .expect("a future");
    assert_eq!(f.against.name, "alt");
    assert_eq!(
        f.verdict,
        Verdict::Clean {
            replayed: 3,
            dropped: 0
        }
    );
}

#[test]
fn deleting_the_cache_directory_changes_no_answer() {
    let fx = linear_clean();
    let repo = fx.repo();
    let t = tip(&fx, "feature");
    let v1 = futures::base_future(&repo, "feature", Some(t), None)
        .unwrap()
        .expect("a future")
        .verdict;
    std::fs::remove_dir_all(fx.path().join(".git/fufu/futures")).expect("drop the cache");
    let v2 = futures::base_future(&repo, "feature", Some(t), None)
        .unwrap()
        .expect("a future")
        .verdict;
    assert_eq!(v1, v2, "the cache is a cache and nothing else");
}

#[test]
fn cache_remove_drops_the_file() {
    let fx = linear_clean();
    let repo = fx.repo();
    futures::base_future(&repo, "feature", Some(tip(&fx, "feature")), None)
        .unwrap()
        .expect("a future");
    let path = cache_file(&fx, "feature");
    assert!(path.exists(), "the first call must have written the cache");
    futures::cache::remove(&repo, "feature").expect("remove the entry");
    assert!(!path.exists(), "remove must drop the file");
    futures::cache::remove(&repo, "feature").expect("removing an absent file is Ok");
}

// --- The remote axis ---

#[test]
fn a_gone_remote_writes_no_cache() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("base");
    set_upstream(&fx, "");
    let f = futures::remote_future(&fx.repo(), "main", Some(tip(&fx, "main")), None)
        .expect("remote_future")
        .expect("a future");
    assert_eq!(f.verdict, Verdict::Gone);
    assert!(
        !cache_file(&fx, "main").exists(),
        "there is nothing worth remembering about an absent ref"
    );
}

#[test]
fn unpushed_commits_are_up_to_date_against_the_remote() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("one");
    // The shared copy sits at the first commit; two more land on the branch.
    set_upstream(&fx, fx.git(&["rev-parse", "main"]).trim());
    fx.write("b.txt", "b\n");
    fx.commit("two");
    fx.write("c.txt", "c\n");
    fx.commit("three");
    let f = futures::remote_future(&fx.repo(), "main", Some(tip(&fx, "main")), None)
        .expect("remote_future")
        .expect("a future");
    // The surface will spell this "2 to publish": up-to-date against the remote
    // means the branch is ahead, not that nothing moved.
    assert_eq!(f.verdict, Verdict::UpToDate { ahead: 2 });
}

#[test]
fn a_remote_that_moved_ahead_fast_forwards() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("a");
    // Capture the branch tip before the remote pulls ahead of it.
    let a_sha = fx.git(&["rev-parse", "main"]).trim().to_string();
    fx.write("b.txt", "b\n");
    fx.commit("b");
    fx.write("c.txt", "c\n");
    fx.commit("c");
    // The shared copy has moved to c; we simulate the branch still at a.
    set_upstream(&fx, fx.git(&["rev-parse", "main"]).trim());
    let f = futures::remote_future(&fx.repo(), "main", Some(oid(&a_sha)), None)
        .expect("remote_future")
        .expect("a future");
    assert_eq!(f.verdict, Verdict::FastForward { behind: 2 });
}

#[test]
fn both_axes_cache_independently() {
    let fx = linear_clean();
    let feature_sha = fx.git(&["rev-parse", "feature"]).trim().to_string();
    // Give feature an upstream pointing at its own tip: the remote verdict
    // is UpToDate { ahead: 0 }.
    fx.git(&["config", "remote.origin.url", "file:///nonexistent"]);
    fx.git(&[
        "config",
        "remote.origin.fetch",
        "+refs/heads/*:refs/remotes/origin/*",
    ]);
    fx.git(&["config", "branch.feature.remote", "origin"]);
    fx.git(&["config", "branch.feature.merge", "refs/heads/feature"]);
    fx.git(&["update-ref", "refs/remotes/origin/feature", &feature_sha]);
    // A fresh handle: a gix Repository serves the config it had at open time.
    let repo = fx.repo();
    let t = tip(&fx, "feature");

    // First call computes both axes and writes both slots.
    let both = futures::futures_for(&repo, "feature", Some(t), None).expect("futures_for");
    assert_eq!(
        both.base.unwrap().verdict,
        Verdict::Clean {
            replayed: 3,
            dropped: 0
        }
    );
    assert_eq!(both.remote.unwrap().verdict, Verdict::UpToDate { ahead: 0 });

    // Poison the base slot only: the remote slot must survive untouched.
    poison(&cache_file(&fx, "feature"));
    let both = futures::futures_for(&repo, "feature", Some(t), None).expect("futures_for");
    assert_eq!(
        both.base.unwrap().verdict,
        Verdict::Clean {
            replayed: 999,
            dropped: 0
        }
    );
    assert_eq!(
        both.remote.unwrap().verdict,
        Verdict::UpToDate { ahead: 0 },
        "poisoning the base slot must not clobber the remote slot"
    );

    // Poison the remote slot only: the base slot must survive untouched.
    poison_remote(&cache_file(&fx, "feature"));
    let both = futures::futures_for(&repo, "feature", Some(t), None).expect("futures_for");
    assert_eq!(
        both.remote.unwrap().verdict,
        Verdict::Clean {
            replayed: 999,
            dropped: 0
        }
    );
    assert_eq!(
        both.base.unwrap().verdict,
        Verdict::Clean {
            replayed: 999,
            dropped: 0
        },
        "poisoning the remote slot must not clobber the base slot"
    );
}

// --- The path ---

#[test]
fn a_slash_in_the_branch_name_round_trips() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("base");
    fx.git(&["branch", "ff/witty-otter"]);
    let repo = fx.repo();
    futures::base_future(
        &repo,
        "ff/witty-otter",
        Some(tip(&fx, "ff/witty-otter")),
        None,
    )
    .unwrap()
    .expect("a future");
    assert!(
        cache_file(&fx, "ff/witty-otter").exists(),
        "the cache must land at fufu/futures/ff/witty-otter"
    );
}

#[test]
fn a_probe_counts_what_the_replay_would_drop() {
    let fx = Fixture::new();
    fx.write("root.txt", "root\n");
    fx.commit("root");
    fx.git(&["branch", "feature"]);
    // The base already holds a.txt with byte-identical content, so the
    // commit that adds it introduces nothing over the base.
    fx.write("a.txt", "same\n");
    fx.commit("base gains a.txt");
    fx.git(&["switch", "feature"]);
    fx.write("a.txt", "same\n");
    fx.commit("adds a.txt");
    fx.write("b.txt", "b\n");
    fx.commit("adds b.txt");

    let repo = fx.repo();
    let v = futures::probe(&repo, tip(&fx, "main"), tip(&fx, "feature"), None).unwrap();
    assert_eq!(
        v,
        Verdict::Clean {
            replayed: 1,
            dropped: 1
        }
    );
}

#[test]
fn the_replayed_tree_never_becomes_findable() {
    let fx = linear_clean();
    let loose_before = loose_count(&fx);
    let objects_before = count_objects_files(&fx);
    let repo = fx.repo();
    assert_eq!(
        futures::probe(&repo, tip(&fx, "main"), tip(&fx, "feature"), None).unwrap(),
        Verdict::Clean {
            replayed: 3,
            dropped: 0
        }
    );
    assert_eq!(
        loose_before,
        loose_count(&fx),
        "the replayed tree leaked into the loose odb"
    );
    assert_eq!(
        objects_before,
        count_objects_files(&fx),
        "the replayed tree leaked into the odb per count-objects"
    );
}

/// The shape of a clone: `origin/HEAD` names trunk, so trunk is a
/// remote-tracking ref even when a local branch of the same name is what you
/// are standing on. Trunk still sits on nothing.
#[test]
fn standing_on_a_remote_only_trunk_has_no_base() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("base");
    set_upstream(&fx, fx.git(&["rev-parse", "main"]).trim());
    fx.git(&[
        "symbolic-ref",
        "refs/remotes/origin/HEAD",
        "refs/remotes/origin/main",
    ]);
    // Identity by ref would read `refs/remotes/origin/main` against
    // `refs/heads/main`, call them different branches, and hand main its own
    // shared copy as a base — the same ref reported once as the base and
    // again as the remote.
    assert!(
        futures::base_for(&fx.repo(), "main")
            .expect("base_for")
            .is_none(),
        "standing on trunk is standing on trunk however trunk is spelled"
    );
}

/// A branch can be configured to track the very ref that is also its base.
/// Both axes then name one set of commits, and one set of commits is one
/// thing to reconcile.
#[test]
fn an_upstream_that_is_also_the_base_is_one_axis() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    let root = fx.commit("base");
    fx.git(&["update-ref", "refs/remotes/origin/main", &root]);
    fx.git(&[
        "symbolic-ref",
        "refs/remotes/origin/HEAD",
        "refs/remotes/origin/main",
    ]);
    fx.git(&["switch", "-q", "-c", "feature"]);
    fx.write("f.txt", "f\n");
    fx.commit("mine");
    // `git branch -u origin/main feature`: the upstream is trunk itself.
    fx.git(&["config", "remote.origin.url", "file:///nonexistent"]);
    fx.git(&[
        "config",
        "remote.origin.fetch",
        "+refs/heads/*:refs/remotes/origin/*",
    ]);
    fx.git(&["config", "branch.feature.remote", "origin"]);
    fx.git(&["config", "branch.feature.merge", "refs/heads/main"]);

    let repo = fx.repo();
    let head = tip(&fx, "feature");
    let futures = futures::futures_for(&repo, "feature", Some(head), None).expect("futures_for");

    assert!(
        futures.base.is_none(),
        "the base is the half that goes quiet: the remote is the noun that also decides the push"
    );
    let remote = futures.remote.expect("the remote axis survives");
    assert_eq!(remote.against.r#ref, "refs/remotes/origin/main");
    assert_eq!(remote.against.role, Role::RemoteAlias);
}
