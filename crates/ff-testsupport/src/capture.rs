//! The capture differential contract: fufu's natively assembled capture tree
//! must equal what real git produces with the hermetic reference recipe
//! `GIT_INDEX_FILE=<tmp> read-tree HEAD && add -A && write-tree` — and the
//! real `.git/index` must stay byte-identical around every capture.
//!
//! Everything here reads the repository through real `git` only. That is the
//! point of the file: the assertions must not be able to agree with fufu by
//! sharing its code.

use std::path::Path;

use crate::fixtures::{Fixture, index_bytes_at};

pub const EMPTY_TREE: &str = "4b825dc642cb6eb9a060e54bf8d69288fbee4904";

/// The one operation log.
pub const OPS_REF: &str = "refs/fufu/ops";

/// The branch pointer fufu should be moving for `dir`'s HEAD state, derived
/// independently via real git.
pub fn chain_ref_via_git(fx: &Fixture, dir: &Path) -> String {
    let sym = fx.try_git_in(dir, &["symbolic-ref", "-q", "HEAD"]);
    if sym.status.success() {
        let full = String::from_utf8(sym.stdout).expect("utf-8 ref");
        let name = full
            .trim()
            .strip_prefix("refs/heads/")
            .expect("HEAD points at a branch")
            .to_string();
        format!("refs/fufu/snap/{name}")
    } else {
        "refs/fufu/snap/@detached".to_string()
    }
}

/// The reference snapshot tree, built by real git in a throwaway index.
/// `excludes` are worktree paths to leave out (mirrors `fufu.maxFileSize`
/// skips): `add -A -- . ':(exclude)<path>'…`.
pub fn git_capture_tree(fx: &Fixture, dir: &Path, excludes: &[&str]) -> String {
    // The throwaway index lives outside the worktree so `add -A` can't see it.
    let index_dir = tempfile::TempDir::new().expect("create temp index dir");
    let index_path = index_dir.path().join("index");
    let idx = index_path.to_str().expect("utf-8 temp index path");
    let env: &[(&str, &str)] = &[("GIT_INDEX_FILE", idx)];

    let head_exists = fx
        .try_git_in(dir, &["rev-parse", "--verify", "--quiet", "HEAD"])
        .status
        .success();
    if head_exists {
        fx.git_env_in(dir, &["read-tree", "HEAD"], env);
    } else {
        fx.git_env_in(dir, &["read-tree", "--empty"], env);
    }

    let mut add: Vec<String> = vec!["add".into(), "-A".into()];
    if !excludes.is_empty() {
        add.push("--".into());
        add.push(".".into());
        for path in excludes {
            add.push(format!(":(exclude){path}"));
        }
    }
    let add_args: Vec<&str> = add.iter().map(String::as_str).collect();
    fx.git_env_in(dir, &add_args, env);

    fx.git_env_in(dir, &["write-tree"], env).trim().to_string()
}

struct RawCommit {
    tree: String,
    parents: Vec<String>,
    author: String,
    committer: String,
    message: String,
}

fn cat_commit(fx: &Fixture, dir: &Path, id: &str) -> RawCommit {
    let raw = fx.git_in(dir, &["cat-file", "commit", id]);
    let mut tree = String::new();
    let mut parents = Vec::new();
    let mut author = String::new();
    let mut committer = String::new();
    let mut in_body = false;
    let mut message = String::new();
    for line in raw.lines() {
        if in_body {
            message.push_str(line);
            message.push('\n');
            continue;
        }
        if line.is_empty() {
            in_body = true;
            continue;
        }
        if let Some(rest) = line.strip_prefix("tree ") {
            tree = rest.to_string();
        } else if let Some(rest) = line.strip_prefix("parent ") {
            parents.push(rest.to_string());
        } else if let Some(rest) = line.strip_prefix("author ") {
            author = rest.to_string();
        } else if let Some(rest) = line.strip_prefix("committer ") {
            committer = rest.to_string();
        }
    }
    RawCommit {
        tree,
        parents,
        author,
        committer,
        message,
    }
}

fn is_fufu(commit: &RawCommit) -> bool {
    commit.author.starts_with("fufu <fufu@local> ")
        && commit.committer.starts_with("fufu <fufu@local> ")
}

/// The guard `ops::is_op_commit` applies, restated in terms of what `git
/// cat-file` shows: fufu's identity AND a `fufu-kind` trailer. The identity
/// alone is not enough — a record commit bears it too, and it hangs off every
/// verb operation.
fn is_op(commit: &RawCommit) -> bool {
    is_fufu(commit) && commit.message.lines().any(|l| l.starts_with("fufu-kind: "))
}

/// Read the operation log through real git only: first-parent walk from
/// `refs/fufu/ops` while commits bear the fufu identity. Newest first; empty
/// when no log exists.
///
/// First-parent is the log and nothing else — that is the shape's whole
/// promise, and the reason this helper can be this short. The journal's slot 1
/// held a pin on its first entry, so the same two lines against it ran off the
/// root and into the user's own history.
pub fn chain_via_git(fx: &Fixture, dir: &Path) -> Vec<String> {
    let tip = fx.try_git_in(dir, &["rev-parse", "--verify", "--quiet", OPS_REF]);
    if !tip.status.success() {
        return Vec::new();
    }
    let mut cur = String::from_utf8(tip.stdout)
        .expect("utf-8 sha")
        .trim()
        .to_string();
    let mut out = Vec::new();
    loop {
        let commit = cat_commit(fx, dir, &cur);
        if !is_op(&commit) {
            break;
        }
        out.push(cur.clone());
        match commit.parents.first() {
            Some(parent) => cur = parent.clone(),
            None => break,
        }
    }
    out
}

