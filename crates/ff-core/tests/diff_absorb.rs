//! Differential contract for `absorb::move_change` in both its spellings:
//! fufu's absorb must produce byte-identical commits to `git commit --fixup`
//! folded in by `git rebase -i --autosquash --update-refs`, a run of sources
//! must land the tree `git rebase -i`'s squash lands, and neither spelling
//! writes a single file — the worktree is byte-identical before and after,
//! and only refs, the index, and the operation log move.

use ff_core::absorb::{Endpoint, MoveOptions, MoveVerb};
use ff_core::gix;
use ff_testsupport::Fixture;
use ff_testsupport::hooks::{STAGED_HOOK, install_hook, staged_marker};

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
    ff_core::Provenance::new("pre", Some("ff absorb".into()))
}

fn oid(hex: &str) -> gix::ObjectId {
    gix::ObjectId::from_hex(hex.trim().as_bytes()).unwrap()
}

fn commit(hex: &str) -> Endpoint {
    Endpoint::Commit(oid(hex))
}

/// A move with every option defaulted but the ends: the verb's word fills
/// in whatever is `None`.
fn opts(verb: MoveVerb, from: Option<Vec<Endpoint>>, into: Option<Endpoint>) -> MoveOptions {
    MoveOptions {
        verb,
        from,
        into,
        paths: Vec::new(),
        message: None,
        verify: ff_core::Verify::Run,
        now: Some(NOW),
        argv: vec!["ff".into(), verb.as_str().into()],
    }
}

/// The call without the unwrap, for the scenarios a hook or a guard refuses.
fn move_result(
    fx: &Fixture,
    opts: &MoveOptions,
) -> ff_core::Result<(ff_core::MoveOutcome, ff_core::ops::VerbContext)> {
    let repo = fx.repo();
    ff_core::absorb::move_change(&repo, opts, &prov())
}

fn move_call(
    fx: &Fixture,
    opts: &MoveOptions,
) -> (ff_core::MoveOutcome, ff_core::ops::VerbContext) {
    move_result(fx, opts).unwrap()
}

/// The move, expected to land.
fn moved(fx: &Fixture, opts: &MoveOptions) -> ff_core::MoveReport {
    match move_call(fx, opts).0 {
        ff_core::MoveOutcome::Moved(r) => *r,
        other => panic!("the move must land, got {other:?}"),
    }
}

/// `ff absorb [--into <rev>] [<paths>]`.
fn absorb_call(
    fx: &Fixture,
    into: Option<&str>,
    paths: Vec<String>,
    now: i64,
) -> (ff_core::MoveOutcome, ff_core::ops::VerbContext) {
    let mut o = opts(MoveVerb::Absorb, None, into.map(commit));
    o.paths = paths;
    o.now = Some(now);
    move_call(fx, &o)
}

/// The same call without the unwrap, for the scenarios a hook refuses.
fn absorb_result(
    fx: &Fixture,
    into: Option<&str>,
    paths: Vec<String>,
    verify: ff_core::Verify,
) -> ff_core::Result<(ff_core::MoveOutcome, ff_core::ops::VerbContext)> {
    let mut o = opts(MoveVerb::Absorb, None, into.map(commit));
    o.paths = paths;
    o.verify = verify;
    move_result(fx, &o)
}

/// `ff lift [--from <rev>] [<paths>]`.
fn lift_call(
    fx: &Fixture,
    from: Option<&str>,
    paths: Vec<String>,
    now: i64,
) -> (ff_core::MoveOutcome, ff_core::ops::VerbContext) {
    let mut o = opts(MoveVerb::Lift, from.map(|f| vec![commit(f)]), None);
    o.paths = paths;
    o.now = Some(now);
    move_call(fx, &o)
}

/// A file's content at a revision, or `None` when the path is not there.
fn file_at(fx: &Fixture, rev: &str, path: &str) -> Option<String> {
    let out = fx.try_git(&["show", &format!("{rev}:{path}")]);
    out.status
        .success()
        .then(|| String::from_utf8(out.stdout).unwrap())
}

fn rev(fx: &Fixture, name: &str) -> String {
    fx.git(&["rev-parse", name]).trim().to_string()
}

