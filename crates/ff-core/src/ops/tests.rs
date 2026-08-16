//! The op log's contract, against real repositories.
//!
//! These live in-crate rather than in `tests/` because the write half is
//! `pub(crate)` by design — a test that could reach it from outside would
//! mean a plugin could too.

use ff_testsupport::Fixture;

use super::*;
use crate::ops::append::{Append, OpDraft, commit_op};
use crate::ops::record::{OpRecord, RefTransition, observe_refs};
use crate::snapshot::{Provenance, TakeOptions};

const NOW: i64 = 1_700_000_000;

fn snap(fx: &Fixture, repo: &gix::Repository, now: i64) -> OpId {
    match capture_with(
        repo,
        &Provenance::new("manual", None),
        &TakeOptions {
            now: Some(now),
            max_file_size: None,
        },
    )
    .expect("capture")
    {
        CaptureOutcome::Created { id, .. } => id,
        other => panic!("expected Created in {:?}, got {other:?}", fx.path()),
    }
}

/// A minimal verb op: the planned world is exactly the observed one, which
/// is what a verb that has not mutated anything yet would write.
fn verb(repo: &gix::Repository, summary: &str, now: i64) -> OpId {
    let head = crate::head::head_state(repo).expect("head");
    let branch = crate::snapshot::chain::chain_name(&head);
    let base = crate::snapshot::chain::base_commit(&head).expect("base");
    let draft = OpDraft {
        kind: OpKind::Op,
        subject: summary.to_string(),
        tree: repo.head_tree_id_or_empty().expect("head tree").detach(),
        branch,
        base,
        session: None,
        skipped: Vec::new(),
        refs: Some(observe_refs(repo).expect("observe")),
        index_tree: Some(crate::index::tree_from_index(repo).expect("index tree")),
        record: Some(OpRecord::new("commit", summary, now)),
        pins: base.into_iter().collect(),
    };
    match commit_op(repo, &draft, now).expect("commit_op") {
        Append::Committed(id) => id,
        Append::Contended => panic!("unexpected contention in a single-threaded test"),
    }
}

fn parents(repo: &gix::Repository, id: OpId) -> Vec<gix::ObjectId> {
    repo.find_commit(id.object_id())
        .expect("op commit")
        .parent_ids()
        .map(|p| p.detach())
        .collect()
}

fn commit_objects(fx: &Fixture) -> usize {
    fx.git(&[
        "cat-file",
        "--batch-all-objects",
        "--batch-check=%(objecttype)",
    ])
    .lines()
    .filter(|line| *line == "commit")
    .count()
}

fn oid(hex: &str) -> gix::ObjectId {
    gix::ObjectId::from_hex(hex.as_bytes()).expect("hex id")
}

// --- shape -----------------------------------------------------------------

#[test]
fn a_capture_is_one_commit_with_no_record() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    let base = fx.commit("init");
    fx.write("a.txt", "one\n");

    // The first capture in a repository also lays the log's floor, so the
    // measurement starts after it: what is under test is the marginal cost of
    // a capture, which is the number the whole storage argument turns on.
    let repo = fx.repo();
    let first = snap(&fx, &repo, NOW);
    fx.write("a.txt", "two\n");
    let before = commit_objects(&fx);
    let id = snap(&fx, &repo, NOW + 1);
    let after = commit_objects(&fx);
    assert_eq!(
        after,
        before + 1,
        "a capture must not double the store: one commit, no record"
    );

    let log = OpLog::open(&repo).unwrap();
    let op = log.get(id).unwrap();
    assert_eq!(op.kind(), OpKind::Capture);
    assert!(op.record().unwrap().is_none(), "a capture has no record");
    assert!(op.index_tree().unwrap().is_none());
    assert_eq!(
        parents(&repo, id),
        vec![first.object_id(), oid(&base)],
        "prev at slot 1, base at slot 2"
    );
}

