//! Capture mechanics against the one log: parent order, identity, one pointer
//! per branch, no-op tiers, reflogs, the gc config guard, and contention.

use ff_core::{CaptureOutcome, Provenance, TakeOptions};
use ff_testsupport::Fixture;
use ff_testsupport::capture::{captures_via_git, chain_via_git};

fn take(fx: &Fixture) -> CaptureOutcome {
    let repo = fx.repo();
    ff_core::capture(&repo, &Provenance::new("manual", None)).expect("take")
}

fn take_created(fx: &Fixture) -> String {
    match take(fx) {
        CaptureOutcome::Created { id, .. } => id.hex(),
        other => panic!("expected Created, got {other:?}"),
    }
}

/// The log's floor — the `init` note reconciliation lays down before the first
/// capture. Every capture below sits on top of it, which is what keeps its
/// parents `[prev, base]` rather than `[base]`.
fn floor(fx: &Fixture) -> String {
    chain_via_git(fx, &fx.path())
        .pop()
        .expect("the log has a floor")
}

fn noop_tip(outcome: CaptureOutcome) -> Option<String> {
    match outcome {
        CaptureOutcome::NoOp { tip, .. } => tip.map(|id| id.hex()),
        other => panic!("expected NoOp, got {other:?}"),
    }
}

fn parents_of(fx: &Fixture, id: &str) -> Vec<String> {
    let out = fx.git(&["rev-list", "--parents", "-n", "1", id]);
    out.split_whitespace().skip(1).map(str::to_string).collect()
}

#[test]
fn parent_order_across_takes() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    let base1 = fx.commit("init");

    // First capture: [the log's floor, HEAD]. The floor is what stops slot 1
    // from being the base commit, which is what would send `git log
    // --first-parent` out through the user's history.
    fx.write("a.txt", "one\n");
    let snap1 = take_created(&fx);
    let floor = floor(&fx);
    assert_eq!(parents_of(&fx, &snap1), vec![floor.clone(), base1.clone()]);

    // Second: [prev operation, HEAD] — order load-bearing.
    fx.write("a.txt", "two\n");
    let snap2 = take_created(&fx);
    assert_eq!(parents_of(&fx, &snap2), vec![snap1.clone(), base1.clone()]);

    // Committing exactly the captured state is a no-op: the head tree now
    // equals the last snapshot's tree, so there is nothing new to record.
    let base2 = fx.commit("landed");
    assert_eq!(
        noop_tip(take(&fx)),
        Some(snap2.clone()),
        "landing the captured state records nothing new"
    );
    fx.write("a.txt", "three\n");
    let snap3 = take_created(&fx);
    assert_eq!(parents_of(&fx, &snap3), vec![snap2.clone(), base2.clone()]);

    // A clean tree whose commit content no snapshot recorded (tier-1 with a
    // moved head tree): the take records the post-commit state itself.
    fx.write("a.txt", "four\n");
    let base3 = fx.commit("landed again");
    let snap4 = take_created(&fx);
    assert_eq!(parents_of(&fx, &snap4), vec![snap3.clone(), base3.clone()]);
    let snap4_tree = fx.git(&["rev-parse", &format!("{snap4}^{{tree}}")]);
    let head_tree = fx.git(&["rev-parse", "HEAD^{tree}"]);
    assert_eq!(snap4_tree, head_tree, "records the post-commit tree");

    // The whole log bears the fufu identity, and the walk stops at the floor.
    assert_eq!(
        captures_via_git(&fx, &fx.path()),
        vec![snap4, snap3, snap2, snap1]
    );
    assert_eq!(
        chain_via_git(&fx, &fx.path()).pop(),
        Some(floor),
        "and the last row is the floor, not a commit of the user's"
    );
}

#[test]
fn an_unborn_branch_captures_with_no_base_edge() {
    let fx = Fixture::new();
    fx.write("first.txt", "1\n");
    let snap1 = take_created(&fx);
    let floor = floor(&fx);
    // Unborn: there is no base, so slot 2 is simply absent — and the floor
    // itself is parentless, since an unborn HEAD gives it no base either.
    assert_eq!(parents_of(&fx, &snap1), vec![floor.clone()]);
    assert!(
        parents_of(&fx, &floor).len() == 1,
        "the floor's only parent is its own record"
    );
    fx.write("second.txt", "2\n");
    let snap2 = take_created(&fx);
    assert_eq!(parents_of(&fx, &snap2), vec![snap1.clone()]);
    assert_eq!(captures_via_git(&fx, &fx.path()), vec![snap2, snap1]);
}

