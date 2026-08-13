//! The snapshot compatibility contract: for every fixture shape, fufu's
//! natively assembled capture tree must equal real git's
//! `GIT_INDEX_FILE=<tmp> read-tree HEAD && add -A && write-tree`, with
//! `.git/index` byte-identical around the capture.

use ff_testsupport::Fixture;
use ff_testsupport::capture::{
    assert_snapshot_matches, assert_snapshot_matches_at, git_capture_tree,
};
use ff_testsupport::scenarios;

#[test]
fn snapshot_matrix() {
    for (name, build) in scenarios() {
        println!("scenario: {name}");
        let fx = Fixture::new();
        build(&fx);
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            assert_snapshot_matches(&fx);
        }));
        if let Err(err) = result {
            let msg = err
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| err.downcast_ref::<&str>().map(|s| s.to_string()))
                .unwrap_or_else(|| "non-string panic".into());
            panic!("scenario '{name}' failed: {msg}");
        }
    }
}

/// An untracked directory that is itself a git repository becomes a gitlink,
/// exactly as `git add -A` records it.
#[test]
fn embedded_repo_gitlink() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    let sub = fx.path().join("vendor/embedded");
    std::fs::create_dir_all(&sub).unwrap();
    fx.git_in(&sub, &["init", "-q", "-b", "main"]);
    fx.write("vendor/embedded/inner.txt", "inner\n");
    fx.git_in(&sub, &["add", "inner.txt"]);
    fx.git_in(&sub, &["commit", "-qm", "inner"]);
    assert_snapshot_matches(&fx);
}

/// CRLF normalization through a `.gitattributes` `text` rule — including a
/// dirty, not-yet-committed `.gitattributes` that must apply to the capture.
#[test]
fn gitattributes_eol() {
    let fx = Fixture::new();
    fx.write(".gitattributes", "*.txt text\n");
    fx.write("committed.txt", "one\r\ntwo\r\n");
    fx.commit("init");
    fx.write("fresh.txt", "a\r\nb\r\n");
    assert_snapshot_matches(&fx);

    // Now a dirty .gitattributes changes the rules before anything commits.
    let fx = Fixture::new();
    fx.write("keep.bin", "x");
    fx.commit("init");
    fx.write(".gitattributes", "*.crlf text\n");
    fx.write("data.crlf", "a\r\nb\r\n");
    assert_snapshot_matches(&fx);
}

// unix-only: exercises the worktree exec bit, which Windows doesn't have.
#[cfg(unix)]
#[test]
fn executable_bit_new_file() {
    use std::os::unix::fs::PermissionsExt;
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    fx.write("run.sh", "#!/bin/sh\necho hi\n");
    let path = fx.path().join("run.sh");
    let mut perms = std::fs::metadata(&path).unwrap().permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&path, perms).unwrap();
    assert_snapshot_matches(&fx);
}

// unix-only: creating symlinks on Windows needs Developer Mode/privilege.
#[cfg(unix)]
#[test]
fn symlink_untracked() {
    let fx = Fixture::new();
    fx.write("target.txt", "t\n");
    fx.commit("init");
    std::os::unix::fs::symlink("target.txt", fx.path().join("link")).unwrap();
    assert_snapshot_matches(&fx);
}

// unix-only: creating symlinks on Windows needs Developer Mode/privilege.
#[cfg(unix)]
#[test]
fn symlink_replaces_file() {
    let fx = Fixture::new();
    fx.write("target.txt", "t\n");
    fx.write("was-file.txt", "file content\n");
    fx.commit("init");
    fx.remove("was-file.txt");
    std::os::unix::fs::symlink("target.txt", fx.path().join("was-file.txt")).unwrap();
    assert_snapshot_matches(&fx);
}

fn conflicted_merge_fixture() -> Fixture {
    let fx = Fixture::new();
    fx.write("conflict.txt", "base\n");
    fx.commit("base");
    fx.git(&["checkout", "-q", "-b", "other"]);
    fx.write("conflict.txt", "theirs\n");
    fx.commit("theirs");
    fx.git(&["checkout", "-q", "main"]);
    fx.write("conflict.txt", "ours\n");
    fx.commit("ours");
    let out = fx.try_git(&["merge", "other"]);
    assert!(!out.status.success(), "merge should conflict");
    fx
}

