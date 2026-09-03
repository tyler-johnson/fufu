//! Contract for `ff done` finishing a resolution: the reader's fixes go back
//! into the steps that owned them, the chain re-runs, and the whole stack
//! lands at once — refs moving one time, every landed commit clean, no
//! conflicted state ever standing in the graph.

use ff_core::gix;
use ff_core::{DoneOutcome, Provenance, ResolveOutcome};
use ff_testsupport::Fixture;
use ff_testsupport::hooks::{STAGED_HOOK, install_hook, staged_marker};

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
        ff_core::Verify::Run,
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
        ff_core::Verify::Run,
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
        ff_core::Verify::Run,
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

// ---------------------------------------------------------------------------
// The pre-commit gate on the resolution landing. This is fufu's
// `rebase --continue`, which under git does run the hook.
// ---------------------------------------------------------------------------

#[test]
fn a_declining_pre_commit_hook_refuses_the_resolution_landing() {
    let fx = Fixture::new();
    ident(&fx);
    let (_base, _f1) = restack_stack(&fx);
    hold_a_restack(&fx);
    open_resolution(&fx, NOW + 10);
    fix(&fx, "f.txt", "three\n");

    let refs_before = head_refs(&fx);
    let ops_before = verb_ops(&fx);
    let index_before = fx.index_bytes();
    install_hook(&fx, "pre-commit", "#!/bin/sh\nexit 1\n");

    let err = done_call(&fx, NOW + 20).unwrap_err();
    assert_eq!(err.id(), "hook/declined");
    assert_eq!(head_refs(&fx), refs_before, "no ref moved");
    assert_eq!(verb_ops(&fx), ops_before, "no operation was journaled");
    assert_eq!(
        fx.index_bytes(),
        index_before,
        "a declined landing restores .git/index byte-for-byte"
    );

    // --no-verify lands it, and the hold clears.
    let repo = fx.repo();
    let outcome = ff_core::done::done(
        &repo,
        false,
        ff_core::Verify::Skip,
        &prov(),
        Some(NOW + 30),
        vec!["ff".into(), "done".into()],
    )
    .map(|(outcome, _ctx)| outcome)
    .unwrap();
    assert!(matches!(outcome, DoneOutcome::Resolved(_)), "{outcome:?}");
}

#[test]
fn the_resolution_landing_gate_sees_the_fixes_staged() {
    let fx = Fixture::new();
    ident(&fx);
    let (_base, _f1) = restack_stack(&fx);
    hold_a_restack(&fx);
    open_resolution(&fx, NOW + 10);
    fix(&fx, "f.txt", "three\n");
    install_hook(&fx, "pre-commit", STAGED_HOOK);

    let report = resolved(&fx, NOW + 20);
    assert_eq!(report.fixed, 1);
    assert_eq!(
        staged_marker(&fx),
        vec!["f.txt"],
        "the gate is told exactly the file the reader fixed"
    );
}

#[test]
fn a_pre_commit_formatter_rewrite_lands_in_the_resolution() {
    let fx = Fixture::new();
    ident(&fx);
    let (_base, _f1) = restack_stack(&fx);
    hold_a_restack(&fx);
    open_resolution(&fx, NOW + 10);
    fix(&fx, "f.txt", "three\n");
    install_hook(
        &fx,
        "pre-commit",
        "#!/bin/sh\nprintf 'formatted\\n' > f.txt\n",
    );

    let report = resolved(&fx, NOW + 20);
    let repo = fx.repo();
    let landed = oid(&report.new_tip);
    assert_eq!(
        file_in(&repo, landed, "f.txt").as_deref(),
        Some("formatted\n"),
        "the hook's formatting is what landed"
    );
    drop(repo);
    assert_eq!(fx.git(&["status", "--porcelain"]).trim(), "");
}

