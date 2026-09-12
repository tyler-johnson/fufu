//! The park is the open commit. Leaving a dirty branch writes nothing —
//! `refs/fufu/open/<branch>` already names the change — and arriving lays
//! that commit's tree back, replays it onto a moved tip with the same id,
//! holds the branch when the replay conflicts, and drops the id when the tip
//! already holds the change. A pre-flight stash park folds into an open
//! commit on the first arrival. Differential where git has an answer.

use ff_core::{ArrivalReport, ResolveOutcome, RewindOptions, SwitchOptions};
use ff_testsupport::{Fixture, legacy_park};

const NOW: i64 = 1_700_000_000;

fn ident(fx: &Fixture) {
    fx.set_config("user.name", "Park User");
    fx.set_config("user.email", "park@test");
}

fn prov() -> ff_core::Provenance {
    ff_core::Provenance::new("pre", Some("ff test".into()))
}

fn switch_to(fx: &Fixture, target: &str, now: i64) -> ff_core::SwitchReport {
    try_switch(fx, target, now).unwrap()
}

fn try_switch(fx: &Fixture, target: &str, now: i64) -> ff_core::Result<ff_core::SwitchReport> {
    let repo = fx.repo();
    ff_core::switch(
        &repo,
        &SwitchOptions {
            target: Some(target.into()),
            now: Some(now),
            argv: vec!["ff".into(), "switch".into(), target.into()],
            ..Default::default()
        },
        &prov(),
    )
    .map(|(report, _)| report)
}

fn undo(fx: &Fixture, now: i64) -> ff_core::RewindReport {
    let repo = fx.repo();
    ff_core::undo(
        &repo,
        &RewindOptions {
            force: false,
            now: Some(now),
            argv: vec!["ff".into(), "undo".into()],
        },
        &prov(),
    )
    .unwrap()
    .0
}

fn redo(fx: &Fixture, now: i64) -> ff_core::RewindReport {
    let repo = fx.repo();
    ff_core::redo(
        &repo,
        &RewindOptions {
            force: false,
            now: Some(now),
            argv: vec!["ff".into(), "redo".into()],
        },
        &prov(),
    )
    .unwrap()
    .0
}

fn resolve(fx: &Fixture, abandon: bool, now: i64) -> ff_core::Result<ResolveOutcome> {
    let repo = fx.repo();
    ff_core::resolve::resolve(
        &repo,
        abandon,
        &prov(),
        Some(now),
        vec!["ff".into(), "resolve".into()],
    )
    .map(|(outcome, _)| outcome)
}

fn open_ref(fx: &Fixture, branch: &str) -> Option<String> {
    let out = fx.try_git(&[
        "rev-parse",
        "--verify",
        "-q",
        &format!("refs/fufu/open/{branch}"),
    ]);
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
}

fn parked_ref(fx: &Fixture, branch: &str) -> Option<String> {
    let out = fx.try_git(&[
        "rev-parse",
        "--verify",
        "-q",
        &format!("refs/fufu/parked/{branch}"),
    ]);
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
}

fn header(fx: &Fixture, sha: &str) -> Option<String> {
    let raw = fx.git(&["cat-file", "commit", sha]);
    raw.lines()
        .take_while(|l| !l.is_empty())
        .find_map(|l| l.strip_prefix("change-id ").map(str::to_string))
}

fn tree_of(fx: &Fixture, rev: &str) -> String {
    fx.git(&["rev-parse", &format!("{rev}^{{tree}}")])
        .trim()
        .to_string()
}

fn status_paths(fx: &Fixture) -> Vec<String> {
    let mut out: Vec<String> = fx
        .git(&["status", "--porcelain=v2"])
        .lines()
        .map(|l| l.rsplit(' ').next().unwrap().to_string())
        .collect();
    out.sort();
    out
}

/// The newest operation's record, read through the public reader.
fn tip_record(fx: &Fixture) -> ff_core::ops::OpRecord {
    let repo = fx.repo();
    let log = ff_core::ops::OpLog::open(&repo).unwrap();
    let op = log.get(log.tip().unwrap().unwrap()).unwrap();
    op.record()
        .unwrap()
        .cloned()
        .expect("a verb op has a record")
}