#[test]
fn one_pointer_per_branch_and_detached() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    let first = fx.commit("init");

    fx.write("a.txt", "main dirty\n");
    take_created(&fx);
    fx.git(&["checkout", "-q", "-b", "feat/nested"]);
    fx.write("a.txt", "feat dirty\n");
    take_created(&fx);
    fx.git(&["checkout", "-q", &first]);
    fx.write("a.txt", "detached dirty\n");
    take_created(&fx);

    for r in [
        "refs/fufu/snap/main",
        "refs/fufu/snap/feat/nested",
        "refs/fufu/snap/@detached",
    ] {
        let out = fx.try_git(&["rev-parse", "--verify", "--quiet", r]);
        assert!(out.status.success(), "missing branch pointer {r}");
    }
    // And all three point into one log.
    let log = fx.git(&["rev-list", "--count", "--first-parent", "refs/fufu/ops"]);
    assert_eq!(
        log.trim(),
        "5",
        "three captures, the floor, and the floor's record"
    );
}

fn object_file_count(fx: &Fixture) -> usize {
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

#[test]
fn tier1_noop_writes_no_objects() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    fx.write("a.txt", "dirty\n");
    let snap = take_created(&fx);

    // Clean up the dirt so the worktree equals the snapshot? No — the tree is
    // still dirty relative to HEAD but equal to the snapshot: tier-2 territory.
    // For tier-1, use a clean tree: check out the state git already has.
    fx.git(&["checkout", "-q", "--", "a.txt"]);
    // Worktree now equals HEAD, but the snapshot tip's tree differs (it holds
    // the dirty state) — this take records the post-restore state.
    let snap2 = take_created(&fx);
    assert_ne!(snap, snap2);

    // Now truly clean AND captured: tier-1, zero object writes.
    let before = object_file_count(&fx);
    for _ in 0..2 {
        assert_eq!(noop_tip(take(&fx)), Some(snap2.clone()));
    }
    assert_eq!(
        object_file_count(&fx),
        before,
        "tier-1 no-op must write zero objects"
    );
}

#[test]
fn noop_without_a_log_creates_nothing() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    assert_eq!(noop_tip(take(&fx)), None);
    // The floor is laid only when a capture is actually going to write. A
    // clean tree writes nothing at all, refs included — which is what lets a
    // read command behave like one.
    for r in ["refs/fufu/snap/main", "refs/fufu/ops"] {
        let out = fx.try_git(&["rev-parse", "--verify", "--quiet", r]);
        assert!(!out.status.success(), "clean tree must not create {r}");
    }
}

#[test]
fn reflog_visible_to_git() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    fx.write("a.txt", "one\n");
    take_created(&fx);
    fx.write("a.txt", "two\n");
    take_created(&fx);
    let log = fx.git(&["reflog", "show", "refs/fufu/snap/main"]);
    assert_eq!(
        log.lines().count(),
        3,
        "two captures and the floor: {log:?}"
    );
    assert!(
        log.contains("manual"),
        "reflog message is the subject: {log:?}"
    );
}

#[test]
fn gc_config_written_once_and_preserving() {
    let fx = Fixture::new();
    // A hand-written config with a comment that must survive byte-for-byte.
    let config_path = fx.path().join(".git/config");
    let mut existing = std::fs::read_to_string(&config_path).unwrap();
    existing.push_str("# user comment\n[custom]\n\tkey = value\n");
    std::fs::write(&config_path, &existing).unwrap();

    fx.write("a.txt", "a\n");
    fx.commit("init");
    fx.write("a.txt", "dirty\n");
    take_created(&fx);

    let after = std::fs::read_to_string(&config_path).unwrap();
    assert!(after.contains("# user comment"), "user content preserved");
    assert!(after.contains("[custom]"), "foreign section preserved");
    assert_eq!(
        fx.git(&["config", "gc.refs/fufu/*.reflogExpire"]).trim(),
        "never"
    );
    assert_eq!(
        fx.git(&["config", "gc.refs/fufu/*.reflogExpireUnreachable"])
            .trim(),
        "never"
    );

    // A second chain creation must not duplicate the section.
    fx.git(&["checkout", "-q", "-b", "feat"]);
    fx.write("a.txt", "feat dirty\n");
    take_created(&fx);
    let again = std::fs::read_to_string(&config_path).unwrap();
    assert_eq!(
        again.matches("reflogExpire = never").count(),
        1,
        "no duplicate keys: {again}"
    );
    assert_eq!(
        after, again,
        "second chain creation leaves config untouched"
    );
}

