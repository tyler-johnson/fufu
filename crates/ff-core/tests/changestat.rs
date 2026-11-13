//! ChangeStat contract: per-file insertions/deletions between HEAD tree and
//! capture chain tip tree, including untracked files.

use ff_testsupport::Fixture;

fn take(fx: &Fixture) {
    ff_core::capture(&fx.repo(), &ff_core::Provenance::new("manual", None)).expect("take");
}

/// A clean working tree produces an empty diffstat.
#[test]
fn clean_tree_is_empty() {
    let fx = Fixture::new();
    fx.write("a.txt", "hello\n");
    fx.commit("init");
    take(&fx);
    let stat = ff_core::change_stat(&fx.repo()).expect("change_stat");
    assert!(stat.files.is_empty());
    assert_eq!(stat.insertions, 0);
    assert_eq!(stat.deletions, 0);
}

/// Modifying a file reports the correct insertions and deletions.
#[test]
fn modified_file_counts_lines() {
    let fx = Fixture::new();
    fx.write("a.txt", "line1\nline2\nline3\n");
    fx.commit("init");
    fx.write("a.txt", "line1\nchanged\nline3\n");
    take(&fx);
    let stat = ff_core::change_stat(&fx.repo()).expect("change_stat");
    assert_eq!(stat.files.len(), 1);
    let f = &stat.files[0];
    assert_eq!(f.kind, ff_core::ChangeKind::Modified);
    assert_eq!(f.insertions, 1);
    assert_eq!(f.deletions, 1);
}

/// Untracked files appear as additions — the whole point of the redesign.
#[test]
fn untracked_file_is_an_addition() {
    let fx = Fixture::new();
    fx.write("existing.txt", "here\n");
    fx.commit("init");
    fx.write("new.txt", "line1\nline2\n");
    take(&fx);
    let stat = ff_core::change_stat(&fx.repo()).expect("change_stat");
    let entry = stat
        .files
        .iter()
        .find(|f| f.path == "new.txt")
        .expect("new.txt");
    assert_eq!(entry.kind, ff_core::ChangeKind::Added);
    assert_eq!(entry.insertions, 2);
    assert_eq!(entry.deletions, 0);
}

/// Removing a tracked file reports the lost lines as deletions.
#[test]
fn deleted_file_counts_removals() {
    let fx = Fixture::new();
    fx.write("a.txt", "line1\nline2\n");
    fx.commit("init");
    fx.remove("a.txt");
    take(&fx);
    let stat = ff_core::change_stat(&fx.repo()).expect("change_stat");
    assert_eq!(stat.files.len(), 1);
    let f = &stat.files[0];
    assert_eq!(f.kind, ff_core::ChangeKind::Deleted);
    assert_eq!(f.deletions, 2);
    assert_eq!(f.insertions, 0);
}

/// Binary files (containing NUL bytes) report zero counts with binary=true.
#[test]
fn binary_file_has_no_counts() {
    let fx = Fixture::new();
    fx.write("bin.dat", "hello\0world");
    fx.commit("init");
    fx.write("bin.dat", "hello\0changed");
    take(&fx);
    let stat = ff_core::change_stat(&fx.repo()).expect("change_stat");
    assert_eq!(stat.files.len(), 1);
    let f = &stat.files[0];
    assert!(f.binary);
    assert_eq!(f.insertions, 0);
    assert_eq!(f.deletions, 0);
}

/// The totals on ChangeStat equal the sum over all files.
#[test]
fn totals_sum_the_files() {
    let fx = Fixture::new();
    fx.write("a.txt", "aaa\nbbb\nccc\n");
    fx.write("b.txt", "xxx\nyyy\nzzz\n");
    fx.commit("init");
    fx.write("a.txt", "aaa\nBBB\nccc\n");
    fx.write("b.txt", "xxx\nYYY\nZZZ\n");
    take(&fx);
    let stat = ff_core::change_stat(&fx.repo()).expect("change_stat");
    let sum_ins: u32 = stat.files.iter().map(|f| f.insertions).sum();
    let sum_del: u32 = stat.files.iter().map(|f| f.deletions).sum();
    assert_eq!(stat.insertions, sum_ins);
    assert_eq!(stat.deletions, sum_del);
}

/// Files are sorted by path in ascending byte order.
#[test]
fn files_are_sorted_by_path() {
    let fx = Fixture::new();
    fx.write("base.txt", "base\n");
    fx.commit("init");
    fx.write("z.txt", "z\n");
    fx.write("a.txt", "a\n");
    take(&fx);
    let stat = ff_core::change_stat(&fx.repo()).expect("change_stat");
    assert_eq!(stat.files[0].path, "a.txt");
    assert_eq!(stat.files[1].path, "z.txt");
}

/// A bare repository returns an empty ChangeStat, not an error.
#[test]
fn bare_repo_is_empty_not_an_error() {
    let fx = Fixture::new_bare();
    let repo = ff_core::discover_isolated(fx.path()).expect("discover");
    let stat = ff_core::change_stat(&repo).expect("change_stat");
    assert!(stat.files.is_empty());
    assert_eq!(stat.insertions, 0);
    assert_eq!(stat.deletions, 0);
}

/// Directory entries must never appear in the diffstat — only blobs.
#[test]
fn directories_are_not_listed() {
    let fx = Fixture::new();
    // Commit a file inside a nested directory.
    fx.write("dir/nested/a.txt", "content\n");
    fx.commit("init");
    // Add a second file in the same directory (triggers directory entries in the diff).
    fx.write("dir/nested/b.txt", "more\n");
    take(&fx);
    let stat = ff_core::change_stat(&fx.repo()).expect("change_stat");
    // Every path must end in .txt — no bare "dir" or "dir/nested" entries.
    for f in &stat.files {
        assert!(f.path.ends_with(".txt"), "path {:?} is not a blob", f.path);
    }
    // Only one file changed.
    assert_eq!(stat.files.len(), 1);
    assert_eq!(stat.files[0].path, "dir/nested/b.txt");
}

