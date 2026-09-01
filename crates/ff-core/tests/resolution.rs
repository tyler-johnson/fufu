//! Contract for `ff done` finishing a resolution: the reader's fixes go back
//! into the steps that owned them, the chain re-runs, and the whole stack
//! lands at once — refs moving one time, every landed commit clean, no
//! conflicted state ever standing in the graph.

use ff_core::gix;
use ff_core::{DoneOutcome, Provenance, ResolveOutcome};
use ff_testsupport::Fixture;

const NOW: i64 = 1_799_999_999;
const OPENER: &str = "<<<<<<<";

/// The replay reads the committer identity from the repo config, which the
/// fixture's hermetic env does not set; git itself gets its identity from env
/// vars, so this is only for the gix side.
fn ident(fx: &Fixture) {
    fx.set_config("user.name", "Fixture Committer");
    fx.set_config("user.email", "committer@fixture.test");
}

fn prov() -> Provenance {
    Provenance::new("pre", Some("ff restack".into()))
}

fn oid(hex: &str) -> gix::ObjectId {
    gix::ObjectId::from_hex(hex.trim().as_bytes()).unwrap()
}

fn resolve_call(fx: &Fixture, abandon: bool, now: i64) -> ff_core::Result<ResolveOutcome> {
    let repo = fx.repo();
    ff_core::resolve::resolve(
        &repo,
        abandon,
        &prov(),
        Some(now),
        vec!["ff".into(), "resolve".into()],
    )
    .map(|(outcome, _ctx)| outcome)
}

/// Open a resolution session, asserting that it opened.
fn open_resolution(fx: &Fixture, now: i64) -> ff_core::ResolveReport {
    match resolve_call(fx, false, now).unwrap() {
        ResolveOutcome::Opened(r) => r,
        other => panic!("a held rewrite must open a resolution, got {other:?}"),
    }
}

fn done_call(fx: &Fixture, now: i64) -> ff_core::Result<DoneOutcome> {
    let repo = fx.repo();
    ff_core::done::done(
        &repo,
        false,
        &prov(),
        Some(now),
        vec!["ff".into(), "done".into()],
    )
    .map(|(outcome, _ctx)| outcome)
}

fn resolved(fx: &Fixture, now: i64) -> ff_core::ResolvedReport {
    match done_call(fx, now).unwrap() {
        DoneOutcome::Resolved(r) => r,
        other => panic!("a finished resolution must land, got {other:?}"),
    }
}