#[test]
fn gc_config_never_overwrites_user_values() {
    let fx = Fixture::new();
    fx.set_config("gc.refs/fufu/*.reflogExpire", "1.day");
    fx.write("a.txt", "a\n");
    fx.commit("init");
    fx.write("a.txt", "dirty\n");
    take_created(&fx);
    assert_eq!(
        fx.git(&["config", "gc.refs/fufu/*.reflogExpire"]).trim(),
        "1.day",
        "user value kept"
    );
    assert_eq!(
        fx.git(&["config", "gc.refs/fufu/*.reflogExpireUnreachable"])
            .trim(),
        "never",
        "missing key appended"
    );
}

#[test]
fn held_lock_reports_contended() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    fx.write("a.txt", "one\n");
    take_created(&fx);

    let lock = fx.path().join(".git/refs/fufu/snap/main.lock");
    std::fs::create_dir_all(lock.parent().unwrap()).unwrap();
    std::fs::write(&lock, "held by someone else").unwrap();

    fx.write("a.txt", "two\n");
    let start = std::time::Instant::now();
    match take(&fx) {
        CaptureOutcome::Contended => {}
        other => panic!("expected Contended, got {other:?}"),
    }
    assert!(
        start.elapsed() < std::time::Duration::from_secs(2),
        "contention must report promptly, not block"
    );
    std::fs::remove_file(&lock).unwrap();

    // After the lock clears, capture resumes and the log is intact.
    take_created(&fx);
    assert_eq!(captures_via_git(&fx, &fx.path()).len(), 2);
}

/// Two racing captures: losers must report Contended (never error, never
/// corrupt the chain).
#[test]
fn concurrent_takes_never_error() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");

    for round in 0..4 {
        fx.write("a.txt", &format!("round {round}\n"));
        let dir = fx.path();
        let results: Vec<CaptureOutcome> = std::thread::scope(|scope| {
            let handles: Vec<_> = (0..2)
                .map(|_| {
                    let dir = dir.clone();
                    scope.spawn(move || {
                        let repo = ff_core::discover_isolated(&dir).expect("discover");
                        ff_core::capture_with(
                            &repo,
                            &Provenance::new("manual", None),
                            &TakeOptions::default(),
                        )
                        .expect("take must not error under contention")
                    })
                })
                .collect();
            handles.into_iter().map(|h| h.join().unwrap()).collect()
        });
        assert!(
            results.iter().any(|r| matches!(
                r,
                CaptureOutcome::Created { .. } | CaptureOutcome::NoOp { .. }
            )),
            "at least one racer must make progress: {results:?}"
        );
    }

    // The log survived the races as a valid fufu log.
    let chain = chain_via_git(&fx, &fx.path());
    assert!(!chain.is_empty());
}

#[test]
fn evolog_orders_snapshots_with_edges() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    let base1 = fx.commit("init");
    fx.write("a.txt", "one\n");
    let snap1 = take_created(&fx);
    fx.write("a.txt", "two\n");
    let snap2 = take_created(&fx);
    fx.write("a.txt", "three\n");
    let base2 = fx.commit("landed");
    fx.write("a.txt", "four\n");
    let snap3 = take_created(&fx);

    let repo = fx.repo();
    let rows = ff_core::evolog(&repo, &ff_core::EvologOptions::default()).expect("evolog");
    let ids: Vec<&str> = rows.iter().map(|s| s.id.as_str()).collect();
    assert_eq!(ids, vec![&snap3, &snap2, &snap1]);

    // Rows carry their edges: base (the HEAD the snapshot sat on) and prev.
    assert_eq!(rows[0].base.as_deref(), Some(base2.as_str()));
    assert_eq!(rows[0].prev.as_deref(), Some(snap2.as_str()));
    assert_eq!(rows[2].prev, None, "oldest snapshot has no prev");
    assert_eq!(rows[2].base.as_deref(), Some(base1.as_str()));

    // The limit caps snapshot rows.
    let rows = ff_core::evolog(
        &repo,
        &ff_core::EvologOptions {
            limit: Some(2),
            ..Default::default()
        },
    )
    .expect("evolog");
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].id, snap3);
    assert_eq!(rows[1].id, snap2);
}

