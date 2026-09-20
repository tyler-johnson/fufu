//! Contract for `ff merge <branch>`: the current branch takes another tree
//! in by one fufu-authored two-parent commit, the base is refused, a branch
//! with nothing of its own fast-forwards, and a conflicting auto-merge
//! records a `merge` hold that `ff resolve` opens and `ff done` lands. A
//! later restack carries the merge and collapses it once the target lands
//! in the base.

use ff_core::gix;
use ff_core::held::{self, Intent};
use ff_core::{DoneOutcome, MergeOutcome, OnConflict, Provenance, ResolveOutcome};
use ff_testsupport::Fixture;

const NOW: i64 = 1_799_999_999;

/// The merge reads the committer identity from the repo config, which the
/// fixture's hermetic env does not set.
fn ident(fx: &Fixture) {
    fx.set_config("user.name", "Fixture Committer");
    fx.set_config("user.email", "committer@fixture.test");
}

fn prov() -> Provenance {
    Provenance::new("pre", Some("ff merge".into()))
}

fn oid(hex: &str) -> gix::ObjectId {
    gix::ObjectId::from_hex(hex.trim().as_bytes()).unwrap()
}

fn merge_call(
    fx: &Fixture,
    target: &str,
    message: Option<&str>,
    now: i64,
) -> ff_core::Result<MergeOutcome> {
    merge_on(fx, target, message, OnConflict::Hold, now).map(|(outcome, _opened)| outcome)
}

/// `merge_call` with the conflict answer settled, and the session it opened.
fn merge_on(
    fx: &Fixture,
    target: &str,
    message: Option<&str>,
    on_conflict: OnConflict,
    now: i64,
) -> ff_core::Result<(MergeOutcome, Option<ff_core::ResolveReport>)> {
    let repo = fx.repo();
    ff_core::merge::merge(
        &repo,
        target,
        message,
        on_conflict,
        &prov(),
        Some(now),
        vec!["ff".into(), "merge".into(), target.into()],
    )
    .map(|(outcome, opened, _ctx)| (outcome, opened))
}

fn merged(fx: &Fixture, target: &str, message: Option<&str>, now: i64) -> ff_core::MergeReport {
    match merge_call(fx, target, message, now).unwrap() {
        MergeOutcome::Merged(r) => r,
        other => panic!("the merge must land, got {other:?}"),
    }
}

fn held_by(fx: &Fixture, target: &str, message: Option<&str>, now: i64) -> ff_core::HeldReport {
    match merge_call(fx, target, message, now).unwrap() {
        MergeOutcome::Held(r) => r,
        other => panic!("a conflicting merge must hold, got {other:?}"),
    }
}

fn resolve_call(fx: &Fixture, now: i64) -> ff_core::Result<ResolveOutcome> {
    let repo = fx.repo();
    ff_core::resolve::resolve(
        &repo,
        false,
        &prov(),
        Some(now),
        vec!["ff".into(), "resolve".into()],
    )
    .map(|(outcome, _ctx)| outcome)
}

fn done_call(fx: &Fixture, abandon: bool, now: i64) -> ff_core::Result<DoneOutcome> {
    let repo = fx.repo();
    ff_core::done::done(
        &repo,
        abandon,
        ff_core::Verify::Run,
        &prov(),
        Some(now),
        vec!["ff".into(), "done".into()],
    )
    .map(|(outcome, _ctx)| outcome)
}

fn restack_call(
    fx: &Fixture,
    onto: Option<&str>,
    now: i64,
) -> ff_core::Result<ff_core::RestackOutcome> {
    let repo = fx.repo();
    ff_core::restack::restack(
        &repo,
        None,
        onto.map(String::from),
        &prov(),
        Some(now),
        vec!["ff".into(), "restack".into()],
    )
    .map(|(outcome, _ctx)| outcome)
}

/// What the reader does: write the fixed content where the markers stood.
fn fix(fx: &Fixture, rel: &str, contents: &str) {
    std::fs::write(fx.path().join(rel), contents).unwrap();
}

/// The newest operation's record, read through the public reader.
fn tip_record(repo: &gix::Repository) -> ff_core::ops::OpRecord {
    let log = ff_core::ops::OpLog::open(repo).unwrap();
    let op = log.get(log.tip().unwrap().unwrap()).unwrap();
    op.record()
        .unwrap()
        .cloned()
        .expect("a verb op has a record")
}

