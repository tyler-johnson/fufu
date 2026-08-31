//! Differential contract for the index writer: after `write_index_for_tree`,
//! real git must see exactly the tree's content staged (`ls-files --stage`
//! parity with its own `read-tree`), agree the worktree is clean when it is,
//! and accept the index for its next operation. Byte-comparison against
//! git's index is impossible by design (no TREE extension, V2/V3 only), so
//! the contract is semantic.

use ff_testsupport::{Fixture, scenarios};

fn head_tree(fx: &Fixture) -> Option<ff_core::gix::ObjectId> {
    let out = fx.try_git(&["rev-parse", "--verify", "HEAD^{tree}"]);
    if !out.status.success() {
        return None;
    }
    let hex = String::from_utf8(out.stdout).unwrap().trim().to_string();
    Some(ff_core::gix::ObjectId::from_hex(hex.as_bytes()).unwrap())
}

#[test]
fn matrix_ls_files_parity_with_git_read_tree() {
    for (name, setup) in scenarios() {
        let fx = Fixture::new();
        setup(&fx);
        let Some(tree) = head_tree(&fx) else {
            continue; // unborn: no tree to write
        };

        // Ours first, from whatever index state the scenario left behind.
        let repo = fx.repo();
        ff_core::index::write_index_for_tree(&repo, tree).unwrap();
        let ours = fx.git(&["ls-files", "--stage"]);

        // Git's own read-tree from the same starting point produces the
        // reference listing.
        fx.git(&["read-tree", "HEAD"]);
        let theirs = fx.git(&["ls-files", "--stage"]);
        assert_eq!(ours, theirs, "scenario {name}: ls-files --stage parity");
    }
}

#[test]
fn clean_worktree_reads_clean_after_rewrite() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.write("dir/b.txt", "b\n");
    fx.write("script.sh", "#!/bin/sh\n");
    fx.git(&["add", "-A"]);
    fx.git(&["update-index", "--chmod=+x", "script.sh"]);
    fx.git(&["commit", "-q", "-m", "init"]);
    fx.git(&["checkout", "-q", "."]); // normalize worktree exec bits
    let tree = head_tree(&fx).unwrap();

    let repo = fx.repo();
    ff_core::index::write_index_for_tree(&repo, tree).unwrap();

    let status = fx.git(&["status", "--porcelain=v2"]);
    assert_eq!(status, "", "clean tree must read clean after index rewrite");
}

#[test]
fn stat_carry_over_preserves_stats_for_unchanged_entries() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.write("b.txt", "b\n");
    fx.commit("init");
    let tree = head_tree(&fx).unwrap();

    let repo = fx.repo();
    let before = repo.index_or_empty().unwrap();
    let a_stat = before
        .entry_by_path("a.txt".into())
        .expect("a.txt in index")
        .stat;
    assert_ne!(a_stat.mtime.secs, 0, "git wrote real stats");
    drop(before);

    ff_core::index::write_index_for_tree(&repo, tree).unwrap();

    let repo = fx.repo();
    let after = repo.index_or_empty().unwrap();
    let entry = after.entry_by_path("a.txt".into()).expect("a.txt survives");
    assert_eq!(
        entry.stat, a_stat,
        "unchanged entry keeps its previous stat data"
    );
}

#[test]
fn switch_shaped_rewrite_zeroes_only_changed_entries() {
    let fx = Fixture::new();
    fx.write("same.txt", "same\n");
    fx.write("differs.txt", "one\n");
    fx.commit("one");
    let tree_one = head_tree(&fx).unwrap();
    fx.write("differs.txt", "two\n");
    fx.commit("two");

    let repo = fx.repo();
    ff_core::index::write_index_for_tree(&repo, tree_one).unwrap();

    let repo = fx.repo();
    let index = repo.index_or_empty().unwrap();
    let same = index.entry_by_path("same.txt".into()).unwrap();
    assert_ne!(same.stat.mtime.secs, 0, "unchanged path keeps stats");
    let differs = index.entry_by_path("differs.txt".into()).unwrap();
    assert_eq!(
        differs.stat.mtime.secs, 0,
        "changed path arrives with zeroed stats (will rehash)"
    );
}

#[test]
fn git_accepts_the_index_for_its_next_operation() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    let tree = head_tree(&fx).unwrap();

    let repo = fx.repo();
    ff_core::index::write_index_for_tree(&repo, tree).unwrap();

    // git must be able to build on the index we wrote: stage a change and
    // commit it without complaint.
    fx.write("a.txt", "changed\n");
    fx.git(&["add", "a.txt"]);
    fx.git(&["commit", "-q", "-m", "after rewrite"]);
    let status = fx.git(&["status", "--porcelain=v2"]);
    assert_eq!(status, "");
}

/// Flatten a decoded cache tree for comparison.
fn flatten_tree_ext(
    node: &ff_core::gix::index::extension::Tree,
    out: &mut Vec<(Vec<u8>, String, Option<u32>, usize)>,
) {
    out.push((
        node.name.to_vec(),
        node.id.to_string(),
        node.num_entries,
        node.children.len(),
    ));
    for child in &node.children {
        flatten_tree_ext(child, out);
    }
}

fn decoded_cache_tree(fx: &Fixture) -> Vec<(Vec<u8>, String, Option<u32>, usize)> {
    let repo = fx.repo();
    let index = repo.index_or_empty().unwrap();
    let tree = index.tree().expect("index carries a TREE extension");
    let mut out = Vec::new();
    flatten_tree_ext(tree, &mut out);
    out
}

