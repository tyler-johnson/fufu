//! Contract for `ff resolve`'s merge door: on the current branch with no
//! hold, behind a base its commits already hold a merge of, resolve takes
//! the base in by one fufu-authored two-parent commit. A conflicting
//! auto-merge records a merge hold and opens the session in one operation;
//! `ff done` lands the merge with the fixes. A standing hold wins over the
//! door, a linear branch refuses naming `ff restack`, and up to date says so.

use ff_core::gix;
use ff_core::held::{self, Intent};
use ff_core::{DoneOutcome, Provenance, ResolveOutcome};
use ff_testsupport::Fixture;

const NOW: i64 = 1_799_999_999;

/// The merge and the replay read the committer identity from the repo
/// config, which the fixture's hermetic env does not set.
fn ident(fx: &Fixture) {
    fx.set_config("user.name", "Fixture Committer");
    fx.set_config("user.email", "committer@fixture.test");
}

fn prov() -> Provenance {
    Provenance::new("pre", Some("ff resolve".into()))
}

fn oid(hex: &str) -> gix::ObjectId {
    gix::ObjectId::from_hex(hex.trim().as_bytes()).unwrap()
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

fn merged(fx: &Fixture, now: i64) -> ff_core::MergeReport {
    match resolve_call(fx, now).unwrap() {
        ResolveOutcome::Merged(r) => r,
        other => panic!("the door must land the merge, got {other:?}"),
    }
}

fn opened(fx: &Fixture, now: i64) -> ff_core::ResolveReport {
    match resolve_call(fx, now).unwrap() {
        ResolveOutcome::Opened(r) => r,
        other => panic!("a conflicting merge must open a session, got {other:?}"),
    }
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

/// Every branch ref as (full ref name, sha), sorted by name.
fn head_refs(fx: &Fixture) -> Vec<(String, String)> {
    let repo = fx.repo();
    let platform = repo.references().unwrap();
    let mut out = Vec::new();
    for reference in platform.prefixed("refs/heads/").unwrap() {
        let reference = reference.unwrap();
        if let Some(id) = reference.target().try_id() {
            out.push((reference.name().as_bstr().to_string(), id.to_string()));
        }
    }
    out.sort();
    out
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

/// `ff switch <branch>`, asserting it worked.
fn switch_to(fx: &Fixture, branch: &str, now: i64) -> ff_core::SwitchReport {
    let repo = fx.repo();
    ff_core::switch(
        &repo,
        &ff_core::SwitchOptions {
            target: Some(branch.to_string()),
            now: Some(now),
            argv: vec!["ff".into(), "switch".into(), branch.to_string()],
            ..Default::default()
        },
        &prov(),
    )
    .map(|(report, _ctx)| report)
    .unwrap()
}

/// Standing on `side`, whose commits hold a merge of `main`, with `main`
/// moved again since on a file of its own. Returns (side's tip, main's tip).
fn merge_holding_side(fx: &Fixture) -> (String, String) {
    fx.write("base.txt", "base\n");
    fx.commit("base");
    fx.git(&["branch", "side"]);
    fx.write("m1.txt", "m1\n");
    fx.commit("m1");
    fx.git(&["switch", "-q", "side"]);
    fx.write("s1.txt", "s1\n");
    fx.commit("s1");
    fx.git(&["merge", "-q", "--no-edit", "main"]);
    fx.git(&["switch", "-q", "main"]);
    fx.write("m2.txt", "m2\n");
    fx.commit("m2");
    fx.git(&["switch", "-q", "side"]);
    (tip(fx, "side"), tip(fx, "main"))
}

/// The same shape, with `side`'s s2 and `main`'s m2 editing the same line
/// of `c.txt`, so the auto-merge conflicts there. Returns (s2, m2).
fn conflicting_side(fx: &Fixture) -> (String, String) {
    fx.write("base.txt", "base\n");
    fx.write("c.txt", "one\n");
    fx.commit("base");
    fx.git(&["branch", "side"]);
    fx.write("m1.txt", "m1\n");
    fx.commit("m1");
    fx.git(&["switch", "-q", "side"]);
    fx.write("s1.txt", "s1\n");
    fx.commit("s1");
    fx.git(&["merge", "-q", "--no-edit", "main"]);
    fx.write("c.txt", "two\n");
    let s2 = fx.commit("s2");
    fx.git(&["switch", "-q", "main"]);
    fx.write("c.txt", "three\n");
    let m2 = fx.commit("m2");
    fx.git(&["switch", "-q", "side"]);
    (s2, m2)
}

#[test]
fn the_door_lands_a_clean_merge() {
    let fx = Fixture::new();
    ident(&fx);
    let (side_before, main_tip) = merge_holding_side(&fx);
    let refs_before = head_refs(&fx);
    let ops_before = verb_ops(&fx);

    let report = merged(&fx, NOW);
    assert_eq!(report.branch, "side");
    assert_eq!(report.target, "main");
    assert_eq!(report.parents, vec![side_before.clone(), main_tip.clone()]);
    assert_eq!(report.arrival, ff_core::ArrivalReport::None);

    let repo = fx.repo();
    let merge = oid(&report.commit);
    assert_eq!(tip(&fx, "side"), report.commit, "side is on the merge");
    assert_eq!(parents(&fx, "side"), vec![side_before.clone(), main_tip]);
    assert_eq!(file_in(&repo, merge, "s1.txt").as_deref(), Some("s1\n"));
    assert_eq!(file_in(&repo, merge, "m2.txt").as_deref(), Some("m2\n"));
    let commit = repo.find_commit(merge).unwrap();
    assert!(
        ff_core::changeid::header_of(&commit.data).is_some(),
        "the merge carries a change id"
    );
    assert_eq!(
        commit.message_raw_sloppy().to_string(),
        "merge main into side\n"
    );
    assert!(
        fx.path().join("m2.txt").is_file(),
        "the working copy is at the merge"
    );

    // Only `side` moved, in one operation.
    let refs_after = head_refs(&fx);
    let moved: Vec<_> = refs_after
        .iter()
        .filter(|r| !refs_before.contains(r))
        .collect();
    assert_eq!(moved.len(), 1, "only side moved: {moved:?}");
    assert_eq!(moved[0].0, "refs/heads/side");
    assert_eq!(verb_ops(&fx), ops_before + 1);
    let record = tip_record(&repo);
    assert_eq!(record.verb, "merge");
    assert_eq!(record.summary, "merge main into side");

    // The branch reads up to date now.
    let onto = ff_core::futures::base_for(&repo, "side").unwrap().unwrap();
    assert_eq!(onto.name, "main");
    let probe = ff_core::futures::probe(&repo, oid(&onto.tip), merge, None).unwrap();
    assert!(
        matches!(probe, ff_core::futures::Verdict::UpToDate { .. }),
        "up to date after the merge: {probe:?}"
    );

    undo(&fx, NOW + 1);
    assert_eq!(tip(&fx, "side"), side_before, "undo puts side back");
    assert_eq!(head_refs(&fx), refs_before, "undo leaves main alone");
    assert!(
        !fx.path().join("m2.txt").exists(),
        "undo moves the tree back"
    );
}

#[test]
fn a_conflicting_merge_holds_and_opens_in_one_op() {
    let fx = Fixture::new();
    ident(&fx);
    let (s2, m2) = conflicting_side(&fx);
    let ops_before = verb_ops(&fx);

    let report = opened(&fx, NOW);
    assert_eq!(report.verb, "merge");
    assert_eq!(report.branch, "side");
    assert_eq!(report.files, vec!["c.txt".to_string()]);
    assert_eq!(report.regions, 1);
    assert_eq!(report.merging.as_deref(), Some("main"));
    let held_report = report.held.as_ref().expect("the door records the hold");
    assert_eq!(held_report.verb, "merge");
    assert_eq!(
        held_report.at,
        ff_core::futures::At::Commit {
            id: m2.clone(),
            subject: "main".into()
        }
    );
    assert_eq!(held_report.paths, vec!["c.txt".to_string()]);
    assert_eq!(held_report.of, 1);

    let repo = fx.repo();
    let hold = held::of(&repo, "side").unwrap().expect("the hold stands");
    assert_eq!(
        hold.intent,
        Intent::Merge {
            branch: "side".into(),
            onto: "refs/heads/main".into(),
            message: None,
        }
    );
    assert_eq!(hold.paths, vec!["c.txt".to_string()]);
    let resolving = held::resolving(&repo, "side")
        .unwrap()
        .expect("the session");
    assert_eq!(resolving.session, report.session);
    assert_eq!(resolving.steps, vec!["main".to_string()]);
    assert_eq!(tip(&fx, "side"), s2, "side's tip is unmoved");

    // The mint op carries the hold and the session; the switch follows it.
    assert_eq!(verb_ops(&fx), ops_before + 2, "mint and switch");
    let log = ff_core::ops::OpLog::open(&repo).unwrap();
    let mint = log
        .iter()
        .flatten()
        .filter(|op| op.kind() == ff_core::ops::OpKind::Op)
        .filter_map(|op| op.record().ok().flatten().cloned())
        .find(|record| record.verb == "resolve")
        .expect("the mint op");
    let t = mint.held.as_ref().expect("the mint records the hold");
    assert_eq!(t.branch, "side");
    assert_eq!(t.old, None);
    assert_eq!(t.new.as_ref(), Some(&hold));
    assert!(mint.resolving.is_some());

    assert_eq!(head_branch(&fx), report.session);
    let text = std::fs::read_to_string(fx.path().join("c.txt")).unwrap();
    assert!(
        text.contains("<<<<<<< the rewrite so far (1/1)"),
        "got: {text}"
    );
    assert!(
        text.contains(">>>>>>> merging \"main\" (1/1)"),
        "got: {text}"
    );
}

#[test]
fn done_lands_the_merge_with_the_fix() {
    let fx = Fixture::new();
    ident(&fx);
    let (s2, m2) = conflicting_side(&fx);
    let report = opened(&fx, NOW);
    let session = report.session.clone();
    let ops_before = verb_ops(&fx);

    fix(&fx, "c.txt", "two and three\n");
    let landed = match done_call(&fx, false, NOW + 1).unwrap() {
        DoneOutcome::Resolved(r) => r,
        other => panic!("done must land the merge, got {other:?}"),
    };
    assert_eq!(landed.verb, "merge");
    assert_eq!(landed.replayed, 0);
    assert_eq!(landed.fixed, 1);
    assert_eq!(landed.branch, "side");
    assert!(landed.still_held.is_none());

    let repo = fx.repo();
    assert_eq!(tip(&fx, "side"), landed.new_tip);
    assert_eq!(parents(&fx, "side"), vec![s2.clone(), m2.clone()]);
    assert_eq!(
        file_in(&repo, oid(&landed.new_tip), "c.txt").as_deref(),
        Some("two and three\n")
    );
    assert_eq!(
        std::fs::read_to_string(fx.path().join("c.txt")).unwrap(),
        "two and three\n"
    );
    assert!(held::of(&repo, "side").unwrap().is_none(), "hold cleared");
    assert!(
        held::resolving(&repo, "side").unwrap().is_none(),
        "session cleared"
    );
    assert!(
        !head_refs(&fx)
            .iter()
            .any(|(name, _)| name == &format!("refs/heads/{session}")),
        "session branch gone"
    );
    assert_eq!(head_branch(&fx), "side");
    assert_eq!(verb_ops(&fx), ops_before + 1, "one op lands");
    let record = tip_record(&repo);
    assert_eq!(record.verb, "merge");
    let t = record.held.as_ref().expect("the landing clears the hold");
    assert!(t.old.is_some() && t.new.is_none());
    assert!(record.resolving.is_some());

    undo(&fx, NOW + 2);
    assert_eq!(tip(&fx, "side"), s2, "undo puts the old tip back");
    assert!(
        held::of(&repo, "side").unwrap().is_some(),
        "the hold is back"
    );
    assert_eq!(
        held::resolving(&repo, "side").unwrap().map(|r| r.session),
        Some(session.clone())
    );
    assert!(
        head_refs(&fx)
            .iter()
            .any(|(name, _)| name == &format!("refs/heads/{session}")),
        "the session branch is back"
    );
    assert_eq!(head_branch(&fx), session);
}

#[test]
fn done_abandon_leaves_the_branch_untouched() {
    let fx = Fixture::new();
    ident(&fx);
    let (s2, _m2) = conflicting_side(&fx);
    let _report = opened(&fx, NOW);

    let abandoned = match done_call(&fx, true, NOW + 1).unwrap() {
        DoneOutcome::Abandoned(r) => r,
        other => panic!("abandon must drop the session, got {other:?}"),
    };
    assert_eq!(abandoned.onto, "side");
    assert_eq!(tip(&fx, "side"), s2);
    let repo = fx.repo();
    assert!(held::of(&repo, "side").unwrap().is_none());
    assert!(held::resolving(&repo, "side").unwrap().is_none());
    assert_eq!(head_branch(&fx), "side");
}

#[test]
fn a_standing_hold_wins_over_the_door() {
    let fx = Fixture::new();
    ident(&fx);
    conflicting_side(&fx);
    // A restack of `side` conflicts on `c.txt` too, and holds.
    let repo = fx.repo();
    let (outcome, _ctx) = ff_core::restack::restack(
        &repo,
        None,
        None,
        &prov(),
        Some(NOW),
        vec!["ff".into(), "restack".into()],
    )
    .unwrap();
    assert!(
        matches!(outcome, ff_core::RestackOutcome::Held(_)),
        "the precondition is a held restack, got {outcome:?}"
    );

    let report = opened(&fx, NOW + 1);
    assert_eq!(report.verb, "restack");
    assert!(report.held.is_none(), "the hold stood before");
    assert!(report.merging.is_none());
}

#[test]
fn a_linear_branch_refuses_naming_restack() {
    let fx = Fixture::new();
    ident(&fx);
    fx.write("f.txt", "one\n");
    fx.commit("base");
    fx.git(&["switch", "-q", "-c", "feature"]);
    fx.write("f.txt", "two\n");
    fx.commit("f1");
    fx.git(&["switch", "-q", "main"]);
    fx.write("g.txt", "g\n");
    fx.commit("m1");
    fx.git(&["switch", "-q", "feature"]);
    let ops_before = verb_ops(&fx);

    let err = resolve_call(&fx, NOW).unwrap_err();
    assert_eq!(err.id(), "held/none");
    let text = err.to_string();
    assert!(
        text.starts_with("nothing is held on feature"),
        "got: {text}"
    );
    assert!(text.contains("ff restack"), "got: {text}");
    assert_eq!(verb_ops(&fx), ops_before, "nothing was written");
}

#[test]
fn an_up_to_date_merge_holding_branch_refuses() {
    let fx = Fixture::new();
    ident(&fx);
    merge_holding_side(&fx);
    merged(&fx, NOW);

    let err = resolve_call(&fx, NOW + 1).unwrap_err();
    assert_eq!(err.id(), "held/none");
    let text = err.to_string();
    assert!(text.contains("up to date"), "got: {text}");
}

#[test]
fn a_standing_merge_hold_that_is_clean_lands() {
    let fx = Fixture::new();
    ident(&fx);
    let (s2, m2) = conflicting_side(&fx);
    let report = opened(&fx, NOW);
    // Back to `side`, leaving the session parked; then move `main` off the
    // conflicting commit onto one of its own, so the merge is clean now.
    switch_to(&fx, "side", NOW + 1);
    let repo = fx.repo();
    // Abandon the session but keep the hold: drop the session by hand,
    // the way a stale record would read, so the hold is what resolve meets.
    let resolving = held::resolving(&repo, "side").unwrap().unwrap();
    assert_eq!(resolving.session, report.session);
    fx.git(&["branch", "-q", "-D", &report.session]);
    held::set_resolving(&repo, "side", None).unwrap();
    fx.git(&["branch", "-f", "main", &format!("{m2}~1")]);
    fx.git(&["switch", "-q", "main"]);
    fx.write("m3.txt", "m3\n");
    let m3 = fx.commit("m3");
    fx.git(&["switch", "-q", "side"]);
    let hold = held::of(&repo, "side").unwrap().expect("the hold stands");

    let landed = merged(&fx, NOW + 2);
    assert_eq!(landed.parents, vec![s2.clone(), m3.clone()]);
    assert_eq!(tip(&fx, "side"), landed.commit);
    assert!(held::of(&repo, "side").unwrap().is_none(), "hold cleared");
    let record = tip_record(&repo);
    assert_eq!(record.verb, "merge");
    let t = record.held.as_ref().expect("the landing records the clear");
    assert_eq!(t.old.as_ref(), Some(&hold));
    assert_eq!(t.new, None);
}

#[test]
fn a_reaim_drops_a_merge_hold() {
    let fx = Fixture::new();
    ident(&fx);
    conflicting_side(&fx);
    opened(&fx, NOW);
    // Back on `side` with the session dropped, so the rewrite is allowed.
    switch_to(&fx, "side", NOW + 1);
    let repo = fx.repo();
    let session = held::resolving(&repo, "side").unwrap().unwrap().session;
    fx.git(&["branch", "-q", "-D", &session]);
    held::set_resolving(&repo, "side", None).unwrap();
    // A base elsewhere, off the root, that `side` replays onto cleanly.
    let root = fx.git(&["rev-list", "--max-parents=0", "HEAD"]);
    fx.git(&["branch", "other", root.trim()]);
    fx.git(&["switch", "-q", "other"]);
    fx.write("o.txt", "o\n");
    fx.commit("o1");
    fx.git(&["switch", "-q", "side"]);

    let (outcome, _ctx) = ff_core::restack::restack(
        &repo,
        None,
        Some("other".into()),
        &prov(),
        Some(NOW + 2),
        vec![
            "ff".into(),
            "restack".into(),
            "--onto".into(),
            "other".into(),
        ],
    )
    .unwrap();
    let report = match outcome {
        ff_core::RestackOutcome::Restacked(r) => r,
        other => panic!("the re-aim lands, got {other:?}"),
    };
    assert_eq!(
        report.dropped_hold,
        Some(ff_core::DroppedHold {
            verb: "merge".into(),
            onto: "main".into(),
        })
    );
    assert!(held::of(&repo, "side").unwrap().is_none());
}

#[test]
fn an_open_change_rides_the_merge() {
    let fx = Fixture::new();
    ident(&fx);
    let (side_before, main_tip) = merge_holding_side(&fx);
    fx.write("x.txt", "edited\n");

    let report = merged(&fx, NOW);
    assert_eq!(report.parents, vec![side_before, main_tip]);
    assert!(
        matches!(report.arrival, ff_core::ArrivalReport::Restored { .. }),
        "the open change comes back over the merge: {:?}",
        report.arrival
    );
    assert_eq!(
        std::fs::read_to_string(fx.path().join("x.txt")).unwrap(),
        "edited\n"
    );
    assert!(fx.path().join("m2.txt").is_file());
    let repo = fx.repo();
    assert!(
        file_in(&repo, oid(&report.commit), "x.txt").is_none(),
        "the edit is not in the merge commit"
    );
}

#[test]
fn a_merge_hold_whose_base_is_already_in_is_released() {
    let fx = Fixture::new();
    ident(&fx);
    conflicting_side(&fx);
    opened(&fx, NOW);
    switch_to(&fx, "side", NOW + 1);
    let repo = fx.repo();
    let session = held::resolving(&repo, "side").unwrap().unwrap().session;
    fx.git(&["branch", "-q", "-D", &session]);
    held::set_resolving(&repo, "side", None).unwrap();
    // Take the base in by hand, under the hold.
    fx.git(&["merge", "-q", "-X", "ours", "--no-edit", "main"]);
    let after = tip(&fx, "side");
    let ops_before = verb_ops(&fx);

    let released = match resolve_call(&fx, NOW + 2).unwrap() {
        ResolveOutcome::Released(r) => r,
        other => panic!("a moot hold is released, got {other:?}"),
    };
    assert_eq!(released.verb, "merge");
    assert_eq!(released.branch, "side");
    assert!(held::of(&repo, "side").unwrap().is_none());
    assert_eq!(tip(&fx, "side"), after);
    assert_eq!(verb_ops(&fx), ops_before, "a release is no operation");
}