/// The commits the newest operation's commit names as parents.
fn tip_parents(fx: &Fixture) -> Vec<String> {
    let repo = fx.repo();
    let log = ff_core::ops::OpLog::open(&repo).unwrap();
    let id = log.tip().unwrap().unwrap().object_id().to_string();
    fx.git(&["log", "-1", "--format=%P", &id])
        .split_whitespace()
        .map(str::to_string)
        .collect()
}

/// `@`'s sha on `main`, as the log reports it.
fn at_sha(fx: &Fixture) -> Option<String> {
    let repo = fx.repo();
    ff_core::open::of_head(&repo)
        .unwrap()
        .map(|id| id.to_string())
}

/// `main` and `other` over one file, with an identity, the log floored.
fn two_branches() -> Fixture {
    let fx = Fixture::new();
    fx.write("a.txt", "base\n");
    fx.write("b.txt", "b\n");
    fx.commit("init");
    fx.git(&["branch", "other"]);
    ident(&fx);
    let repo = fx.repo();
    ff_core::ops::reconcile(&repo, NOW - 10).unwrap();
    fx
}

/// Advance `branch` with plain git plumbing to a commit carrying `tree`
/// over its tip, without touching HEAD or the worktree.
fn advance(fx: &Fixture, branch: &str, path: &str, content: &str) -> String {
    let blob = {
        let tmp = fx.path().join(".ff-advance-blob");
        std::fs::write(&tmp, content).unwrap();
        let sha = fx
            .git(&["hash-object", "-w", tmp.to_str().unwrap()])
            .trim()
            .to_string();
        std::fs::remove_file(tmp).unwrap();
        sha
    };
    let tip = fx.git(&["rev-parse", branch]).trim().to_string();
    let base_tree = tree_of(fx, &tip);
    let index = fx.path().join(".ff-advance-index");
    let env = [("GIT_INDEX_FILE", index.to_str().unwrap())];
    fx.git_env_in(&fx.path(), &["read-tree", &base_tree], &env);
    fx.git_env_in(
        &fx.path(),
        &[
            "update-index",
            "--add",
            "--cacheinfo",
            &format!("100644,{blob},{path}"),
        ],
        &env,
    );
    let tree = fx
        .git_env_in(&fx.path(), &["write-tree"], &env)
        .trim()
        .to_string();
    std::fs::remove_file(index).ok();
    let commit = fx
        .git(&["commit-tree", &tree, "-p", &tip, "-m", "advance"])
        .trim()
        .to_string();
    fx.git(&["update-ref", &format!("refs/heads/{branch}"), &commit]);
    commit
}

#[test]
fn leaving_writes_nothing_and_the_park_is_the_open_commit() {
    let fx = two_branches();
    fx.write("a.txt", "wip\n");
    fx.write("new.txt", "loose\n");
    let repo = fx.repo();
    ff_core::capture(&repo, &prov()).unwrap();
    let at = at_sha(&fx).expect("a dirty tree has an open commit");

    let report = switch_to(&fx, "other", NOW);

    assert_eq!(report.parked.as_deref(), Some(at.as_str()));
    assert_eq!(open_ref(&fx, "main").as_deref(), Some(at.as_str()));
    assert!(
        fx.git(&["stash", "list"]).is_empty(),
        "nothing on refs/stash"
    );
    assert!(
        !fx.try_git(&["rev-parse", "--verify", "-q", "refs/stash"])
            .status
            .success()
    );
    assert!(parked_ref(&fx, "main").is_none());
    assert_eq!(
        fx.git(&["status", "--porcelain=v2"]),
        "",
        "clean on arrival"
    );
    let all = fx.git(&["log", "--all", "--format=%H"]);
    assert!(all.contains(&at), "git log --all shows the open commit");
}