#[test]
fn parent_slots_are_prev_base_record_then_pins() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    let base = fx.commit("init");
    fx.write("a.txt", "one\n");

    let repo = fx.repo();
    let first = snap(&fx, &repo, NOW);
    fx.write("a.txt", "two\n");
    let second = snap(&fx, &repo, NOW + 1);
    assert_eq!(
        parents(&repo, second),
        vec![first.object_id(), oid(&base)],
        "a capture's parents are [prev, base] — the snapshot shape, unchanged"
    );

    let op = verb(&repo, "commit: land it", NOW + 2);
    let slots = parents(&repo, op);
    assert_eq!(slots[0], second.object_id(), "slot 1 is always the chain");
    assert_eq!(slots[1], oid(&base), "slot 2 is always the base");
    let record = slots[2];
    assert!(
        repo.find_commit(record).unwrap().parent_ids().count() == 0,
        "the record commit is parentless"
    );
    assert_eq!(
        slots.len(),
        3,
        "the only pin here is the base, already in slot 2: {slots:?}"
    );
    assert!(
        !is_op_commit(&repo, record).unwrap(),
        "the record must never pass the op guard — restoring from one \
         would wipe the worktree and write three metadata files"
    );
}

#[test]
fn the_two_refs_move_together_and_neither_moves_alone() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    fx.write("a.txt", "one\n");

    let repo = fx.repo();
    let id = snap(&fx, &repo, NOW);
    let log = OpLog::open(&repo).unwrap();
    assert_eq!(log.tip().unwrap(), Some(id));
    assert_eq!(log.branch_tip("main").unwrap(), Some(id));

    // Move the branch pointer out from under the next append. Its CAS
    // fails, and the transaction must take the log tip's edit down with it.
    let head_hex = fx.git(&["rev-parse", "HEAD"]).trim().to_string();
    let bogus = oid(&head_hex);
    crate::refs::write_ref(
        &repo,
        "refs/fufu/snap/main",
        bogus,
        gix::refs::transaction::PreviousValue::Any,
        NOW,
        "test: desync the pointer",
    )
    .unwrap();

    fx.write("a.txt", "two\n");
    let outcome = capture_with(
        &repo,
        &Provenance::new("manual", None),
        &TakeOptions {
            now: Some(NOW + 1),
            max_file_size: None,
        },
    );
    // The pointer no longer names an op, so the dedup read fails loudly
    // rather than the capture landing half-applied.
    assert!(outcome.is_err(), "a pointer that is not an op is refused");
    assert_eq!(
        crate::refs::ref_target(&repo, OPS_REF).unwrap(),
        Some(id.object_id()),
        "the log tip must not move when its partner edit cannot"
    );
}

// --- the ref table ---------------------------------------------------------

#[test]
fn a_capture_inherits_the_refs_blob_verbatim() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");

    let repo = fx.repo();
    let op = verb(&repo, "commit: land it", NOW);
    fx.write("a.txt", "one\n");
    let cap = snap(&fx, &repo, NOW + 1);

    let log = OpLog::open(&repo).unwrap();
    let verb_op = log.get(op).unwrap();
    let capture_op = log.get(cap).unwrap();
    assert_eq!(
        capture_op.refs_blob_oid(),
        verb_op.refs_blob_oid(),
        "the same blob, the same oid, no write"
    );
    assert_eq!(
        capture_op.refs().unwrap(),
        verb_op.refs().unwrap(),
        "the table means LAST SEEN by fufu, and a capture saw nothing"
    );

    // A branch moved outside fufu: the capture that follows must not
    // quietly absorb it into the table, or the move vanishes from the log.
    fx.git(&["branch", "sidebar"]);
    fx.write("a.txt", "two\n");
    let after = snap(&fx, &repo, NOW + 2);
    let table = log.get(after).unwrap();
    assert!(
        !table
            .refs()
            .unwrap()
            .expect("a table")
            .refs
            .contains_key("refs/heads/sidebar"),
        "an observing capture would erase the foreign move forever"
    );
}

#[test]
fn the_trailers_and_op_json_agree() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    fx.write("a.txt", "one\n");

    let repo = fx.repo();
    snap(&fx, &repo, NOW);
    fx.write("a.txt", "two\n");
    snap(&fx, &repo, NOW + 1);
    let id = verb(&repo, "commit: land it", NOW + 2);

    let log = OpLog::open(&repo).unwrap();
    let op = log.get(id).unwrap();
    let record = op.record().unwrap().expect("a verb op records").clone();
    assert_eq!(record.branch.as_deref(), op.branch());
    assert_eq!(
        record.base.as_deref(),
        op.base().map(|b| b.to_string()).as_deref()
    );
    assert_eq!(
        record.prev.as_deref(),
        op.prev().map(|p| p.hex()).as_deref()
    );
    assert_eq!(
        record.prev_on_branch.as_deref(),
        op.prev_on_branch().map(|p| p.hex()).as_deref()
    );
    assert_eq!(
        record.prev_segment.as_deref(),
        match op.prev_segment() {
            Some(SegmentLink::At(id)) => Some(id.to_string()),
            _ => None,
        }
        .as_deref()
    );
}