/// A conflicted path is captured from its worktree side (the marker file),
/// which is what `add -A` stages.
#[test]
fn conflicted_merge_capture() {
    let fx = conflicted_merge_fixture();
    assert_snapshot_matches(&fx);
}

/// Delete/modify conflict: one side deleted, one side modified.
#[test]
fn conflicted_delete_modify_capture() {
    let fx = Fixture::new();
    fx.write("dm.txt", "base\n");
    fx.commit("base");
    fx.git(&["checkout", "-q", "-b", "other"]);
    fx.git(&["rm", "-q", "dm.txt"]);
    fx.git(&["commit", "-qm", "delete"]);
    fx.git(&["checkout", "-q", "main"]);
    fx.write("dm.txt", "modified\n");
    fx.commit("modify");
    let out = fx.try_git(&["merge", "other"]);
    assert!(!out.status.success(), "merge should conflict");
    assert_snapshot_matches(&fx);
}

/// Add/add conflict: both sides add the same path with different content.
#[test]
fn conflicted_add_add_capture() {
    let fx = Fixture::new();
    fx.write("seed.txt", "seed\n");
    fx.commit("base");
    fx.git(&["checkout", "-q", "-b", "other"]);
    fx.write("aa.txt", "theirs\n");
    fx.commit("theirs adds");
    fx.git(&["checkout", "-q", "main"]);
    fx.write("aa.txt", "ours\n");
    fx.commit("ours adds");
    let out = fx.try_git(&["merge", "other"]);
    assert!(!out.status.success(), "merge should conflict");
    assert_snapshot_matches(&fx);
}

/// A conflicted path deleted from the worktree during the conflict: gone from
/// the snapshot, exactly as `add -A` records the deletion.
#[test]
fn conflicted_worktree_deleted() {
    let fx = conflicted_merge_fixture();
    fx.remove("conflict.txt");
    assert_snapshot_matches(&fx);
}

/// Intent-to-add captures the real worktree content, not the empty
/// placeholder blob the index holds.
#[test]
fn intent_to_add_capture() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    fx.write("later.txt", "real content, not a placeholder\n");
    fx.git(&["add", "-N", "later.txt"]);
    assert_snapshot_matches(&fx);
}

/// `git rm --cached`: staged deletion, file still on disk — the worktree side
/// wins, as `add -A` re-adds it.
#[test]
fn staged_rm_cached() {
    let fx = Fixture::new();
    fx.write("keep.txt", "still here\n");
    fx.commit("init");
    fx.git(&["rm", "-q", "--cached", "keep.txt"]);
    assert_snapshot_matches(&fx);
}

#[test]
fn unborn_untracked() {
    let fx = Fixture::new();
    fx.write("first.txt", "first\n");
    fx.write("deep/nested.txt", "nested\n");
    assert_snapshot_matches(&fx);
}

#[test]
fn detached_capture() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    let first = fx.commit("one");
    fx.write("a.txt", "aa\n");
    fx.commit("two");
    fx.git(&["checkout", "-q", &first]);
    fx.write("a.txt", "detached dirty\n");
    assert_snapshot_matches(&fx);
}

/// Captures in a linked worktree use that worktree's private index and HEAD,
/// and land on that worktree's branch chain.
#[test]
fn linked_worktree() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    let wt = fx.root().join("linked");
    let wt_str = wt.to_str().unwrap();
    fx.git(&["worktree", "add", "-q", "-b", "linked-branch", wt_str]);
    fx.write("a.txt", "main dirty\n");
    std::fs::write(wt.join("a.txt"), "linked dirty\n").unwrap();
    std::fs::write(wt.join("only-linked.txt"), "linked untracked\n").unwrap();
    assert_snapshot_matches_at(&fx, &wt);
    assert_snapshot_matches(&fx);
}