/// The verb operations in the log, captures and notes excluded.
fn verb_ops(fx: &Fixture) -> usize {
    let repo = fx.repo();
    let log = ff_core::ops::OpLog::open(&repo).unwrap();
    log.iter()
        .flatten()
        .filter(|op| op.kind() == ff_core::ops::OpKind::Op)
        .count()
}

fn tip(fx: &Fixture, branch: &str) -> String {
    fx.git(&["rev-parse", branch]).trim().to_string()
}

fn head_branch(fx: &Fixture) -> String {
    fx.git(&["symbolic-ref", "--short", "HEAD"])
        .trim()
        .to_string()
}

fn parents(fx: &Fixture, rev: &str) -> Vec<String> {
    fx.git(&["rev-list", "--parents", "-1", rev])
        .split_whitespace()
        .skip(1)
        .map(String::from)
        .collect()
}

fn subject(fx: &Fixture, rev: &str) -> String {
    fx.git(&["log", "-1", "--format=%s", rev])
        .trim()
        .to_string()
}

fn undo(fx: &Fixture, now: i64) {
    let repo = fx.repo();
    ff_core::undo(
        &repo,
        &ff_core::RewindOptions {
            force: false,
            now: Some(now),
            argv: vec!["ff".into(), "undo".into()],
        },
        &prov(),
    )
    .unwrap();
}

/// The content of one path in a commit's tree.
fn file_in(repo: &gix::Repository, commit: gix::ObjectId, path: &str) -> Option<String> {
    let tree = repo
        .find_object(commit)
        .unwrap()
        .into_commit()
        .tree()
        .unwrap();
    let entry = tree.lookup_entry_by_path(path).unwrap()?;
    let blob = repo.find_object(entry.id().detach()).unwrap();
    Some(String::from_utf8_lossy(&blob.data).into_owned())
}

/// The base the branch sits on, as `ff status` names it.
fn base_name(fx: &Fixture, branch: &str) -> String {
    let repo = fx.repo();
    ff_core::futures::base_for(&repo, branch)
        .unwrap()
        .expect("a base")
        .name
}

/// Two features linear off main: `feature-b` forked from an older main
/// with `b1`, main moved by `m1`, `feature-a` off that with `a1`. Standing
/// on `feature-a`. Returns (a1, b1).
fn two_features(fx: &Fixture) -> (String, String) {
    fx.write("base.txt", "base\n");
    fx.commit("base");
    fx.git(&["switch", "-q", "-c", "feature-b"]);
    fx.write("b.txt", "b\n");
    let b1 = fx.commit("b1");
    fx.git(&["switch", "-q", "main"]);
    fx.write("m1.txt", "m1\n");
    fx.commit("m1");
    fx.git(&["switch", "-q", "-c", "feature-a"]);
    fx.write("a.txt", "a\n");
    let a1 = fx.commit("a1");
    (a1, b1)
}

/// The same shape, with `a1` and `b1` editing the same line of `c.txt`, so
/// the auto-merge conflicts there. Returns (a1, b1).
fn conflicting_features(fx: &Fixture) -> (String, String) {
    fx.write("base.txt", "base\n");
    fx.write("c.txt", "one\n");
    fx.commit("base");
    fx.git(&["switch", "-q", "-c", "feature-b"]);
    fx.write("b.txt", "b\n");
    fx.write("c.txt", "three\n");
    let b1 = fx.commit("b1");
    fx.git(&["switch", "-q", "main"]);
    fx.write("m1.txt", "m1\n");
    fx.commit("m1");
    fx.git(&["switch", "-q", "-c", "feature-a"]);
    fx.write("a.txt", "a\n");
    fx.write("c.txt", "two\n");
    let a1 = fx.commit("a1");
    (a1, b1)
}