#[test]
fn round_trip_resumes_the_same_open_commit() {
    let fx = two_branches();
    fx.write("a.txt", "staged\n");
    fx.git(&["add", "a.txt"]);
    fx.write("a.txt", "staged then more\n");
    fx.write("new.txt", "loose\n");
    let before_paths = status_paths(&fx);
    let before_bytes = ff_testsupport::capture::git_capture_tree(&fx, &fx.path(), &[]);

    let away = switch_to(&fx, "other", NOW);
    let back = switch_to(&fx, "main", NOW + 10);

    match &back.arrival {
        ArrivalReport::Restored { open, folded, .. } => {
            assert_eq!(Some(open.as_str()), away.parked.as_deref());
            assert!(folded.is_none());
        }
        other => panic!("expected Restored, got {other:?}"),
    }
    assert_eq!(open_ref(&fx, "main"), away.parked, "same sha after return");
    assert_eq!(
        ff_testsupport::capture::git_capture_tree(&fx, &fx.path(), &[]),
        before_bytes,
        "worktree bytes identical"
    );
    // The index is not carried: a staged hunk comes back as an unstaged
    // edit, so status names the same paths in different columns.
    assert_eq!(status_paths(&fx), before_paths);
    let status = fx.git(&["status", "--porcelain=v2"]);
    assert!(
        status
            .lines()
            .any(|l| l.starts_with("1 .M ") && l.ends_with("a.txt")),
        "a.txt is an unstaged edit now: {status}"
    );
}

#[test]
fn a_moved_tip_replays_the_open_commit_with_the_same_id() {
    let fx = two_branches();
    fx.write("a.txt", "wip\n");
    let away = switch_to(&fx, "other", NOW);
    let old = away.parked.unwrap();
    let old_id = header(&fx, &old).expect("the open commit wears an id");

    // main advances underneath the park, touching the other file.
    let new_tip = advance(&fx, "main", "b.txt", "advanced\n");

    let back = switch_to(&fx, "main", NOW + 10);
    let new = match &back.arrival {
        ArrivalReport::Restored { open, .. } => open.clone(),
        other => panic!("expected Restored, got {other:?}"),
    };
    assert_ne!(new, old, "replayed onto the new tip: a new sha");
    assert_eq!(header(&fx, &new).as_deref(), Some(old_id.as_str()));
    assert_eq!(open_ref(&fx, "main").as_deref(), Some(new.as_str()));
    assert_eq!(
        fx.git(&["log", "-1", "--format=%P", &new]).trim(),
        new_tip,
        "its parent is the new tip"
    );
    assert_eq!(
        std::fs::read_to_string(fx.path().join("a.txt")).unwrap(),
        "wip\n"
    );
    assert_eq!(
        std::fs::read_to_string(fx.path().join("b.txt")).unwrap(),
        "advanced\n"
    );
    // The three-way merge, as git would write it.
    let merged = fx
        .git(&[
            "merge-tree",
            "--write-tree",
            "--merge-base",
            &format!("{old}^"),
            &new_tip,
            &old,
        ])
        .trim()
        .to_string();
    assert_eq!(tree_of(&fx, &new), merged);
    // One open commit for the branch on the log: the op pins the replay,
    // and the ref names it.
    let opens: Vec<String> = fx
        .git(&["log", "--all", "--format=%H %P"])
        .lines()
        .filter(|l| l.ends_with(&format!(" {new_tip}")))
        .filter(|l| header(&fx, l.split(' ').next().unwrap()).as_deref() == Some(old_id.as_str()))
        .map(|l| l.split(' ').next().unwrap().to_string())
        .collect();
    assert_eq!(
        opens,
        vec![new.clone()],
        "exactly one open commit over the tip"
    );
    assert!(
        tip_parents(&fx).contains(&old),
        "the switch pins the commit it replayed from"
    );
}