/// Files over `fufu.maxFileSize` are skipped: the tree matches git-with-
/// excludes, and the commit body lists the skips. Already-staged big blobs
/// are captured as-is.
#[test]
fn oversize_excluded() {
    let fx = Fixture::new();
    fx.write("small.txt", "small\n");
    fx.commit("init");
    fx.write("big.bin", &"x".repeat(4096));
    fx.write("also-small.txt", "fine\n");

    let repo = fx.repo();
    let outcome = ff_core::take_with(
        &repo,
        &ff_core::Provenance::new("manual", None),
        &ff_core::TakeOptions {
            now: None,
            max_file_size: Some(1024),
        },
    )
    .expect("take");
    let ff_core::SnapOutcome::Created {
        id, skipped_files, ..
    } = outcome
    else {
        panic!("expected Created, got {outcome:?}");
    };
    assert_eq!(skipped_files, vec!["big.bin".to_string()]);

    let reference = git_capture_tree(&fx, &fx.path(), &["big.bin"]);
    let native_tree = fx
        .git(&["rev-parse", &format!("{id}^{{tree}}")])
        .trim()
        .to_string();
    assert_eq!(native_tree, reference, "tree must match git-with-excludes");

    let body = fx.git(&["log", "-1", "--format=%b", &id]);
    assert!(
        body.contains("Skipped (fufu.maxFileSize):") && body.contains("big.bin"),
        "commit body must list the skip: {body:?}"
    );

    // A user-staged big blob is captured as-is — excluding it would be forgery.
    fx.git(&["add", "big.bin"]);
    let outcome = ff_core::take_with(
        &repo,
        &ff_core::Provenance::new("manual", None),
        &ff_core::TakeOptions {
            now: None,
            max_file_size: Some(1024),
        },
    )
    .expect("take");
    let ff_core::SnapOutcome::Created {
        id, skipped_files, ..
    } = outcome
    else {
        panic!("expected Created, got {outcome:?}");
    };
    assert!(skipped_files.is_empty(), "staged blobs are never skipped");
    let listed = fx.git(&["ls-tree", "-r", "--name-only", &format!("{id}^{{tree}}")]);
    assert!(listed.lines().any(|l| l == "big.bin"));
}

/// A regular sparse checkout (skip-worktree flags, full index) captures
/// faithfully: excluded paths are not deltas, so the HEAD-tree layering
/// preserves them. Note the reference recipe itself can't express this
/// (a throwaway index loses skip-worktree flags), so this asserts directly.
#[test]
fn sparse_checkout_preserves_excluded_paths() {
    let fx = Fixture::new();
    fx.write("keep/a.txt", "a\n");
    fx.write("drop/b.txt", "b\n");
    fx.commit("init");
    fx.git(&["sparse-checkout", "set", "--cone", "keep"]);
    assert!(
        !fx.path().join("drop/b.txt").exists(),
        "cone excluded drop/"
    );
    fx.write("keep/a.txt", "sparse dirty\n");

    let index = fx.index_bytes();
    let repo = fx.repo();
    let outcome = ff_core::take(&repo, &ff_core::Provenance::new("manual", None)).expect("take");
    assert_eq!(index, fx.index_bytes(), "index stays untouched");
    let ff_core::SnapOutcome::Created { id, .. } = outcome else {
        panic!("expected Created, got {outcome:?}");
    };
    let listed = fx.git(&["ls-tree", "-r", "--name-only", &format!("{id}^{{tree}}")]);
    let names: Vec<&str> = listed.lines().collect();
    assert!(
        names.contains(&"drop/b.txt"),
        "sparse-excluded path preserved"
    );
    let blob = fx.git(&["show", &format!("{id}:keep/a.txt")]);
    assert_eq!(blob, "sparse dirty\n", "dirty file captured");
}

/// The sparse *index format* collapses entries to trees; capture cannot read
/// it faithfully, so it declines loudly rather than capturing wrong.
#[test]
fn sparse_index_declines() {
    let fx = Fixture::new();
    fx.write("keep/a.txt", "a\n");
    fx.write("drop/b.txt", "b\n");
    fx.commit("init");
    fx.git(&["sparse-checkout", "set", "--cone", "--sparse-index", "keep"]);
    fx.write("keep/a.txt", "dirty\n");
    let index = fx.index_bytes();
    let repo = fx.repo();
    let err = ff_core::take(&repo, &ff_core::Provenance::new("manual", None));
    match err {
        Err(err) => assert!(
            err.to_string().contains("sparse"),
            "error must name sparse checkout: {err}"
        ),
        Ok(outcome) => panic!("sparse-index capture must decline, got {outcome:?}"),
    }
    assert_eq!(
        index,
        fx.index_bytes(),
        "declining must not touch the index"
    );
}

/// Repeated captures with no changes in between are pure no-ops.
#[test]
fn take_is_idempotent() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    fx.write("a.txt", "changed\n");
    fx.write("new.txt", "untracked\n");
    assert_snapshot_matches(&fx); // Created
    assert_snapshot_matches(&fx); // NoOp
    assert_snapshot_matches(&fx); // NoOp, still stable
}