#[test]
fn a_merge_lands_one_commit_with_two_parents() {
    let fx = Fixture::new();
    ident(&fx);
    let (a1, b1) = two_features(&fx);
    let ops_before = verb_ops(&fx);

    let report = merged(&fx, "feature-b", None, NOW);
    assert_eq!(report.branch, "feature-a");
    assert_eq!(report.target, "feature-b");
    assert!(!report.fast_forward);
    assert_eq!(report.from, a1);
    assert_eq!(report.parents, vec![a1.clone(), b1.clone()]);
    assert_eq!(report.arrival, ff_core::ArrivalReport::None);

    let repo = fx.repo();
    let merge = oid(&report.commit);
    assert_eq!(tip(&fx, "feature-a"), report.commit);
    assert_eq!(parents(&fx, "feature-a"), vec![a1.clone(), b1.clone()]);
    assert_eq!(file_in(&repo, merge, "a.txt").as_deref(), Some("a\n"));
    assert_eq!(file_in(&repo, merge, "b.txt").as_deref(), Some("b\n"));
    assert_eq!(file_in(&repo, merge, "m1.txt").as_deref(), Some("m1\n"));
    let commit = repo.find_commit(merge).unwrap();
    assert_eq!(
        commit.message_raw_sloppy().to_string(),
        "merge feature-b into feature-a\n"
    );
    assert!(
        ff_core::changeid::header_of(&commit.data).is_some(),
        "the merge carries a change id"
    );
    assert!(
        fx.path().join("b.txt").is_file(),
        "the working copy is at the merge"
    );

    // The base is untouched: no parent recorded, and trunk still the base.
    assert_eq!(
        ff_core::branchmeta::read(&repo, "feature-a")
            .unwrap()
            .parent,
        None
    );
    assert_eq!(base_name(&fx, "feature-a"), "main");
    assert_eq!(tip(&fx, "feature-b"), b1, "the target does not move");

    assert_eq!(verb_ops(&fx), ops_before + 1, "one operation");
    let record = tip_record(&repo);
    assert_eq!(record.verb, "merge");
    assert_eq!(record.summary, "merge feature-b into feature-a");

    undo(&fx, NOW + 1);
    assert_eq!(tip(&fx, "feature-a"), a1, "undo puts feature-a back");
    assert_eq!(tip(&fx, "feature-b"), b1, "undo leaves feature-b alone");
    assert!(
        !fx.path().join("b.txt").exists(),
        "undo moves the tree back"
    );
}

#[test]
fn dash_m_sets_the_message() {
    let fx = Fixture::new();
    ident(&fx);
    two_features(&fx);
    let report = merged(&fx, "feature-b", Some("take b"), NOW);
    assert_eq!(subject(&fx, &report.commit), "take b");
}

#[test]
fn a_remote_tracking_target_merges() {
    let fx = Fixture::new_cloned();
    let (a1, b1) = two_features(&fx);
    fx.git(&["push", "-q", "origin", "main", "feature-b"]);
    fx.git(&["branch", "-q", "-D", "feature-b"]);
    let tracking = tip(&fx, "origin/feature-b");
    assert_eq!(tracking, b1);

    let report = merged(&fx, "origin/feature-b", None, NOW);
    assert_eq!(report.target, "origin/feature-b");
    assert_eq!(report.parents, vec![a1, tracking]);
    assert_eq!(
        subject(&fx, &report.commit),
        "merge origin/feature-b into feature-a"
    );
}

#[test]
fn the_open_change_rides_the_merge() {
    let fx = Fixture::new();
    ident(&fx);
    two_features(&fx);
    fx.write("x.txt", "edited\n");

    let report = merged(&fx, "feature-b", None, NOW);
    assert!(
        matches!(report.arrival, ff_core::ArrivalReport::Restored { .. }),
        "the open change comes back over the merge: {:?}",
        report.arrival
    );
    assert_eq!(
        std::fs::read_to_string(fx.path().join("x.txt")).unwrap(),
        "edited\n"
    );
    assert!(fx.path().join("b.txt").is_file());
    let repo = fx.repo();
    assert!(
        file_in(&repo, oid(&report.commit), "x.txt").is_none(),
        "the edit is not in the merge commit"
    );
}