#[test]
fn a_conflicting_arrival_holds_the_branch() {
    let fx = two_branches();
    fx.write("a.txt", "wip\n");
    let repo = fx.repo();
    ff_core::describe::set_pending(
        &repo,
        Some("the plan".into()),
        &prov(),
        Some(NOW - 5),
        Vec::new(),
    )
    .unwrap();
    let away = switch_to(&fx, "other", NOW);
    let open = away.parked.unwrap();
    let id = header(&fx, &open).unwrap();
    let new_tip = advance(&fx, "main", "a.txt", "conflicting\n");

    let back = switch_to(&fx, "main", NOW + 10);
    match &back.arrival {
        ArrivalReport::Held {
            open: held, paths, ..
        } => {
            assert_eq!(held, &open);
            assert_eq!(paths, &vec!["a.txt".to_string()]);
        }
        other => panic!("expected Held, got {other:?}"),
    }
    assert_eq!(fx.git(&["symbolic-ref", "HEAD"]).trim(), "refs/heads/main");
    assert_eq!(fx.git(&["rev-parse", "HEAD"]).trim(), new_tip);
    assert_eq!(fx.git(&["status", "--porcelain=v2"]), "", "tree clean");
    assert_eq!(
        ff_core::held::of(&fx.repo(), "main")
            .unwrap()
            .map(|h| h.intent),
        Some(ff_core::held::Intent::Arrive {
            branch: "main".into(),
            open: open.clone(),
        })
    );
    assert!(open_ref(&fx, "main").is_none(), "the open ref is gone");
    let meta = ff_core::branchmeta::read(&fx.repo(), "main").unwrap();
    assert!(
        meta.change_id.is_none(),
        "the id travels with the held commit"
    );
    assert!(meta.pending_description.is_none());
    fx.git(&["cat-file", "-e", &open]);
    assert!(tip_parents(&fx).contains(&open), "pinned by the op");
    let record = tip_record(&fx);
    let held = record.held.expect("the hold is recorded");
    assert_eq!(held.branch, "main");
    assert!(held.old.is_none() && held.new.is_some());
    let cid = record.change_id.expect("the id drop is recorded");
    assert_eq!(cid.old.as_deref(), Some(id.as_str()));
    assert!(cid.new.is_none());
    let desc = record
        .description
        .expect("the description drop is recorded");
    assert_eq!(desc.old.as_deref(), Some("the plan"));
    assert!(desc.new.is_none());
}

#[test]
fn a_tip_that_already_holds_the_change_lands_it() {
    let fx = two_branches();
    fx.write("a.txt", "wip\n");
    let away = switch_to(&fx, "other", NOW);
    let open = away.parked.unwrap();
    assert!(
        ff_core::branchmeta::read(&fx.repo(), "main")
            .unwrap()
            .change_id
            .is_some()
    );
    advance(&fx, "main", "a.txt", "wip\n");

    let back = switch_to(&fx, "main", NOW + 10);
    match &back.arrival {
        ArrivalReport::Landed { open: landed, .. } => assert_eq!(landed, &open),
        other => panic!("expected Landed, got {other:?}"),
    }
    assert!(open_ref(&fx, "main").is_none());
    assert!(
        ff_core::branchmeta::read(&fx.repo(), "main")
            .unwrap()
            .change_id
            .is_none(),
        "the id goes with the landed change"
    );
    assert_eq!(fx.git(&["status", "--porcelain=v2"]), "");
}

#[test]
fn a_legacy_park_folds_into_the_open_commit_on_arrival() {
    let fx = two_branches();
    fx.write("a.txt", "wip\n");
    fx.write("new.txt", "loose\n");
    // The pre-flight shape, on a branch whose metadata has no id.
    let stash = legacy_park(&fx, "main");
    assert!(
        ff_core::branchmeta::read(&fx.repo(), "main")
            .unwrap()
            .change_id
            .is_none()
    );
    switch_to(&fx, "other", NOW);

    let back = switch_to(&fx, "main", NOW + 10);
    let open = match &back.arrival {
        ArrivalReport::Restored {
            open,
            folded,
            files,
        } => {
            assert_eq!(folded.as_deref(), Some(stash.as_str()));
            assert_eq!(files, &vec!["a.txt".to_string(), "new.txt".to_string()]);
            open.clone()
        }
        other => panic!("expected Restored, got {other:?}"),
    };
    assert!(fx.git(&["stash", "list"]).is_empty(), "the entry is spent");
    assert!(parked_ref(&fx, "main").is_none());
    assert_eq!(open_ref(&fx, "main").as_deref(), Some(open.as_str()));
    assert_eq!(
        std::fs::read_to_string(fx.path().join("a.txt")).unwrap(),
        "wip\n"
    );
    assert_eq!(
        std::fs::read_to_string(fx.path().join("new.txt")).unwrap(),
        "loose\n"
    );
    let record = tip_record(&fx);
    assert!(
        record.stash.iter().any(|e| matches!(
            e,
            ff_core::ops::StashEffect::Drop { branch, stash: s } if branch == "main" && *s == stash
        )),
        "the drop is recorded: {:?}",
        record.stash
    );
    let meta = ff_core::branchmeta::read(&fx.repo(), "main").unwrap();
    let minted = meta.change_id.expect("a legacy park with no id gets one");
    assert_eq!(header(&fx, &open).as_deref(), Some(minted.as_str()));
    let cid = record.change_id.expect("the mint is recorded");
    assert!(cid.old.is_none());
    assert_eq!(cid.new.as_deref(), Some(minted.as_str()));
}

