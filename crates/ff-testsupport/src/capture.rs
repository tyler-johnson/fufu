//! The snapshot differential contract: fufu's natively assembled capture tree
//! must equal what real git produces with the hermetic reference recipe
//! `GIT_INDEX_FILE=<tmp> read-tree HEAD && add -A && write-tree` — and the
//! real `.git/index` must stay byte-identical around every capture.

use std::path::Path;

use crate::fixtures::{Fixture, index_bytes_at};

pub const EMPTY_TREE: &str = "4b825dc642cb6eb9a060e54bf8d69288fbee4904";

/// The chain ref fufu should be using for `dir`'s HEAD state, derived
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
}

fn cat_commit(fx: &Fixture, dir: &Path, id: &str) -> RawCommit {
    let raw = fx.git_in(dir, &["cat-file", "commit", id]);
    let mut tree = String::new();
    let mut parents = Vec::new();
    let mut author = String::new();
    let mut committer = String::new();
    for line in raw.lines() {
        if line.is_empty() {
            break;
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
    }
}

fn is_fufu(commit: &RawCommit) -> bool {
    commit.author.starts_with("fufu <fufu@local> ")
        && commit.committer.starts_with("fufu <fufu@local> ")
}

/// Read the current branch's snapshot chain through real git only:
/// first-parent walk from the chain ref while commits bear the fufu identity.
/// Newest first; empty when no chain exists.
pub fn chain_via_git(fx: &Fixture, dir: &Path) -> Vec<String> {
    let chain_ref = chain_ref_via_git(fx, dir);
    let tip = fx.try_git_in(dir, &["rev-parse", "--verify", "--quiet", &chain_ref]);
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
        if !is_fufu(&commit) {
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

/// The core snapshot assertion: run a native capture, then check
/// - `.git/index` stayed byte-identical,
/// - `Created` ⇒ the snapshot tree equals the reference recipe's tree, the
///   ref moved to it, and the commit has fufu's identity and the right
///   parent order (prev snapshot first, HEAD second),
/// - `NoOp` ⇒ the reference recipe agrees nothing new would be recorded.
pub fn assert_snapshot_matches(fx: &Fixture) {
    assert_snapshot_matches_at(fx, &fx.path());
}

pub fn assert_snapshot_matches_at(fx: &Fixture, dir: &Path) {
    let before = index_bytes_at(dir);
    let chain_ref = chain_ref_via_git(fx, dir);
    let prev_tip: Option<String> = {
        let out = fx.try_git_in(dir, &["rev-parse", "--verify", "--quiet", &chain_ref]);
        out.status
            .success()
            .then(|| String::from_utf8(out.stdout).unwrap().trim().to_string())
    };
    let head: Option<String> = {
        let out = fx.try_git_in(dir, &["rev-parse", "--verify", "--quiet", "HEAD"]);
        out.status
            .success()
            .then(|| String::from_utf8(out.stdout).unwrap().trim().to_string())
    };

    let repo = ff_core::discover_isolated(dir).expect("discover repo");
    let outcome = ff_core::take_with(
        &repo,
        &ff_core::Provenance::new("manual", None),
        &ff_core::TakeOptions::default(),
    );
    drop(repo);
    let after = index_bytes_at(dir);
    assert_eq!(before, after, "take must leave .git/index byte-identical");
    let outcome = outcome.expect("take");

    let reference = git_capture_tree(fx, dir, &[]);
    match outcome {
        ff_core::SnapOutcome::Created { id, r#ref, .. } => {
            assert_eq!(r#ref, chain_ref, "snapshot went to the wrong chain");
            let tip = fx
                .git_in(dir, &["rev-parse", &chain_ref])
                .trim()
                .to_string();
            assert_eq!(tip, id, "chain ref must point at the new snapshot");

            let commit = cat_commit(fx, dir, &id);
            if commit.tree != reference {
                let diff = fx.git_in(dir, &["diff-tree", "-r", &reference, &commit.tree]);
                panic!(
                    "snapshot tree diverges from `read-tree HEAD && add -A && write-tree`\n\
                     reference {reference} vs native {}\n{diff}",
                    commit.tree
                );
            }
            assert!(
                is_fufu(&commit),
                "snapshot must be authored and committed by fufu <fufu@local>: \
                 author={:?} committer={:?}",
                commit.author,
                commit.committer
            );
            let expected_parents: Vec<String> = [prev_tip.clone(), head.clone()]
                .into_iter()
                .flatten()
                .collect();
            assert_eq!(
                commit.parents, expected_parents,
                "parent order must be [prev snapshot, HEAD]"
            );
        }
        ff_core::SnapOutcome::NoOp { tip, .. } => {
            assert_eq!(tip, prev_tip, "NoOp must report the existing tip");
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
        ff_core::SnapOutcome::Contended { .. } => {
            panic!("unexpected contention in a single-threaded test")
        }
    }
}