#[test]
fn a_conflict_holds_and_names_the_merge() {
    let fx = Fixture::new();
    ident(&fx);
    let (a1, b1) = conflicting_features(&fx);
    let ops_before = verb_ops(&fx);

    let report = held_by(&fx, "feature-b", None, NOW);
    assert_eq!(report.verb, "merge");
    assert_eq!(report.branch, "feature-a");
    assert_eq!(
        report.at,
        ff_core::futures::At::Commit {
            id: b1.clone(),
            subject: "feature-b".into()
        }
    );
    assert_eq!(report.paths, vec!["c.txt".to_string()]);
    assert_eq!(report.of, 1);

    let repo = fx.repo();
    let hold = held::of(&repo, "feature-a")
        .unwrap()
        .expect("the hold stands");
    assert_eq!(
        hold.intent,
        Intent::Merge {
            branch: "feature-a".into(),
            onto: "refs/heads/feature-b".into(),
            message: None,
        }
    );
    assert_eq!(hold.paths, vec!["c.txt".to_string()]);
    assert_eq!(tip(&fx, "feature-a"), a1, "the tip is unmoved");
    assert_eq!(
        std::fs::read_to_string(fx.path().join("c.txt")).unwrap(),
        "two\n",
        "nothing was written to the working copy"
    );

    assert_eq!(verb_ops(&fx), ops_before + 1, "one op records the hold");
    let record = tip_record(&repo);
    assert_eq!(record.verb, "hold");
    assert_eq!(record.summary, "hold merge of feature-a with feature-b");
}

/// `--resolve`: the hold rides the mint of the session branch and HEAD
/// moves onto the session, the merge door's two operations. One undo is
/// back on the branch with the session open; a second removes the session
/// and the hold together.
#[test]
fn merge_resolve_opens_the_session_over_the_hold() {
    let fx = Fixture::new();
    ident(&fx);
    let (a1, b1) = conflicting_features(&fx);
    let ops_before = verb_ops(&fx);

    let (outcome, opened) = merge_on(&fx, "feature-b", None, OnConflict::Resolve, NOW).unwrap();
    let report = match outcome {
        MergeOutcome::Held(r) => r,
        other => panic!("the auto-merge conflicts and holds, got {other:?}"),
    };
    assert_eq!(report.verb, "merge");
    assert_eq!(report.branch, "feature-a");
    let opened = opened.expect("--resolve opens the session");
    assert_eq!(opened.verb, "merge");
    assert_eq!(opened.merging.as_deref(), Some("feature-b"));
    assert_eq!(opened.files, vec!["c.txt".to_string()]);
    assert_eq!(opened.regions, 1);
    assert_eq!(opened.held.as_ref(), Some(&report));
    assert_eq!(
        report.at,
        ff_core::futures::At::Commit {
            id: b1.clone(),
            subject: "feature-b".into()
        }
    );

    let repo = fx.repo();
    let hold = held::of(&repo, "feature-a")
        .unwrap()
        .expect("the hold stands");
    assert_eq!(
        hold.intent,
        Intent::Merge {
            branch: "feature-a".into(),
            onto: "refs/heads/feature-b".into(),
            message: None,
        }
    );
    assert_eq!(
        held::resolving(&repo, "feature-a")
            .unwrap()
            .map(|r| r.session),
        Some(opened.session.clone())
    );
    assert_eq!(head_branch(&fx), opened.session);
    assert_eq!(tip(&fx, "feature-a"), a1, "the tip is unmoved");
    let text = std::fs::read_to_string(fx.path().join("c.txt")).unwrap();
    assert!(
        text.contains(">>>>>>> merging \"feature-b\" (1/1)"),
        "got: {text}"
    );

    // The mint carries the hold; no `hold` operation of its own.
    assert_eq!(verb_ops(&fx), ops_before + 2, "mint and switch");
    let log = ff_core::ops::OpLog::open(&repo).unwrap();
    let records: Vec<ff_core::ops::OpRecord> = log
        .iter()
        .flatten()
        .filter(|op| op.kind() == ff_core::ops::OpKind::Op)
        .filter_map(|op| op.record().ok().flatten().cloned())
        .collect();
    assert!(!records.iter().any(|r| r.verb == "hold"));
    let mint = records
        .iter()
        .find(|r| r.verb == "resolve")
        .expect("the mint op");
    let t = mint.held.as_ref().expect("the mint records the hold");
    assert_eq!(t.old, None);
    assert_eq!(t.new.as_ref(), Some(&hold));

    undo(&fx, NOW + 1);
    assert_eq!(head_branch(&fx), "feature-a");
    assert!(held::of(&repo, "feature-a").unwrap().is_some());
    assert!(held::resolving(&repo, "feature-a").unwrap().is_some());

    undo(&fx, NOW + 2);
    assert_eq!(head_branch(&fx), "feature-a");
    assert!(held::of(&repo, "feature-a").unwrap().is_none());
    assert!(held::resolving(&repo, "feature-a").unwrap().is_none());
    assert_eq!(
        std::fs::read_to_string(fx.path().join("c.txt")).unwrap(),
        "two\n"
    );
}