#[test]
fn undo_and_redo_of_a_switch_resync_both_open_refs() {
    let fx = two_branches();
    // A park on other, made earlier.
    switch_to(&fx, "other", NOW - 5);
    fx.write("b.txt", "other's wip\n");
    switch_to(&fx, "main", NOW - 3);
    let other_open = open_ref(&fx, "other").expect("other is parked");
    fx.write("a.txt", "main's wip\n");

    let report = switch_to(&fx, "other", NOW);
    let main_open = report.parked.clone().unwrap();
    assert_eq!(
        open_ref(&fx, "other").as_deref(),
        Some(other_open.as_str()),
        "other's park resumed: the same commit, open now"
    );
    assert_eq!(open_ref(&fx, "main").as_deref(), Some(main_open.as_str()));

    undo(&fx, NOW + 10);
    assert_eq!(fx.git(&["symbolic-ref", "HEAD"]).trim(), "refs/heads/main");
    assert_eq!(
        open_ref(&fx, "main").as_deref(),
        Some(main_open.as_str()),
        "the origin's open ref is back"
    );
    assert_eq!(
        open_ref(&fx, "other").as_deref(),
        Some(other_open.as_str()),
        "the destination's park is back"
    );
    assert_eq!(
        std::fs::read_to_string(fx.path().join("a.txt")).unwrap(),
        "main's wip\n"
    );

    redo(&fx, NOW + 20);
    assert_eq!(fx.git(&["symbolic-ref", "HEAD"]).trim(), "refs/heads/other");
    assert_eq!(open_ref(&fx, "main").as_deref(), Some(main_open.as_str()));
    assert_eq!(
        open_ref(&fx, "other").as_deref(),
        Some(other_open.as_str()),
        "the resumed change is open on other again"
    );
    assert_eq!(
        std::fs::read_to_string(fx.path().join("b.txt")).unwrap(),
        "other's wip\n"
    );
}

#[test]
fn a_dirty_tree_with_no_identity_refuses_to_leave() {
    let fx = Fixture::new();
    fx.write("a.txt", "base\n");
    fx.commit("init");
    fx.git(&["branch", "other"]);
    // No user.name: captures state `none`, so nothing can stand as the park.
    fx.write("a.txt", "wip\n");
    let head_before = fx.git(&["symbolic-ref", "HEAD"]);
    let status_before = fx.git(&["status", "--porcelain=v2"]);
    let refs_before = fx.git(&[
        "for-each-ref",
        "--format=%(refname) %(objectname)",
        "refs/heads",
    ]);

    let err = try_switch(&fx, "other", NOW).unwrap_err();
    assert_eq!(
        err.to_string(),
        "the open change on main has no commit to stand as its park: set user.name and \
         user.email so captures can write one, then switch again"
    );
    assert_eq!(
        fx.git(&["symbolic-ref", "HEAD"]),
        head_before,
        "HEAD unmoved"
    );
    assert_eq!(fx.git(&["status", "--porcelain=v2"]), status_before);
    assert_eq!(
        fx.git(&[
            "for-each-ref",
            "--format=%(refname) %(objectname)",
            "refs/heads"
        ]),
        refs_before
    );
}

/// A held arrival on `main`: the park and the description made with an
/// identity, `main` advanced under it in conflict. Returns the held commit.
fn held_arrival() -> (Fixture, String) {
    let fx = two_branches();
    fx.write("a.txt", "wip\n");
    let repo = fx.repo();
    ff_core::describe::set_pending(
        &repo,
        Some("the plan".into()),
        &prov(),
        Some(NOW - 5),
        Vec::new(),
    )
    .unwrap();
    let away = switch_to(&fx, "other", NOW);
    let open = away.parked.unwrap();
    advance(&fx, "main", "a.txt", "conflicting\n");
    let back = switch_to(&fx, "main", NOW + 10);
    assert!(matches!(back.arrival, ArrivalReport::Held { .. }));
    (fx, open)
}