// --- walking ---------------------------------------------------------------

#[test]
fn the_walk_ends_at_the_root_and_never_enters_user_history() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    let c0 = fx.commit("init");
    fx.write("a.txt", "one\n");

    let repo = fx.repo();
    let mut written = vec![snap(&fx, &repo, NOW)];
    written.push(verb(&repo, "commit: land it", NOW + 1));
    fx.write("a.txt", "two\n");
    written.push(snap(&fx, &repo, NOW + 2));
    // The floor the first capture laid down is part of the log too, and it is
    // the row this walk has to stop on.
    let log = OpLog::open(&repo).unwrap();
    let floor = log.iter().last().unwrap().unwrap().id();
    written.insert(0, floor);
    let walked: Vec<OpId> = log.iter().map(|op| op.unwrap().id()).collect();
    written.reverse();
    assert_eq!(walked, written, "newest first, every op, nothing else");
    assert!(
        !walked.iter().any(|id| id.hex() == c0),
        "the journal's first-parent walk ran off the root into the user's \
         own commits; a stated link cannot"
    );
    assert!(log.iter().last().unwrap().unwrap().prev().is_none());
}

#[test]
fn a_branch_walk_follows_its_own_pointer() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    fx.write("a.txt", "one\n");

    let repo = fx.repo();
    let main_one = snap(&fx, &repo, NOW);
    let floor = OpLog::open(&repo)
        .unwrap()
        .iter()
        .last()
        .unwrap()
        .unwrap()
        .id();

    fx.git(&["switch", "-q", "-c", "feat"]);
    fx.write("a.txt", "two\n");
    let repo = fx.repo();
    let feat_one = snap(&fx, &repo, NOW + 1);

    let log = OpLog::open(&repo).unwrap();
    let on_main: Vec<OpId> = log.iter_branch("main").map(|op| op.unwrap().id()).collect();
    let on_feat: Vec<OpId> = log.iter_branch("feat").map(|op| op.unwrap().id()).collect();
    assert_eq!(on_main, vec![main_one, floor]);
    assert_eq!(on_feat, vec![feat_one], "the branch link skips main's op");
    assert_eq!(
        log.get(feat_one).unwrap().prev(),
        Some(main_one),
        "the log link does not"
    );
}

#[test]
fn capture_dedups_against_the_branch_pointer_not_the_log_tip() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    fx.write("a.txt", "one\n");

    let repo = fx.repo();
    let on_main = snap(&fx, &repo, NOW);

    // Arrive on a fresh branch carrying the same worktree. The log tip is
    // main's op, whose tree is identical — dedup against it would swallow
    // this capture whole and leave the new branch with no floor at all.
    fx.git(&["switch", "-q", "-c", "feat"]);
    let repo = fx.repo();
    let outcome = capture_with(
        &repo,
        &Provenance::new("manual", None),
        &TakeOptions {
            now: Some(NOW + 1),
            max_file_size: None,
        },
    )
    .unwrap();
    let CaptureOutcome::Created { id, .. } = outcome else {
        panic!("a branch with no ops of its own must capture: {outcome:?}");
    };
    let log = OpLog::open(&repo).unwrap();
    assert_eq!(log.get(id).unwrap().prev(), Some(on_main));
    assert_eq!(log.get(id).unwrap().prev_on_branch(), None);

    // Second time round, with its own pointer in place, it dedups.
    let again = capture_with(
        &repo,
        &Provenance::new("manual", None),
        &TakeOptions {
            now: Some(NOW + 2),
            max_file_size: None,
        },
    )
    .unwrap();
    assert_eq!(
        again,
        CaptureOutcome::NoOp {
            tip: Some(id),
            warnings: Vec::new()
        }
    );
}

// --- contention ------------------------------------------------------------