/// What the reader does: write the fixed content where the markers stood.
fn fix(fx: &Fixture, rel: &str, contents: &str) {
    std::fs::write(fx.path().join(rel), contents).unwrap();
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

/// The verb operations in the log — captures and notes excluded, so the count
/// says "one more thing the user asked for happened".
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

/// A commit's tree, as (path, contents) pairs for every blob in it.
fn tree_files(repo: &gix::Repository, commit: gix::ObjectId) -> Vec<(String, String)> {
    let tree = repo
        .find_object(commit)
        .unwrap()
        .into_commit()
        .tree()
        .unwrap();
    let mut out = Vec::new();
    let mut recorder = gix::traverse::tree::Recorder::default();
    tree.traverse().breadthfirst(&mut recorder).unwrap();
    for entry in recorder.records {
        if !entry.mode.is_blob() {
            continue;
        }
        let blob = repo.find_object(entry.oid).unwrap();
        out.push((
            entry.filepath.to_string(),
            String::from_utf8_lossy(&blob.data).into_owned(),
        ));
    }
    out.sort();
    out
}

/// The content of one path in a commit's tree.
fn file_in(repo: &gix::Repository, commit: gix::ObjectId, path: &str) -> Option<String> {
    tree_files(repo, commit)
        .into_iter()
        .find(|(p, _)| p == path)
        .map(|(_, c)| c)
}

/// Every commit from `tip` back to (and excluding) `stop`, newest-first.
fn commits_between(repo: &gix::Repository, tip: gix::ObjectId, stop: gix::ObjectId) -> Vec<String> {
    let walk = repo
        .rev_walk(Some(tip))
        .with_boundary(Some(stop))
        .all()
        .unwrap();
    walk.map(|info| info.unwrap().id.to_string()).collect()
}

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

/// A three-commit stack on `feature` whose first commit cannot replay over
/// `main`: `f1` and `m1` both rewrite line 1 of `f.txt`. Leaves the fixture
/// standing on `feature`. Returns (base, f1).
fn restack_stack(fx: &Fixture) -> (String, String) {
    fx.write("f.txt", "one\n");
    let base = fx.commit("base");
    fx.git(&["switch", "-q", "-c", "feature"]);
    fx.write("f.txt", "two\n");
    let f1 = fx.commit("f1");
    fx.write("a.txt", "a1\n");
    let _f2 = fx.commit("f2");
    fx.write("b.txt", "b1\n");
    let _f3 = fx.commit("f3");
    fx.git(&["switch", "-q", "main"]);
    fx.write("f.txt", "three\n");
    let _m1 = fx.commit("m1");
    fx.git(&["switch", "-q", "feature"]);
    (base, f1)
}

fn hold_a_restack(fx: &Fixture) {
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
}

/// A stack whose open change cannot be folded into `c1`: the target, the tip
/// and the open change all rewrite `f.txt`, so the fold itself conflicts. The
/// change also touches `g.txt`, which nothing else does, so the fold has
/// something to land cleanly as well. Leaves the fixture on `main` with the
/// change open. Returns (c1, c2).
fn absorb_stack(fx: &Fixture) -> (String, String) {
    fx.write("f.txt", "one\n");
    fx.write("g.txt", "g0\n");
    let _c0 = fx.commit("base");
    fx.write("f.txt", "A\n");
    let c1 = fx.commit("c1");
    fx.write("f.txt", "B\n");
    fx.write("h.txt", "h1\n");
    let c2 = fx.commit("c2");
    fx.write("f.txt", "C\n");
    fx.write("g.txt", "gopen\n");
    (c1, c2)
}

fn hold_an_absorb(fx: &Fixture, into: &str, paths: Vec<String>) {
    let repo = fx.repo();
    let (outcome, _ctx) = ff_core::absorb::absorb(
        &repo,
        Some(oid(into)),
        paths,
        &prov(),
        Some(NOW),
        vec!["ff".into(), "absorb".into()],
    )
    .unwrap();
    assert!(
        matches!(outcome, ff_core::AbsorbOutcome::Held(_)),
        "the precondition is a held absorb, got {outcome:?}"
    );
}

/// A session on `c1` whose amend the commit ahead cannot replay over: `c1`,
/// `c2` and the session all rewrite line 2 of `shared.txt`. Returns the
/// session branch and the anchor.
fn done_stack(fx: &Fixture) -> (String, String) {
    fx.write("shared.txt", "line1\nbase\nline3\n");
    let _c0 = fx.commit("c0");
    fx.write("shared.txt", "line1\nc1-edit\nline3\n");
    let c1 = fx.commit("c1");
    fx.write("shared.txt", "line1\nc2-edit\nline3\n");
    let _c2 = fx.commit("c2");

    let repo = fx.repo();
    let (outcome, _ctx) = ff_core::edit::edit(
        &repo,
        &c1,
        &prov(),
        Some(NOW),
        vec!["ff".into(), "edit".into()],
    )
    .unwrap();
    let session = match outcome {
        ff_core::EditOutcome::Opened(r) => r.session,
        other => panic!("a session must open, got {other:?}"),
    };
    drop(repo);
    fx.write("shared.txt", "line1\nsession-edit\nline3\n");

    let held = done_call(fx, NOW + 10).unwrap();
    assert!(
        matches!(held, DoneOutcome::Held(_)),
        "the precondition is a held session landing, got {held:?}"
    );
    (session, c1)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[test]
fn a_resolved_restack_lands_every_commit_clean() {
    let fx = Fixture::new();
    ident(&fx);
    restack_stack(&fx);
    hold_a_restack(&fx);
    let was = tip(&fx, "feature");

    open_resolution(&fx, NOW + 100);
    fix(&fx, "f.txt", "RESOLVED\n");
    let report = resolved(&fx, NOW + 200);

    assert_eq!(report.verb, "restack");
    assert_eq!(report.branch, "feature");
    assert_eq!(report.fixed, 1, "one region waited and one was fixed");
    assert_eq!(report.replayed, 3, "the whole three-commit stack replayed");
    assert!(report.still_held.is_none(), "nothing is left waiting");

    let now = tip(&fx, "feature");
    assert_ne!(now, was, "the branch moved");
    assert_eq!(now, report.new_tip, "the report names the branch's new tip");

    // The promise the feature exists to make: not one landed commit carries a
    // marker, from the new tip all the way back to the base.
    let repo = fx.repo();
    let base = oid(&tip(&fx, "main"));
    let landed = commits_between(&repo, oid(&now), base);
    assert_eq!(landed.len(), 3, "three commits landed: {landed:?}");
    for id in &landed {
        for (path, contents) in tree_files(&repo, oid(id)) {
            assert!(
                !contents.contains(OPENER),
                "{id} still carries markers in {path}:\n{contents}"
            );
        }
    }
    // And the working tree is the resolved content, not the markers.
    let on_disk = std::fs::read_to_string(fx.path().join("f.txt")).unwrap();
    assert_eq!(on_disk, "RESOLVED\n");
}

#[test]
fn the_fix_lands_in_the_commit_that_owned_it() {
    let fx = Fixture::new();
    ident(&fx);
    restack_stack(&fx);
    hold_a_restack(&fx);

    open_resolution(&fx, NOW + 100);
    fix(&fx, "f.txt", "RESOLVED\n");
    let report = resolved(&fx, NOW + 200);

    // The stack landed as f1', f2', f3'. Read f1' — the commit that owned the
    // conflict — and assert the fix is in ITS tree, not only in the tip's.
    // That is what separates this from resolving at the end and squashing.
    let repo = fx.repo();
    let base = oid(&tip(&fx, "main"));
    let landed = commits_between(&repo, oid(&report.new_tip), base);
    let f1_new = oid(landed.last().expect("the oldest landed commit is f1'"));

    assert_eq!(
        file_in(&repo, f1_new, "f.txt").as_deref(),
        Some("RESOLVED\n"),
        "the conflicting commit's own tree carries the fix"
    );
    assert_eq!(
        file_in(&repo, f1_new, "a.txt"),
        None,
        "f1' has only its own content: the later commits did not slide down into it"
    );
    assert_eq!(
        file_in(&repo, oid(&report.new_tip), "b.txt").as_deref(),
        Some("b1\n"),
        "and the tip still carries what the last commit added"
    );
}

#[test]
fn landing_a_resolution_is_one_operation() {
    let fx = Fixture::new();
    ident(&fx);
    restack_stack(&fx);
    hold_a_restack(&fx);
    open_resolution(&fx, NOW + 100);
    fix(&fx, "f.txt", "RESOLVED\n");

    let before_refs = head_refs(&fx);
    let before_ops = verb_ops(&fx);
    let report = resolved(&fx, NOW + 200);

    assert_eq!(
        verb_ops(&fx),
        before_ops + 1,
        "the landing is exactly one verb operation, preamble and all"
    );

    // One `ff undo` takes the whole resolution back: the refs, the hold and
    // the session all together.
    let repo = fx.repo();
    ff_core::undo(
        &repo,
        &ff_core::RewindOptions {
            force: false,
            now: Some(NOW + 300),
            argv: vec!["ff".into(), "undo".into()],
        },
        &prov(),
    )
    .unwrap();
    drop(repo);

    assert_eq!(head_refs(&fx), before_refs, "undo puts every ref back");
    let repo = fx.repo();
    assert!(
        ff_core::held::of(&repo, "feature").unwrap().is_some(),
        "undo puts the hold back"
    );
    let session = ff_core::held::resolving(&repo, "feature")
        .unwrap()
        .expect("undo puts the resolution session back");
    assert_eq!(
        session.hold.paths,
        vec!["f.txt".to_string()],
        "the session that came back is the one that was resolving"
    );
    assert_ne!(
        report.new_tip,
        tip(&fx, "feature"),
        "the branch is off the landed tip again"
    );
}

#[test]
fn markers_left_behind_refuse() {
    let fx = Fixture::new();
    ident(&fx);
    restack_stack(&fx);
    hold_a_restack(&fx);
    open_resolution(&fx, NOW + 100);

    let before_refs = head_refs(&fx);
    let before_ops = verb_ops(&fx);
    // No fix: the markers are exactly where `ff resolve` left them.
    let err = done_call(&fx, NOW + 200).expect_err("unfixed markers refuse");

    assert_eq!(err.id(), "held/unresolved");
    assert!(
        err.to_string().contains("f.txt"),
        "the refusal names the file still carrying markers: {err}"
    );
    assert_eq!(head_refs(&fx), before_refs, "a refusal moves no ref");
    assert_eq!(
        verb_ops(&fx),
        before_ops,
        "and appends no verb op — the pre-verb capture aside"
    );
    assert!(
        ff_core::held::resolving(&fx.repo(), "feature")
            .unwrap()
            .is_some(),
        "the session stays open, so the reader can fix and try again"
    );
}

#[test]
fn a_repository_that_moved_refuses() {
    let fx = Fixture::new();
    ident(&fx);
    restack_stack(&fx);
    hold_a_restack(&fx);
    open_resolution(&fx, NOW + 100);
    fix(&fx, "f.txt", "RESOLVED\n");

    // The world moves under the session: a commit lands on the branch being
    // resolved, so the conflicts the reader was given are not the repository's
    // conflicts any more.
    fx.write("unrelated.txt", "later\n");
    let _later = fx.commit("later");

    let before_refs = head_refs(&fx);
    let before_ops = verb_ops(&fx);
    let err = done_call(&fx, NOW + 200).expect_err("a moved repository refuses");

    assert_eq!(err.id(), "held/moved");
    assert_eq!(head_refs(&fx), before_refs, "a refusal moves no ref");
    assert_eq!(verb_ops(&fx), before_ops, "and appends no verb op");
}

#[test]
fn a_resolved_done_lands_the_session() {
    let fx = Fixture::new();
    ident(&fx);
    let (session, _c1) = done_stack(&fx);

    open_resolution(&fx, NOW + 100);
    fix(&fx, "shared.txt", "line1\nRESOLVED\nline3\n");
    let report = resolved(&fx, NOW + 200);

    assert_eq!(report.verb, "done");
    assert_eq!(
        report.branch, "main",
        "a session lands on the branch it was opened from"
    );

    // The session is over: its branch is gone, HEAD is back on `main`.
    assert!(
        ff_core::branchmeta::read(&fx.repo(), "main")
            .unwrap()
            .session
            .is_none(),
        "no session is left standing"
    );
    let heads = head_refs(&fx);
    assert!(
        !heads.iter().any(|(name, _)| name.ends_with(&session)),
        "the session branch is gone: {heads:?}"
    );
    assert_eq!(fx.git(&["symbolic-ref", "--short", "HEAD"]).trim(), "main");
    assert_eq!(tip(&fx, "main"), report.new_tip);

    // The amend landed in the anchor and the fix landed in the commit that
    // owned the conflict — `c2`, which is what could not replay over the
    // amend. Resolving at the end and squashing would have put both in one
    // place; this is the difference.
    let repo = fx.repo();
    let landed = commits_between(
        &repo,
        oid(&report.new_tip),
        oid(&fx.git(&["rev-parse", "HEAD~2"])),
    );
    let anchor_new = oid(landed
        .last()
        .expect("the oldest landed commit is the anchor"));
    assert_eq!(
        file_in(&repo, anchor_new, "shared.txt").as_deref(),
        Some("line1\nsession-edit\nline3\n"),
        "the amended commit carries the session's own content"
    );
    assert_eq!(
        file_in(&repo, oid(&report.new_tip), "shared.txt").as_deref(),
        Some("line1\nRESOLVED\nline3\n"),
        "and the commit that conflicted carries the reader's fix"
    );
    for id in &landed {
        for (path, contents) in tree_files(&repo, oid(id)) {
            assert!(
                !contents.contains(OPENER),
                "{id} still carries markers in {path}"
            );
        }
    }
}

#[test]
fn a_tangled_stack_refuses_a_fix_that_still_conflicts_and_takes_one_that_does_not() {
    let fx = Fixture::new();
    ident(&fx);
    // Two commits on `feature` rewrite the same line of `f.txt`, and `main`
    // rewrites it a third way: the first replay conflicts and the second lands
    // on the same region, so the chain stops before it — a tangle. Only the
    // first conflict's markers reach the working tree.
    fx.write("f.txt", "one\nrest\n");
    let _base = fx.commit("base");
    fx.git(&["switch", "-q", "-c", "feature"]);
    fx.write("f.txt", "F1\nrest\n");
    let _f1 = fx.commit("f1");
    fx.write("f.txt", "F2\nrest\n");
    let _f2 = fx.commit("f2");
    fx.git(&["switch", "-q", "main"]);
    fx.write("f.txt", "M\nrest\n");
    let _m1 = fx.commit("m1");
    fx.git(&["switch", "-q", "feature"]);
    hold_a_restack(&fx);

    let opened = open_resolution(&fx, NOW + 100);
    assert_eq!(
        opened.tangled.as_deref(),
        Some("f2"),
        "the chain stopped before the tangled commit"
    );

    // A fix that resolves what the reader was shown, but leaves the commit
    // behind the tangle unable to replay, lands nothing and says which commit
    // is stuck. The session stays open, so the way forward is another edit.
    let before_refs = head_refs(&fx);
    fix(&fx, "f.txt", "R\nrest\n");
    let err = done_call(&fx, NOW + 200).expect_err("a fix that still conflicts refuses");
    assert_eq!(err.id(), "held/unresolved");
    assert!(
        err.to_string().contains("f2"),
        "the refusal names the commit that is still stuck: {err}"
    );
    assert_eq!(head_refs(&fx), before_refs, "a refusal moves no ref");

    // A tangle is not a dead end: a fix the tangled commit can replay over
    // lands the whole stack, from the same still-open session.
    fix(&fx, "f.txt", "F2\nrest\n");
    let report = resolved(&fx, NOW + 300);
    assert!(report.replayed >= 1, "the stack landed: {report:?}");
    assert!(report.still_held.is_none(), "nothing is left waiting");

    let repo = fx.repo();
    let landed = commits_between(&repo, oid(&report.new_tip), oid(&tip(&fx, "main")));
    for id in &landed {
        for (path, contents) in tree_files(&repo, oid(id)) {
            assert!(
                !contents.contains(OPENER),
                "{id} still carries markers in {path}"
            );
        }
    }
    assert_eq!(
        std::fs::read_to_string(fx.path().join("f.txt")).unwrap(),
        "F2\nrest\n"
    );
    assert!(
        ff_core::held::of(&fx.repo(), "feature").unwrap().is_none(),
        "the hold is cleared once the resolution lands"
    );
    assert!(
        ff_core::held::resolving(&fx.repo(), "feature")
            .unwrap()
            .is_none(),
        "and so is the session"
    );
}

/// A later commit's merge used to fold its own change silently into a
/// standing mark, so the block `ff resolve` laid down was not the block the
/// step that owned it had written — and `ff done` could land no fix at all:
/// `no marker block to resolve at f.env`. The chain stops at the fold now, so
/// the reader is shown the mark as its owning step wrote it, and the fix
/// lands in that step.
#[test]
fn a_fold_into_a_mark_does_not_strand_the_resolution() {
    let fx = Fixture::new();
    ident(&fx);
    fx.write("f.env", "");
    let _base = fx.commit("base");
    fx.git(&["switch", "-q", "-c", "feature"]);
    fx.write("f.env", "alpha\n\n");
    let _f1 = fx.commit("feature 1");
    fx.write("f.env", "alpha\nbeta\n");
    let _f2 = fx.commit("feature 2");
    fx.git(&["switch", "-q", "main"]);
    fx.write("f.env", "gamma\n\ndelta");
    let _m1 = fx.commit("main advances");
    fx.git(&["switch", "-q", "feature"]);
    hold_a_restack(&fx);

    let opened = open_resolution(&fx, NOW + 100);
    assert_eq!(
        opened.tangled.as_deref(),
        Some("feature 2"),
        "the chain stopped before the commit that would fold into the mark"
    );

    // What the reader is shown is feature 1's own side — the same two lines
    // `git rebase` stops on — not feature 2's "beta" folded into it.
    let shown = std::fs::read_to_string(fx.path().join("f.env")).unwrap();
    assert!(
        shown.contains("alpha\n\n"),
        "the mark carries the owning commit's side: {shown}"
    );
    assert!(
        !shown.contains("beta"),
        "and not the later commit's change: {shown}"
    );

    // Resolving it the way scrubbing the marker lines would: both sides kept.
    fix(&fx, "f.env", "gamma\n\ndelta\nalpha\n\n");
    let report = resolved(&fx, NOW + 200);
    assert_eq!(report.fixed, 1, "one region waited and one was fixed");
    assert_eq!(report.replayed, 2, "both commits landed: {report:?}");
    assert!(report.still_held.is_none(), "nothing is left waiting");

    let repo = fx.repo();
    let landed = commits_between(&repo, oid(&report.new_tip), oid(&tip(&fx, "main")));
    assert_eq!(landed.len(), 2, "two commits landed: {landed:?}");
    for id in &landed {
        for (path, contents) in tree_files(&repo, oid(id)) {
            assert!(
                !contents.contains(OPENER),
                "{id} still carries markers in {path}:\n{contents}"
            );
        }
    }
    assert_eq!(
        file_in(&repo, oid(landed.last().expect("feature 1'")), "f.env").as_deref(),
        Some("gamma\n\ndelta\nalpha\n\n"),
        "the fix lands in the commit that owned the conflict"
    );
    assert_eq!(
        file_in(&repo, oid(&report.new_tip), "f.env").as_deref(),
        Some("gamma\n\ndelta\nalpha\nbeta\n"),
        "and the commit above it replays its own change over the fix"
    );
}

#[test]
fn a_restacks_open_change_comes_back() {
    let fx = Fixture::new();
    ident(&fx);
    restack_stack(&fx);
    // An open change in a file the rewrite does not touch.
    fx.write("open.txt", "dirty\n");
    hold_a_restack(&fx);

    let opened = open_resolution(&fx, NOW + 100);
    assert!(
        opened.parked.is_some(),
        "a restack carries the open change, so resolving parks it"
    );
    assert!(
        !fx.path().join("open.txt").exists(),
        "the parked change is out of the way while the markers stand"
    );

    fix(&fx, "f.txt", "RESOLVED\n");
    let report = resolved(&fx, NOW + 200);

    assert_eq!(
        std::fs::read_to_string(fx.path().join("open.txt")).unwrap(),
        "dirty\n",
        "the change the restack carried is open again on the new tip"
    );
    let repo = fx.repo();
    assert_eq!(
        file_in(&repo, oid(&report.new_tip), "open.txt"),
        None,
        "and it is still open, not committed into the stack"
    );
}

#[test]
fn an_absorbs_open_change_does_not_come_back_twice() {
    let fx = Fixture::new();
    ident(&fx);
    let (c1, _c2) = absorb_stack(&fx);
    hold_an_absorb(&fx, &c1, Vec::new());

    let opened = open_resolution(&fx, NOW + 100);
    assert!(
        opened.parked.is_none(),
        "an absorb has already folded the open change into the chain's trees, \
         so there is nothing to park"
    );
    // The fold's own conflict is what the reader is shown, labeled as step one
    // of the chain — an absorb's fold IS step one, and a block nobody can
    // attribute is a block that lands inside a commit.
    let on_disk = std::fs::read_to_string(fx.path().join("f.txt")).unwrap();
    assert!(
        on_disk.contains("<<<<<<< the rewrite so far (1/"),
        "the fold's markers are fufu's own: {on_disk}"
    );

    // `B` is the content `c2` replays over, so the whole stack lands.
    fix(&fx, "f.txt", "B\n");
    let report = resolved(&fx, NOW + 200);
    assert_eq!(report.verb, "absorb");
    assert_eq!(report.branch, "main");

    let repo = fx.repo();
    let landed = commits_between(
        &repo,
        oid(&report.new_tip),
        oid(&fx.git(&["rev-parse", "HEAD~2"])),
    );
    let c1_new = oid(landed.last().expect("the oldest landed commit is c1'"));
    assert_eq!(
        file_in(&repo, c1_new, "g.txt").as_deref(),
        Some("gopen\n"),
        "the open change landed in the commit it was absorbed into"
    );
    assert_eq!(
        file_in(&repo, c1_new, "f.txt").as_deref(),
        Some("B\n"),
        "and the conflicted half landed as the reader resolved it"
    );
    assert_eq!(
        file_in(&repo, oid(&report.new_tip), "h.txt").as_deref(),
        Some("h1\n"),
        "the descendant still carries its own content"
    );
    for id in &landed {
        for (path, contents) in tree_files(&repo, oid(id)) {
            assert!(
                !contents.contains(OPENER),
                "{id} still carries markers in {path}:\n{contents}"
            );
        }
    }
    drop(repo);

    assert_eq!(
        fx.git(&["status", "--porcelain"]).trim(),
        "",
        "the working tree is clean: nothing came back to be applied a second time"
    );
}

#[test]
fn a_filtered_absorb_with_other_changes_refuses_to_open() {
    let fx = Fixture::new();
    ident(&fx);
    fx.write("a.txt", "start\n");
    fx.write("b.txt", "b-base\n");
    let _c0 = fx.commit("base");
    fx.write("a.txt", "A\n");
    let c1 = fx.commit("c1");
    fx.write("a.txt", "B\n");
    let _c2 = fx.commit("c2");
    fx.write("a.txt", "C\n");
    hold_an_absorb(&fx, &c1, vec!["a.txt".into()]);

    // A change outside the filter: it is not in the chain, so laying the
    // markers down would overwrite it.
    fx.write("b.txt", "unselected work\n");

    let before_refs = head_refs(&fx);
    let before_ops = verb_ops(&fx);
    let err = resolve_call(&fx, false, NOW + 100)
        .expect_err("a filtered absorb with work outside the filter refuses");

    assert_eq!(err.id(), "held/unsupported");
    assert!(
        err.to_string().contains("b.txt"),
        "the refusal names the work that would be overwritten: {err}"
    );
    assert_eq!(
        std::fs::read_to_string(fx.path().join("b.txt")).unwrap(),
        "unselected work\n",
        "nothing was written"
    );
    assert!(
        !std::fs::read_to_string(fx.path().join("a.txt"))
            .unwrap()
            .contains(OPENER),
        "and no markers went in"
    );
    assert_eq!(head_refs(&fx), before_refs, "a refusal moves no ref");
    assert_eq!(verb_ops(&fx), before_ops, "and appends no verb op");
    assert!(
        ff_core::held::of(&fx.repo(), "main").unwrap().is_some(),
        "the hold still stands"
    );
    assert!(
        ff_core::held::resolving(&fx.repo(), "main")
            .unwrap()
            .is_none(),
        "and no session was opened"
    );
}

#[test]
fn a_resolved_lift_lands_the_open_change_back() {
    let fx = Fixture::new();
    ident(&fx);
    // `c1` introduces `doc.txt` and `c2` edits it: lifting the file out of
    // `c1` makes `c2`'s edit a modification of nothing.
    fx.write("f0.txt", "base\n");
    let _c0 = fx.commit("base");
    fx.write("doc.txt", "v1\n");
    let c1 = fx.commit("c1");
    fx.write("doc.txt", "v2\n");
    let _c2 = fx.commit("c2");

    let repo = fx.repo();
    let (outcome, _ctx) = ff_core::absorb::lift(
        &repo,
        Some(oid(&c1)),
        vec!["doc.txt".into()],
        &prov(),
        Some(NOW),
        vec!["ff".into(), "lift".into()],
    )
    .unwrap();
    assert!(
        matches!(outcome, ff_core::LiftOutcome::Held(_)),
        "the precondition is a held lift, got {outcome:?}"
    );
    drop(repo);

    let opened = open_resolution(&fx, NOW + 100);
    assert!(opened.parked.is_none(), "a lift parks nothing either");

    let report = resolved(&fx, NOW + 200);
    assert_eq!(report.verb, "lift");
    assert!(report.still_held.is_none());

    let repo = fx.repo();
    for (path, contents) in tree_files(&repo, oid(&report.new_tip)) {
        assert!(
            !contents.contains(OPENER),
            "the landed tip carries no markers in {path}"
        );
    }
    assert!(
        ff_core::held::of(&repo, "main").unwrap().is_none()
            && ff_core::held::resolving(&repo, "main").unwrap().is_none(),
        "the landing cleared both the hold and the session"
    );
}

#[test]
fn abandoning_a_resolution_through_done_is_the_same_act() {
    let fx = Fixture::new();
    ident(&fx);
    restack_stack(&fx);
    hold_a_restack(&fx);
    open_resolution(&fx, NOW + 100);
    fix(&fx, "f.txt", "RESOLVED\n");

    let repo = fx.repo();
    let (outcome, _ctx) = ff_core::done::done(
        &repo,
        true,
        &prov(),
        Some(NOW + 200),
        vec!["ff".into(), "done".into(), "--abandon".into()],
    )
    .unwrap();
    drop(repo);
    assert!(
        matches!(outcome, DoneOutcome::Abandoned(_)),
        "`ff done --abandon` over a resolution abandons it, got {outcome:?}"
    );

    let repo = fx.repo();
    assert!(
        ff_core::held::of(&repo, "feature").unwrap().is_none(),
        "the hold is dropped"
    );
    assert!(
        ff_core::held::resolving(&repo, "feature")
            .unwrap()
            .is_none(),
        "and the session with it"
    );
    drop(repo);
    assert_eq!(
        std::fs::read_to_string(fx.path().join("f.txt")).unwrap(),
        "two\n",
        "the working tree is back to the branch's own content"
    );
}

#[test]
fn abandoning_an_absorbs_resolution_gives_the_open_change_back() {
    let fx = Fixture::new();
    ident(&fx);
    let (c1, _c2) = absorb_stack(&fx);
    let before_tip = tip(&fx, "main");
    hold_an_absorb(&fx, &c1, Vec::new());

    // `ff resolve` spends the open change: it folds it into the chain's trees
    // and the markers take the working tree's place. Abandoning has to hand
    // it back — nothing here parked it, so the session's own record is the
    // only place it survives.
    open_resolution(&fx, NOW + 100);
    match resolve_call(&fx, true, NOW + 200).unwrap() {
        ResolveOutcome::Abandoned(r) => assert_eq!(r.verb, "absorb"),
        other => panic!("--abandon must abandon, got {other:?}"),
    }

    assert_eq!(tip(&fx, "main"), before_tip, "an abandon moves no ref");
    assert_eq!(
        std::fs::read_to_string(fx.path().join("f.txt")).unwrap(),
        "C\n",
        "the open change is open again, exactly as it was"
    );
    assert_eq!(
        std::fs::read_to_string(fx.path().join("g.txt")).unwrap(),
        "gopen\n",
        "including the half of it the fold landed cleanly"
    );
    assert_eq!(
        std::fs::read_to_string(fx.path().join("h.txt")).unwrap(),
        "h1\n",
        "and the tip's own files are back on disk"
    );
    let repo = fx.repo();
    assert!(
        ff_core::held::of(&repo, "main").unwrap().is_none()
            && ff_core::held::resolving(&repo, "main").unwrap().is_none(),
        "the hold and the session are both dropped"
    );
}