#[test]
fn resolve_lays_a_held_arrival_into_the_open_change() {
    let (fx, open) = held_arrival();
    let id = header(&fx, &open).unwrap();

    let outcome = resolve(&fx, false, NOW + 20).unwrap();
    let report = match outcome {
        ResolveOutcome::Laid(r) => r,
        other => panic!("expected Laid, got {other:?}"),
    };
    assert_eq!(report.branch, "main");
    assert_eq!(report.held, open);
    assert_eq!(report.paths, vec!["a.txt".to_string()]);
    assert!(report.regions > 0);
    let text = std::fs::read_to_string(fx.path().join("a.txt")).unwrap();
    assert!(
        text.contains("<<<<<<<"),
        "markers are in the working copy: {text}"
    );
    assert!(ff_core::held::of(&fx.repo(), "main").unwrap().is_none());
    let meta = ff_core::branchmeta::read(&fx.repo(), "main").unwrap();
    assert_eq!(
        meta.change_id.as_deref(),
        Some(id.as_str()),
        "the id is back"
    );
    assert_eq!(meta.change_born, Some(NOW - 5));
    assert_eq!(meta.pending_description.as_deref(), Some("the plan"));
    let laid = open_ref(&fx, "main").expect("the marker commit is the open commit");
    assert_eq!(header(&fx, &laid).as_deref(), Some(id.as_str()));
    assert_eq!(
        fx.git(&["log", "-1", "--format=%P", &laid]).trim(),
        fx.git(&["rev-parse", "main"]).trim()
    );
    match &report.arrival {
        ArrivalReport::Restored { open, .. } => assert_eq!(open, &laid),
        other => panic!("{other:?}"),
    }

    // The op is undoable: the hold comes back, the markers go.
    undo(&fx, NOW + 30);
    assert!(ff_core::held::of(&fx.repo(), "main").unwrap().is_some());
    assert_eq!(fx.git(&["status", "--porcelain=v2"]), "");
    assert!(open_ref(&fx, "main").is_none());
    assert!(
        ff_core::branchmeta::read(&fx.repo(), "main")
            .unwrap()
            .change_id
            .is_none()
    );
}

#[test]
fn resolve_abandon_drops_a_held_arrival_and_names_the_commit() {
    let (fx, open) = held_arrival();
    let outcome = resolve(&fx, true, NOW + 20).unwrap();
    let report = match outcome {
        ResolveOutcome::Abandoned(r) => r,
        other => panic!("expected Abandoned, got {other:?}"),
    };
    assert_eq!(report.left.as_deref(), Some(open.as_str()));
    assert_eq!(report.verb, "switch");
    assert!(ff_core::held::of(&fx.repo(), "main").unwrap().is_none());
    fx.git(&["cat-file", "-e", &open]);
    undo(&fx, NOW + 30);
    assert!(
        ff_core::held::of(&fx.repo(), "main").unwrap().is_some(),
        "undo brings the hold back"
    );
}

#[test]
fn resolve_refuses_a_held_arrival_over_an_open_change() {
    let (fx, _open) = held_arrival();
    fx.write("b.txt", "new work\n");
    let err = resolve(&fx, false, NOW + 20).unwrap_err();
    assert_eq!(err.id(), "held/unsupported");
    assert!(
        err.to_string()
            .contains("main has an open change; ff commit it or ff switch away"),
        "{err}"
    );
}

#[test]
fn resolve_resumes_a_held_arrival_whose_tip_moved_clean() {
    let (fx, open) = held_arrival();
    // main moves back to a tip the change applies on: the conflict is gone.
    let parent = fx
        .git(&["rev-parse", &format!("{open}^")])
        .trim()
        .to_string();
    let repo = fx.repo();
    ff_core::ops::reconcile(&repo, NOW + 15).unwrap();
    fx.git(&["update-ref", "refs/heads/main", &parent]);
    fx.git(&["read-tree", "-u", "--reset", "HEAD"]);

    let outcome = resolve(&fx, false, NOW + 20).unwrap();
    let report = match outcome {
        ResolveOutcome::Laid(r) => r,
        other => panic!("expected Laid, got {other:?}"),
    };
    assert_eq!(report.regions, 0);
    assert!(report.paths.is_empty());
    assert_eq!(
        std::fs::read_to_string(fx.path().join("a.txt")).unwrap(),
        "wip\n",
        "the change is simply open again"
    );
    assert!(ff_core::held::of(&fx.repo(), "main").unwrap().is_none());
    assert_eq!(
        open_ref(&fx, "main").as_deref(),
        Some(open.as_str()),
        "the same open commit, on the tip it was made on"
    );
}