#[test]
fn resolve_opens_and_done_lands_the_held_merge() {
    let fx = Fixture::new();
    ident(&fx);
    let (a1, b1) = conflicting_features(&fx);
    held_by(&fx, "feature-b", Some("take b"), NOW);

    let opened = match resolve_call(&fx, NOW + 1).unwrap() {
        ResolveOutcome::Opened(r) => r,
        other => panic!("resolve must open the held merge, got {other:?}"),
    };
    assert_eq!(opened.verb, "merge");
    assert_eq!(opened.merging.as_deref(), Some("feature-b"));
    assert!(opened.held.is_none(), "the hold stood before");
    assert_eq!(head_branch(&fx), opened.session);
    let text = std::fs::read_to_string(fx.path().join("c.txt")).unwrap();
    assert!(
        text.contains(">>>>>>> merging \"feature-b\" (1/1)"),
        "got: {text}"
    );

    fix(&fx, "c.txt", "two and three\n");
    let landed = match done_call(&fx, false, NOW + 2).unwrap() {
        DoneOutcome::Resolved(r) => r,
        other => panic!("done must land the merge, got {other:?}"),
    };
    assert_eq!(landed.verb, "merge");
    assert_eq!(landed.replayed, 0);
    assert_eq!(landed.branch, "feature-a");

    let repo = fx.repo();
    assert_eq!(tip(&fx, "feature-a"), landed.new_tip);
    assert_eq!(parents(&fx, "feature-a"), vec![a1.clone(), b1.clone()]);
    assert_eq!(
        file_in(&repo, oid(&landed.new_tip), "c.txt").as_deref(),
        Some("two and three\n")
    );
    assert_eq!(subject(&fx, &landed.new_tip), "take b");
    assert!(held::of(&repo, "feature-a").unwrap().is_none());
    assert_eq!(head_branch(&fx), "feature-a");

    undo(&fx, NOW + 3);
    assert_eq!(tip(&fx, "feature-a"), a1);
    assert!(
        held::of(&repo, "feature-a").unwrap().is_some(),
        "the hold is back"
    );
    assert_eq!(
        held::resolving(&repo, "feature-a")
            .unwrap()
            .map(|r| r.session),
        Some(opened.session.clone())
    );
    assert_eq!(head_branch(&fx), opened.session);
}

#[test]
fn done_abandon_leaves_the_branch_untouched_and_the_hold_standing() {
    let fx = Fixture::new();
    ident(&fx);
    let (a1, _b1) = conflicting_features(&fx);
    held_by(&fx, "feature-b", None, NOW);
    match resolve_call(&fx, NOW + 1).unwrap() {
        ResolveOutcome::Opened(_) => {}
        other => panic!("resolve must open the held merge, got {other:?}"),
    }

    let closed = match done_call(&fx, true, NOW + 2).unwrap() {
        DoneOutcome::Abandoned(r) => r,
        other => panic!("abandon must close the session, got {other:?}"),
    };
    assert_eq!(closed.onto, "feature-a");
    assert_eq!(
        closed.held,
        Some(ff_core::KeptHold {
            verb: "merge".into(),
            onto: Some("feature-b".into()),
        })
    );
    assert_eq!(tip(&fx, "feature-a"), a1);
    let repo = fx.repo();
    assert!(
        held::of(&repo, "feature-a").unwrap().is_some(),
        "the merge hold stands"
    );
    assert!(held::resolving(&repo, "feature-a").unwrap().is_none());
    assert_eq!(head_branch(&fx), "feature-a");
}