/// Record a gitlink at `path` pointing at `sha` and commit it, without
/// `add -A`, which would sweep a gitlink whose directory is missing back out.
fn commit_gitlink(fx: &Fixture, path: &str, sha: &str, msg: &str) {
    fx.git(&[
        "update-index",
        "--add",
        "--cacheinfo",
        &format!("160000,{sha},{path}"),
    ]);
    fx.git(&["commit", "-qm", msg]);
}

fn tree_of(fx: &Fixture, rev: &str) -> ff_core::gix::ObjectId {
    let hex = fx.git(&["rev-parse", &format!("{rev}^{{tree}}")]);
    ff_core::gix::ObjectId::from_hex(hex.trim().as_bytes()).expect("tree id")
}

/// Issue #23: a gitlink whose path is missing from the working copy is
/// measured as one deleted `Subproject commit` line, not a refused blob diff.
#[test]
fn missing_gitlink_is_one_deleted_line() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    let sha = fx.commit("init");
    commit_gitlink(&fx, "sub", &sha, "add sub");
    take(&fx);
    let stat = ff_core::change_stat(&fx.repo()).expect("change_stat");
    assert_eq!(stat.files.len(), 1, "{:?}", stat.files);
    let f = &stat.files[0];
    assert_eq!(f.path, "sub");
    assert_eq!(f.kind, ff_core::ChangeKind::Deleted);
    assert_eq!((f.insertions, f.deletions), (0, 1));
    assert!(!f.binary);
}

/// An untracked embedded repository is captured as a gitlink and measured as
/// one added line.
#[test]
fn embedded_repository_is_one_added_line() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    let inner = fx.path().join("inner");
    fx.write("inner/f.txt", "f\n");
    fx.git_in(&inner, &["init", "-q"]);
    fx.git_in(&inner, &["add", "f.txt"]);
    fx.git_in(&inner, &["commit", "-qm", "inner"]);
    take(&fx);
    let stat = ff_core::change_stat(&fx.repo()).expect("change_stat");
    let f = stat
        .files
        .iter()
        .find(|f| f.path == "inner")
        .expect("inner in the diff");
    assert_eq!(f.kind, ff_core::ChangeKind::Added);
    assert_eq!((f.insertions, f.deletions), (1, 0));
}

/// The patch for a deleted gitlink is git's: one hunk removing the
/// `Subproject commit` line, from mode 160000.
#[test]
fn missing_gitlink_patch_is_gits() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    let sha = fx.commit("init");
    commit_gitlink(&fx, "sub", &sha, "add sub");
    take(&fx);
    let diff = ff_core::change_diff(
        &fx.repo(),
        &ff_core::DiffOptions {
            hunks: true,
            ..Default::default()
        },
    )
    .expect("change_diff");
    let f = &diff.files[0];
    assert_eq!(f.old_mode.as_deref(), Some("160000"));
    let hunks = f.hunks.as_ref().expect("hunks asked for");
    assert_eq!(hunks.len(), 1);
    assert_eq!(hunks[0].header, "@@ -1 +0,0 @@");
    assert_eq!(hunks[0].lines.len(), 1);
    assert_eq!(hunks[0].lines[0].kind, ff_core::patch::LineKind::Delete);
    assert_eq!(hunks[0].lines[0].text, format!("Subproject commit {sha}"));
}

/// Retargeting a gitlink is one line out, one line in.
#[test]
fn retargeted_gitlink_is_modified() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    let a = fx.commit("init");
    fx.write("a.txt", "b\n");
    let b = fx.commit("second");
    commit_gitlink(&fx, "sub", &a, "sub at a");
    commit_gitlink(&fx, "sub", &b, "sub at b");
    let stat = ff_core::tree_diff(
        &fx.repo(),
        tree_of(&fx, "HEAD~1"),
        tree_of(&fx, "HEAD"),
        &ff_core::DiffOptions::default(),
    )
    .expect("tree_diff");
    assert_eq!(stat.files.len(), 1);
    let f = &stat.files[0];
    assert_eq!(f.kind, ff_core::ChangeKind::Modified);
    assert_eq!((f.insertions, f.deletions), (1, 1));
}

/// A file becoming a gitlink diffs the file's text against the
/// `Subproject commit` line.
#[test]
fn file_to_gitlink_is_a_typechange() {
    let fx = Fixture::new();
    fx.write("sub", "x\n");
    let sha = fx.commit("init");
    fx.git(&["rm", "-q", "--cached", "sub"]);
    commit_gitlink(&fx, "sub", &sha, "sub becomes a gitlink");
    let stat = ff_core::tree_diff(
        &fx.repo(),
        tree_of(&fx, "HEAD~1"),
        tree_of(&fx, "HEAD"),
        &ff_core::DiffOptions {
            hunks: true,
            ..Default::default()
        },
    )
    .expect("tree_diff");
    assert_eq!(stat.files.len(), 1);
    let f = &stat.files[0];
    assert_eq!(f.kind, ff_core::ChangeKind::TypeChange);
    assert_eq!((f.insertions, f.deletions), (1, 1));
    let lines: Vec<_> = f.hunks.as_ref().expect("hunks")[0]
        .lines
        .iter()
        .map(|l| (l.kind, l.text.clone()))
        .collect();
    assert_eq!(
        lines,
        vec![
            (ff_core::patch::LineKind::Delete, "x".to_string()),
            (
                ff_core::patch::LineKind::Insert,
                format!("Subproject commit {sha}")
            ),
        ]
    );
}