/// The nested landing an absorb's resolution runs through `absorb_with`,
/// with the gate's window standing open over it. The rewritten index must
/// not make the re-entry read the worktree as clean.
#[test]
fn an_absorbs_resolution_lands_under_the_gate() {
    let fx = Fixture::new();
    ident(&fx);
    let (c1, _c2) = absorb_stack(&fx);
    hold_an_absorb(&fx, &c1, Vec::new());
    open_resolution(&fx, NOW + 100);
    fix(&fx, "f.txt", "B\n");
    install_hook(&fx, "pre-commit", STAGED_HOOK);

    let report = resolved(&fx, NOW + 200);
    assert_eq!(report.verb, "absorb");
    // The gate is told the worktree against HEAD, so only `g.txt` shows:
    // the reader fixed `f.txt` back to the content `c2` — HEAD — already
    // holds, which is exactly why the stack can land.
    assert_eq!(staged_marker(&fx), vec!["g.txt"]);
    let repo = fx.repo();
    let landed = commits_between(
        &repo,
        oid(&report.new_tip),
        oid(&fx.git(&["rev-parse", "HEAD~2"])),
    );
    let c1_new = oid(landed.last().expect("the oldest landed commit is c1'"));
    assert_eq!(file_in(&repo, c1_new, "g.txt").as_deref(), Some("gopen\n"));
}

// ---------------------------------------------------------------------------
// The cascade a hold stopped resumes when the hold lands
// ---------------------------------------------------------------------------

/// Record `parent` as the branch `branch` sits on, the way `ff start
/// <branch>` does.
fn stacked_on(fx: &Fixture, branch: &str, parent: &str) {
    let repo = fx.repo();
    let mut meta = ff_core::branchmeta::read(&repo, branch).unwrap();
    meta.parent = Some(parent.to_string());
    ff_core::branchmeta::write(&repo, branch, &meta).unwrap();
}

fn is_ancestor(fx: &Fixture, ancestor: &str, of: &str) -> bool {
    fx.try_git(&["merge-base", "--is-ancestor", ancestor, of])
        .status
        .success()
}

/// `main` ← `feat` ← `top` ← `deeper`, each parent recorded, and `main`
/// moved so `feat`'s `f1` cannot replay onto it: `f1` and `m1` both rewrite
/// `f.txt`. `top`'s `x1` adds `x.txt`, or rewrites `f.txt` too when
/// `top_conflicts`, so it cannot follow the landed `feat` either; `deeper`'s
/// `y1` adds `y.txt`. Leaves the fixture on `feat`. Returns (base, x1, y1).
fn cascade_stack(fx: &Fixture, top_conflicts: bool) -> (String, String, String) {
    fx.write("f.txt", "one\n");
    let base = fx.commit("base");
    fx.git(&["switch", "-q", "-c", "feat"]);
    fx.write("f.txt", "two\n");
    let _f1 = fx.commit("f1");
    fx.git(&["switch", "-q", "-c", "top"]);
    if top_conflicts {
        fx.write("f.txt", "x\n");
    } else {
        fx.write("x.txt", "x\n");
    }
    let x1 = fx.commit("x1");
    fx.git(&["switch", "-q", "-c", "deeper"]);
    fx.write("y.txt", "y\n");
    let y1 = fx.commit("y1");
    fx.git(&["switch", "-q", "main"]);
    fx.write("f.txt", "three\n");
    let _m1 = fx.commit("m1");
    fx.git(&["switch", "-q", "feat"]);
    stacked_on(fx, "top", "feat");
    stacked_on(fx, "deeper", "top");
    (base, x1, y1)
}

fn restack_feat(fx: &Fixture, now: i64) -> ff_core::RestackOutcome {
    let repo = fx.repo();
    ff_core::restack::restack(
        &repo,
        Some("feat".into()),
        None,
        &prov(),
        Some(now),
        vec!["ff".into(), "restack".into(), "feat".into()],
    )
    .unwrap()
    .0
}

/// `ff restack feat` holds on `feat`, with `top` and `deeper` untouched.
fn hold_feat(fx: &Fixture, x1: &str, y1: &str) {
    let outcome = restack_feat(fx, NOW);
    assert!(
        matches!(outcome, ff_core::RestackOutcome::Held(_)),
        "the precondition is a held restack of feat, got {outcome:?}"
    );
    assert_eq!(tip(fx, "top"), x1, "a hold leaves the subtree alone");
    assert_eq!(tip(fx, "deeper"), y1, "a hold leaves the subtree alone");
}