#[test]
fn a_verb_retries_past_a_capture_and_refuses_past_a_verb() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    fx.write("a.txt", "one\n");

    let repo = fx.repo();
    let first = snap(&fx, &repo, NOW);
    assert!(
        append::retryable(&repo, Some(first.object_id())).unwrap(),
        "an unmoved tip means we lost a lock, not a race"
    );

    fx.write("a.txt", "two\n");
    let capture_tip = snap(&fx, &repo, NOW + 1);
    assert!(
        append::retryable(&repo, Some(first.object_id())).unwrap(),
        "a capture moved no ref, so the plan is still good"
    );

    verb(&repo, "commit: land it", NOW + 2);
    assert!(
        !append::retryable(&repo, Some(capture_tip.object_id())).unwrap(),
        "a verb op moved refs: the planned table describes a world that \
         has moved, and retrying would write it anyway"
    );
}

// --- resolution ------------------------------------------------------------

/// Twenty ops in sixteen first-nibble buckets: a shared one-character
/// prefix is guaranteed by pigeonhole, not by luck.
fn many_ops(fx: &Fixture) -> Vec<OpId> {
    fx.write("a.txt", "a\n");
    fx.commit("init");
    let repo = fx.repo();
    (0..20)
        .map(|i| {
            fx.write("a.txt", &format!("v{i}\n"));
            snap(fx, &repo, NOW + i)
        })
        .collect()
}

#[test]
fn prefixes_resolve_and_collisions_are_refused() {
    let fx = Fixture::new();
    let ids = many_ops(&fx);
    let repo = fx.repo();
    let log = OpLog::open(&repo).unwrap();

    // A full letters id resolves to itself.
    let last = *ids.last().unwrap();
    assert_eq!(log.resolve(&last.to_string()).unwrap(), last);

    // Some first hex nibble is shared by two ops; its letter is ambiguous.
    let mut buckets: std::collections::HashMap<char, Vec<OpId>> = std::collections::HashMap::new();
    for id in &ids {
        buckets
            .entry(id.hex().chars().next().unwrap())
            .or_default()
            .push(*id);
    }
    let (nibble, _) = buckets
        .iter()
        .find(|(_, v)| v.len() > 1)
        .expect("twenty ids in sixteen buckets collide");
    let letter = crate::snapid::encode(&nibble.to_string());
    let err = log.resolve(&letter).unwrap_err();
    assert_eq!(err.id(), "op/ambiguous", "{}", err);

    // Nothing matching at all.
    let err = log.resolve("zzzzzzzzzzzz").unwrap_err();
    assert_eq!(err.id(), "op/not-found");
    // Hex is refused where an operation belongs, even though it would
    // resolve: that is what keeps the two address spaces apart.
    let err = log.resolve(&last.hex()).unwrap_err();
    assert_eq!(err.id(), "op/not-found");
}

#[test]
fn at_walks_back_and_stops_at_the_floor() {
    let fx = Fixture::new();
    let ids = many_ops(&fx);
    let repo = fx.repo();
    let log = OpLog::open(&repo).unwrap();

    assert_eq!(log.resolve("@").unwrap(), ids[19]);
    assert_eq!(log.resolve("@^").unwrap(), ids[18]);
    assert_eq!(log.resolve("@~3").unwrap(), ids[16]);
    // Git's two spellings for the same walk, and a chain of them, because an
    // op's first parent is the op before it and nothing here says otherwise.
    assert_eq!(log.resolve("@^^^").unwrap(), ids[16]);
    assert_eq!(log.resolve("@~2^").unwrap(), ids[16]);
    // Suffixes ride a letters id as readily as they ride `@`.
    assert_eq!(log.resolve(&format!("{}^", ids[19])).unwrap(), ids[18]);

    let err = log.resolve("@~999").unwrap_err();
    assert_eq!(err.id(), "op/floor", "{}", err);

    // The spelling this replaces, refused by the front end in both spaces.
    let err = log.resolve("@-").unwrap_err();
    assert_eq!(err.id(), "op/not-found", "{}", err);

    // Parent 2 is the base commit — the other address space, and the one
    // crossing that has a name.
    let err = log.resolve("@^2").unwrap_err();
    assert_eq!(err.id(), "usage/rev-in-op-position", "{}", err);
    assert!(err.to_string().contains("base()"), "{err}");
}