/// The same walk, with the log's floor dropped: what a caller means when it
/// says "the captures I took". The floor is the `init` note reconciliation
/// lays down before anything else, and it is on every log.
pub fn captures_via_git(fx: &Fixture, dir: &Path) -> Vec<String> {
    let mut ids = chain_via_git(fx, dir);
    ids.pop();
    ids
}

/// The core capture assertion: run a native capture, then check
/// - `.git/index` stayed byte-identical,
/// - `Created` ⇒ the capture tree equals the reference recipe's tree, BOTH
///   refs moved to it, and the commit has fufu's identity and the right
///   parent order (previous operation first, HEAD second),
/// - `NoOp` ⇒ the reference recipe agrees nothing new would be recorded.
///
/// The parent-order assertion carries over from the snapshot chain unchanged,
/// and that is by construction rather than by luck: a capture's parents are
/// `[prev, base]` and it has no record commit, so base still sits at slot 2.
pub fn assert_snapshot_matches(fx: &Fixture) {
    assert_snapshot_matches_at(fx, &fx.path());
}

pub fn assert_snapshot_matches_at(fx: &Fixture, dir: &Path) {
    let before = index_bytes_at(dir);
    let chain_ref = chain_ref_via_git(fx, dir);
    let rev = |name: &str| -> Option<String> {
        let out = fx.try_git_in(dir, &["rev-parse", "--verify", "--quiet", name]);
        out.status
            .success()
            .then(|| String::from_utf8(out.stdout).unwrap().trim().to_string())
    };
    // Parent 1 is the previous operation anywhere on the log, and the branch
    // pointer is a pointer INTO that log — so the two are read separately, and
    // it is the log's tip that the parent slot has to match.
    let prev_log_tip: Option<String> = rev(OPS_REF);
    let prev_tip: Option<String> = rev(&chain_ref);
    let head: Option<String> = rev("HEAD");

    let repo = ff_core::discover_isolated(dir).expect("discover repo");
    let outcome = ff_core::capture_with(
        &repo,
        &ff_core::Provenance::new("manual", None),
        &ff_core::TakeOptions::default(),
    );
    drop(repo);
    let after = index_bytes_at(dir);
    assert_eq!(
        before, after,
        "capture must leave .git/index byte-identical"
    );
    let outcome = outcome.expect("capture");

    let reference = git_capture_tree(fx, dir, &[]);
    match outcome {
        ff_core::CaptureOutcome::Created { id, .. } => {
            let id = id.hex();
            let log_tip = fx.git_in(dir, &["rev-parse", OPS_REF]).trim().to_string();
            assert_eq!(log_tip, id, "the log must point at the new operation");
            let tip = fx
                .git_in(dir, &["rev-parse", &chain_ref])
                .trim()
                .to_string();
            assert_eq!(tip, id, "the branch pointer must move with the log");

            let commit = cat_commit(fx, dir, &id);
            if commit.tree != reference {
                let diff = fx.git_in(dir, &["diff-tree", "-r", &reference, &commit.tree]);
                panic!(
                    "capture tree diverges from `read-tree HEAD && add -A && write-tree`\n\
                     reference {reference} vs native {}\n{diff}",
                    commit.tree
                );
            }
            assert!(
                is_fufu(&commit),
                "a capture must be authored and committed by fufu <fufu@local>: \
                 author={:?} committer={:?}",
                commit.author,
                commit.committer
            );
            // Slot 1 is the operation this one follows. Usually that is the
            // log tip read a moment ago — but the first capture in a
            // repository also lays the log's floor, and then the floor is what
            // it follows. Reading it back off the walk covers both without
            // asking fufu what it thinks it wrote.
            let walked = chain_via_git(fx, dir);
            assert_eq!(walked.first().map(String::as_str), Some(id.as_str()));
            let prev = walked.get(1).cloned();
            if let Some(before) = &prev_log_tip {
                assert_eq!(
                    prev.as_deref(),
                    Some(before.as_str()),
                    "slot 1 must be the operation the log was already on"
                );
            }
            let expected_parents: Vec<String> =
                [prev.clone(), head.clone()].into_iter().flatten().collect();
            assert_eq!(
                commit.parents, expected_parents,
                "parent order must be [previous operation, HEAD]"
            );
        }
        ff_core::CaptureOutcome::NoOp { tip, .. } => {
            let tip = tip.map(|id| id.hex());
            assert_eq!(tip, prev_tip, "NoOp must report the existing branch tip");
            let expected_tree = match &prev_tip {
                Some(p) => fx
                    .git_in(dir, &["rev-parse", &format!("{p}^{{tree}}")])
                    .trim()
                    .to_string(),
                None => match &head {
                    Some(h) => fx
                        .git_in(dir, &["rev-parse", &format!("{h}^{{tree}}")])
                        .trim()
                        .to_string(),
                    None => EMPTY_TREE.to_string(),
                },
            };
            assert_eq!(
                reference, expected_tree,
                "NoOp but the reference recipe found new content to record"
            );
        }
        ff_core::CaptureOutcome::Contended => {
            panic!("unexpected contention in a single-threaded test")
        }
    }
}