fn moved_names(cascade: &ff_core::Cascade) -> Vec<&str> {
    cascade.moved.iter().map(|m| m.branch.as_str()).collect()
}

#[test]
fn landing_a_held_restack_replays_its_subtree() {
    let fx = Fixture::new();
    ident(&fx);
    let (_base, x1, y1) = cascade_stack(&fx, false);
    hold_feat(&fx, &x1, &y1);

    open_resolution(&fx, NOW + 100);
    fix(&fx, "f.txt", "RESOLVED\n");
    let report = resolved(&fx, NOW + 200);

    assert_eq!(report.verb, "restack");
    assert_eq!(report.branch, "feat");
    assert_eq!(report.replayed, 1, "feat's own commit landed");
    assert!(report.still_held.is_none(), "a hold above is not feat's");
    assert_eq!(
        moved_names(&report.cascade),
        vec!["top", "deeper"],
        "the subtree the hold stopped replays from the landed tip, parent before child"
    );
    assert!(report.cascade.held.is_empty());
    assert!(report.cascade.skipped.is_empty());

    assert!(is_ancestor(&fx, "main", "feat"), "feat sits on main");
    assert!(is_ancestor(&fx, "feat", "top"), "top sits on the new feat");
    assert!(
        is_ancestor(&fx, "top", "deeper"),
        "deeper sits on the new top"
    );
    assert_ne!(tip(&fx, "top"), x1);
    assert_ne!(tip(&fx, "deeper"), y1);
    let repo = fx.repo();
    assert_eq!(
        file_in(&repo, oid(&tip(&fx, "deeper")), "f.txt").as_deref(),
        Some("RESOLVED\n"),
        "the fix reaches the top of the stack"
    );
}

#[test]
fn the_resumed_cascade_rides_the_landing_operation() {
    let fx = Fixture::new();
    ident(&fx);
    let (_base, x1, y1) = cascade_stack(&fx, false);
    hold_feat(&fx, &x1, &y1);
    open_resolution(&fx, NOW + 100);
    fix(&fx, "f.txt", "RESOLVED\n");

    let before_refs = head_refs(&fx);
    let before_ops = verb_ops(&fx);
    let report = resolved(&fx, NOW + 200);
    assert_eq!(moved_names(&report.cascade), vec!["top", "deeper"]);
    assert_eq!(
        verb_ops(&fx),
        before_ops + 1,
        "the landing and its cascade are one verb operation"
    );

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

    assert_eq!(
        head_refs(&fx),
        before_refs,
        "one undo puts feat, top, and deeper back"
    );
    assert_eq!(tip(&fx, "top"), x1);
    assert_eq!(tip(&fx, "deeper"), y1);
    let repo = fx.repo();
    assert!(
        ff_core::held::of(&repo, "feat").unwrap().is_some(),
        "undo puts the hold back"
    );
    assert!(
        ff_core::held::resolving(&repo, "feat").unwrap().is_some(),
        "undo puts the resolution session back"
    );
}