#[test]
fn open_change_reports_pending_description_and_tip() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    let base = fx.commit("init");
    fx.write("a.txt", "dirty\n");
    let snap = take_created(&fx);

    let repo = fx.repo();
    ff_core::branchmeta::write(
        &repo,
        "main",
        &ff_core::branchmeta::BranchMeta {
            pending_description: Some("fix the frobnicator".into()),
            forked_from: None,
            parent: None,
        },
    )
    .expect("write meta");

    let open = ff_core::open_change(&repo).expect("open_change");
    assert_eq!(open.branch, "main");
    assert_eq!(open.id.as_deref(), Some(snap.as_str()));
    assert_eq!(open.base.as_deref(), Some(base.as_str()));
    assert!(open.base_short.is_some());
    assert_eq!(open.subject.as_deref(), Some("fix the frobnicator"));
    assert!(open.time.is_some());
    assert!(!open.clean, "tip tree differs from HEAD tree");

    // No identity → no pending hash.
    assert_eq!(open.pending, None);

    // With identity, pending is Some and stable.
    fx.set_config("user.name", "Pending User");
    fx.set_config("user.email", "pending@test");
    let repo = fx.repo();
    let open1 = ff_core::open_change(&repo).expect("open_change");
    let pending1 = open1.pending.clone();
    assert!(pending1.is_some(), "pending with identity + dirty tree");
    let pending1_str = pending1.as_ref().unwrap();
    assert_eq!(pending1_str.len(), 40);
    assert!(pending1_str.chars().all(|c| c.is_ascii_hexdigit()));

    // Stability: second call gives same hash.
    let open2 = ff_core::open_change(&repo).expect("open_change");
    assert_eq!(open2.pending, pending1, "pending hash is stable");

    // Changing the pending description changes the hash.
    ff_core::branchmeta::write(
        &repo,
        "main",
        &ff_core::branchmeta::BranchMeta {
            pending_description: Some("different plan".into()),
            forked_from: None,
            parent: None,
        },
    )
    .expect("write meta");
    let open3 = ff_core::open_change(&repo).expect("open_change");
    assert!(
        open3.pending != pending1,
        "changing description changes pending hash"
    );
}

#[test]
fn open_change_clean_flips_with_the_tree() {
    let fx = Fixture::new();
    fx.set_config("user.name", "Test User");
    fx.set_config("user.email", "test@test");
    fx.write("a.txt", "a\n");
    fx.commit("init");

    // No chain yet: clean by definition, no pending (clean + no description).
    let repo = fx.repo();
    let open = ff_core::open_change(&repo).expect("open_change");
    assert!(open.clean);
    assert_eq!(open.id, None);
    assert_eq!(open.pending, None, "no chain + no description → no pending");

    // Dirty + captured: tip tree diverges from HEAD.
    fx.write("a.txt", "dirty\n");
    take_created(&fx);
    let open = ff_core::open_change(&repo).expect("open_change");
    assert!(!open.clean);
    assert!(open.pending.is_some(), "dirty + identity → pending");

    // Landing the captured state: HEAD tree catches up to the tip.
    fx.commit("landed");
    let open = ff_core::open_change(&repo).expect("open_change");
    assert!(open.clean);
    assert_eq!(open.pending, None, "clean + no description → no pending");
}

#[test]
fn open_change_unborn_and_detached() {
    let fx = Fixture::new();
    fx.set_config("user.name", "Test User");
    fx.set_config("user.email", "test@test");
    let repo = fx.repo();
    let open = ff_core::open_change(&repo).expect("open_change");
    assert_eq!(open.branch, "main");
    assert_eq!(open.base, None, "unborn has no base");
    assert!(open.clean, "no chain yet");

    // Unborn + no tip + described has no timestamp source → pending == None.
    ff_core::branchmeta::write(
        &repo,
        "main",
        &ff_core::branchmeta::BranchMeta {
            pending_description: Some("unborn plan".into()),
            forked_from: None,
            parent: None,
        },
    )
    .expect("write meta");
    let open = ff_core::open_change(&repo).expect("open_change");
    assert_eq!(
        open.pending, None,
        "unborn + no tip + described → no timestamp source"
    );
    // Clear the description before continuing.
    ff_core::branchmeta::write(
        &repo,
        "main",
        &ff_core::branchmeta::BranchMeta {
            pending_description: None,
            forked_from: None,
            parent: None,
        },
    )
    .expect("clear meta");

    // A snapshot on the unborn branch: tip exists, still no base.
    fx.write("a.txt", "a\n");
    take_created(&fx);
    let open = ff_core::open_change(&repo).expect("open_change");
    assert_eq!(open.base, None);
    assert!(open.id.is_some());
    assert!(!open.clean, "tip tree is not the empty tree");
    assert!(open.pending.is_some(), "unborn + tip + identity → pending");

    // Detached HEAD gets the @detached chain.
    let base = fx.commit("init");
    fx.git(&["checkout", "-q", &base]);
    let open = ff_core::open_change(&repo).expect("open_change");
    assert_eq!(open.branch, "@detached");
    assert_eq!(open.base.as_deref(), Some(base.as_str()));
}

