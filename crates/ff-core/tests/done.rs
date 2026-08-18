//! Contract for `done::done`: `ff edit`'s session ended, landed or
//! abandoned — the amend, the replay, and the return, fused into one
//! operation.

use ff_core::gix;
use ff_core::{ArrivalReport, DoneOutcome, EditOutcome, Provenance};
use ff_testsupport::Fixture;

const NOW: i64 = 1_799_999_999;

/// `done` reads the committer identity from the repo config, which the
/// fixture's hermetic env does not set; git itself gets its identity from
/// env vars, so this is only for the gix side.
fn ident(fx: &Fixture) {
    fx.set_config("user.name", "Fixture Committer");
    fx.set_config("user.email", "committer@fixture.test");
}

fn prov() -> Provenance {
    Provenance::new("pre", Some("ff done".into()))
}

fn edit_call(fx: &Fixture, rev: &str, now: i64) -> (EditOutcome, ff_core::ops::VerbContext) {
    let repo = fx.repo();
    ff_core::edit::edit(
        &repo,
        rev,
        &prov(),
        Some(now),
        vec!["ff".into(), "edit".into()],
    )
    .unwrap()
}

fn opened(outcome: EditOutcome) -> ff_core::EditReport {
    match outcome {
        EditOutcome::Opened(report) => report,
        other => panic!("a session must open, got {other:?}"),
    }
}

fn done_call(fx: &Fixture, abandon: bool, now: i64) -> (DoneOutcome, ff_core::ops::VerbContext) {
    let repo = fx.repo();
    ff_core::done::done(
        &repo,
        abandon,
        &prov(),
        Some(now),
        vec!["ff".into(), "done".into()],
    )
    .unwrap()
}

fn done_err(fx: &Fixture, abandon: bool, now: i64) -> ff_core::Error {
    let repo = fx.repo();
    ff_core::done::done(
        &repo,
        abandon,
        &prov(),
        Some(now),
        vec!["ff".into(), "done".into()],
    )
    .unwrap_err()
}

fn landed(outcome: DoneOutcome) -> ff_core::DoneReport {
    match outcome {
        DoneOutcome::Done(report) => report,
        other => panic!("the session must land, got {other:?}"),
    }
}

fn abandoned(outcome: DoneOutcome) -> ff_core::AbandonReport {
    match outcome {
        DoneOutcome::Abandoned(report) => report,
        other => panic!("the session must abandon, got {other:?}"),
    }
}