#[test]
fn an_op_past_the_live_tip_reads_as_trimmed() {
    let fx = Fixture::new();
    let ids = many_ops(&fx);
    let repo = fx.repo();
    let log = OpLog::open(&repo).unwrap();
    let dropped = ids[19];

    // Rewind the log the way trim leaves it: the object is still there, the
    // ref no longer reaches it.
    crate::refs::write_ref(
        &repo,
        OPS_REF,
        ids[18].object_id(),
        gix::refs::transaction::PreviousValue::Any,
        NOW,
        "test: rewind the log",
    )
    .unwrap();

    assert!(log.get(dropped).is_ok(), "the object still decodes");
    let err = log.live(dropped).unwrap_err();
    assert_eq!(err.id(), "op/trimmed", "{}", err);
    assert!(log.live(ids[18]).is_ok());
}

#[test]
fn the_index_catches_up_rather_than_rebuilding() {
    let fx = Fixture::new();
    let ids = many_ops(&fx);
    let repo = fx.repo();

    // The file is written by every append, so it is in sync here — over the
    // ops the helper made plus the floor underneath them.
    assert_eq!(
        index::status(&repo).unwrap(),
        index::Status::InSync { ids: ids.len() + 1 }
    );

    // Simulate an append this process never saw: rewind the header to an
    // older tip and let the read path walk the difference back in.
    let stale = ids[10].hex();
    let path = index::path(&repo, index::Kind::Live);
    let mut bytes = std::fs::read(&path).unwrap();
    bytes[..40].copy_from_slice(stale.as_bytes());
    std::fs::write(&path, &bytes).unwrap();

    let log = OpLog::open(&repo).unwrap();
    assert_eq!(log.resolve(&ids[19].to_string()).unwrap(), ids[19]);
    assert_eq!(
        std::fs::read(&path).unwrap(),
        bytes,
        "the read path never writes: catching up stays in memory"
    );
}

/// A verb that recorded its plan and died before mutating: the planned table
/// says main moved, reality still holds the old value. The next reconcile has
/// to absorb the difference *and* say out loud that an operation may not have
/// completed — a silent absorption would look identical to the user having run
/// git themselves.
///
/// This lives in-crate because staging the crash means writing a plan that no
/// mutation follows, and that is the `pub(crate)` write half.
#[test]
fn write_ahead_crash_is_labeled_incomplete() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    let head = fx.commit("init");
    let repo = fx.repo();
    crate::ops::reconcile(&repo, NOW).unwrap();

    fx.write("a.txt", "planned\n");
    fx.git(&["add", "-A"]);
    let planned_tree = fx.git(&["write-tree"]).trim().to_string();
    let planned = fx
        .git(&["commit-tree", &planned_tree, "-p", &head, "-m", "planned"])
        .trim()
        .to_string();
    fx.git(&["reset", "-q", "--mixed", &head]); // leave reality at `head`

    let repo = fx.repo();
    let mut table = observe_refs(&repo).unwrap();
    table.refs.insert("refs/heads/main".into(), planned.clone());
    let mut record = OpRecord::new("commit", "close on main (test)", NOW + 1);
    record.refs = vec![RefTransition {
        name: "refs/heads/main".into(),
        old: Some(head.clone()),
        new: Some(planned.clone()),
    }];
    let head_state = crate::head::head_state(&repo).unwrap();
    crate::ops::verb::append_op(
        &repo,
        OpKind::Op,
        crate::ops::verb::VerbOp {
            record,
            planned: table,
            tree: repo.head_tree_id_or_empty().unwrap().detach(),
            index_tree: crate::index::tree_from_index(&repo).unwrap(),
            branch: crate::snapshot::chain::chain_name(&head_state),
            base: crate::snapshot::chain::base_commit(&head_state).unwrap(),
            session: None,
            pins: &[oid(&planned)],
        },
        NOW + 1,
    )
    .unwrap();

    let report = crate::ops::reconcile(&repo, NOW + 2).unwrap();
    assert_eq!(report.foreign.len(), 1);
    let log = OpLog::open(&repo).unwrap();
    let op = log.get(log.tip().unwrap().unwrap()).unwrap();
    assert!(
        op.summary().contains("may not have completed"),
        "a crash between append and mutation is loud: {}",
        op.summary()
    );
}