#[test]
fn the_base_is_refused() {
    let fx = Fixture::new();
    ident(&fx);
    let (a1, _b1) = two_features(&fx);
    let ops_before = verb_ops(&fx);

    // Trunk is the base with no parent recorded.
    let err = merge_call(&fx, "main", None, NOW).unwrap_err();
    assert_eq!(err.id(), "merge/base");
    let text = err.to_string();
    assert!(text.contains("main is feature-a's base"), "got: {text}");
    assert!(
        text.contains("ff pull") && text.contains("ff restack"),
        "got: {text}"
    );
    assert!(
        err.exits().iter().any(|e| e == "ff pull"),
        "{:?}",
        err.exits()
    );
    assert!(
        err.exits().iter().any(|e| e == "ff restack"),
        "{:?}",
        err.exits()
    );
    assert_eq!(tip(&fx, "feature-a"), a1);
    assert_eq!(verb_ops(&fx), ops_before, "a refusal appends no operation");

    // Re-aimed at feature-b, the recorded parent is the base and main is
    // not.
    match restack_call(&fx, Some("feature-b"), NOW + 1).unwrap() {
        ff_core::RestackOutcome::Restacked(_) => {}
        other => panic!("the re-aim lands, got {other:?}"),
    }
    assert_eq!(base_name(&fx, "feature-a"), "feature-b");
    let ops_before = verb_ops(&fx);
    let err = merge_call(&fx, "feature-b", None, NOW + 2).unwrap_err();
    assert_eq!(err.id(), "merge/base");
    assert_eq!(verb_ops(&fx), ops_before);
    let report = merged(&fx, "main", None, NOW + 3);
    assert_eq!(report.target, "main");
    assert!(!report.fast_forward);
    assert_eq!(base_name(&fx, "feature-a"), "feature-b", "the base stands");
}

#[test]
fn an_ancestor_and_a_missing_target_refuse() {
    let fx = Fixture::new();
    ident(&fx);
    two_features(&fx);
    let report = merged(&fx, "feature-b", None, NOW);
    let ops_before = verb_ops(&fx);

    let err = merge_call(&fx, "feature-b", None, NOW + 1).unwrap_err();
    assert_eq!(err.id(), "merge/nothing");
    assert!(
        err.to_string()
            .contains("feature-b is already in feature-a"),
        "got: {err}"
    );
    let err = merge_call(&fx, "feature-a", None, NOW + 2).unwrap_err();
    assert_eq!(err.id(), "merge/nothing");
    assert!(
        err.to_string()
            .contains("feature-a is the branch you are on"),
        "got: {err}"
    );
    let err = merge_call(&fx, "nope", None, NOW + 3).unwrap_err();
    assert_eq!(err.id(), "branch/not-found");
    assert_eq!(tip(&fx, "feature-a"), report.commit);
    assert_eq!(verb_ops(&fx), ops_before, "no refusal appends an operation");
}

#[test]
fn a_branch_with_nothing_of_its_own_fast_forwards() {
    let fx = Fixture::new();
    ident(&fx);
    fx.write("base.txt", "base\n");
    let base = fx.commit("base");
    fx.git(&["switch", "-q", "-c", "feature-b"]);
    fx.write("b.txt", "b\n");
    let b1 = fx.commit("b1");
    fx.git(&["switch", "-q", "main"]);
    fx.write("m1.txt", "m1\n");
    fx.commit("m1");
    fx.git(&["switch", "-q", "-c", "feature-a", &base]);
    assert_eq!(base_name(&fx, "feature-a"), "main");
    let ops_before = verb_ops(&fx);

    let report = merged(&fx, "feature-b", None, NOW);
    assert!(report.fast_forward);
    assert_eq!(report.from, base);
    assert_eq!(report.commit, b1);
    assert!(report.parents.is_empty());
    assert_eq!(tip(&fx, "feature-a"), b1);
    assert_eq!(parents(&fx, "feature-a").len(), 1, "no merge commit");
    assert!(fx.path().join("b.txt").is_file());

    assert_eq!(verb_ops(&fx), ops_before + 1);
    let repo = fx.repo();
    let record = tip_record(&repo);
    assert_eq!(record.verb, "merge");
    assert_eq!(record.summary, "fast-forward feature-a to feature-b");

    undo(&fx, NOW + 1);
    assert_eq!(tip(&fx, "feature-a"), base);
    assert!(!fx.path().join("b.txt").exists());
}