#[test]
fn a_child_that_conflicts_on_resumption_holds_and_the_landing_stands() {
    let fx = Fixture::new();
    ident(&fx);
    let (_base, x1, y1) = cascade_stack(&fx, true);
    hold_feat(&fx, &x1, &y1);

    open_resolution(&fx, NOW + 100);
    fix(&fx, "f.txt", "RESOLVED\n");
    let report = resolved(&fx, NOW + 200);

    assert!(is_ancestor(&fx, "main", "feat"), "feat landed");
    assert_eq!(report.new_tip, tip(&fx, "feat"));
    assert!(
        report.still_held.is_none(),
        "a hold inside the cascade is the child's, not the landing's"
    );
    assert!(report.cascade.moved.is_empty());
    assert_eq!(report.cascade.held.len(), 1, "{:?}", report.cascade);
    let held = &report.cascade.held[0];
    assert_eq!(held.branch, "top");
    assert_eq!(held.base, "feat");
    assert_eq!(held.left_alone, vec!["deeper".to_string()]);
    assert_eq!(held.report.of, 1, "top's own commit, not feat's");

    assert_eq!(tip(&fx, "top"), x1, "the held child stays where it stood");
    assert_eq!(tip(&fx, "deeper"), y1, "and so does everything above it");
    let repo = fx.repo();
    let hold = ff_core::held::of(&repo, "top")
        .unwrap()
        .expect("top holds its own restack");
    assert_eq!(
        hold.intent,
        ff_core::held::Intent::Restack {
            branch: "top".into(),
            onto: "refs/heads/feat".into(),
        }
    );
    assert!(
        ff_core::held::of(&repo, "feat").unwrap().is_none(),
        "feat's hold cleared with the landing"
    );
    assert!(
        ff_core::held::of(&repo, "deeper").unwrap().is_none(),
        "a branch left alone holds nothing"
    );
}

#[test]
fn a_held_child_is_skipped_on_resumption() {
    let fx = Fixture::new();
    ident(&fx);
    let (_base, x1, y1) = cascade_stack(&fx, false);
    hold_feat(&fx, &x1, &y1);
    let standing = ff_core::held::Held {
        intent: ff_core::held::Intent::Restack {
            branch: "top".into(),
            onto: "refs/heads/feat".into(),
        },
        at: ff_core::futures::At::OpenChange,
        paths: vec!["x.txt".into()],
        time: NOW,
    };
    ff_core::held::set(&fx.repo(), "top", Some(standing.clone())).unwrap();

    open_resolution(&fx, NOW + 100);
    fix(&fx, "f.txt", "RESOLVED\n");
    let report = resolved(&fx, NOW + 200);

    assert!(is_ancestor(&fx, "main", "feat"), "feat landed");
    assert!(report.still_held.is_none());
    assert!(report.cascade.moved.is_empty());
    assert!(report.cascade.held.is_empty());
    assert_eq!(report.cascade.skipped.len(), 1, "{:?}", report.cascade);
    let skip = &report.cascade.skipped[0];
    assert_eq!(skip.branch, "top");
    assert_eq!(skip.reason, ff_core::SkipReason::AlreadyHeld);
    assert_eq!(skip.left_alone, vec!["deeper".to_string()]);

    assert_eq!(tip(&fx, "top"), x1, "a held child is not moved");
    assert_eq!(tip(&fx, "deeper"), y1);
    assert_eq!(
        ff_core::held::of(&fx.repo(), "top").unwrap(),
        Some(standing),
        "the standing hold is untouched"
    );
}

#[test]
fn landing_a_held_absorb_replays_its_subtree() {
    let fx = Fixture::new();
    ident(&fx);
    // `absorb_stack`'s shape, on `feat` with `top` and `deeper` above it.
    fx.write("f.txt", "one\n");
    fx.write("g.txt", "g0\n");
    let _c0 = fx.commit("base");
    fx.git(&["switch", "-q", "-c", "feat"]);
    fx.write("f.txt", "A\n");
    let c1 = fx.commit("c1");
    fx.write("f.txt", "B\n");
    fx.write("h.txt", "h1\n");
    let _c2 = fx.commit("c2");
    fx.git(&["switch", "-q", "-c", "top"]);
    fx.write("x.txt", "x\n");
    let x1 = fx.commit("x1");
    fx.git(&["switch", "-q", "-c", "deeper"]);
    fx.write("y.txt", "y\n");
    let y1 = fx.commit("y1");
    fx.git(&["switch", "-q", "feat"]);
    stacked_on(&fx, "top", "feat");
    stacked_on(&fx, "deeper", "top");
    fx.write("f.txt", "C\n");
    fx.write("g.txt", "gopen\n");
    hold_an_absorb(&fx, &c1, Vec::new());
    assert_eq!(tip(&fx, "top"), x1, "a hold leaves the subtree alone");

    open_resolution(&fx, NOW + 100);
    fix(&fx, "f.txt", "B\n");
    let report = resolved(&fx, NOW + 200);

    assert_eq!(report.verb, "absorb");
    assert_eq!(report.branch, "feat");
    assert!(report.still_held.is_none());
    assert_eq!(
        moved_names(&report.cascade),
        vec!["top", "deeper"],
        "the subtree follows the absorbed feat"
    );
    assert_eq!(report.new_tip, tip(&fx, "feat"));
    assert!(is_ancestor(&fx, "feat", "top"), "top sits on the new feat");
    assert!(
        is_ancestor(&fx, "top", "deeper"),
        "deeper sits on the new top"
    );
    assert_ne!(tip(&fx, "top"), x1);
    assert_ne!(tip(&fx, "deeper"), y1);
    let repo = fx.repo();
    assert_eq!(
        file_in(&repo, oid(&tip(&fx, "deeper")), "g.txt").as_deref(),
        Some("gopen\n"),
        "what the absorb folded in reaches the top of the stack"
    );
    assert_eq!(
        file_in(&repo, oid(&tip(&fx, "deeper")), "y.txt").as_deref(),
        Some("y\n")
    );
}