/// Every worktree file as (repo-relative path, bytes), sorted by path.
fn worktree_files(fx: &Fixture) -> Vec<(String, Vec<u8>)> {
    let root = fx.path();
    let mut out = Vec::new();
    let mut dirs = vec![root.clone()];
    while let Some(dir) = dirs.pop() {
        for entry in std::fs::read_dir(&dir).unwrap() {
            let entry = entry.unwrap();
            let name = entry.file_name();
            if name == ".git" {
                continue;
            }
            let path = entry.path();
            if path.is_dir() {
                dirs.push(path);
            } else {
                let rel = path
                    .strip_prefix(&root)
                    .unwrap()
                    .to_string_lossy()
                    .replace(std::path::MAIN_SEPARATOR, "/");
                out.push((rel, std::fs::read(&path).unwrap()));
            }
        }
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
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

/// How many anonymous branches exist.
fn anon_count(fx: &Fixture) -> usize {
    fx.git(&["for-each-ref", "refs/heads/ff/", "--format=%(refname)"])
        .lines()
        .count()
}

/// Loose objects in the fixture's object store — `git count-objects -v`,
/// never `git rev-list --all --count`: the latter cannot see an unreachable
/// commit, so it can never fail to prove a refusal wrote nothing reachable.
fn loose_objects(fx: &Fixture) -> u64 {
    let out = fx.git(&["count-objects", "-v"]);
    for line in out.lines() {
        if let Some(n) = line.strip_prefix("count: ") {
            return n.trim().parse().unwrap();
        }
    }
    panic!("git count-objects -v had no count line: {out}");
}

/// The shared stack: `main` with four commits `c0..c3`, distinct files per
/// commit so nothing overlaps, and `mid` at `c1`.
fn base(fx: &Fixture) -> (String, String, String, String) {
    fx.write("c0.txt", "c0\n");
    let c0 = fx.commit("c0");
    fx.write("c1.txt", "c1\n");
    let c1 = fx.commit("c1");
    fx.write("c2.txt", "c2\n");
    let c2 = fx.commit("c2");
    fx.write("c3.txt", "c3\n");
    let c3 = fx.commit("c3");
    fx.git(&["branch", "mid", &c1]);
    (c0, c1, c2, c3)
}

#[test]
fn done_amends_and_replays() {
    let fx = Fixture::new();
    ident(&fx);
    let (_c0, c1, _c2, c3) = base(&fx);
    let commits_before = fx.git(&["rev-list", "--count", "main"]).trim().to_string();

    let (edit_outcome, _ctx) = edit_call(&fx, &c1, NOW);
    let edit_report = opened(edit_outcome);

    fx.write("c1.txt", "c1 edited\n");
    let (outcome, _ctx) = done_call(&fx, false, NOW + 100);
    let report = landed(outcome);

    assert_eq!(
        std::fs::read_to_string(fx.path().join("c1.txt")).unwrap(),
        "c1 edited\n",
        "c1's content must be the edited one on main"
    );
    assert_eq!(
        fx.git(&["rev-list", "--count", "main"]).trim(),
        commits_before,
        "main must have the same number of commits"
    );
    assert_eq!(report.replayed, 2, "c2 and c3 must be replayed");
    assert_eq!(report.moved, vec!["mid".to_string()]);
    assert_ne!(
        fx.git(&["rev-parse", "mid"]).trim(),
        c1,
        "mid must have moved"
    );
    assert_eq!(anon_count(&fx), 0, "the session branch must be gone");
    assert_eq!(
        fx.git(&["rev-parse", "--abbrev-ref", "HEAD"]).trim(),
        "main"
    );
    assert_eq!(fx.git(&["status", "--porcelain"]).trim(), "");
    assert!(!report.unchanged);
    assert_ne!(report.new_tip, c3);

    let record = tip_record(&fx.repo());
    assert_eq!(record.verb, "done");
    assert_eq!(
        record.edit_session,
        Some(ff_core::ops::SessionTransition {
            branch: edit_report.session,
            old: Some(ff_core::branchmeta::Session {
                onto: "main".into(),
                at: c1,
            }),
            new: None,
        })
    );
}

#[test]
fn done_carries_the_parked_change_back() {
    let fx = Fixture::new();
    ident(&fx);
    let (_c0, c1, _c2, _c3) = base(&fx);

    fx.write("wip.txt", "wip\n");
    let (edit_outcome, _ctx) = edit_call(&fx, &c1, NOW);
    let edit_report = opened(edit_outcome);
    assert!(edit_report.parked.is_some(), "the dirty tree must park");
    assert!(!fx.path().join("wip.txt").exists());

    fx.write("c1.txt", "c1 edited\n");
    let (outcome, _ctx) = done_call(&fx, false, NOW + 100);
    let report = landed(outcome);

    assert_eq!(
        std::fs::read_to_string(fx.path().join("wip.txt")).unwrap(),
        "wip\n",
        "the parked change must be back in the worktree"
    );
    assert!(
        matches!(report.arrival, ArrivalReport::Restored { .. }),
        "the arrival must restore, got {:?}",
        report.arrival
    );
}

#[test]
fn done_with_no_edits_is_unchanged() {
    let fx = Fixture::new();
    ident(&fx);
    let (_c0, c1, _c2, c3) = base(&fx);

    let (edit_outcome, _ctx) = edit_call(&fx, &c1, NOW);
    let _ = opened(edit_outcome);

    let (outcome, _ctx) = done_call(&fx, false, NOW + 100);
    let report = landed(outcome);

    assert!(report.unchanged);
    assert_eq!(
        report.amended.as_deref(),
        Some(report.editing.as_str()),
        "the commit is unchanged: amended is exactly editing, not merely present"
    );
    assert_eq!(report.replayed, 0, "no plan runs when nothing changed");
    assert_eq!(
        fx.git(&["rev-parse", "main"]).trim(),
        c3,
        "main's tip must be byte-identical to before"
    );
    assert_eq!(anon_count(&fx), 0, "the session branch must be gone");
    assert_eq!(
        fx.git(&["rev-parse", "--abbrev-ref", "HEAD"]).trim(),
        "main"
    );

    let files = worktree_files(&fx);
    let names: Vec<&str> = files.iter().map(|(p, _)| p.as_str()).collect();
    for want in ["c0.txt", "c1.txt", "c2.txt", "c3.txt"] {
        assert!(names.contains(&want), "{want} must be on disk: {names:?}");
    }
}

#[test]
fn done_on_the_tip_is_an_amend() {
    let fx = Fixture::new();
    ident(&fx);
    let (_c0, _c1, _c2, c3) = base(&fx);

    let (edit_outcome, _ctx) = edit_call(&fx, &c3, NOW);
    let _ = opened(edit_outcome);

    fx.write("c3.txt", "c3 edited\n");
    let (outcome, _ctx) = done_call(&fx, false, NOW + 100);
    let report = landed(outcome);

    assert_eq!(report.replayed, 0, "nothing waited ahead of the tip");
    assert_ne!(report.new_tip, c3, "the tip must be amended, not reused");
    assert_eq!(
        fx.git(&["rev-parse", "main"]).trim(),
        report.new_tip,
        "main's tip must be the amended commit"
    );
}

#[test]
fn a_conflicting_replay_refuses() {
    let fx = Fixture::new();
    ident(&fx);

    // `shared.txt` is edited three ways at the same line: once by c1, once
    // (independently) by c2, and once inside the session — so replaying c2
    // over the session's amend of c1 conflicts.
    fx.write("shared.txt", "line1\nbase\nline3\n");
    fx.write("c0.txt", "c0\n");
    let _c0 = fx.commit("c0");
    fx.write("shared.txt", "line1\nc1-edit\nline3\n");
    let c1 = fx.commit("c1");
    fx.write("shared.txt", "line1\nc2-edit\nline3\n");
    let _c2 = fx.commit("c2");
    fx.write("c3.txt", "c3\n");
    let c3 = fx.commit("c3");
    fx.git(&["branch", "mid", &c1]);

    let (edit_outcome, _ctx) = edit_call(&fx, &c1, NOW);
    let edit_report = opened(edit_outcome);
    fx.write("shared.txt", "line1\nsession-edit\nline3\n");

    let before = loose_objects(&fx);
    let err = done_err(&fx, false, NOW + 100);
    let after = loose_objects(&fx);

    assert_eq!(err.id(), "held/rewrite-conflict", "{err}");

    // The refusal still appends its pre-verb capture and assembles the
    // exact worktree tree, so the loose count is not the property that
    // failed — it goes up regardless. What actually proves nothing moved is
    // below: no ref moved, no `done` op landed, the session is intact.
    assert!(
        after > before,
        "a refusal still writes its pre-verb capture and the assembled tree"
    );

    assert_eq!(
        fx.git(&["rev-parse", "main"]).trim(),
        c3,
        "main's tip must be unchanged"
    );
    assert_eq!(
        fx.git(&["rev-parse", "mid"]).trim(),
        c1,
        "mid must be unchanged"
    );
    assert_eq!(
        ff_core::branchmeta::read(&fx.repo(), &edit_report.session)
            .unwrap()
            .session,
        Some(ff_core::branchmeta::Session {
            onto: "main".into(),
            at: c1,
        }),
        "the session branch's metadata must still hold the session"
    );
    assert_eq!(
        fx.git(&["rev-parse", "--abbrev-ref", "HEAD"]).trim(),
        edit_report.session,
        "HEAD must still be on the session branch"
    );
    let ops = ff_core::ops::read_ops(&fx.repo(), 1).unwrap();
    assert_ne!(
        ops[0].verb, "done",
        "no done op may land on a refused replay"
    );
}

#[test]
fn done_without_a_session_refuses() {
    let fx = Fixture::new();
    ident(&fx);
    let _ = base(&fx);

    let err = done_err(&fx, false, NOW);
    assert_eq!(err.id(), "session/none", "{err}");
}

#[test]
fn a_moved_session_refuses_but_abandons() {
    let fx = Fixture::new();
    ident(&fx);
    let (_c0, c1, _c2, c3) = base(&fx);

    let (edit_outcome, _ctx) = edit_call(&fx, &c1, NOW);
    let edit_report = opened(edit_outcome);

    // A commit of the session branch's own — allowed here, this is a
    // fixture repo, not the dev repo this task must leave alone.
    fx.write("extra.txt", "extra\n");
    fx.git(&["add", "-A"]);
    fx.git(&["commit", "-q", "-m", "extra"]);

    let err = done_err(&fx, false, NOW + 100);
    assert_eq!(err.id(), "session/moved", "{err}");
    assert!(
        err.exits().iter().any(|e| e.contains("ff done --abandon")),
        "{err}"
    );

    let (outcome, _ctx) = done_call(&fx, true, NOW + 200);
    let report = abandoned(outcome);
    assert_eq!(report.session, edit_report.session);
    assert_eq!(
        fx.git(&["rev-parse", "--abbrev-ref", "HEAD"]).trim(),
        "main"
    );
    assert_eq!(
        fx.git(&["rev-parse", "main"]).trim(),
        c3,
        "main must be untouched by an abandon"
    );
    assert_eq!(anon_count(&fx), 0, "the session branch must be gone");
}

#[test]
fn abandon_stashes_the_edits() {
    let fx = Fixture::new();
    ident(&fx);
    let (_c0, c1, _c2, c3) = base(&fx);

    let (edit_outcome, _ctx) = edit_call(&fx, &c1, NOW);
    let _ = opened(edit_outcome);
    fx.write("c1.txt", "c1 edited\n");

    let (outcome, _ctx) = done_call(&fx, true, NOW + 100);
    let report = abandoned(outcome);

    assert_eq!(
        fx.git(&["rev-parse", "--abbrev-ref", "HEAD"]).trim(),
        "main"
    );
    assert_eq!(fx.git(&["rev-parse", "main"]).trim(), c3);
    assert_eq!(anon_count(&fx), 0, "the session branch must be gone");
    let stashed = report.stashed.expect("the dirty edit must stash");
    let sha = gix::ObjectId::from_hex(stashed.as_bytes()).unwrap();
    assert!(
        ff_core::stash::stash_contains(&fx.repo(), sha).unwrap(),
        "the stashed sha must be reachable from refs/stash"
    );
}

#[test]
fn undo_takes_the_whole_session_back_in_one() {
    let fx = Fixture::new();
    ident(&fx);

    // Bootstrap the log so the undo has a floor to land on: the fixture's
    // commits are plain git, so without it the first fufu verb is the
    // oldest operation on the log.
    let repo = fx.repo();
    ff_core::ops::reconcile(&repo, NOW - 10).unwrap();

    let (_c0, c1, _c2, c3) = base(&fx);
    let mid_before = fx.git(&["rev-parse", "mid"]).trim().to_string();

    let (edit_outcome, _ctx) = edit_call(&fx, &c1, NOW);
    let edit_report = opened(edit_outcome);
    fx.write("c1.txt", "c1 edited\n");

    let (outcome, _ctx) = done_call(&fx, false, NOW + 100);
    let _ = landed(outcome);

    let opts = ff_core::RewindOptions {
        force: false,
        now: Some(NOW + 200),
        argv: vec!["ff".into(), "undo".into()],
    };
    ff_core::undo(&fx.repo(), &opts, &prov()).unwrap();

    assert_eq!(
        fx.git(&["rev-parse", "--abbrev-ref", "HEAD"]).trim(),
        edit_report.session,
        "HEAD must be back on the session branch"
    );
    assert_eq!(
        ff_core::branchmeta::read(&fx.repo(), &edit_report.session)
            .unwrap()
            .session,
        Some(ff_core::branchmeta::Session {
            onto: "main".into(),
            at: c1,
        }),
        "the session metadata must be restored"
    );
    assert_eq!(fx.git(&["rev-parse", "main"]).trim(), c3);
    assert_eq!(fx.git(&["rev-parse", "mid"]).trim(), mid_before);
    assert_eq!(
        std::fs::read_to_string(fx.path().join("c1.txt")).unwrap(),
        "c1 edited\n",
        "the worktree must hold the session's edit again"
    );
}

/// A rewrite of the anchor in place — absorb amending the session tip — must
/// not block the landing: the anchor's *position* is what lands, and the
/// session branch changed sha, not position. Before the fix this refused
/// with `session/moved` and a false "commits of its own".
#[test]
fn an_absorb_inside_a_session_still_lands() {
    let fx = Fixture::new();
    ident(&fx);
    let (_c0, c1, _c2, _c3) = base(&fx);
    let commits_before = fx.git(&["rev-list", "--count", "main"]).trim().to_string();

    let (edit_outcome, _ctx) = edit_call(&fx, &c1, NOW);
    let _ = opened(edit_outcome);

    fx.write("c1.txt", "c1 absorbed\n");
    let repo = fx.repo();
    let (_outcome, _ctx) = ff_core::absorb::absorb(
        &repo,
        None,
        Vec::new(),
        &prov(),
        Some(NOW + 50),
        vec!["ff".into(), "absorb".into()],
    )
    .unwrap();

    let (outcome, _ctx) = done_call(&fx, false, NOW + 100);
    let report = landed(outcome);

    assert_eq!(
        std::fs::read_to_string(fx.path().join("c1.txt")).unwrap(),
        "c1 absorbed\n",
        "the absorbed content must land at c1's position on main"
    );
    assert_eq!(
        fx.git(&["rev-list", "--count", "main"]).trim(),
        commits_before,
        "main must have the same number of commits"
    );
    assert_eq!(report.replayed, 2, "c2 and c3 must be replayed");
    assert_ne!(
        fx.git(&["rev-parse", "mid"]).trim(),
        c1,
        "mid must have moved"
    );
    assert_eq!(anon_count(&fx), 0, "the session branch must be gone");
}

/// The control for the first-parent test: a commit genuinely landed on the
/// session still refuses, and the exits it names must work. Without this
/// test, deleting the refusal outright would still let the absorb test pass.
#[test]
fn a_commit_on_the_session_still_refuses() {
    let fx = Fixture::new();
    ident(&fx);
    let (_c0, c1, _c2, _c3) = base(&fx);

    let (edit_outcome, _ctx) = edit_call(&fx, &c1, NOW);
    let _ = opened(edit_outcome);

    fx.git(&["commit", "--allow-empty", "-q", "-m", "mine"]);

    let err = done_err(&fx, false, NOW + 100);
    assert_eq!(err.id(), "session/moved", "{err}");
    assert!(
        err.exits().iter().any(|e| e.contains("ff undo")),
        "the refusal must name the exit that works: {err}"
    );
    assert!(
        err.exits().iter().all(|e| !e.contains("ff restack")),
        "ff restack is not an exit — it is what caused this: {err}"
    );
}

/// The other rewrite-in-place verb: a reword of the session's own commit
/// must land too, carrying the new message.
#[test]
fn a_described_session_still_lands() {
    let fx = Fixture::new();
    ident(&fx);
    let (_c0, c1, _c2, _c3) = base(&fx);

    let (edit_outcome, _ctx) = edit_call(&fx, &c1, NOW);
    let _ = opened(edit_outcome);

    let repo = fx.repo();
    let target = gix::ObjectId::from_hex(c1.as_bytes()).unwrap();
    let (_outcome, _ctx) = ff_core::describe::reword(
        &repo,
        target,
        "c1 reworded".into(),
        &prov(),
        Some(NOW + 50),
        vec!["ff".into(), "describe".into()],
    )
    .unwrap();

    let (outcome, _ctx) = done_call(&fx, false, NOW + 100);
    let report = landed(outcome);

    assert_eq!(
        fx.git(&["log", "--format=%s", "main"])
            .lines()
            .collect::<Vec<_>>(),
        vec!["c3", "c2", "c1 reworded", "c0"],
        "main must carry the new message at c1's position"
    );
    assert_eq!(report.replayed, 2, "c2 and c3 must be replayed");
    assert_eq!(anon_count(&fx), 0, "the session branch must be gone");
}

#[test]
fn the_session_branch_is_not_left_behind() {
    let fx = Fixture::new();
    ident(&fx);
    let (_c0, c1, _c2, _c3) = base(&fx);

    let (edit_outcome, _ctx) = edit_call(&fx, &c1, NOW);
    let edit_report = opened(edit_outcome);

    fx.write("c1.txt", "c1 edited\n");
    let (outcome, _ctx) = done_call(&fx, false, NOW + 100);
    let report = landed(outcome);
    assert_eq!(report.session, edit_report.session);

    assert_eq!(anon_count(&fx), 0, "refs/heads/ff/* must hold no leftover");
    assert_eq!(
        ff_core::branchmeta::read(&fx.repo(), &report.session)
            .unwrap()
            .session,
        None,
        "the session branch's metadata must no longer report a session"
    );
}

/// A session that changes both the content and the message of the edited
/// commit is one act: both halves must land. The two halves have separate
/// tests — `a_described_session_still_lands` (message only) and
/// `an_absorb_inside_a_session_still_lands` (tree only) — but before the
/// fix the tree change carried no message, so the reword was dropped
/// silently here.
#[test]
fn a_session_that_rewords_and_edits_lands_both() {
    let fx = Fixture::new();
    ident(&fx);
    let (_c0, c1, _c2, _c3) = base(&fx);
    let commits_before = fx.git(&["rev-list", "--count", "main"]).trim().to_string();

    let (edit_outcome, _ctx) = edit_call(&fx, &c1, NOW);
    let _ = opened(edit_outcome);

    let repo = fx.repo();
    let target = gix::ObjectId::from_hex(c1.as_bytes()).unwrap();
    let (_outcome, _ctx) = ff_core::describe::reword(
        &repo,
        target,
        "c1 reworded".into(),
        &prov(),
        Some(NOW + 50),
        vec!["ff".into(), "describe".into()],
    )
    .unwrap();

    fx.write("c1.txt", "c1 edited\n");
    let (outcome, _ctx) = done_call(&fx, false, NOW + 100);
    let report = landed(outcome);

    assert_eq!(
        fx.git(&["log", "--format=%s", "main"])
            .lines()
            .collect::<Vec<_>>(),
        vec!["c3", "c2", "c1 reworded", "c0"],
        "main must carry the new message at c1's position"
    );
    assert_eq!(
        fx.git(&["show", "main~2:c1.txt"]).trim(),
        "c1 edited",
        "c1's content must be the edited one at its position on main"
    );
    assert_eq!(
        fx.git(&["rev-list", "--count", "main"]).trim(),
        commits_before,
        "main must have the same number of commits"
    );
    assert_eq!(report.replayed, 2, "c2 and c3 must be replayed");
    assert_eq!(anon_count(&fx), 0, "the session branch must be gone");
}
