//! Differential contract for `restack::restack`: fufu's replay must produce
//! byte-identical commits to `git rebase --update-refs --onto`, matched
//! ref-for-ref and byte-for-byte down to author and committer identity. The
//! open-change half has no rebase oracle — git refuses to rebase a dirty
//! tree — so it answers to real git's own `status` and worktree contents
//! instead. And a restack of a branch you are not standing on must leave the
//! worktree untouched: refs and objects only.

use ff_core::futures::At;
use ff_core::gix;
use ff_testsupport::Fixture;

const NOW: i64 = 1_799_999_999;

/// fufu's side must be configured to the same identity real git will use for
/// the oracle's rebase (`GIT_COMMITTER_NAME`/`GIT_COMMITTER_EMAIL` from
/// `Fixture`'s hermetic env, which beats repo config) — otherwise the
/// objects can never match byte-for-byte.
fn ident(fx: &Fixture) {
    fx.set_config("user.name", "Fixture Committer");
    fx.set_config("user.email", "committer@fixture.test");
}

fn prov() -> ff_core::Provenance {
    ff_core::Provenance::new("pre", Some("ff restack".into()))
}

fn restack_call(
    fx: &Fixture,
    branch: Option<&str>,
    onto: Option<&str>,
) -> (ff_core::RestackOutcome, ff_core::ops::VerbContext) {
    let repo = fx.repo();
    ff_core::restack::restack(
        &repo,
        branch.map(String::from),
        onto.map(String::from),
        &prov(),
        Some(NOW),
        vec![],
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

/// The newest operation's record, read through the public reader.
fn tip_record(repo: &gix::Repository) -> ff_core::ops::OpRecord {
    let log = ff_core::ops::OpLog::open(repo).unwrap();
    let op = log.get(log.tip().unwrap().unwrap()).unwrap();
    op.record()
        .unwrap()
        .cloned()
        .expect("a verb op has a record")
}

/// The user-visible history over the given refs.
fn log_of(fx: &Fixture, refs: &[&str]) -> String {
    let mut args = vec!["log", "--format=%H|%T|%an|%ad|%cn|%cd|%s", "--date=raw"];
    args.extend_from_slice(refs);
    fx.git(&args)
}

/// Run the git oracle: `--onto <onto> <old_base> <branch>` names the range's
/// floor explicitly instead of letting git infer it, which is what fufu
/// does, and it checks the branch out itself so the test needs no switch
/// beforehand. `--update-refs` is what carries a branch sitting inside the
/// range. `--no-keep-empty` brings the oracle onto fufu's rule: git by
/// default keeps a commit that started empty, and fufu never writes an empty
/// commit — without the flag the two sides would disagree on exactly the
/// case this file now covers. The flag changes nothing at all on a range
/// with no empty commits (measured: byte-identical shas with and without
/// it), which is why every fixture already in this file stays
/// byte-identically green.
fn git_oracle_restack(fx: &Fixture, onto: &str, old_base: &str, branch: &str, now: i64) {
    fx.git_env_in(
        &fx.path(),
        &[
            "rebase",
            "--update-refs",
            "--no-keep-empty",
            "--onto",
            onto,
            old_base,
            branch,
        ],
        &[("GIT_COMMITTER_DATE", &format!("@{now} +0000"))],
    );
}

/// The standard stack, shared by every test that needs it so the two sides
/// of a differential cannot drift apart in setup:
///
/// ```text
/// c0 ─ c1 ─────────── m2        (main)
///       └─ f1 ─ f2 ─ f3         (feature, with `mid` at f2)
/// ```
fn stack(fx: &Fixture) -> [String; 6] {
    fx.write("root.txt", "root\n");
    let c0 = fx.commit("c0");
    fx.write("m.txt", "m\n");
    let c1 = fx.commit("c1");
    fx.git(&["switch", "-q", "-c", "feature"]);
    fx.write("a.txt", "a\n");
    let f1 = fx.commit("f1");
    fx.write("b.txt", "b\n");
    let f2 = fx.commit("f2");
    fx.git(&["branch", "mid"]);
    fx.write("c.txt", "c\n");
    let f3 = fx.commit("f3");
    fx.git(&["switch", "-q", "main"]);
    fx.write("d.txt", "d\n");
    let m2 = fx.commit("m2");
    [c0, c1, f1, f2, f3, m2]
}

/// The standard stack with one commit of each empty kind in the range:
/// `dup` replays to an empty diff over `m2`, and `marker` started empty.
/// Both fufu and the `--no-keep-empty` oracle must drop them — this is the
/// case the differential asserts on full shas:
///
/// ```text
/// c0 ─ c1 ─────────────────── m2            (main; m2 adds shared.txt)
///       └─ f1 ─ dup ─ marker ─ f3           (feature, with `mid` at dup)
/// ```
///
/// `dup` and `m2` write byte-identical `shared.txt` on purpose: differing
/// bytes would make fufu's replay a conflict while git would still drop the
/// commit by patch-id — a failure with nothing to do with the drop.
fn empty_stack(fx: &Fixture) -> [String; 3] {
    fx.write("root.txt", "root\n");
    let _c0 = fx.commit("c0");
    fx.write("m.txt", "m\n");
    let c1 = fx.commit("c1");
    fx.git(&["switch", "-q", "-c", "feature"]);
    fx.write("a.txt", "a\n");
    let _f1 = fx.commit("f1");
    fx.write("shared.txt", "shared\n");
    let dup = fx.commit("dup");
    fx.git(&["branch", "mid"]);
    let marker = fx.commit("marker"); // nothing written: it started empty
    fx.write("c.txt", "c\n");
    let _f3 = fx.commit("f3");
    fx.git(&["switch", "-q", "main"]);
    fx.write("shared.txt", "shared\n");
    let _m2 = fx.commit("m2");
    [c1, dup, marker]
}

#[test]
fn restack_matches_git_rebase() {
    let fx_ff = Fixture::new();
    ident(&fx_ff);
    let [_c0, c1_ff, _f1, f2_ff, _f3, _m2] = stack(&fx_ff);

    let fx_git = Fixture::new();
    ident(&fx_git);
    let [_c0, c1_git, _f1, _f2, _f3, _m2] = stack(&fx_git);

    assert_eq!(c1_ff, c1_git, "setup must be lockstep before any rewrite");

    fx_ff.git(&["switch", "-q", "feature"]);
    let (outcome, _ctx) = restack_call(&fx_ff, Some("feature"), None);
    let report = match outcome {
        ff_core::RestackOutcome::Restacked(r) => r,
        other => panic!("the restack must land, got {other:?}"),
    };

    git_oracle_restack(&fx_git, "main", &c1_git, "feature", NOW);

    // `feature` is the branch fufu was asked to move: it must still land on
    // the exact commit `git rebase --update-refs` produces.
    let ff_sha = fx_ff.git(&["rev-parse", "feature"]).trim().to_string();
    let git_sha = fx_git.git(&["rev-parse", "feature"]).trim().to_string();
    assert_eq!(ff_sha, git_sha, "feature diverged between fufu and git");

    let log_ff = log_of(&fx_ff, &["main", "feature"]);
    let log_git = log_of(&fx_git, &["main", "feature"]);
    assert_eq!(
        log_ff, log_git,
        "every replayed commit must be byte-identical\nfufu:\n{log_ff}\ngit:\n{log_git}"
    );

    // `mid` sits inside the replayed range at f2. `git rebase --update-refs`
    // carries it along with the tip — real git's side moved it, checked
    // above via `git_oracle_restack`'s own `--update-refs` flag and
    // confirmed here. fufu deliberately does not: `ff restack` moves only
    // the branch it was asked to move, so `mid` is left exactly where it
    // stood and reported as diverged rather than carried. This is the one
    // point in the file where fufu's replay is not the same operation as
    // git's, on purpose.
    assert_eq!(
        fx_git.git(&["rev-parse", "mid"]).trim(),
        fx_git.git(&["rev-parse", "feature^"]).trim(),
        "the oracle's own --update-refs must have carried mid, or this test is not proving what \
         its comment claims"
    );
    assert_eq!(
        fx_ff.git(&["rev-parse", "mid"]).trim(),
        f2_ff,
        "fufu must leave mid exactly where it stood"
    );
    assert!(!report.moved.contains(&"mid".to_string()));
    assert!(report.diverged.contains(&"mid".to_string()));

    assert_eq!(report.replayed, 3);
    assert!(!report.fast_forward);
    assert_eq!(
        report.new_tip,
        fx_ff.git(&["rev-parse", "feature"]).trim().to_string()
    );
}

/// A fork with an eight-line file, `feature` editing lines 2 and then 7
/// while `main` edits line 5 — the same file, three commits, disjoint
/// regions. This is the case that proves fufu's replay is doing a real
/// three-way merge, not merely re-parenting commits whose trees never
/// interact.
fn overlap_stack(fx: &Fixture) -> String {
    let base: String = (1..=8).map(|n| format!("{n}\n")).collect();
    fx.write("nums.txt", &base);
    let c0 = fx.commit("c0");

    fx.git(&["switch", "-q", "-c", "feature"]);
    let mut lines: Vec<String> = (1..=8).map(|n| n.to_string()).collect();
    lines[1] = "two-feature".into();
    fx.write("nums.txt", &format!("{}\n", lines.join("\n")));
    let _f1 = fx.commit("f1");
    lines[6] = "seven-feature".into();
    fx.write("nums.txt", &format!("{}\n", lines.join("\n")));
    let _f2 = fx.commit("f2");

    fx.git(&["switch", "-q", "main"]);
    let mut main_lines: Vec<String> = (1..=8).map(|n| n.to_string()).collect();
    main_lines[4] = "five-main".into();
    fx.write("nums.txt", &format!("{}\n", main_lines.join("\n")));
    let _m1 = fx.commit("m1");

    c0
}

#[test]
fn restack_overlapping_edit_matches_git() {
    let fx_ff = Fixture::new();
    ident(&fx_ff);
    let c0_ff = overlap_stack(&fx_ff);

    let fx_git = Fixture::new();
    ident(&fx_git);
    let c0_git = overlap_stack(&fx_git);

    assert_eq!(c0_ff, c0_git, "setup must be lockstep before any rewrite");

    fx_ff.git(&["switch", "-q", "feature"]);
    let (outcome, _ctx) = restack_call(&fx_ff, Some("feature"), None);
    let report = match outcome {
        ff_core::RestackOutcome::Restacked(r) => r,
        other => panic!("the restack must land, got {other:?}"),
    };

    git_oracle_restack(&fx_git, "main", &c0_git, "feature", NOW);

    for branch in ["main", "feature"] {
        let ff_sha = fx_ff.git(&["rev-parse", branch]).trim().to_string();
        let git_sha = fx_git.git(&["rev-parse", branch]).trim().to_string();
        assert_eq!(ff_sha, git_sha, "{branch} diverged between fufu and git");
    }

    let log_ff = log_of(&fx_ff, &["main", "feature"]);
    let log_git = log_of(&fx_git, &["main", "feature"]);
    assert_eq!(
        log_ff, log_git,
        "every replayed commit must be byte-identical\nfufu:\n{log_ff}\ngit:\n{log_git}"
    );

    assert_eq!(report.replayed, 2);
    assert!(!report.fast_forward);
    assert_eq!(
        report.new_tip,
        fx_ff.git(&["rev-parse", "feature"]).trim().to_string()
    );
}

/// `feature` forks from `main` with no commits of its own; `main` then
/// advances. There is nothing to replay, so both git and fufu must simply
/// move the ref.
fn fastforward_stack(fx: &Fixture) -> String {
    fx.write("root.txt", "root\n");
    let c0 = fx.commit("c0");
    fx.git(&["switch", "-q", "-c", "feature"]);
    fx.git(&["switch", "-q", "main"]);
    fx.write("m.txt", "m\n");
    let _m1 = fx.commit("m1");
    c0
}

#[test]
fn restack_fast_forward_matches_git() {
    let fx_ff = Fixture::new();
    ident(&fx_ff);
    let c0_ff = fastforward_stack(&fx_ff);

    let fx_git = Fixture::new();
    ident(&fx_git);
    let c0_git = fastforward_stack(&fx_git);

    assert_eq!(c0_ff, c0_git, "setup must be lockstep before any rewrite");

    let (outcome, _ctx) = restack_call(&fx_ff, Some("feature"), None);
    let report = match outcome {
        ff_core::RestackOutcome::Restacked(r) => r,
        other => panic!("the restack must land, got {other:?}"),
    };

    git_oracle_restack(&fx_git, "main", &c0_git, "feature", NOW);

    let ff_feature = fx_ff.git(&["rev-parse", "feature"]).trim().to_string();
    let ff_main = fx_ff.git(&["rev-parse", "main"]).trim().to_string();
    let git_feature = fx_git.git(&["rev-parse", "feature"]).trim().to_string();
    let git_main = fx_git.git(&["rev-parse", "main"]).trim().to_string();

    assert_eq!(
        ff_feature, ff_main,
        "fufu must fast-forward feature to main"
    );
    assert_eq!(
        git_feature, git_main,
        "git must fast-forward feature to main"
    );
    assert_eq!(ff_feature, git_feature);

    assert!(report.fast_forward);
    assert_eq!(report.replayed, 0);
}

#[test]
fn restack_carries_the_open_change() {
    // No rebase oracle here: git refuses to rebase a dirty tree, so this
    // test answers to real git's own reading of the result instead.
    let fx = Fixture::new();
    ident(&fx);
    let [_c0, _c1, _f1, _f2, _f3, _m2] = stack(&fx);
    fx.git(&["switch", "-q", "feature"]);

    // root.txt predates the fork and no commit in feature's range touches
    // it, so this is an open change with nothing in the replay to fold into.
    fx.write("root.txt", "root-open\n");

    let (outcome, _ctx) = restack_call(&fx, Some("feature"), None);
    let report = match outcome {
        ff_core::RestackOutcome::Restacked(r) => r,
        other => panic!("the restack must land, got {other:?}"),
    };

    assert_eq!(
        std::fs::read_to_string(fx.path().join("root.txt")).unwrap(),
        "root-open\n"
    );
    assert!(fx.path().join("d.txt").exists(), "d.txt is main's m2");

    // The load-bearing assertion: git, not fufu, saying the index and
    // worktree are consistent — exactly one path open, and it is the one
    // the restack could not fold anywhere.
    let status = fx.git(&["status", "--porcelain=v2"]);
    let lines: Vec<&str> = status.lines().collect();
    assert_eq!(
        lines.len(),
        1,
        "expected exactly one changed path: {status}"
    );
    assert!(
        lines[0].ends_with("root.txt"),
        "the changed path must be root.txt: {status}"
    );

    assert!(report.still_open);
    assert!(report.files > 0);
}

#[test]
fn restack_off_branch_writes_no_file() {
    let fx = Fixture::new();
    ident(&fx);
    // `stack` leaves HEAD on main, so this is already "standing elsewhere".
    let [_c0, _c1, _f1, _f2, _f3, _m2] = stack(&fx);
    let feature_before = fx.git(&["rev-parse", "feature"]).trim().to_string();
    let mid_before = fx.git(&["rev-parse", "mid"]).trim().to_string();

    let files_before = worktree_files(&fx);
    let (outcome, _ctx) = restack_call(&fx, Some("feature"), None);
    let report = match outcome {
        ff_core::RestackOutcome::Restacked(r) => r,
        other => panic!("the restack must land, got {other:?}"),
    };
    let files_after = worktree_files(&fx);

    // A restack of a branch you are not standing on is refs and objects
    // only; this is the test that would fail if the worktree transition
    // ever ran unconditionally.
    assert_eq!(files_before, files_after);

    assert_ne!(fx.git(&["rev-parse", "feature"]).trim(), feature_before);
    // `mid` is not the branch restack was asked to move: it is left exactly
    // where it stood, reported as diverged rather than carried.
    assert_eq!(fx.git(&["rev-parse", "mid"]).trim(), mid_before);
    assert!(report.diverged.contains(&"mid".to_string()));
    assert_eq!(report.files, 0);
}

#[test]
fn restack_conflict_holds_and_leaves_the_world_alone() {
    let fx = Fixture::new();
    ident(&fx);
    fx.write("f.txt", "1\n2\n3\n4\n5\n");
    let _c0 = fx.commit("c0");
    fx.git(&["switch", "-q", "-c", "feature"]);
    // Same line, both sides: main's line-3 edit and feature's line-3 edit
    // disagree, so the replay of f1 onto main's new tip conflicts.
    fx.write("f.txt", "1\n2\n3-feature\n4\n5\n");
    let f1 = fx.commit("f1");
    fx.git(&["branch", "mid"]);
    fx.write("f.txt", "1\n2\n3-feature-again\n4\n5\n");
    let _f2 = fx.commit("f2");
    fx.git(&["switch", "-q", "main"]);
    fx.write("f.txt", "1\n2\n3-main\n4\n5\n");
    let _m1 = fx.commit("m1");
    fx.git(&["switch", "-q", "feature"]);

    // Prime the op log: the first fufu call on a fixture bootstraps it from
    // observed state, which would otherwise masquerade as the hold's own
    // capture.
    ff_core::ops::reconcile(&fx.repo(), NOW).unwrap();

    let feature_before = fx.git(&["rev-parse", "feature"]).trim().to_string();
    let mid_before = fx.git(&["rev-parse", "mid"]).trim().to_string();

    let (outcome, _ctx) = restack_call(&fx, Some("feature"), None);
    let report = match outcome {
        ff_core::RestackOutcome::Held(r) => r,
        other => panic!("the conflicting restack must hold, got {other:?}"),
    };

    assert_eq!(report.branch, "feature");
    assert_eq!(
        report.at,
        At::Commit {
            id: f1.clone(),
            subject: "f1".into()
        },
        "the replay stops on the first conflicting commit"
    );
    assert_eq!(
        report.of, 2,
        "both commits were in the stack the report sizes"
    );
    assert_eq!(report.paths, vec!["f.txt".to_string()]);
    assert_eq!(fx.git(&["rev-parse", "feature"]).trim(), feature_before);
    assert_eq!(fx.git(&["rev-parse", "mid"]).trim(), mid_before);

    // `begin_verb` reconciles and captures before every verb runs, and a
    // conflicting restack is necessarily a case where a capture has
    // something to record, so the log legitimately grows by one capture even
    // though the restack itself moved nothing. The hold's own operation is
    // the property that actually holds; the log tip moving is expected, not
    // a failure.
    let record = tip_record(&fx.repo());
    assert_eq!(record.verb, "hold");
    assert!(record.held.is_some_and(|t| t.new.is_some()));
}

#[test]
fn restack_over_an_emptied_commit_matches_git_no_keep_empty() {
    let fx_ff = Fixture::new();
    ident(&fx_ff);
    let [c1_ff, dup_ff, marker_ff] = empty_stack(&fx_ff);

    let fx_git = Fixture::new();
    ident(&fx_git);
    let [c1_git, _dup_git, _marker_git] = empty_stack(&fx_git);

    assert_eq!(c1_ff, c1_git, "setup must be lockstep before any rewrite");

    fx_ff.git(&["switch", "-q", "feature"]);
    let (outcome, _ctx) = restack_call(&fx_ff, Some("feature"), None);
    let report = match outcome {
        ff_core::RestackOutcome::Restacked(r) => r,
        other => panic!("the restack must land, got {other:?}"),
    };

    git_oracle_restack(&fx_git, "main", &c1_git, "feature", NOW);

    // `feature` is the branch fufu was asked to move: it must still land on
    // the exact commit `git rebase --update-refs` produces.
    let ff_sha = fx_ff.git(&["rev-parse", "feature"]).trim().to_string();
    let git_sha = fx_git.git(&["rev-parse", "feature"]).trim().to_string();
    assert_eq!(ff_sha, git_sha, "feature diverged between fufu and git");

    // Both dropped commits are gone from feature's history on the fufu side.
    let feature_history = fx_ff.git(&["rev-list", "feature"]);
    assert!(
        !feature_history.contains(&dup_ff),
        "dup must be gone from feature"
    );
    assert!(
        !feature_history.contains(&marker_ff),
        "marker must be gone from feature"
    );

    let log_ff = log_of(&fx_ff, &["main", "feature"]);
    let log_git = log_of(&fx_git, &["main", "feature"]);
    assert_eq!(
        log_ff, log_git,
        "every surviving commit must be byte-identical\nfufu:\n{log_ff}\ngit:\n{log_git}"
    );

    // `mid` sits on `dup`, the commit both sides drop. `git rebase
    // --update-refs` follows a ref on a dropped commit to the nearest
    // surviving ancestor — real git's side moved it off `dup`, checked below.
    // fufu deliberately does not: `ff restack` moves only the branch it was
    // asked to move, so `mid` is left exactly where it stood, still pointing
    // at the original (now unreachable from `feature`) `dup` commit, and
    // reported as diverged rather than carried.
    assert_ne!(
        fx_git.git(&["rev-parse", "mid"]).trim(),
        dup_ff,
        "the oracle's own --update-refs must have moved mid off the dropped commit, or this \
         test is not proving what its comment claims"
    );
    assert_eq!(
        fx_ff.git(&["rev-parse", "mid"]).trim(),
        dup_ff,
        "fufu must leave mid exactly where it stood, pointing at the dropped commit"
    );
    assert!(!report.moved.contains(&"mid".to_string()));
    assert!(report.diverged.contains(&"mid".to_string()));

    assert_eq!(
        report.replayed, 2,
        "f1 and f3 survive; dup and marker are dropped"
    );
    assert!(!report.fast_forward);
    assert_eq!(
        report.new_tip,
        fx_ff.git(&["rev-parse", "feature"]).trim().to_string()
    );
}

#[test]
fn the_restack_report_names_what_it_dropped() {
    let fx = Fixture::new();
    ident(&fx);
    let [_c1, dup, marker] = empty_stack(&fx);

    fx.git(&["switch", "-q", "feature"]);
    let (outcome, _ctx) = restack_call(&fx, Some("feature"), None);
    let report = match outcome {
        ff_core::RestackOutcome::Restacked(r) => r,
        other => panic!("the restack must land, got {other:?}"),
    };

    assert_eq!(
        report.dropped.len(),
        2,
        "one entry per dropped commit, oldest-first"
    );
    assert_eq!(report.dropped[0].old, dup);
    assert_eq!(report.dropped[0].subject, "dup");
    assert_eq!(report.dropped[1].old, marker);
    assert_eq!(report.dropped[1].subject, "marker");

    // `replayed` counts only the commits that survived: the dropped ones
    // are not in it.
    assert_eq!(
        report.replayed, 2,
        "the dropped commits are not counted as replayed"
    );

    // The branch still ends up where the differential says it does.
    assert_eq!(
        report.new_tip,
        fx.git(&["rev-parse", "feature"]).trim().to_string()
    );
    // `mid` is not the branch restack was asked to move: it is left exactly
    // where it stood, reported as diverged rather than carried.
    assert!(!report.moved.contains(&"mid".to_string()));
    assert!(report.diverged.contains(&"mid".to_string()));
}
