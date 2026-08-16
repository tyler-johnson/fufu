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