#[test]
fn pending_hash_matches_git_commit_tree() {
    let fx = Fixture::new();
    fx.set_config("user.name", "Pending User");
    fx.set_config("user.email", "pending@test");
    fx.write("a.txt", "a\n");
    fx.commit("init");
    fx.write("a.txt", "dirty\n");
    take_created(&fx);
    let repo = fx.repo();
    ff_core::branchmeta::write(
        &repo,
        "main",
        &ff_core::branchmeta::BranchMeta {
            pending_description: Some("plan the work".into()),
            forked_from: None,
            parent: None,
        },
    )
    .expect("write meta");
    let pending = ff_core::open_change(&repo)
        .expect("open_change")
        .pending
        .expect("pending");
    // git's own oracle: commit-tree with identical tree/parent/message/identity/time.
    let tip_time = fx
        .git(&["log", "-1", "--format=%ct", "refs/fufu/snap/main"])
        .trim()
        .to_string();
    let tree = fx
        .git(&["rev-parse", "refs/fufu/snap/main^{tree}"])
        .trim()
        .to_string();
    let date = format!("@{tip_time} +0000");
    let sha = fx.git_env_in(
        &fx.path(),
        &["commit-tree", &tree, "-p", "HEAD", "-m", "plan the work"],
        &[
            ("GIT_AUTHOR_NAME", "Pending User"),
            ("GIT_AUTHOR_EMAIL", "pending@test"),
            ("GIT_COMMITTER_NAME", "Pending User"),
            ("GIT_COMMITTER_EMAIL", "pending@test"),
            ("GIT_AUTHOR_DATE", &date),
            ("GIT_COMMITTER_DATE", &date),
        ],
    );
    assert_eq!(sha.trim(), pending, "pending hash == git commit-tree");
}

#[test]
fn pending_empty_commit_parent_is_head() {
    // Clean tree, no snapshot chain, but a pending description: the close
    // would mint an empty commit whose parent is HEAD.
    let fx = Fixture::new();
    fx.set_config("user.name", "Pending User");
    fx.set_config("user.email", "pending@test");
    fx.write("a.txt", "a\n");
    let head = fx.commit("init");

    // Write the pending description for "main".
    let repo = fx.repo();
    ff_core::branchmeta::write(
        &repo,
        "main",
        &ff_core::branchmeta::BranchMeta {
            pending_description: Some("plan the work".into()),
            forked_from: None,
            parent: None,
        },
    )
    .expect("write meta");

    // open_change should report a pending hash.
    let open = ff_core::open_change(&repo).expect("open_change");
    let pending = open.pending.expect("pending");

    // Oracle: git commit-tree with the same tree, parent == HEAD, same message.
    let head_time = fx
        .git(&["log", "-1", "--format=%ct", "HEAD"])
        .trim()
        .to_string();
    let tree = fx.git(&["rev-parse", "HEAD^{tree}"]).trim().to_string();
    let date = format!("@{head_time} +0000");
    let sha = fx.git_env_in(
        &fx.path(),
        &["commit-tree", &tree, "-p", &head, "-m", "plan the work"],
        &[
            ("GIT_AUTHOR_NAME", "Pending User"),
            ("GIT_AUTHOR_EMAIL", "pending@test"),
            ("GIT_COMMITTER_NAME", "Pending User"),
            ("GIT_COMMITTER_EMAIL", "pending@test"),
            ("GIT_AUTHOR_DATE", &date),
            ("GIT_COMMITTER_DATE", &date),
        ],
    );
    assert_eq!(
        sha.trim(),
        pending,
        "pending hash == git commit-tree with HEAD as parent"
    );
}

#[test]
fn snapshot_subject_records_provenance() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    fx.write("a.txt", "dirty\n");
    let repo = fx.repo();
    let outcome = ff_core::capture(
        &repo,
        &Provenance::new("manual", Some("checkpoint   before\nrefactor".into())),
    )
    .expect("take");
    let CaptureOutcome::Created { id, .. } = outcome else {
        panic!("expected Created");
    };
    let subject = fx.git(&["log", "-1", "--format=%s", &id.hex()]);
    assert_eq!(subject.trim(), "manual: checkpoint before refactor");
}