#[test]
fn abandoning_a_hold_replays_nothing() {
    let fx = Fixture::new();
    ident(&fx);
    let (_base, x1, y1) = cascade_stack(&fx, false);
    hold_feat(&fx, &x1, &y1);
    let feat = tip(&fx, "feat");

    // A bare hold, and a hold with a session open over it: both abandons
    // leave the whole stack where it stood.
    let abandoned = resolve_call(&fx, true, NOW + 100).unwrap();
    assert!(
        matches!(abandoned, ResolveOutcome::Abandoned(_)),
        "{abandoned:?}"
    );
    assert_eq!(tip(&fx, "feat"), feat);
    assert_eq!(tip(&fx, "top"), x1);
    assert_eq!(tip(&fx, "deeper"), y1);
    assert!(ff_core::held::of(&fx.repo(), "feat").unwrap().is_none());

    hold_feat(&fx, &x1, &y1);
    open_resolution(&fx, NOW + 200);
    let abandoned = resolve_call(&fx, true, NOW + 300).unwrap();
    assert!(
        matches!(abandoned, ResolveOutcome::Abandoned(_)),
        "{abandoned:?}"
    );
    assert_eq!(tip(&fx, "feat"), feat, "feat stays off main");
    assert_eq!(tip(&fx, "top"), x1, "the subtree stays where it stood");
    assert_eq!(tip(&fx, "deeper"), y1);
    assert!(!is_ancestor(&fx, "main", "feat"));
    let repo = fx.repo();
    assert!(ff_core::held::of(&repo, "feat").unwrap().is_none());
    assert!(ff_core::held::resolving(&repo, "feat").unwrap().is_none());
}

#[test]
fn a_released_hold_lands_with_its_cascade_on_the_rerun() {
    let fx = Fixture::new();
    ident(&fx);
    let (base, x1, y1) = cascade_stack(&fx, false);
    hold_feat(&fx, &x1, &y1);

    // main moves back off the conflict and on to something feat's commit
    // does not touch.
    fx.git(&["switch", "-q", "main"]);
    fx.git(&["reset", "-q", "--hard", &base]);
    fx.write("z.txt", "z\n");
    let m2 = fx.commit("m2");
    fx.git(&["switch", "-q", "feat"]);

    let released = resolve_call(&fx, false, NOW + 100).unwrap();
    assert!(
        matches!(released, ResolveOutcome::Released(_)),
        "the world moved out of the conflict: {released:?}"
    );
    assert!(
        ff_core::held::of(&fx.repo(), "feat").unwrap().is_none(),
        "the release clears the hold"
    );
    assert_eq!(tip(&fx, "top"), x1, "a release moves nothing");
    assert_eq!(tip(&fx, "deeper"), y1);

    // The verb that recorded the hold lands the rewrite when it is re-run,
    // and the re-run cascades on its own.
    let report = match restack_feat(&fx, NOW + 200) {
        ff_core::RestackOutcome::Restacked(r) => r,
        other => panic!("the re-run lands, got {other:?}"),
    };
    assert_eq!(moved_names(&report.cascade), vec!["top", "deeper"]);
    assert!(is_ancestor(&fx, &m2, "feat"), "feat sits on the new main");
    assert!(is_ancestor(&fx, "feat", "top"));
    assert!(is_ancestor(&fx, "top", "deeper"));
    assert_ne!(tip(&fx, "top"), x1);
    assert_ne!(tip(&fx, "deeper"), y1);
}