fn tree_of(fx: &Fixture, rev: &str) -> String {
    fx.git(&["rev-parse", &format!("{rev}^{{tree}}")])
        .trim()
        .to_string()
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

/// A base commit — the target must not be a root, or `target^` in the oracle
/// cannot resolve — then four commits `c1..c4` on `main`, `mid` at `c2`.
/// Nobody switches branches, so `main` stays HEAD throughout.
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

/// Run the git oracle: commit the open change as a `fixup!` of `target`, let
/// autosquash fold it in, and fix the committer date to the same `now` fufu
/// was given. `GIT_SEQUENCE_EDITOR=:` keeps git's own autosquash-rewritten
/// todo, including its `update-ref` lines — a hand-written todo would
/// silently disable `--update-refs`.
fn git_oracle_absorb(fx: &Fixture, target: &str, now: i64) {
    let upstream = fx
        .git(&["rev-parse", &format!("{target}^")])
        .trim()
        .to_string();
    fx.git(&["commit", "-q", "-a", &format!("--fixup={target}")]);
    fx.git_env_in(
        &fx.path(),
        &["rebase", "-i", "--autosquash", "--update-refs", &upstream],
        &[
            ("GIT_SEQUENCE_EDITOR", ":"),
            ("GIT_COMMITTER_DATE", &format!("@{now} +0000")),
        ],
    );
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
fn absorb_into_mid_stack_matches_git() {
    let fx_ff = Fixture::new();
    ident(&fx_ff);
    let [_c0, c1_ff, _c2_ff, _c3_ff, _c4_ff] = stack(&fx_ff);

    let fx_git = Fixture::new();
    ident(&fx_git);
    let [_c0, c1_git, _c2_git, _c3_git, _c4_git] = stack(&fx_git);

    assert_eq!(c1_ff, c1_git, "setup must be lockstep before any rewrite");

    // The open change touches f1.txt, which only c1 introduced: no
    // descendant sees it, so the merge into c1 is conflict-free.
    fx_ff.write("f1.txt", "one-prime\n");
    fx_git.write("f1.txt", "one-prime\n");

    let (outcome, _ctx) = absorb_call(&fx_ff, Some(&c1_ff), Vec::new(), NOW);
    let report = match outcome {
        ff_core::MoveOutcome::Moved(r) => *r,
        other => panic!("the absorb must land, got {other:?}"),
    };
    git_oracle_absorb(&fx_git, &c1_git, NOW);

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
    assert_eq!(report.into.id, c1_ff);
    assert_eq!(report.into.new.as_deref(), Some(new_c1.as_str()));
    assert_eq!(report.restacked, 3);
    assert_eq!(report.moved, vec!["mid".to_string()]);
    assert!(!report.still_open);
}

#[test]
fn absorb_overlapping_edit_matches_git() {
    let fx_ff = Fixture::new();
    ident(&fx_ff);
    fx_ff.write("f0.txt", "base\n");
    let _c0_ff = fx_ff.commit("base");
    fx_ff.write("doc.txt", "l1\nl2\nl3\n");
    let c1_ff = fx_ff.commit("c1");
    fx_ff.write("doc.txt", "l1\nl2\nl3\ntail\n");
    let _c2_ff = fx_ff.commit("c2");

    let fx_git = Fixture::new();
    ident(&fx_git);
    fx_git.write("f0.txt", "base\n");
    let _c0_git = fx_git.commit("base");
    fx_git.write("doc.txt", "l1\nl2\nl3\n");
    let c1_git = fx_git.commit("c1");
    fx_git.write("doc.txt", "l1\nl2\nl3\ntail\n");
    let _c2_git = fx_git.commit("c2");

    assert_eq!(c1_ff, c1_git, "setup must be lockstep before any rewrite");

    // c2 edited the file's tail; the open change edits its head. The merge
    // must resolve both edits — this is the case that proves fufu's
    // `merge(base: tip_tree, ours: target_tree, theirs: open_tree)` is git's
    // autosquash.
    fx_ff.write("doc.txt", "head\nl2\nl3\ntail\n");
    fx_git.write("doc.txt", "head\nl2\nl3\ntail\n");

    let (outcome, _ctx) = absorb_call(&fx_ff, Some(&c1_ff), Vec::new(), NOW);
    let report = match outcome {
        ff_core::MoveOutcome::Moved(r) => *r,
        other => panic!("the absorb must land, got {other:?}"),
    };
    git_oracle_absorb(&fx_git, &c1_git, NOW);

    let ff_main = fx_ff.git(&["rev-parse", "main"]).trim().to_string();
    let git_main = fx_git.git(&["rev-parse", "main"]).trim().to_string();
    assert_eq!(ff_main, git_main, "main diverged between fufu and git");

    let log_ff = fx_ff.git(&[
        "log",
        "--format=%H|%T|%an|%ad|%cn|%cd|%s",
        "--date=raw",
        "main",
    ]);
    let log_git = fx_git.git(&[
        "log",
        "--format=%H|%T|%an|%ad|%cn|%cd|%s",
        "--date=raw",
        "main",
    ]);
    assert_eq!(
        log_ff, log_git,
        "every rewritten commit must be byte-identical\nfufu:\n{log_ff}\ngit:\n{log_git}"
    );

    assert_eq!(report.into.id, c1_ff);
    assert_eq!(
        report.into.new.as_deref(),
        Some(fx_ff.git(&["rev-parse", "main~1"]).trim())
    );
    assert_eq!(report.restacked, 1);
    assert!(!report.still_open);
}

#[test]
fn absorb_writes_no_files() {
    let fx = Fixture::new();
    ident(&fx);
    fx.write("a.txt", "a1\n");
    let c1 = fx.commit("c1");
    fx.write("a.txt", "a2\n");
    let c2 = fx.commit("c2");

    // Revert the worktree to c1's content: the worktree's tree is now
    // exactly c1's tree, a value plain read-only git can name.
    fx.write("a.txt", "a1\n");
    let files_before = worktree_files(&fx);

    let (outcome, ctx) = absorb_call(&fx, None, Vec::new(), NOW);
    match outcome {
        ff_core::MoveOutcome::Moved(r) => assert!(!r.still_open),
        other => panic!("the absorb must land, got {other:?}"),
    }

    // Nothing was added to or removed from the working tree.
    let files_after = worktree_files(&fx);
    assert_eq!(
        files_before, files_after,
        "the worktree must be byte-identical before and after"
    );

    // And the captured worktree tree really is the worktree's tree:
    // `begin_verb` resolved it to c1's tree, which is the worktree's tree
    // before and after alike.
    let worktree_tree = fx
        .git(&["rev-parse", &format!("{c1}^{{tree}}")])
        .trim()
        .to_string();
    assert_eq!(ctx.pre_tree.to_string(), worktree_tree);
    let _ = c2;
}

#[test]
fn a_conflicting_fold_holds_and_moves_nothing() {
    let fx = Fixture::new();
    ident(&fx);
    fx.write("f.txt", "x\nrest\n");
    let _c0 = fx.commit("base");
    fx.write("f.txt", "A\nrest\n");
    let c1 = fx.commit("c1");
    fx.git(&["branch", "mid"]);
    fx.write("f.txt", "A2\nrest\n");
    let _c2 = fx.commit("c2");

    // The merge runs against base = the tip's tree, so all three sides must
    // differ on the same line: c1 rewrote line 1, c2 rewrote it again, and
    // the open change rewrites it a third way. Folding it into c1 conflicts.
    fx.write("f.txt", "C\nrest\n");

    let main_before = fx.git(&["rev-parse", "main"]).trim().to_string();
    let mid_before = fx.git(&["rev-parse", "mid"]).trim().to_string();

    let (outcome, _ctx) = absorb_call(&fx, Some(&c1), Vec::new(), NOW);

    let report = match outcome {
        ff_core::MoveOutcome::Held(r) => r,
        other => panic!("a conflicting fold must hold, got {other:?}"),
    };

    assert_eq!(report.verb, "absorb");
    assert_eq!(
        report.at,
        ff_core::futures::At::OpenChange,
        "the fold cannot apply the open change to the target"
    );
    assert!(
        report.paths.iter().any(|p| p == "f.txt"),
        "the report must name the conflicting path: {:?}",
        report.paths
    );

    // The hold is recorded on the branch underfoot, naming the target and
    // carrying the (empty) path filter the user gave.
    let held = ff_core::held::of(&fx.repo(), "main")
        .unwrap()
        .expect("the hold must stand on the branch underfoot");
    match &held.intent {
        ff_core::held::Intent::Absorb {
            from, into, paths, ..
        } => {
            assert_eq!(from, &vec!["@".to_string()]);
            assert_eq!(into, &c1);
            assert!(paths.is_empty(), "no filter was given");
        }
        other => panic!("the intent must be Absorb, got {other:?}"),
    }

    assert_eq!(
        fx.git(&["rev-parse", "main"]).trim(),
        main_before,
        "a hold moves no ref"
    );
    assert_eq!(
        fx.git(&["rev-parse", "mid"]).trim(),
        mid_before,
        "a hold moves no ref"
    );
    let ops = ff_core::ops::read_ops(&fx.repo(), 1).unwrap();
    assert_eq!(
        ops[0].verb, "hold",
        "the newest op must be the hold, not an absorb"
    );
}

#[test]
fn absorb_paths_filter_selects() {
    let fx = Fixture::new();
    ident(&fx);
    fx.write("a.txt", "a1\n");
    fx.write("b.txt", "b1\n");
    let c1 = fx.commit("c1");
    fx.write("c.txt", "c\n");
    let _c2 = fx.commit("c2");

    fx.write("a.txt", "a2\n");
    fx.write("b.txt", "b2\n");
    fx.backdate();
    let (outcome, _ctx) = absorb_call(&fx, Some(&c1), vec!["a.txt".into()], NOW);
    let report = match outcome {
        ff_core::MoveOutcome::Moved(r) => *r,
        other => panic!("the absorb must land, got {other:?}"),
    };

    assert_eq!(report.paths, vec!["a.txt".to_string()]);
    assert!(report.still_open);

    // Only a.txt was folded in: the target carries the new a.txt and the
    // old b.txt, and b.txt is what the open change now holds.
    let new_c1 = fx.git(&["rev-parse", "main~1"]).trim().to_string();
    assert_eq!(fx.git(&["show", &format!("{new_c1}:a.txt")]), "a2\n");
    assert_eq!(fx.git(&["show", &format!("{new_c1}:b.txt")]), "b1\n");
    // `trim_end`, not `trim`: the leading marker is the signal — a staged
    // `M ` would mean the absorb wrote the worktree.
    assert_eq!(
        fx.git(&["status", "--porcelain"]).trim_end(),
        " M b.txt",
        "b.txt is still open and unstaged; a.txt is clean"
    );
}

#[test]
fn absorb_clean_tree_is_nothing_to_absorb() {
    let fx = Fixture::new();
    ident(&fx);
    fx.write("a.txt", "a\n");
    let c1 = fx.commit("c1");
    fx.write("b.txt", "b\n");
    let _c2 = fx.commit("c2");

    // Prime the op log so a refusal's "nothing moved" claim is meaningful:
    // the very first fufu call on a fixture always bootstraps the log from
    // observed state, which is not itself evidence of a mutation.
    ff_core::ops::reconcile(&fx.repo(), NOW).unwrap();
    let tip_before = ff_core::ops::OpLog::open(&fx.repo())
        .unwrap()
        .tip()
        .unwrap();

    let (outcome, _ctx) = absorb_call(&fx, Some(&c1), Vec::new(), NOW);
    match outcome {
        ff_core::MoveOutcome::Nothing { branch, .. } => {
            assert_eq!(branch, "main");
        }
        other => panic!("a clean tree must not absorb, got {other:?}"),
    }

    let tip_after = ff_core::ops::OpLog::open(&fx.repo())
        .unwrap()
        .tip()
        .unwrap();
    assert_eq!(
        tip_before, tip_after,
        "nothing to absorb appends no operation"
    );
}

#[test]
fn absorb_records_the_rewrite_map() {
    let fx = Fixture::new();
    ident(&fx);
    let [_c0, c1, _c2, _c3, _c4] = stack(&fx);

    fx.write("f1.txt", "one-prime\n");
    let (outcome, _ctx) = absorb_call(&fx, Some(&c1), Vec::new(), NOW);
    match outcome {
        ff_core::MoveOutcome::Moved(r) => {
            assert_eq!(r.into.id, c1);
            assert_eq!(r.restacked, 3);
        }
        other => panic!("the absorb must land, got {other:?}"),
    }

    let record = tip_record(&fx.repo());
    assert_eq!(record.verb, "absorb");
    let rewrites = record.rewrites.clone();
    assert_eq!(rewrites.len(), 4, "the target and its three descendants");
    assert_eq!(rewrites[0].old, c1);
    for i in 1..rewrites.len() {
        let parent = fx
            .git(&["rev-parse", &format!("{}^", rewrites[i].old)])
            .trim()
            .to_string();
        assert_eq!(
            parent,
            rewrites[i - 1].old,
            "rewrite {i}'s old sha must be the previous entry's child"
        );
    }

    let names: Vec<&str> = record.refs.iter().map(|t| t.name.as_str()).collect();
    assert!(names.contains(&"refs/heads/main"));
    assert!(names.contains(&"refs/heads/mid"));
}

#[test]
fn lift_moves_no_files_and_grows_the_open_change() {
    let fx = Fixture::new();
    ident(&fx);
    fx.write("a.txt", "a1\n");
    let _c1 = fx.commit("c1");
    fx.write("a.txt", "a2\n");
    fx.write("b.txt", "b\n");
    let c2 = fx.commit("c2");

    let files_before = worktree_files(&fx);

    let (outcome, _ctx) = lift_call(&fx, None, vec!["a.txt".into()], NOW);
    let report = match outcome {
        ff_core::MoveOutcome::Moved(r) => *r,
        other => panic!("the lift must land, got {other:?}"),
    };
    assert_eq!(report.from[0].id, c2);
    assert!(
        report.from[0].new.is_some(),
        "b.txt still introduces its own content: the source keeps its own identity"
    );
    assert!(
        !report.dropped.iter().any(|d| d.old == c2),
        "b.txt still introduces its own content: the target is not dropped"
    );

    // Neither verb writes a file: the worktree is byte-identical, and what
    // real git sees is the reattribution — a.txt, modified and unstaged,
    // and nothing else.
    let files_after = worktree_files(&fx);
    assert_eq!(
        files_before, files_after,
        "the worktree must be byte-identical before and after"
    );
    let status_raw = fx.git(&["status", "--porcelain=v2"]);
    let status = status_raw.trim_end();
    let lines: Vec<&str> = status.lines().collect();
    assert_eq!(lines.len(), 1, "exactly one path is open: {status}");
    assert!(
        lines[0].starts_with("1 .M") && lines[0].ends_with("a.txt"),
        "a.txt is modified and unstaged (index matches HEAD, worktree differs): {status}"
    );
    // And the reattribution is the lift itself: HEAD's a.txt is the
    // parent's content again.
    let new_tip = fx.git(&["rev-parse", "main"]).trim().to_string();
    assert_ne!(new_tip, c2);
    assert_eq!(fx.git(&["show", &format!("{new_tip}:a.txt")]), "a1\n");
    assert_eq!(fx.git(&["show", &format!("{new_tip}:b.txt")]), "b\n");
}

#[test]
fn lift_everything_drops_the_commit() {
    let fx = Fixture::new();
    ident(&fx);
    fx.write("f0.txt", "base\n");
    let _c0 = fx.commit("base");
    fx.write("a.txt", "a1\n");
    let c1 = fx.commit("c1");

    let (outcome, _ctx) = lift_call(&fx, None, vec!["a.txt".into()], NOW);
    let report = match outcome {
        ff_core::MoveOutcome::Moved(r) => *r,
        other => panic!("the lift must land, got {other:?}"),
    };

    // A lift that takes the commit's only introduction leaves it introducing
    // nothing, and fufu writes no empty commit: the commit is gone.
    assert!(
        report.from[0].dropped,
        "a.txt was the commit's only introduction"
    );
    assert_eq!(
        report.dropped.len(),
        1,
        "the lifted commit is named in dropped"
    );
    assert_eq!(report.dropped[0].old, c1);
    assert!(
        !fx.try_git(&["merge-base", "--is-ancestor", &c1, "main"])
            .status
            .success(),
        "the lifted commit is no longer in the branch's history"
    );
}

#[test]
fn a_conflicting_lift_holds_and_moves_nothing() {
    let fx = Fixture::new();
    ident(&fx);
    fx.write("f0.txt", "base\n");
    let _c0 = fx.commit("base");
    fx.write("doc.txt", "v1\n");
    let c1 = fx.commit("c1");
    fx.write("doc.txt", "v2\n");
    let c2 = fx.commit("c2");
    fx.git(&["branch", "mid"]);

    // c2 edited the file c1 created; lifting the file out of c1 would make
    // c2's edit a modification of nothing — a modify/delete conflict that
    // holds the lift.
    let main_before = fx.git(&["rev-parse", "main"]).trim().to_string();
    let mid_before = fx.git(&["rev-parse", "mid"]).trim().to_string();

    let (outcome, _ctx) = lift_call(&fx, Some(&c1), vec!["doc.txt".into()], NOW);

    let report = match outcome {
        ff_core::MoveOutcome::Held(r) => r,
        other => panic!("a conflicting lift must hold, got {other:?}"),
    };

    assert_eq!(report.verb, "lift");
    assert_eq!(
        report.at,
        ff_core::futures::At::Commit {
            id: c2.clone(),
            subject: "c2".into()
        },
        "the report names the descendant that cannot replay"
    );

    // The hold names the target and records the paths that were lifted.
    let held = ff_core::held::of(&fx.repo(), "main")
        .unwrap()
        .expect("the hold must stand on the branch underfoot");
    match &held.intent {
        ff_core::held::Intent::Lift {
            from, into, paths, ..
        } => {
            assert_eq!(from, &vec![c1.clone()]);
            assert_eq!(into, "@");
            assert_eq!(paths, &vec!["doc.txt".to_string()]);
        }
        other => panic!("the intent must be Lift, got {other:?}"),
    }

    assert_eq!(
        fx.git(&["rev-parse", "main"]).trim(),
        main_before,
        "a hold moves no ref"
    );
    assert_eq!(
        fx.git(&["rev-parse", "mid"]).trim(),
        mid_before,
        "a hold moves no ref"
    );
    let ops = ff_core::ops::read_ops(&fx.repo(), 1).unwrap();
    assert_eq!(
        ops[0].verb, "hold",
        "the newest op must be the hold, not a lift"
    );
}

#[test]
fn absorb_undoes_a_lift() {
    let fx = Fixture::new();
    ident(&fx);
    fx.write("f0.txt", "base\n");
    let _c0 = fx.commit("base");
    fx.write("a.txt", "a1\n");
    let c1 = fx.commit("c1");
    let original_tip_tree = fx
        .git(&["rev-parse", &format!("{c1}^{{tree}}")])
        .trim()
        .to_string();

    // Lift the path out of HEAD, then absorb it back in. The round trip must
    // land on the original tip's tree — the content never moved.
    let (outcome, _ctx) = lift_call(&fx, None, vec!["a.txt".into()], NOW);
    match outcome {
        ff_core::MoveOutcome::Moved(r) => {
            assert!(
                r.from[0].dropped,
                "a.txt was the commit's only introduction"
            );
            assert_eq!(r.dropped.len(), 1, "the lifted commit is named in dropped");
            assert_eq!(r.dropped[0].old, c1);
            assert!(
                !fx.try_git(&["merge-base", "--is-ancestor", &c1, "main"])
                    .status
                    .success(),
                "the lifted commit is no longer in the branch's history"
            );
        }
        other => panic!("the lift must land, got {other:?}"),
    }
    let (outcome, _ctx) = absorb_call(&fx, None, Vec::new(), NOW);
    match outcome {
        ff_core::MoveOutcome::Moved(r) => assert!(!r.still_open),
        other => panic!("the absorb must land, got {other:?}"),
    }

    let final_tree = fx.git(&["rev-parse", "main^{tree}"]).trim().to_string();
    assert_eq!(
        final_tree, original_tip_tree,
        "the round trip must land on the original tip's tree"
    );
}

#[test]
fn absorb_into_head_is_an_amend() {
    let fx = Fixture::new();
    ident(&fx);
    fx.write("a.txt", "a\n");
    let _c1 = fx.commit("c1");
    fx.write("a.txt", "b\n");
    let c2 = fx.commit("c2");

    fx.write("a.txt", "c\n");
    let (outcome, _ctx) = absorb_call(&fx, None, Vec::new(), NOW);
    let report = match outcome {
        ff_core::MoveOutcome::Moved(r) => *r,
        other => panic!("the absorb must land, got {other:?}"),
    };

    // The target is the tip: no merge runs, and the open change is empty
    // once the absorb lands.
    assert_eq!(report.into.id, c2);
    assert_eq!(
        report.into.new.as_deref(),
        Some(fx.git(&["rev-parse", "main"]).trim())
    );
    assert_ne!(report.into.new.as_deref(), Some(c2.as_str()));
    assert_eq!(report.restacked, 0);
    assert!(report.moved.is_empty());
    assert!(!report.still_open);
    assert_eq!(fx.git(&["status", "--porcelain"]).trim(), "");
    assert_eq!(fx.git(&["show", "main:a.txt"]), "c\n");
}

// ---------------------------------------------------------------------------
// The pre-commit gate. `ff absorb` makes worktree content into commit content,
// so it runs the same hook a close does, over the same staged index.
// ---------------------------------------------------------------------------

#[test]
fn a_declining_pre_commit_hook_refuses_the_absorb() {
    let fx = Fixture::new();
    ident(&fx);
    fx.write("a.txt", "a\n");
    let c1 = fx.commit("c1");
    fx.write("b.txt", "b\n");
    let c2 = fx.commit("c2");
    install_hook(&fx, "pre-commit", "#!/bin/sh\nexit 1\n");
    fx.write("a.txt", "changed\n");
    let index_before = fx.index_bytes();

    let err = absorb_result(&fx, Some(&c1), Vec::new(), ff_core::Verify::Run).unwrap_err();
    assert_eq!(err.id(), "hook/declined");
    assert_eq!(
        fx.git(&["rev-parse", "main"]).trim(),
        c2,
        "the branch is unmoved and the target unrewritten"
    );
    assert_eq!(fx.git(&["show", &format!("{c1}:a.txt")]), "a\n");
    assert_eq!(
        fx.index_bytes(),
        index_before,
        "a declined absorb restores .git/index byte-for-byte"
    );
    assert_eq!(
        std::fs::read_to_string(fx.path().join("a.txt")).unwrap(),
        "changed\n",
        "the open change is still open"
    );
    let ops = ff_core::ops::read_ops(&fx.repo(), 0).unwrap();
    assert!(ops.iter().all(|op| op.verb != "absorb"), "{ops:?}");
}

#[test]
fn no_verify_skips_the_absorb_gate() {
    let fx = Fixture::new();
    ident(&fx);
    fx.write("a.txt", "a\n");
    let c1 = fx.commit("c1");
    fx.write("b.txt", "b\n");
    fx.commit("c2");
    install_hook(&fx, "pre-commit", "#!/bin/sh\nexit 1\n");
    fx.write("a.txt", "changed\n");

    let (outcome, _ctx) = absorb_result(&fx, Some(&c1), Vec::new(), ff_core::Verify::Skip).unwrap();
    let report = match outcome {
        ff_core::MoveOutcome::Moved(r) => *r,
        other => panic!("--no-verify must land, got {other:?}"),
    };
    let new_c1 = report.into.new.expect("the target survived");
    assert_eq!(fx.git(&["show", &format!("{new_c1}:a.txt")]), "changed\n");
}

#[test]
fn a_pre_commit_formatter_rewrite_is_what_gets_absorbed() {
    let fx = Fixture::new();
    ident(&fx);
    fx.write("a.txt", "a\n");
    let c1 = fx.commit("c1");
    fx.write("b.txt", "b\n");
    fx.commit("c2");
    install_hook(
        &fx,
        "pre-commit",
        "#!/bin/sh\nprintf 'formatted\\n' > a.txt\n",
    );
    fx.write("a.txt", "unformatted\n");

    let (outcome, _ctx) = absorb_call(&fx, Some(&c1), Vec::new(), NOW);
    let report = match outcome {
        ff_core::MoveOutcome::Moved(r) => *r,
        other => panic!("the absorb must land, got {other:?}"),
    };
    let new_c1 = report.into.new.expect("the target survived");
    assert_eq!(
        fx.git(&["show", &format!("{new_c1}:a.txt")]),
        "formatted\n",
        "the hook's formatting is what folded in"
    );
    assert_eq!(fx.git(&["status", "--porcelain"]).trim(), "");
}

#[test]
fn the_absorb_gate_sees_exactly_what_is_folding_in() {
    let fx = Fixture::new();
    ident(&fx);
    fx.write("a.txt", "a\n");
    let c1 = fx.commit("c1");
    fx.write("b.txt", "b\n");
    fx.commit("c2");
    install_hook(&fx, "pre-commit", STAGED_HOOK);
    fx.write("a.txt", "changed\n");
    fx.write("new.txt", "new\n");

    let (outcome, _ctx) = absorb_call(&fx, Some(&c1), Vec::new(), NOW);
    assert!(matches!(outcome, ff_core::MoveOutcome::Moved(_)));
    assert_eq!(
        staged_marker(&fx),
        vec!["a.txt", "new.txt"],
        "the whole open change is staged when nothing narrows it"
    );
}

#[test]
fn the_absorb_gate_sees_only_the_selected_paths() {
    let fx = Fixture::new();
    ident(&fx);
    fx.write("a.txt", "a\n");
    fx.write("b.txt", "b\n");
    let c1 = fx.commit("c1");
    fx.write("c.txt", "c\n");
    fx.commit("c2");
    install_hook(&fx, "pre-commit", STAGED_HOOK);
    fx.write("a.txt", "a changed\n");
    fx.write("b.txt", "b changed\n");

    let (outcome, _ctx) = absorb_call(&fx, Some(&c1), vec!["a.txt".into()], NOW);
    assert!(matches!(outcome, ff_core::MoveOutcome::Moved(_)));
    assert_eq!(
        staged_marker(&fx),
        vec!["a.txt"],
        "a path filter stages exactly its slice"
    );
    assert_eq!(
        std::fs::read_to_string(fx.path().join("b.txt")).unwrap(),
        "b changed\n",
        "the unselected edit survives the hook run"
    );
}

#[test]
fn lift_runs_no_hook() {
    let fx = Fixture::new();
    ident(&fx);
    fx.write("a.txt", "a\n");
    fx.commit("c1");
    fx.write("a.txt", "b\n");
    fx.commit("c2");
    // A hook that would refuse anything it was asked about. `ff lift` never
    // makes worktree content into commit content, so it is never asked.
    install_hook(&fx, "pre-commit", "#!/bin/sh\nexit 1\n");
    install_hook(&fx, "commit-msg", "#!/bin/sh\nexit 1\n");

    let (outcome, _ctx) = lift_call(&fx, None, vec!["a.txt".into()], NOW);
    assert!(matches!(outcome, ff_core::MoveOutcome::Moved(_)));
}

// ---------------------------------------------------------------------------
// One move, two spellings: `--from` a run of commits, `--into` any commit on
// the line or the open change. The shapes below are the ladder the module
// doc names, and git's `rebase -i` squash is the oracle where it has one.
// ---------------------------------------------------------------------------

/// `base` on `main`, then `c1 ← c2 ← c3` on `feat`, each adding its own
/// file. The run sits on a branch off trunk, the way a move's default
/// target expects: the commit under a run that reaches the bottom of the
/// branch is trunk's, and a bare absorb refuses to rewrite it.
fn run_stack(fx: &Fixture) -> [String; 4] {
    fx.write("f0.txt", "base\n");
    let c0 = fx.commit("base");
    fx.git(&["switch", "-q", "-c", "feat"]);
    fx.write("f1.txt", "one\n");
    let c1 = fx.commit("c1");
    fx.write("f2.txt", "two\n");
    let c2 = fx.commit("c2");
    fx.write("f3.txt", "three\n");
    let c3 = fx.commit("c3");
    [c0, c1, c2, c3]
}

/// The git oracle for a run folding into the commit under it: `rebase -i`
/// with the run's commits marked `fixup`, committer date pinned to `now`.
fn git_oracle_squash(fx: &Fixture, onto: &str, now: i64) {
    let upstream = fx
        .git(&["rev-parse", &format!("{onto}^")])
        .trim()
        .to_string();
    fx.git_env_in(
        &fx.path(),
        &["rebase", "-i", &upstream],
        &[
            ("GIT_SEQUENCE_EDITOR", "sed -i -e '2,$s/^pick/fixup/'"),
            ("GIT_COMMITTER_DATE", &format!("@{now} +0000")),
        ],
    );
}

#[test]
fn a_run_of_commits_folds_into_the_commit_under_it() {
    let fx = Fixture::new();
    ident(&fx);
    let [c0, c1, c2, c3] = run_stack(&fx);
    let fx_git = Fixture::new();
    ident(&fx_git);
    let [_c0, c1_git, _c2, _c3] = run_stack(&fx_git);
    assert_eq!(c1, c1_git, "setup must be lockstep before any rewrite");

    // `ff absorb --from c1..c3`: c2 and c3 fold into c1, the commit under
    // them, which is absorb's default target.
    let report = moved(
        &fx,
        &opts(MoveVerb::Absorb, Some(vec![commit(&c2), commit(&c3)]), None),
    );
    git_oracle_squash(&fx_git, &c1_git, NOW);

    assert_eq!(report.verb, "absorb");
    assert_eq!(report.into.id, c1);
    let new_c1 = report.into.new.clone().expect("the target survives");
    assert_eq!(rev(&fx, "feat"), new_c1);
    assert_eq!(
        tree_of(&fx, "feat"),
        tree_of(&fx, &c3),
        "the target now carries everything the run did"
    );
    assert_eq!(
        tree_of(&fx, "feat"),
        tree_of(&fx_git, "feat"),
        "the same tree git's squash lands"
    );
    assert_eq!(rev(&fx, "feat~1"), c0, "the run is gone: c1' sits on base");
    assert_eq!(fx.git(&["rev-list", "--count", "feat"]).trim(), "2");

    let ids: Vec<&str> = report.from.iter().map(|s| s.id.as_str()).collect();
    assert_eq!(ids, vec![c2.as_str(), c3.as_str()], "deepest first");
    assert!(report.from.iter().all(|s| s.dropped && s.new.is_none()));
    assert_eq!(report.dropped.len(), 2, "both sources are named dropped");
    assert_eq!(
        report.files,
        vec!["f2.txt".to_string(), "f3.txt".to_string()]
    );
    assert_eq!(report.restacked, 0);
    assert!(!report.still_open);

    let record = tip_record(&fx.repo());
    assert_eq!(record.verb, "absorb");
    assert!(
        record.summary.starts_with("move "),
        "one shape for both spellings: {}",
        record.summary
    );
    let ops = ff_core::ops::read_ops(&fx.repo(), 0).unwrap();
    assert_eq!(
        ops.iter().filter(|op| op.verb == "absorb").count(),
        1,
        "one operation"
    );
}

#[test]
fn a_run_lifts_into_the_open_change() {
    let fx = Fixture::new();
    ident(&fx);
    let [_c0, c1, c2, c3] = run_stack(&fx);
    let files_before = worktree_files(&fx);

    // `ff lift --from c1..c3`: c2 and c3 come out of the history and into
    // the open change, which is lift's default target.
    let report = moved(
        &fx,
        &opts(MoveVerb::Lift, Some(vec![commit(&c2), commit(&c3)]), None),
    );

    assert_eq!(report.verb, "lift");
    assert_eq!(report.into.id, "@");
    assert_eq!(report.into.new, None);
    assert_eq!(rev(&fx, "feat"), c1, "the tip is c1 again, untouched");
    assert_eq!(worktree_files(&fx), files_before, "no file moved");
    assert!(report.from.iter().all(|s| s.dropped));
    assert!(
        !report.still_open,
        "still_open is only ever the open change's as a source"
    );

    // The open change holds both commits' work: both files are untracked
    // additions against the tip.
    let status = fx.git(&["status", "--porcelain"]);
    assert!(status.contains("?? f2.txt"), "{status}");
    assert!(status.contains("?? f3.txt"), "{status}");
}

#[test]
fn a_path_filter_leaves_the_rest_in_each_source() {
    let fx = Fixture::new();
    ident(&fx);
    fx.write("f0.txt", "base\n");
    let _c0 = fx.commit("base");
    fx.git(&["switch", "-q", "-c", "feat"]);
    fx.write("a.txt", "a1\n");
    let c1 = fx.commit("c1");
    fx.write("a.txt", "a2\n");
    fx.write("b.txt", "b\n");
    let c2 = fx.commit("c2");
    fx.write("a.txt", "a3\n");
    fx.write("c.txt", "c\n");
    let c3 = fx.commit("c3");

    let mut o = opts(
        MoveVerb::Absorb,
        Some(vec![commit(&c2), commit(&c3)]),
        Some(commit(&c1)),
    );
    o.paths = vec!["a.txt".into()];
    let report = moved(&fx, &o);

    assert_eq!(report.files, vec!["a.txt".to_string()]);
    assert_eq!(report.paths, vec!["a.txt".to_string()]);
    // c1 carries a.txt's final content; c2 and c3 keep their own files and
    // survive, so nothing is dropped.
    assert!(report.dropped.is_empty(), "{:?}", report.dropped);
    assert_eq!(fx.git(&["rev-list", "--count", "feat"]).trim(), "4");
    assert_eq!(file_at(&fx, "feat~2", "a.txt").as_deref(), Some("a3\n"));
    assert_eq!(file_at(&fx, "feat~2", "b.txt"), None);
    assert_eq!(file_at(&fx, "feat~1", "b.txt").as_deref(), Some("b\n"));
    assert_eq!(file_at(&fx, "feat~1", "a.txt").as_deref(), Some("a3\n"));
    assert_eq!(file_at(&fx, "feat", "c.txt").as_deref(), Some("c\n"));
    assert_eq!(
        tree_of(&fx, "feat"),
        tree_of(&fx, &c3),
        "the tip's tree is unchanged"
    );
    assert_eq!(
        report.restacked, 0,
        "every replayed commit was an end of the move"
    );
}

#[test]
fn a_closed_target_above_the_sources_gains_them() {
    let fx = Fixture::new();
    ident(&fx);
    let [c0, c1, c2, c3] = run_stack(&fx);

    // `ff lift --from HEAD~2 --into HEAD`: c1 moves up into c3, and c2 is
    // replayed without it.
    let report = moved(
        &fx,
        &opts(MoveVerb::Lift, Some(vec![commit(&c1)]), Some(commit(&c3))),
    );

    assert_eq!(
        tree_of(&fx, "feat"),
        tree_of(&fx, &c3),
        "HEAD's tree is unchanged"
    );
    assert_eq!(fx.git(&["rev-list", "--count", "feat"]).trim(), "3");
    assert_eq!(rev(&fx, "feat~2"), c0);
    assert_eq!(report.from[0].id, c1);
    assert!(report.from[0].dropped, "c1 introduces nothing now");
    assert_eq!(report.into.id, c3);
    assert_eq!(report.into.new.as_deref(), Some(rev(&fx, "feat").as_str()));
    assert_eq!(report.restacked, 1, "c2 was replayed");
    assert_eq!(fx.git(&["log", "-1", "--format=%s", "feat~1"]).trim(), "c2");
    assert_eq!(
        file_at(&fx, "feat~1", "f1.txt"),
        None,
        "c2 replayed without c1's file"
    );
    assert_eq!(file_at(&fx, "feat", "f1.txt").as_deref(), Some("one\n"));
    let _ = c2;
}

#[test]
fn a_target_inside_the_run_takes_both_sides() {
    let fx = Fixture::new();
    ident(&fx);
    let [c0, c1, c2, c3] = run_stack(&fx);

    // Sources c1 and c3 around the target c2: the run is contiguous with
    // the target in the gap, and c2 takes both.
    let report = moved(
        &fx,
        &opts(
            MoveVerb::Absorb,
            Some(vec![commit(&c1), commit(&c3)]),
            Some(commit(&c2)),
        ),
    );

    assert_eq!(fx.git(&["rev-list", "--count", "feat"]).trim(), "2");
    assert_eq!(rev(&fx, "feat~1"), c0);
    assert_eq!(tree_of(&fx, "feat"), tree_of(&fx, &c3));
    assert_eq!(fx.git(&["log", "-1", "--format=%s", "feat"]).trim(), "c2");
    assert_eq!(report.into.id, c2);
    assert!(report.from.iter().all(|s| s.dropped));
    assert_eq!(report.restacked, 0);
}

#[test]
fn a_gap_is_refused_with_the_range_spelling() {
    let fx = Fixture::new();
    ident(&fx);
    let [_c0, c1, c2, c3] = run_stack(&fx);
    fx.write("f4.txt", "four\n");
    let c4 = fx.commit("c4");

    let err = move_result(
        &fx,
        &opts(
            MoveVerb::Absorb,
            Some(vec![commit(&c1), commit(&c3)]),
            Some(commit(&c4)),
        ),
    )
    .unwrap_err();
    assert_eq!(err.id(), "usage/move-gap", "{err}");
    let exits = err.exits();
    assert!(
        exits.iter().any(|e| e
            == &format!(
                "ff absorb --from {}~..{}",
                ff_core::sha::short(&c1),
                ff_core::sha::short(&c3)
            )),
        "the exit spells the whole run: {exits:?}"
    );
    assert_eq!(rev(&fx, "feat"), c4, "a refusal moves nothing");
    let _ = c2;
}

#[test]
fn an_endpoint_off_the_line_is_refused() {
    let fx = Fixture::new();
    ident(&fx);
    let [_c0, c1, _c2, c3] = run_stack(&fx);
    fx.git(&["switch", "-q", "-c", "side", &c1]);
    fx.write("s.txt", "side\n");
    let side = fx.commit("side");
    fx.git(&["switch", "-q", "feat"]);

    let err = move_result(&fx, &opts(MoveVerb::Lift, Some(vec![commit(&side)]), None)).unwrap_err();
    assert_eq!(err.id(), "rewrite/not-in-history", "{err}");
    let err = move_result(&fx, &opts(MoveVerb::Absorb, None, Some(commit(&side)))).unwrap_err();
    assert_eq!(err.id(), "rewrite/not-in-history", "{err}");
    assert_eq!(rev(&fx, "feat"), c3);
}

#[test]
fn the_target_alone_is_refused() {
    let fx = Fixture::new();
    ident(&fx);
    let [_c0, _c1, c2, c3] = run_stack(&fx);

    let err = move_result(
        &fx,
        &opts(MoveVerb::Absorb, Some(vec![commit(&c2)]), Some(commit(&c2))),
    )
    .unwrap_err();
    assert_eq!(err.id(), "usage/move-into-self", "{err}");

    // The bare words keep their own ids: absorb aimed at the open change
    // with default sources, lift from the open change with default target.
    let err = move_result(&fx, &opts(MoveVerb::Absorb, None, Some(Endpoint::Open))).unwrap_err();
    assert_eq!(err.id(), "usage/absorb-into-open", "{err}");
    let err =
        move_result(&fx, &opts(MoveVerb::Lift, Some(vec![Endpoint::Open]), None)).unwrap_err();
    assert_eq!(err.id(), "usage/lift-from-open", "{err}");
    assert_eq!(rev(&fx, "feat"), c3);
}

#[test]
fn a_default_target_trunk_holds_is_refused() {
    let fx = Fixture::new();
    ident(&fx);
    fx.write("f0.txt", "base\n");
    let base = fx.commit("base");
    fx.set_config("fufu.trunk", "main");
    fx.git(&["switch", "-q", "-c", "feat"]);
    fx.write("f1.txt", "one\n");
    let c1 = fx.commit("c1");
    fx.write("f2.txt", "two\n");
    let c2 = fx.commit("c2");

    // `ff absorb --from main..`: the commit under the run is trunk's tip.
    let err = move_result(
        &fx,
        &opts(MoveVerb::Absorb, Some(vec![commit(&c1), commit(&c2)]), None),
    )
    .unwrap_err();
    assert_eq!(err.id(), "absorb/into-trunk", "{err}");
    assert!(err.to_string().contains("on main"), "{err}");
    let exits = err.exits();
    assert!(
        exits.iter().any(|e| e
            == &format!(
                "ff absorb --from {}..{} --into {}",
                ff_core::sha::short(&c1),
                ff_core::sha::short(&c2),
                ff_core::sha::short(&c1)
            )),
        "the exit folds the run into its own lowest commit: {exits:?}"
    );
    assert_eq!(rev(&fx, "feat"), c2);
    assert_eq!(rev(&fx, "main"), base);

    // Named, the same target is allowed: trunk is rewritten on purpose.
    let report = moved(
        &fx,
        &opts(
            MoveVerb::Absorb,
            Some(vec![commit(&c1)]),
            Some(commit(&base)),
        ),
    );
    assert_eq!(report.into.id, base);
}

#[test]
fn dash_m_rewords_a_closed_target_under_commit_msg() {
    let fx = Fixture::new();
    ident(&fx);
    let [_c0, _c1, _c2, _c3] = run_stack(&fx);
    install_hook(
        &fx,
        "commit-msg",
        "#!/bin/sh\nprintf '%s [hooked]\\n' \"$(cat \"$1\")\" > \"$1\"\n",
    );
    fx.write("f3.txt", "three, edited\n");

    let mut o = opts(MoveVerb::Absorb, None, None);
    o.message = Some("c3, reworded".into());
    let report = moved(&fx, &o);
    assert_eq!(
        fx.git(&["log", "-1", "--format=%B", "feat"]).trim(),
        "c3, reworded [hooked]",
        "the target's message went through commit-msg"
    );
    assert_eq!(
        file_at(&fx, "feat", "f3.txt").as_deref(),
        Some("three, edited\n")
    );
    assert!(report.into.new.is_some());
    assert_eq!(
        report.into.subject.as_deref(),
        Some("c3, reworded [hooked]"),
        "the report names the target by its new subject"
    );

    // A declining hook refuses before anything is planned.
    install_hook(&fx, "commit-msg", "#!/bin/sh\nexit 1\n");
    fx.write("f3.txt", "three, edited again\n");
    let tip = rev(&fx, "feat");
    let err = move_result(&fx, &o).unwrap_err();
    assert_eq!(err.id(), "hook/declined", "{err}");
    assert_eq!(rev(&fx, "feat"), tip, "nothing moved");

    // `--no-verify` skips it, and a target higher up takes the message too.
    o.verify = ff_core::Verify::Skip;
    o.message = Some("c3, again".into());
    moved(&fx, &o);
    assert_eq!(
        fx.git(&["log", "-1", "--format=%s", "feat"]).trim(),
        "c3, again"
    );
}

#[test]
fn dash_m_describes_the_open_change() {
    let fx = Fixture::new();
    ident(&fx);
    let [_c0, _c1, c2, _c3] = run_stack(&fx);

    let mut o = opts(MoveVerb::Lift, None, None);
    o.message = Some("the lifted work\n".into());
    let report = moved(&fx, &o);
    assert_eq!(report.into.id, "@");
    assert_eq!(rev(&fx, "feat"), c2);

    let meta = ff_core::branchmeta::read(&fx.repo(), "feat").unwrap();
    assert_eq!(
        meta.pending_description.as_deref(),
        Some("the lifted work"),
        "the message is the open change's pending description"
    );
    assert!(
        meta.change_id.is_some(),
        "a described change has an identity"
    );

    let record = tip_record(&fx.repo());
    let description = record.description.expect("the description is journaled");
    assert_eq!(description.new.as_deref(), Some("the lifted work"));
    assert_eq!(description.old, None);
    assert!(record.change_id.is_some(), "and the mint with it");

    // One undo takes the description and the mint back with the move.
    let repo = fx.repo();
    ff_core::undo(
        &repo,
        &ff_core::RewindOptions {
            force: false,
            now: Some(NOW + 100),
            argv: vec!["ff".into(), "undo".into()],
        },
        &prov(),
    )
    .unwrap();
    let meta = ff_core::branchmeta::read(&repo, "feat").unwrap();
    assert_eq!(meta.pending_description, None);
    assert_eq!(meta.change_id, None);
}

#[test]
fn pre_commit_runs_only_when_the_open_change_is_a_source() {
    let fx = Fixture::new();
    ident(&fx);
    let [_c0, c1, c2, c3] = run_stack(&fx);
    install_hook(&fx, "pre-commit", "#!/bin/sh\nexit 1\n");

    // Closed sources: nothing on disk becomes commit content, so the gate
    // is not asked.
    let report = moved(
        &fx,
        &opts(MoveVerb::Absorb, Some(vec![commit(&c3)]), Some(commit(&c2))),
    );
    assert_eq!(report.into.id, c2);
    let tip = rev(&fx, "feat");

    // The open change among the sources: it is.
    fx.write("f1.txt", "one, edited\n");
    let err = move_result(&fx, &opts(MoveVerb::Absorb, None, Some(commit(&c1)))).unwrap_err();
    assert_eq!(err.id(), "hook/declined", "{err}");
    assert_eq!(rev(&fx, "feat"), tip);
}

#[test]
fn the_two_spellings_land_the_same_tree_and_say_so() {
    let fx_a = Fixture::new();
    ident(&fx_a);
    let [_c0, c1, c2, _c3] = run_stack(&fx_a);
    let fx_l = Fixture::new();
    ident(&fx_l);
    let [_c0, c1_l, c2_l, _c3] = run_stack(&fx_l);
    assert_eq!(c1, c1_l);

    let absorb = moved(
        &fx_a,
        &opts(MoveVerb::Absorb, Some(vec![commit(&c2)]), Some(commit(&c1))),
    );
    let lift = moved(
        &fx_l,
        &opts(
            MoveVerb::Lift,
            Some(vec![commit(&c2_l)]),
            Some(commit(&c1_l)),
        ),
    );

    assert_eq!(
        rev(&fx_a, "feat"),
        rev(&fx_l, "feat"),
        "the same commits, byte for byte"
    );
    assert_eq!(absorb.verb, "absorb");
    assert_eq!(lift.verb, "lift");
    let absorb = ff_core::MoveReport {
        verb: "lift".into(),
        ..absorb
    };
    assert_eq!(absorb, lift, "the same report, modulo the spelling");
}

#[test]
fn a_conflicting_upper_fold_holds_at_the_target() {
    let fx = Fixture::new();
    ident(&fx);
    fx.write("f0.txt", "base\n");
    fx.commit("base");
    fx.git(&["switch", "-q", "-c", "feat"]);
    fx.write("f.txt", "a\n");
    let t = fx.commit("target");
    fx.write("f.txt", "b\n");
    let m = fx.commit("middle");
    fx.write("f.txt", "c\n");
    let b = fx.commit("above");
    let before = rev(&fx, "feat");

    // `above` folds into `target` across `middle`, and all three rewrote
    // the same line: the fold above the target conflicts, and the move
    // holds there — at the target, since the open change is not a source.
    let (outcome, _ctx) = move_call(
        &fx,
        &opts(MoveVerb::Lift, Some(vec![commit(&b)]), Some(commit(&t))),
    );
    let report = match outcome {
        ff_core::MoveOutcome::Held(r) => r,
        other => panic!("a conflicting fold must hold, got {other:?}"),
    };
    assert_eq!(report.verb, "lift");
    assert_eq!(
        report.at,
        ff_core::futures::At::Commit {
            id: t.clone(),
            subject: "target".into()
        }
    );
    assert_eq!(report.paths, vec!["f.txt".to_string()]);
    assert_eq!(rev(&fx, "feat"), before, "a hold moves no ref");

    let held = ff_core::held::of(&fx.repo(), "feat")
        .unwrap()
        .expect("held");
    match &held.intent {
        ff_core::held::Intent::Lift { from, into, .. } => {
            assert_eq!(from, &vec![b.clone()]);
            assert_eq!(into, &t);
        }
        other => panic!("the intent must be Lift, got {other:?}"),
    }
    let _ = m;
}

#[test]
fn undo_of_a_run_move_restores_every_source() {
    let fx = Fixture::new();
    ident(&fx);
    let [_c0, c1, c2, c3] = run_stack(&fx);

    moved(
        &fx,
        &opts(MoveVerb::Absorb, Some(vec![commit(&c2), commit(&c3)]), None),
    );
    assert_eq!(fx.git(&["rev-list", "--count", "feat"]).trim(), "2");

    let repo = fx.repo();
    ff_core::undo(
        &repo,
        &ff_core::RewindOptions {
            force: false,
            now: Some(NOW + 100),
            argv: vec!["ff".into(), "undo".into()],
        },
        &prov(),
    )
    .unwrap();
    drop(repo);
    assert_eq!(rev(&fx, "feat"), c3, "the tip is back");
    assert_eq!(rev(&fx, "feat~1"), c2);
    assert_eq!(rev(&fx, "feat~2"), c1);
    assert_eq!(fx.git(&["status", "--porcelain"]).trim(), "");
}
