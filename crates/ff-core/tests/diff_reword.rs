//! Differential contract for `describe::reword`: fufu's rewrite must produce
//! byte-identical commits to `git rebase -i --update-refs` with a `reword`.
//! Trees, author identity, and (for untouched commits) committer identity all
//! round-trip; the committer is refreshed to `now` on every rewritten
//! commit, exactly as a real rebase refreshes it; merge commits are
//! re-parented rather than refused; and the write-ahead operation record
//! carries the whole rewrite map so undo is a true inverse.

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
    ff_core::Provenance::new("pre", Some("ff describe".into()))
}

fn oid(hex: &str) -> gix::ObjectId {
    gix::ObjectId::from_hex(hex.trim().as_bytes()).unwrap()
}

fn reword(
    fx: &Fixture,
    target: &str,
    message: &str,
    now: i64,
) -> (ff_core::RewordReport, ff_core::ops::VerbContext) {
    let repo = fx.repo();
    ff_core::describe::reword(
        &repo,
        oid(target),
        message.to_string(),
        ff_core::Verify::Run,
        &prov(),
        Some(now),
        vec!["ff".into(), "describe".into()],
    )
    .unwrap()
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

/// Four commits on `main`, `mid` created at `c2`, `other` at `c3`. Nobody
/// switches branches, so `main` stays HEAD throughout.
fn stack(fx: &Fixture) -> [String; 4] {
    fx.write("f1.txt", "one\n");
    let c1 = fx.commit("c1");
    fx.write("f2.txt", "two\n");
    let c2 = fx.commit("c2");
    fx.git(&["branch", "mid"]);
    fx.write("f3.txt", "three\n");
    let c3 = fx.commit("c3");
    fx.git(&["branch", "other"]);
    fx.write("f4.txt", "four\n");
    let c4 = fx.commit("c4");
    [c1, c2, c3, c4]
}

/// Pure-POSIX in-place first-line edit of git's own rebase todo: `pick` →
/// `reword`. No `sed -i` — its flag is not portable across the CI matrix.
/// Editing the todo in place (rather than replacing it) is what keeps
/// `--update-refs` working: git only moves branches for `update-ref` lines it
/// generated itself.
const SEQ_SCRIPT: &str = r#"f="$1"
{ read -r first; printf 'reword%s\n' "${first#pick}"; cat; } < "$f" > "$f.ff"
mv "$f.ff" "$f"
"#;

/// Run the git oracle: `git rebase -i --update-refs` with a `reword` on
/// `target`, using the same new message and committer date fufu's side used.
/// Verified end-to-end against git 2.50.1: trees byte-identical, author
/// preserved per commit, committer refreshed to `now` on every rewritten
/// commit and untouched below the target, no extra headers, every local head
/// inside the range moved.
fn git_oracle(fx: &Fixture, target: &str, message: &str, now: i64) {
    let upstream = fx
        .git(&["rev-parse", &format!("{target}^")])
        .trim()
        .to_string();
    std::fs::write(fx.path().join(".git/ff_seq.sh"), SEQ_SCRIPT).unwrap();
    std::fs::write(fx.path().join(".git/FF_MSG"), format!("{message}\n")).unwrap();
    fx.git_env_in(
        &fx.path(),
        &["rebase", "-i", "--update-refs", &upstream],
        &[
            ("GIT_SEQUENCE_EDITOR", "sh .git/ff_seq.sh"),
            ("GIT_EDITOR", "cp .git/FF_MSG"),
            ("GIT_COMMITTER_DATE", &format!("@{now} +0000")),
        ],
    );
}

/// The user-visible history only — `main`, `mid`, `other` — not fufu's own
/// operation log, which only one side of the comparison ever writes.
fn full_log(fx: &Fixture) -> String {
    fx.git(&[
        "log",
        "--format=%H|%T|%an|%ad|%cn|%cd|%s",
        "--date=raw",
        "main",
        "mid",
        "other",
    ])
}

#[test]
fn reword_of_a_mid_stack_commit_matches_git_rebase_reword() {
    let fx_ff = Fixture::new();
    ident(&fx_ff);
    let [c1_ff, c2_ff, _c3_ff, _c4_ff] = stack(&fx_ff);

    let fx_git = Fixture::new();
    ident(&fx_git);
    let [c1_git, c2_git, _c3_git, _c4_git] = stack(&fx_git);

    assert_eq!(c1_ff, c1_git, "setup must be lockstep before any rewrite");
    assert_eq!(c2_ff, c2_git, "setup must be lockstep before any rewrite");

    let (report, _ctx) = reword(&fx_ff, &c2_ff, "c2 reworded", NOW);
    git_oracle(&fx_git, &c2_git, "c2 reworded", NOW);

    for branch in ["main", "mid", "other"] {
        let ff_sha = fx_ff.git(&["rev-parse", branch]).trim().to_string();
        let git_sha = fx_git.git(&["rev-parse", branch]).trim().to_string();
        assert_eq!(ff_sha, git_sha, "{branch} diverged between fufu and git");
    }

    assert!(
        fx_ff.try_git(&["cat-file", "-e", &c1_ff]).status.success(),
        "c1 must survive unchanged on the fufu side"
    );
    assert!(
        fx_git
            .try_git(&["cat-file", "-e", &c1_git])
            .status
            .success(),
        "c1 must survive unchanged on the git side"
    );

    let log_ff = full_log(&fx_ff);
    let log_git = full_log(&fx_git);
    assert_eq!(
        log_ff, log_git,
        "full log must be identical\nfufu:\n{log_ff}\ngit:\n{log_git}"
    );

    let new_mid = fx_ff.git(&["rev-parse", "mid"]).trim().to_string();
    assert_eq!(report.restacked, 2);
    assert_eq!(report.moved, vec!["mid".to_string(), "other".to_string()]);
    assert_eq!(report.subject, "c2 reworded");
    assert_eq!(report.published, 0);
    assert_eq!(report.old, c2_ff);
    assert_eq!(report.new, new_mid);
}

#[test]
fn a_reword_leaves_the_index_and_worktree_untouched() {
    let fx = Fixture::new();
    ident(&fx);
    fx.write("a.txt", "a\n");
    let _c1 = fx.commit("c1");
    fx.write("b.txt", "b\n");
    let c2 = fx.commit("c2");
    fx.write("c.txt", "c\n");
    let _c3 = fx.commit("c3");

    fx.backdate();
    let index_before = fx.index_bytes();
    let tree_before = fx.git(&["rev-parse", "HEAD^{tree}"]).trim().to_string();
    let files_before: Vec<(&str, String)> = ["a.txt", "b.txt", "c.txt"]
        .iter()
        .map(|f| (*f, std::fs::read_to_string(fx.path().join(f)).unwrap()))
        .collect();

    reword(&fx, &c2, "c2 reworded", NOW);

    let index_after = fx.index_bytes();
    assert_eq!(
        index_before, index_after,
        "reword must leave .git/index byte-identical"
    );
    let tree_after = fx.git(&["rev-parse", "HEAD^{tree}"]).trim().to_string();
    assert_eq!(tree_before, tree_after, "the tip's tree must be unchanged");
    for (name, contents_before) in &files_before {
        let contents_after = std::fs::read_to_string(fx.path().join(name)).unwrap();
        assert_eq!(*contents_before, contents_after, "{name} must be unchanged");
    }
}

#[test]
fn undo_restores_every_moved_ref_and_the_old_commits_survive() {
    let fx = Fixture::new();
    ident(&fx);
    let [_c1, c2, _c3, _c4] = stack(&fx);

    let main_before = fx.git(&["rev-parse", "main"]).trim().to_string();
    let mid_before = fx.git(&["rev-parse", "mid"]).trim().to_string();
    let other_before = fx.git(&["rev-parse", "other"]).trim().to_string();

    reword(&fx, &c2, "c2 reworded", NOW);

    let record = tip_record(&fx.repo());
    assert_eq!(record.verb, "describe");
    let rewrites = record.rewrites.clone();
    assert_eq!(rewrites.len(), 3, "target plus its two descendants");

    let opts = ff_core::RewindOptions {
        force: false,
        now: Some(NOW + 100),
        argv: vec!["ff".into(), "undo".into()],
    };
    ff_core::undo(&fx.repo(), &opts, &prov()).unwrap();

    assert_eq!(fx.git(&["rev-parse", "main"]).trim(), main_before);
    assert_eq!(fx.git(&["rev-parse", "mid"]).trim(), mid_before);
    assert_eq!(fx.git(&["rev-parse", "other"]).trim(), other_before);

    for r in &rewrites {
        assert!(
            fx.try_git(&["cat-file", "-e", &r.old]).status.success(),
            "old commit {} must remain reachable after undo",
            r.old
        );
        assert!(
            fx.try_git(&["cat-file", "-e", &r.new]).status.success(),
            "new commit {} must remain reachable after undo",
            r.new
        );
    }
}

#[test]
fn the_operation_record_carries_the_rewrite_map() {
    let fx = Fixture::new();
    ident(&fx);
    let [_c1, c2, _c3, _c4] = stack(&fx);

    reword(&fx, &c2, "c2 reworded", NOW);

    let record = tip_record(&fx.repo());
    assert_eq!(record.verb, "describe");
    assert_eq!(record.rewrites.len(), 3);
    assert_eq!(record.rewrites[0].old, c2);
    for i in 1..record.rewrites.len() {
        let parent = fx
            .git(&["rev-parse", &format!("{}^", record.rewrites[i].old)])
            .trim()
            .to_string();
        assert_eq!(
            parent,
            record.rewrites[i - 1].old,
            "rewrite {i}'s old sha must be the previous entry's child"
        );
    }

    let names: Vec<&str> = record.refs.iter().map(|t| t.name.as_str()).collect();
    assert!(names.contains(&"refs/heads/main"));
    assert!(names.contains(&"refs/heads/mid"));
    assert!(names.contains(&"refs/heads/other"));

    let bare = ff_core::ops::OpRecord::new("describe", "test", NOW);
    let json = serde_json::to_string(&bare).unwrap();
    assert!(
        !json.contains("rewrites"),
        "an empty rewrite map must be omitted from the record: {json}"
    );
    let round_tripped: ff_core::ops::OpRecord = serde_json::from_str(&json).unwrap();
    assert_eq!(round_tripped.rewrites, Vec::new());
}

#[test]
fn rewording_a_commit_that_is_not_in_history_is_refused() {
    let fx = Fixture::new();
    ident(&fx);
    fx.write("base.txt", "base\n");
    let _base = fx.commit("base");
    fx.git(&["branch", "other"]);
    fx.write("main.txt", "main\n");
    let _main_only = fx.commit("main only");
    fx.git(&["switch", "other"]);
    fx.write("other.txt", "other\n");
    let other_commit = fx.commit("other only");
    fx.git(&["switch", "main"]);

    // Prime the op log so a refusal's "nothing moved" claim is meaningful:
    // the very first fufu call on a fixture always bootstraps the log from
    // observed state, which is not itself evidence of a mutation.
    ff_core::ops::reconcile(&fx.repo(), NOW).unwrap();
    let tip_before = ff_core::ops::OpLog::open(&fx.repo())
        .unwrap()
        .tip()
        .unwrap();

    let main_before = fx.git(&["rev-parse", "main"]).trim().to_string();
    let other_before = fx.git(&["rev-parse", "other"]).trim().to_string();

    let repo = fx.repo();
    let err = ff_core::describe::reword(
        &repo,
        oid(&other_commit),
        "nope".into(),
        ff_core::Verify::Run,
        &prov(),
        Some(NOW),
        vec!["ff".into(), "describe".into()],
    )
    .unwrap_err();

    assert_eq!(err.id(), "rewrite/not-in-history");
    assert_eq!(fx.git(&["rev-parse", "main"]).trim(), main_before);
    assert_eq!(fx.git(&["rev-parse", "other"]).trim(), other_before);
    let tip_after = ff_core::ops::OpLog::open(&fx.repo())
        .unwrap()
        .tip()
        .unwrap();
    assert_eq!(
        tip_before, tip_after,
        "the operation log must not move on a refusal"
    );
}

#[test]
fn a_merge_commit_in_the_range_is_re_parented_not_refused() {
    let fx = Fixture::new();
    ident(&fx);
    fx.write("pre.txt", "pre\n");
    let pre = fx.commit("pre");
    fx.git(&["branch", "side"]);
    fx.write("target.txt", "target\n");
    let target = fx.commit("target");
    fx.git(&["switch", "side"]);
    fx.write("side.txt", "side\n");
    let side = fx.commit("side");
    fx.git(&["switch", "main"]);
    fx.git(&["merge", "--no-ff", "-m", "merge side", "side"]);
    let old_merge = fx.git(&["rev-parse", "HEAD"]).trim().to_string();
    fx.write("top.txt", "top\n");
    let _top = fx.commit("top");
    let _ = pre;

    let (report, _ctx) = reword(&fx, &target, "target reworded", NOW);

    let new_main_tip = fx.git(&["rev-parse", "main"]).trim().to_string();
    let new_merge = fx
        .git(&["rev-parse", &format!("{new_main_tip}^")])
        .trim()
        .to_string();
    let new_merge_p1 = fx
        .git(&["rev-parse", &format!("{new_merge}^1")])
        .trim()
        .to_string();
    let new_merge_p2 = fx
        .git(&["rev-parse", &format!("{new_merge}^2")])
        .trim()
        .to_string();

    assert_eq!(
        new_merge_p1, report.new,
        "the merge's first parent must be the rewritten mainline commit"
    );
    assert_eq!(
        new_merge_p2, side,
        "the merge's second parent is outside the affected set and must be unchanged"
    );

    let old_merge_tree = fx
        .git(&["rev-parse", &format!("{old_merge}^{{tree}}")])
        .trim()
        .to_string();
    let new_merge_tree = fx
        .git(&["rev-parse", &format!("{new_merge}^{{tree}}")])
        .trim()
        .to_string();
    assert_eq!(
        old_merge_tree, new_merge_tree,
        "the merge's tree must be unchanged"
    );
}

#[test]
fn rewording_to_the_same_message_changes_nothing() {
    let fx = Fixture::new();
    ident(&fx);
    fx.write("a.txt", "a\n");
    let c1 = fx.commit("c1");
    let c1_time: i64 = fx
        .git(&["log", "-1", "--format=%ct", &c1])
        .trim()
        .parse()
        .unwrap();

    // For the rewrite to land on the exact same sha, the committer signature
    // fufu computes must match the original commit's exactly — same
    // identity (fixed by `ident`) and same timestamp.
    let (report, _ctx) = reword(&fx, &c1, "c1", c1_time);

    assert_eq!(report.old, c1);
    assert_eq!(report.new, c1);
    assert_eq!(report.restacked, 0);
    assert!(report.moved.is_empty());

    // No new operation was appended: the tip's record is not a reword
    // (verb `describe` carrying a non-empty rewrite map), whatever `begin_verb`
    // left behind on the way in (a bootstrap note or a no-op capture).
    let record = tip_record(&fx.repo());
    assert!(
        !(record.verb == "describe" && !record.rewrites.is_empty()),
        "a no-op reword must not append a new operation"
    );
}