/// The hold a cascade writes on a child names the parent as its base, and
/// resolving it replans against the parent as it now stands. The parent's
/// rewrite kept none of its old commits, so a walk bounded at the merge
/// base hands back the parent's own old commits as the child's; replayed
/// onto their absorbed selves they conflict, on content the child never
/// touched. The replan bounds the range where the child forked from the
/// parent's history instead, so the resolution is about the child's commit
/// alone.
#[test]
fn resolving_a_child_hold_replays_only_its_own_commits() {
    let fx = Fixture::new();
    ident(&fx);
    fx.write("a.txt", "base\n");
    let _base = fx.commit("base");
    fx.git(&["switch", "-q", "-c", "feat"]);
    fx.write("a.txt", "one\n");
    let c1 = fx.commit("c1");
    fx.write("c.txt", "c\n");
    let _c2 = fx.commit("c2");
    fx.git(&["switch", "-q", "-c", "top"]);
    fx.write("a.txt", "one x\n");
    let x1 = fx.commit("x1");
    fx.git(&["switch", "-q", "feat"]);
    stacked_on(&fx, "top", "feat");

    // The absorb lands and its cascade holds top: c1 and x1 now disagree
    // about the line.
    fx.write("a.txt", "one, edited\n");
    let repo = fx.repo();
    let (outcome, _ctx) = ff_core::absorb::absorb(
        &repo,
        Some(oid(&c1)),
        Vec::new(),
        ff_core::Verify::Run,
        &prov(),
        Some(NOW),
        vec!["ff".into(), "absorb".into()],
    )
    .unwrap();
    drop(repo);
    let report = match outcome {
        ff_core::AbsorbOutcome::Absorbed(r) => r,
        other => panic!("the absorb lands, got {other:?}"),
    };
    assert_eq!(report.cascade.held.len(), 1, "{:?}", report.cascade);
    assert_eq!(report.cascade.held[0].branch, "top");
    assert_eq!(report.cascade.held[0].report.of, 1);
    assert_eq!(tip(&fx, "top"), x1);

    fx.git(&["switch", "-q", "top"]);
    let opened = open_resolution(&fx, NOW + 100);
    assert_eq!(opened.verb, "restack");
    assert_eq!(
        opened.of, 1,
        "the resolution is about top's one commit, not feat's two as well"
    );
    assert_eq!(opened.steps, 1);
    let shown = std::fs::read_to_string(fx.path().join("a.txt")).unwrap();
    assert!(
        shown.contains("one x"),
        "x1's side is what the reader sees: {shown}"
    );
    assert!(shown.contains("one, edited"), "{shown}");

    fix(&fx, "a.txt", "one, edited x\n");
    let report = resolved(&fx, NOW + 200);
    assert_eq!(report.branch, "top");
    assert_eq!(report.replayed, 1, "top's own commit and nothing of feat's");
    assert!(report.still_held.is_none());
    assert!(
        is_ancestor(&fx, "feat", "top"),
        "top sits on the absorbed feat"
    );
    let repo = fx.repo();
    let landed = commits_between(&repo, oid(&report.new_tip), oid(&tip(&fx, "feat")));
    assert_eq!(landed.len(), 1, "one commit above feat: {landed:?}");
    assert_eq!(
        file_in(&repo, oid(&report.new_tip), "a.txt").as_deref(),
        Some("one, edited x\n")
    );
    assert_eq!(
        file_in(&repo, oid(&report.new_tip), "c.txt").as_deref(),
        Some("c\n"),
        "feat's own commits are beneath, through the absorbed feat"
    );
}