#[test]
fn synthesized_cache_tree_matches_git_read_tree() {
    // Includes the ordering trap: `foo-bar` sorts before `foo` in cache-tree
    // (raw name) order but after it in tree-object order (`foo/`).
    let fx = Fixture::new();
    fx.write("foo/inner.txt", "1\n");
    fx.write("foo-bar/inner.txt", "2\n");
    fx.write("foo-bar/deep/leaf.txt", "3\n");
    fx.write("a.txt", "a\n");
    fx.write("zoo/z.txt", "z\n");
    fx.commit("init");
    let tree = head_tree(&fx).unwrap();

    let repo = fx.repo();
    ff_core::index::write_index_for_tree(&repo, tree).unwrap();
    let ours = decoded_cache_tree(&fx);

    fx.git(&["read-tree", "HEAD"]);
    let theirs = decoded_cache_tree(&fx);
    assert_eq!(ours, theirs, "cache tree structure matches git's");
}

#[test]
fn matrix_cache_tree_matches_git() {
    for (name, setup) in scenarios() {
        let fx = Fixture::new();
        setup(&fx);
        let Some(tree) = head_tree(&fx) else {
            continue;
        };
        let repo = fx.repo();
        ff_core::index::write_index_for_tree(&repo, tree).unwrap();
        let ours = decoded_cache_tree(&fx);
        fx.git(&["read-tree", "HEAD"]);
        let theirs = decoded_cache_tree(&fx);
        assert_eq!(ours, theirs, "scenario {name}: cache tree parity");
    }
}

#[test]
fn git_write_tree_trusts_the_synthesized_cache_tree() {
    // `git write-tree` reads the cache tree when valid; producing the
    // original tree id proves git both accepts and uses what we wrote.
    let fx = Fixture::new();
    fx.write("foo/inner.txt", "1\n");
    fx.write("foo-bar/inner.txt", "2\n");
    fx.write("a.txt", "a\n");
    fx.commit("init");
    let tree = head_tree(&fx).unwrap();

    let repo = fx.repo();
    ff_core::index::write_index_for_tree(&repo, tree).unwrap();

    let written = fx.git(&["write-tree"]);
    assert_eq!(
        written.trim(),
        tree.to_string(),
        "write-tree reproduces the tree"
    );
}

#[test]
fn skip_hash_config_is_honored() {
    // With index.skipHash set, the trailing checksum is all zeroes; without
    // it, it is a real hash. Both must be readable by git.
    for (config, expect_null) in [(Some("true"), true), (None, false)] {
        let fx = Fixture::new();
        fx.write("a.txt", "a\n");
        fx.commit("init");
        if let Some(value) = config {
            fx.set_config("index.skipHash", value);
        }
        let tree = head_tree(&fx).unwrap();
        let repo = fx.repo();
        ff_core::index::write_index_for_tree(&repo, tree).unwrap();

        let bytes = fx.index_bytes();
        assert!(bytes.len() > 20);
        let trailer = &bytes[bytes.len() - 20..];
        let is_null = trailer.iter().all(|b| *b == 0);
        assert_eq!(
            is_null, expect_null,
            "skipHash={config:?} → null trailer {expect_null}"
        );
        // Readable either way.
        fx.git(&["status", "--porcelain=v2"]);
        fx.git(&["ls-files", "--stage"]);
    }
}

/// A partial close leaves the unselected edits on disk, so the index it
/// writes deliberately does *not* describe the worktree — and the stat data
/// for those paths must not be carried over from the old index.
///
/// It would be carried without help: for a path the slice did not touch, the
/// old index and the new entry hold the same blob, so the id-and-mode test
/// matches and the stale stat rides along. The next status then trusts the
/// stat, never opens the file, and the remainder stops being the open change.
///
/// The symptom only shows where filesystem timestamps are coarse enough for
/// the stale mtime to compare equal — it was invisible on ext4 and lost the
/// open change on Windows every time — so this asserts the mechanism rather
/// than the symptom, and fails on any platform.
#[test]
fn a_partial_close_zeroes_stats_for_what_it_leaves_behind() {
    let fx = Fixture::new();
    fx.set_config("user.name", "Index Tester");
    fx.set_config("user.email", "index@test.test");
    fx.write("a.txt", "a1\n");
    fx.write("b.txt", "b1\n");
    fx.commit("first");
    fx.write("a.txt", "a2\n");
    fx.write("b.txt", "b2\n");

    // The carry-over has something to carry: b.txt's entry is stat'd from
    // the commit above. Without this the test could pass on a zeroed index
    // while proving nothing.
    let before = fx.repo().index_or_empty().unwrap();
    let b_before = before.entry_by_path("b.txt".into()).expect("b.txt staged");
    assert_ne!(
        b_before.stat.mtime.secs, 0,
        "precondition: b.txt starts with real stat data to carry over"
    );
    drop(before);

    let repo = fx.repo();
    ff_core::close(
        &repo,
        &ff_core::CloseOptions {
            message: Some("a: second".into()),
            paths: vec!["a.txt".into()],
            argv: Vec::new(),
            sign: Default::default(),
            ..Default::default()
        },
        &ff_core::Provenance::new("pre", Some("ff commit".into())),
    )
    .expect("partial close");

    let after = fx.repo().index_or_empty().unwrap();
    let b_after = after.entry_by_path("b.txt".into()).expect("b.txt in index");
    assert_eq!(
        b_after.stat.mtime.secs, 0,
        "b.txt was left dirty in the worktree, so its stat must be zeroed \
         and rehashed rather than trusted"
    );
}