#[test]
fn a_held_restack_stands_across_a_clean_merge() {
    let fx = Fixture::new();
    ident(&fx);
    fx.write("a.txt", "one\n");
    fx.commit("base");
    fx.git(&["switch", "-q", "-c", "feature-b"]);
    fx.write("b.txt", "b\n");
    fx.commit("b1");
    fx.git(&["switch", "-q", "-c", "feature-c", "main"]);
    fx.write("b.txt", "other\n");
    fx.commit("c1");
    fx.git(&["switch", "-q", "-c", "feature-a", "main"]);
    fx.write("a.txt", "two\n");
    let a1 = fx.commit("a1");
    fx.git(&["switch", "-q", "main"]);
    fx.write("a.txt", "three\n");
    fx.commit("m1");
    fx.git(&["switch", "-q", "feature-a"]);

    // The precondition: a held restack on feature-a.
    match restack_call(&fx, None, NOW).unwrap() {
        ff_core::RestackOutcome::Held(_) => {}
        other => panic!("the restack holds, got {other:?}"),
    }
    let repo = fx.repo();
    let hold = held::of(&repo, "feature-a").unwrap().expect("held");
    assert!(matches!(hold.intent, Intent::Restack { .. }));

    // A clean merge on a disjoint file lands and leaves the hold as it was.
    let report = merged(&fx, "feature-b", None, NOW + 1);
    assert_eq!(report.from, a1);
    assert_eq!(held::of(&repo, "feature-a").unwrap(), Some(hold.clone()));
    let record = tip_record(&repo);
    assert_eq!(record.verb, "merge");
    assert!(record.held.is_none(), "no hold transition on a clean merge");

    // A conflicting merge under the standing hold is the second hold the
    // one-hold rule refuses.
    let err = merge_call(&fx, "feature-c", None, NOW + 2).unwrap_err();
    assert_eq!(err.id(), "held/already-held");
    assert_eq!(tip(&fx, "feature-a"), report.commit);
    assert_eq!(held::of(&repo, "feature-a").unwrap(), Some(hold));
}

#[test]
fn restack_carries_the_merge_and_collapses_it_once_the_target_lands() {
    let fx = Fixture::new();
    ident(&fx);
    let (_a1, b1) = two_features(&fx);
    let merge = merged(&fx, "feature-b", None, NOW).commit;

    // Main moves: the restack carries the merge, its second parent still b1.
    fx.git(&["switch", "-q", "main"]);
    fx.write("m2.txt", "m2\n");
    fx.commit("m2");
    fx.git(&["switch", "-q", "feature-a"]);
    let report = match restack_call(&fx, None, NOW + 1).unwrap() {
        ff_core::RestackOutcome::Restacked(r) => r,
        other => panic!("the restack lands, got {other:?}"),
    };
    assert!(report.flattened.is_empty(), "{:?}", report.flattened);
    let carried = parents(&fx, "feature-a");
    assert_eq!(carried.len(), 2, "the merge is carried: {carried:?}");
    assert_eq!(carried[1], b1, "its second parent is still b1");
    assert_ne!(tip(&fx, "feature-a"), merge);
    let repo = fx.repo();
    assert_eq!(
        file_in(&repo, oid(&tip(&fx, "feature-a")), "m2.txt").as_deref(),
        Some("m2\n")
    );
    let carried_merge = tip(&fx, "feature-a");

    // feature-b lands in main, and main moves again: b1 is beneath the base
    // now, so the merge has one real side and collapses.
    fx.git(&["switch", "-q", "main"]);
    fx.git(&["merge", "-q", "--no-edit", "feature-b"]);
    fx.write("m3.txt", "m3\n");
    fx.commit("m3");
    fx.git(&["switch", "-q", "feature-a"]);
    let report = match restack_call(&fx, None, NOW + 2).unwrap() {
        ff_core::RestackOutcome::Restacked(r) => r,
        other => panic!("the restack lands, got {other:?}"),
    };
    assert_eq!(parents(&fx, "feature-a").len(), 1, "a straight line again");
    let collapsed = report.dropped.iter().any(|d| d.old == carried_merge)
        || report.flattened.iter().any(|f| f.old == carried_merge);
    assert!(
        collapsed,
        "the report names the merge: dropped {:?}, flattened {:?}",
        report.dropped, report.flattened
    );
    let tip_id = oid(&tip(&fx, "feature-a"));
    assert_eq!(file_in(&repo, tip_id, "a.txt").as_deref(), Some("a\n"));
    assert_eq!(file_in(&repo, tip_id, "b.txt").as_deref(), Some("b\n"));
    assert_eq!(file_in(&repo, tip_id, "m3.txt").as_deref(), Some("m3\n"));
}
