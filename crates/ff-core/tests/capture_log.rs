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
    let log = fx.git(&[
        "rev-list",
        "--count",
        "--first-parent",
        "refs/fufu/wt/main/ops",
    ]);
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
    for r in ["refs/fufu/snap/main", "refs/fufu/wt/main/ops"] {
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

// --- the open commit ---------------------------------------------------------
//
// A dirty capture writes the open change as a real commit — the operation's
// tree over HEAD, the user at the change's birth, the pending description,
// the `change-id` header — names it at `refs/fufu/open/<branch>`, states it
// in `fufu-open`, and carries it as its last parent. The `@` row's sha is
// that commit's, and the close moves the branch onto it.

const NOW: i64 = 1_700_000_000;

fn take_at(fx: &Fixture, now: i64) -> String {
    let repo = fx.repo();
    match ff_core::capture_with(
        &repo,
        &Provenance::new("manual", None),
        &TakeOptions {
            now: Some(now),
            max_file_size: None,
        },
    )
    .expect("take")
    {
        CaptureOutcome::Created { id, .. } => id.hex(),
        other => panic!("expected Created, got {other:?}"),
    }
}

fn open_ref(fx: &Fixture) -> Option<String> {
    let out = fx.try_git_in(
        &fx.path(),
        &["rev-parse", "--verify", "-q", "refs/fufu/open/main"],
    );
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// `git cat-file -p` of a commit, as a map of header name to value, with the
/// message under `message`.
fn commit_fields(fx: &Fixture, id: &str) -> std::collections::HashMap<String, String> {
    let raw = fx.git(&["cat-file", "-p", id]);
    let (headers, message) = raw.split_once("\n\n").unwrap_or((&raw, ""));
    let mut fields: std::collections::HashMap<String, String> = headers
        .lines()
        .filter_map(|line| line.split_once(' '))
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
    fields.insert("message".into(), message.to_string());
    fields
}

fn describe(fx: &Fixture, text: &str, now: i64) {
    ff_core::describe::set_pending(
        &fx.repo(),
        Some(text.into()),
        &Provenance::new("pre", Some("ff describe".into())),
        Some(now),
        Vec::new(),
    )
    .expect("describe");
}

#[test]
fn a_dirty_capture_writes_the_open_commit() {
    let fx = Fixture::new();
    fx.set_config("user.name", "Open User");
    fx.set_config("user.email", "open@test");
    fx.write("a.txt", "a\n");
    let head = fx.commit("init");
    fx.write("a.txt", "dirty\n");
    let snap = take_at(&fx, NOW);

    let open = open_ref(&fx).expect("the ref names the open commit");
    let fields = commit_fields(&fx, &open);
    let snap_tree = fx.git(&["rev-parse", &format!("{snap}^{{tree}}")]);
    assert_eq!(
        fields["tree"],
        snap_tree.trim(),
        "tree == the capture's tree"
    );
    assert_eq!(fields["parent"], head, "parent == HEAD");
    let repo = fx.repo();
    let meta = ff_core::branchmeta::read(&repo, "main").expect("meta");
    assert_eq!(meta.change_born, Some(NOW), "the capture records the birth");
    assert_eq!(
        fields["author"],
        format!("Open User <open@test> {NOW} +0000"),
        "authored at the birth"
    );
    assert_eq!(
        fields["change-id"],
        meta.change_id.expect("an id was minted")
    );
    assert_eq!(fields["message"], "", "no description yet");
    assert_eq!(
        parents_of(&fx, &snap).last().map(String::as_str),
        Some(open.as_str()),
        "the capture carries the open commit as its last parent"
    );
    let op = ff_core::ops::OpLog::open(&repo)
        .unwrap()
        .get(ff_core::OpId::new(
            ff_core::gix::ObjectId::from_hex(snap.as_bytes()).unwrap(),
        ))
        .unwrap();
    assert_eq!(
        op.open_commit().map(|id| id.to_string()),
        Some(open.clone()),
        "and states it in the trailer"
    );

    let change = ff_core::open_change(&repo).expect("open_change");
    assert_eq!(
        change.pending.as_deref(),
        Some(open.as_str()),
        "the @ row's sha"
    );
    assert!(!change.clean);

    // A second identical capture no-ops, and the sha holds.
    assert!(matches!(take(&fx), CaptureOutcome::NoOp { .. }));
    assert_eq!(open_ref(&fx).as_deref(), Some(open.as_str()));
    // A later capture of a different tree moves the sha; the letters hold.
    fx.write("a.txt", "dirtier\n");
    take_at(&fx, NOW + 10);
    let moved = open_ref(&fx).expect("still open");
    assert_ne!(moved, open, "a new tree is a new open commit");
    let fields = commit_fields(&fx, &moved);
    assert_eq!(
        fields["author"],
        format!("Open User <open@test> {NOW} +0000"),
        "still authored at the birth"
    );
    assert_eq!(
        fields["committer"],
        format!("Open User <open@test> {} +0000", NOW + 10),
        "committed at the capture"
    );
    assert_eq!(fields["change-id"], meta_id(&fx), "the letters hold");
}

fn meta_id(fx: &Fixture) -> String {
    ff_core::branchmeta::read(&fx.repo(), "main")
        .expect("meta")
        .change_id
        .expect("an id")
}

#[test]
fn describe_moves_the_sha_and_not_the_letters() {
    let fx = Fixture::new();
    fx.set_config("user.name", "Open User");
    fx.set_config("user.email", "open@test");
    fx.write("a.txt", "a\n");
    fx.commit("init");
    fx.write("a.txt", "dirty\n");
    take_at(&fx, NOW);
    let before = open_ref(&fx).expect("open");
    let letters = meta_id(&fx);

    describe(&fx, "fix the frobnicator", NOW + 1);
    let after = open_ref(&fx).expect("still open");
    assert_ne!(after, before, "the message is part of the commit");
    let fields = commit_fields(&fx, &after);
    assert_eq!(fields["message"], "fix the frobnicator\n");
    assert_eq!(fields["change-id"], letters, "the letters do not move");
    let change = ff_core::open_change(&fx.repo()).expect("open_change");
    assert_eq!(change.pending.as_deref(), Some(after.as_str()));
    assert_eq!(change.change_id.as_deref(), Some(letters.as_str()));
    assert_eq!(change.subject.as_deref(), Some("fix the frobnicator"));
}

#[test]
fn a_clean_capture_deletes_the_open_ref() {
    let fx = Fixture::new();
    fx.set_config("user.name", "Open User");
    fx.set_config("user.email", "open@test");
    fx.write("a.txt", "a\n");
    fx.commit("init");
    fx.write("a.txt", "dirty\n");
    take_at(&fx, NOW);
    assert!(open_ref(&fx).is_some());

    // The user lands exactly the captured tree with git: the tree is HEAD's
    // again, the capture no-ops, and the ref goes with it.
    fx.commit("landed");
    assert!(matches!(take(&fx), CaptureOutcome::NoOp { .. }));
    assert_eq!(open_ref(&fx), None, "a clean tree has no open commit");
    let change = ff_core::open_change(&fx.repo()).expect("open_change");
    assert!(change.clean);
    assert_eq!(change.pending, None);

    // A description on a clean tree is letters and no sha: no empty commit,
    // ever.
    describe(&fx, "next up", NOW + 2);
    assert_eq!(open_ref(&fx), None);
    let change = ff_core::open_change(&fx.repo()).expect("open_change");
    assert!(change.change_id.is_some(), "the describe minted an id");
    assert_eq!(change.pending, None, "and no commit wears it yet");
}

#[test]
fn no_identity_means_no_open_commit() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    fx.write("a.txt", "dirty\n");
    let snap = take_at(&fx, NOW);
    assert_eq!(open_ref(&fx), None, "nobody to author it");
    let repo = fx.repo();
    let change = ff_core::open_change(&repo).expect("open_change");
    assert_eq!(change.pending, None);
    assert!(change.change_id.is_some(), "the id is minted regardless");
    let op = ff_core::ops::OpLog::open(&repo)
        .unwrap()
        .get(ff_core::OpId::new(
            ff_core::gix::ObjectId::from_hex(snap.as_bytes()).unwrap(),
        ))
        .unwrap();
    assert!(
        op.states_open(),
        "the capture states `none` rather than nothing"
    );
    assert_eq!(op.open_commit(), None);
    // Stated `none` holds the no-op: an unchanged tree never appends again
    // for want of an identity.
    assert!(matches!(take(&fx), CaptureOutcome::NoOp { .. }));
}

#[test]
fn open_change_clean_flips_with_the_tree() {
    let fx = Fixture::new();
    fx.set_config("user.name", "Test User");
    fx.set_config("user.email", "test@test");
    fx.write("a.txt", "a\n");
    fx.commit("init");

    // No chain yet: clean by definition, no open commit.
    let repo = fx.repo();
    let open = ff_core::open_change(&repo).expect("open_change");
    assert!(open.clean);
    assert_eq!(open.id, None);
    assert_eq!(open.pending, None, "no chain → no open commit");

    // Dirty + captured: tip tree diverges from HEAD.
    fx.write("a.txt", "dirty\n");
    take_created(&fx);
    let open = ff_core::open_change(&repo).expect("open_change");
    assert!(!open.clean);
    assert!(open.pending.is_some(), "dirty + identity → an open commit");

    // Landing the captured state with git: HEAD tree catches up to the tip,
    // and the ref, whose parent is no longer HEAD, stops describing it.
    fx.commit("landed");
    let open = ff_core::open_change(&repo).expect("open_change");
    assert!(open.clean);
    assert_eq!(open.pending, None, "clean → no open commit");
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

    // A capture on the unborn branch: a parentless open commit.
    fx.write("a.txt", "a\n");
    take_created(&fx);
    let open = ff_core::open_change(&repo).expect("open_change");
    assert_eq!(open.base, None);
    assert!(open.id.is_some());
    assert!(!open.clean, "tip tree is not the empty tree");
    let pending = open
        .pending
        .expect("unborn + tip + identity → an open commit");
    let fields = commit_fields(&fx, &pending);
    assert!(
        !fields.contains_key("parent"),
        "no commit to sit on: {fields:?}"
    );

    // Detached HEAD gets the @detached chain, and no open commit: there is
    // no branch to close onto.
    let base = fx.commit("init");
    fx.git(&["checkout", "-q", &base]);
    fx.write("a.txt", "detached\n");
    take_created(&fx);
    let open = ff_core::open_change(&repo).expect("open_change");
    assert_eq!(open.branch, "@detached");
    assert_eq!(open.base.as_deref(), Some(base.as_str()));
    assert_eq!(open.pending, None);
    assert!(
        fx.try_git_in(
            &fx.path(),
            &["rev-parse", "--verify", "-q", "refs/fufu/open/@detached"]
        )
        .status
        .code()
            != Some(0),
        "nothing is written for a detached tree"
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
