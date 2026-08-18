//! Differential contract for `done::done`: ending `ff edit`'s session must
//! produce byte-identical commits — full sha for full sha — to git's own
//! `commit --amend` plus `rebase --onto --update-refs`, and the landing must
//! leave a worktree and an index that plain git reads as clean.

use ff_testsupport::Fixture;

const NOW: i64 = 1_799_999_999;

/// fufu's side must be configured to the same identity real git will use
/// for the oracle's amend and rebase (`GIT_COMMITTER_NAME`/
/// `GIT_COMMITTER_EMAIL` from `Fixture`'s hermetic env, which beats repo
/// config) — otherwise the objects can never match byte-for-byte.
fn ident(fx: &Fixture) {
    fx.set_config("user.name", "Fixture Committer");
    fx.set_config("user.email", "committer@fixture.test");
}

fn prov() -> ff_core::Provenance {
    ff_core::Provenance::new("pre", Some("ff done".into()))
}

fn edit_call(fx: &Fixture, rev: &str) -> (ff_core::EditOutcome, ff_core::ops::VerbContext) {
    ff_core::edit::edit(
        &fx.repo(),
        rev,
        &prov(),
        Some(NOW),
        vec!["ff".into(), "edit".into()],
    )
    .unwrap()
}

fn done_call(fx: &Fixture) -> (ff_core::DoneOutcome, ff_core::ops::VerbContext) {
    ff_core::done::done(
        &fx.repo(),
        false,
        &prov(),
        Some(NOW),
        vec!["ff".into(), "done".into()],
    )
    .unwrap()
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

/// A base commit — the session's commit must not be the root, or the
/// oracle's detach/amend pair has no parent to replay onto — then four
/// commits `c1..c4` on `main`, `mid` at `c2`. Nobody switches branches, so
/// `main` stays HEAD throughout.
fn stack(fx: &Fixture) -> [String; 5] {
    fx.write("f0.txt", "base\n");
    let c0 = fx.commit("base");
    fx.write("f1.txt", "one\n");
    let c1 = fx.commit("c1");
    fx.write("f2.txt", "two\n");
    let c2 = fx.commit("c2");
    fx.git(&["branch", "mid"]);
    fx.write("f3.txt", "three\n");
    let c3 = fx.commit("c3");
    fx.write("f4.txt", "four\n");
    let c4 = fx.commit("c4");
    [c0, c1, c2, c3, c4]
}

/// Run the git oracle: detach on the session's commit, apply the session's
/// edit, amend the commit with the worktree's content, and replay what
/// waited ahead onto the amend. The committer date is pinned on both
/// commit-writing steps — the amend and the rebase — and
/// `--amend --no-edit` preserves the message byte-for-byte, where a
/// `commit-tree` pipeline would append a newline.
fn git_oracle_done(fx: &Fixture, target: &str, path: &str, content: &str, now: i64) {
    fx.git(&["checkout", "-q", "--detach", target]);
    fx.write(path, content);
    fx.git_env_in(
        &fx.path(),
        &["commit", "-q", "--amend", "--no-edit", "-a"],
        &[("GIT_COMMITTER_DATE", &format!("@{now} +0000"))],
    );
    let amended = fx.git(&["rev-parse", "HEAD"]).trim().to_string();
    fx.git_env_in(
        &fx.path(),
        &[
            "rebase",
            "-q",
            "--update-refs",
            "--onto",
            &amended,
            target,
            "main",
        ],
        &[("GIT_COMMITTER_DATE", &format!("@{now} +0000"))],
    );
}

/// The tip's oracle: the same detach/edit/amend, with `main` moved to the
/// amend and no rebase — nothing waited ahead.
fn git_oracle_tip(fx: &Fixture, target: &str, path: &str, content: &str, now: i64) {
    fx.git(&["checkout", "-q", "--detach", target]);
    fx.write(path, content);
    fx.git_env_in(
        &fx.path(),
        &["commit", "-q", "--amend", "--no-edit", "-a"],
        &[("GIT_COMMITTER_DATE", &format!("@{now} +0000"))],
    );
    fx.git(&["branch", "-f", "main", "HEAD"]);
}

/// The user-visible history over `main` and `mid`.
fn log_of(fx: &Fixture) -> String {
    fx.git(&[
        "log",
        "--format=%H|%T|%an|%ad|%cn|%cd|%s",
        "--date=raw",
        "main",
        "mid",
    ])
}

#[test]
fn done_mid_stack_matches_git() {
    let fx_ff = Fixture::new();
    ident(&fx_ff);
    let [_c0, c1_ff, _c2_ff, _c3_ff, _c4_ff] = stack(&fx_ff);

    let fx_git = Fixture::new();
    ident(&fx_git);
    let [_c0, c1_git, _c2_git, _c3_git, _c4_git] = stack(&fx_git);

    assert_eq!(c1_ff, c1_git, "setup must be lockstep before any rewrite");

    // The session edits f1.txt, which only c1 introduced: no descendant
    // sees it, so the replay onto the amend is conflict-free.
    let (outcome, _ctx) = edit_call(&fx_ff, &c1_ff);
    match outcome {
        ff_core::EditOutcome::Opened(_) => {}
        other => panic!("a session must open, got {other:?}"),
    }
    fx_ff.write("f1.txt", "one-prime\n");

    let (outcome, _ctx) = done_call(&fx_ff);
    let report = match outcome {
        ff_core::DoneOutcome::Done(report) => report,
        other => panic!("the session must land, got {other:?}"),
    };
    git_oracle_done(&fx_git, &c1_git, "f1.txt", "one-prime\n", NOW);

    for branch in ["main", "mid"] {
        let ff_sha = fx_ff.git(&["rev-parse", branch]).trim().to_string();
        let git_sha = fx_git.git(&["rev-parse", branch]).trim().to_string();
        assert_eq!(ff_sha, git_sha, "{branch} diverged between fufu and git");
    }

    let log_ff = log_of(&fx_ff);
    let log_git = log_of(&fx_git);
    assert_eq!(
        log_ff, log_git,
        "every rewritten commit must be byte-identical\nfufu:\n{log_ff}\ngit:\n{log_git}"
    );

    let new_c1 = fx_ff.git(&["rev-parse", "main~3"]).trim().to_string();
    assert_eq!(report.editing, c1_ff);
    assert_eq!(report.amended.as_deref(), Some(new_c1.as_str()));
    assert_eq!(report.replayed, 3);
    assert_eq!(report.moved, vec!["mid".to_string()]);
}

#[test]
fn done_overlapping_edit_matches_git() {
    let fx_ff = Fixture::new();
    ident(&fx_ff);
    fx_ff.write("f0.txt", "base\n");
    let _c0_ff = fx_ff.commit("base");
    // c1 introduces `doc.txt`; c2 rewrites its tail, so the session's head
    // edit and c2's tail edit meet only in the replay's three-way merge.
    fx_ff.write("doc.txt", "l1\nl2\nl3\n");
    let c1_ff = fx_ff.commit("c1");
    fx_ff.write("doc.txt", "l1\nl2\nl3\ntail\n");
    let _c2_ff = fx_ff.commit("c2");
    fx_ff.git(&["branch", "mid"]);
    fx_ff.write("f3.txt", "three\n");
    let _c3_ff = fx_ff.commit("c3");
    fx_ff.write("f4.txt", "four\n");
    let _c4_ff = fx_ff.commit("c4");

    let fx_git = Fixture::new();
    ident(&fx_git);
    fx_git.write("f0.txt", "base\n");
    let _c0_git = fx_git.commit("base");
    fx_git.write("doc.txt", "l1\nl2\nl3\n");
    let c1_git = fx_git.commit("c1");
    fx_git.write("doc.txt", "l1\nl2\nl3\ntail\n");
    let _c2_git = fx_git.commit("c2");
    fx_git.git(&["branch", "mid"]);
    fx_git.write("f3.txt", "three\n");
    let _c3_git = fx_git.commit("c3");
    fx_git.write("f4.txt", "four\n");
    let _c4_git = fx_git.commit("c4");

    assert_eq!(c1_ff, c1_git, "setup must be lockstep before any rewrite");

    // c2 edited the file's tail; the session edits its head. The session
    // content is anchored at c1, so it holds no `tail` — replaying c2 over
    // the amend must three-way-merge both edits, because the trivial path,
    // where c2's tree is reused untouched, would drop the session's head
    // edit and the shas could never match.
    let (outcome, _ctx) = edit_call(&fx_ff, &c1_ff);
    match outcome {
        ff_core::EditOutcome::Opened(_) => {}
        other => panic!("a session must open, got {other:?}"),
    }
    fx_ff.write("doc.txt", "head\nl2\nl3\n");

    let (outcome, _ctx) = done_call(&fx_ff);
    let report = match outcome {
        ff_core::DoneOutcome::Done(report) => report,
        other => panic!("the session must land, got {other:?}"),
    };
    git_oracle_done(&fx_git, &c1_git, "doc.txt", "head\nl2\nl3\n", NOW);

    for branch in ["main", "mid"] {
        let ff_sha = fx_ff.git(&["rev-parse", branch]).trim().to_string();
        let git_sha = fx_git.git(&["rev-parse", branch]).trim().to_string();
        assert_eq!(ff_sha, git_sha, "{branch} diverged between fufu and git");
    }

    let log_ff = log_of(&fx_ff);
    let log_git = log_of(&fx_git);
    assert_eq!(
        log_ff, log_git,
        "every rewritten commit must be byte-identical\nfufu:\n{log_ff}\ngit:\n{log_git}"
    );

    // The replay merged rather than fast-forwarded: the final history
    // holds the session's head edit and c2's tail alike.
    assert_eq!(fx_ff.git(&["show", "main:doc.txt"]), "head\nl2\nl3\ntail\n");

    let new_c1 = fx_ff.git(&["rev-parse", "main~3"]).trim().to_string();
    assert_eq!(report.editing, c1_ff);
    assert_eq!(report.amended.as_deref(), Some(new_c1.as_str()));
    assert_eq!(report.replayed, 3);
    assert_eq!(report.moved, vec!["mid".to_string()]);
}

#[test]
fn done_on_the_tip_matches_git() {
    let fx_ff = Fixture::new();
    ident(&fx_ff);
    let [_c0, _c1, _c2, _c3, c4_ff] = stack(&fx_ff);

    let fx_git = Fixture::new();
    ident(&fx_git);
    let [_c0, _c1, _c2, _c3, c4_git] = stack(&fx_git);

    assert_eq!(c4_ff, c4_git, "setup must be lockstep before any rewrite");

    let (outcome, _ctx) = edit_call(&fx_ff, &c4_ff);
    match outcome {
        ff_core::EditOutcome::Opened(_) => {}
        other => panic!("a session must open, got {other:?}"),
    }
    fx_ff.write("f4.txt", "four-prime\n");

    let (outcome, _ctx) = done_call(&fx_ff);
    let report = match outcome {
        ff_core::DoneOutcome::Done(report) => report,
        other => panic!("the session must land, got {other:?}"),
    };
    git_oracle_tip(&fx_git, &c4_git, "f4.txt", "four-prime\n", NOW);

    for branch in ["main", "mid"] {
        let ff_sha = fx_ff.git(&["rev-parse", branch]).trim().to_string();
        let git_sha = fx_git.git(&["rev-parse", branch]).trim().to_string();
        assert_eq!(ff_sha, git_sha, "{branch} diverged between fufu and git");
    }

    let log_ff = log_of(&fx_ff);
    let log_git = log_of(&fx_git);
    assert_eq!(
        log_ff, log_git,
        "every rewritten commit must be byte-identical\nfufu:\n{log_ff}\ngit:\n{log_git}"
    );

    assert_eq!(report.editing, c4_ff);
    assert_eq!(
        report.amended.as_deref(),
        Some(fx_ff.git(&["rev-parse", "main"]).trim()),
        "the amend is the tip itself"
    );
    assert_eq!(report.replayed, 0, "nothing waited ahead of the tip");
    assert!(report.moved.is_empty(), "no branch waits ahead of the tip");
}

#[test]
fn done_leaves_the_worktree_agreeing_with_git() {
    let fx_ff = Fixture::new();
    ident(&fx_ff);
    let [_c0, c1_ff, _c2_ff, _c3_ff, _c4_ff] = stack(&fx_ff);

    let fx_git = Fixture::new();
    ident(&fx_git);
    let [_c0, c1_git, _c2_git, _c3_git, _c4_git] = stack(&fx_git);

    assert_eq!(c1_ff, c1_git, "setup must be lockstep before any rewrite");

    let (outcome, _ctx) = edit_call(&fx_ff, &c1_ff);
    match outcome {
        ff_core::EditOutcome::Opened(_) => {}
        other => panic!("a session must open, got {other:?}"),
    }
    fx_ff.write("f1.txt", "one-prime\n");

    let (outcome, _ctx) = done_call(&fx_ff);
    match outcome {
        ff_core::DoneOutcome::Done(_) => {}
        other => panic!("the session must land, got {other:?}"),
    }
    git_oracle_done(&fx_git, &c1_git, "f1.txt", "one-prime\n", NOW);

    // fufu writes the index itself rather than letting git do it, so
    // git's own status agreeing on both sides is a claim about the index,
    // not a restatement of the tree comparison.
    assert_eq!(
        worktree_files(&fx_ff),
        worktree_files(&fx_git),
        "every worktree file, byte for byte"
    );
    let status_ff = fx_ff.git(&["status", "--porcelain=v2"]);
    let status_git = fx_git.git(&["status", "--porcelain=v2"]);
    assert_eq!(
        status_ff, status_git,
        "git's own status must agree on both sides"
    );
    assert_eq!(status_ff, "", "the landing must be clean");
}

#[test]
fn no_session_branch_survives_the_landing() {
    let fx_ff = Fixture::new();
    ident(&fx_ff);
    let [_c0, c1_ff, _c2_ff, _c3_ff, _c4_ff] = stack(&fx_ff);

    let (outcome, _ctx) = edit_call(&fx_ff, &c1_ff);
    let edit_report = match outcome {
        ff_core::EditOutcome::Opened(report) => report,
        other => panic!("a session must open, got {other:?}"),
    };
    fx_ff.write("f1.txt", "one-prime\n");

    let (outcome, _ctx) = done_call(&fx_ff);
    let report = match outcome {
        ff_core::DoneOutcome::Done(report) => report,
        other => panic!("the session must land, got {other:?}"),
    };
    assert_eq!(report.session, edit_report.session);

    // The oracle never had a session branch; fufu must not leave one.
    assert_eq!(
        fx_ff.git(&["for-each-ref", "--format=%(refname)", "refs/heads/ff/"]),
        "",
        "refs/heads/ff/ must hold no leftover"
    );
}
