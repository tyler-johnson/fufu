//! The patch layer's contract: the same tree diff the stat surfaces walk,
//! read down to the line, in the format git spells.

use ff_core::patch::LineKind;
use ff_core::{DiffOptions, FileStat};
use ff_testsupport::Fixture;

fn take(fx: &Fixture) {
    ff_core::capture(&fx.repo(), &ff_core::Provenance::new("manual", None)).expect("take");
}

fn patched(fx: &Fixture, paths: &[&str]) -> Vec<FileStat> {
    ff_core::change_diff(
        &fx.repo(),
        &DiffOptions {
            hunks: true,
            paths: paths.iter().map(|p| p.to_string()).collect(),
        },
    )
    .expect("change_diff")
    .files
}

fn only(fx: &Fixture, path: &str) -> FileStat {
    patched(fx, &[])
        .into_iter()
        .find(|f| f.path == path)
        .unwrap_or_else(|| panic!("{path} not in the diff"))
}

/// The default depth is unchanged: nobody asked for content, so no file
/// carries any — which is what keeps every existing `--json` payload
/// byte-identical to what it emitted before this layer existed.
#[test]
fn stat_only_carries_no_hunks() {
    let fx = Fixture::new();
    fx.write("a.txt", "one\n");
    fx.commit("init");
    fx.write("a.txt", "two\n");
    take(&fx);
    let stat = ff_core::change_stat(&fx.repo()).expect("change_stat");
    assert_eq!(stat.files.len(), 1);
    assert!(stat.files[0].hunks.is_none(), "stat depth invented content");
    assert!(stat.files[0].old_mode.is_none());
    assert!(stat.files[0].new_id.is_none());
}

/// A modification: one hunk, context around the changed line, and counts in
/// the header that match what the lines actually say.
#[test]
fn a_modification_carries_its_lines() {
    let fx = Fixture::new();
    fx.write("a.txt", "1\n2\n3\n4\n5\n");
    fx.commit("init");
    fx.write("a.txt", "1\n2\nthree\n4\n5\n");
    take(&fx);

    let file = only(&fx, "a.txt");
    let hunks = file.hunks.expect("hunks");
    assert_eq!(hunks.len(), 1);
    let hunk = &hunks[0];
    assert_eq!(hunk.header, "@@ -1,5 +1,5 @@");
    assert_eq!(hunk.old_start, 0);
    assert_eq!(hunk.old_lines, 5);
    assert_eq!(hunk.new_lines, 5);

    let kinds: Vec<LineKind> = hunk.lines.iter().map(|l| l.kind).collect();
    assert_eq!(
        kinds,
        vec![
            LineKind::Context,
            LineKind::Context,
            LineKind::Delete,
            LineKind::Insert,
            LineKind::Context,
            LineKind::Context,
        ]
    );
    let texts: Vec<&str> = hunk.lines.iter().map(|l| l.text.as_str()).collect();
    assert_eq!(texts, vec!["1", "2", "3", "three", "4", "5"]);
    assert!(
        hunk.lines.iter().all(|l| !l.no_newline),
        "every line ends in a newline here"
    );

    // The `diff --git` header's two halves, both present on a modification.
    assert_eq!(file.old_mode.as_deref(), Some("100644"));
    assert_eq!(file.new_mode.as_deref(), Some("100644"));
    assert_ne!(file.old_id, file.new_id);
}

/// An addition numbers its empty side from the line *before* the insertion
/// point — `-0,0`, git's spelling, and the one `git apply` reads back.
#[test]
fn an_addition_numbers_the_empty_side_from_zero() {
    let fx = Fixture::new();
    fx.write("kept.txt", "x\n");
    fx.commit("init");
    fx.write("new.txt", "a\nb\n");
    take(&fx);

    let file = only(&fx, "new.txt");
    assert_eq!(file.kind, ff_core::ChangeKind::Added);
    let hunks = file.hunks.expect("hunks");
    assert_eq!(hunks.len(), 1);
    assert_eq!(hunks[0].header, "@@ -0,0 +1,2 @@");
    assert!(hunks[0].lines.iter().all(|l| l.kind == LineKind::Insert));
    assert!(file.old_mode.is_none(), "nothing was there before");
    assert_eq!(file.new_mode.as_deref(), Some("100644"));
}

#[test]
fn a_deletion_empties_the_new_side() {
    let fx = Fixture::new();
    fx.write("gone.txt", "a\nb\n");
    fx.write("kept.txt", "x\n");
    fx.commit("init");
    fx.remove("gone.txt");
    take(&fx);

    let file = only(&fx, "gone.txt");
    assert_eq!(file.kind, ff_core::ChangeKind::Deleted);
    let hunks = file.hunks.expect("hunks");
    assert_eq!(hunks[0].header, "@@ -1,2 +0,0 @@");
    assert!(hunks[0].lines.iter().all(|l| l.kind == LineKind::Delete));
    assert!(file.new_mode.is_none(), "nothing is there now");
}

/// A single-line side drops its count, the way git writes `@@ -1 +1 @@`.
#[test]
fn a_one_line_side_drops_its_count() {
    let fx = Fixture::new();
    fx.write("a.txt", "one\n");
    fx.commit("init");
    fx.write("a.txt", "two\n");
    take(&fx);
    let hunks = only(&fx, "a.txt").hunks.expect("hunks");
    assert_eq!(hunks[0].header, "@@ -1 +1 @@");
}

/// Changes further apart than the context they would each print land in
/// separate hunks; closer together, they share one.
#[test]
fn distant_changes_split_into_separate_hunks() {
    let body: String = (1..=30).map(|n| format!("{n}\n")).collect();
    let fx = Fixture::new();
    fx.write("a.txt", &body);
    fx.commit("init");
    let edited: String = (1..=30)
        .map(|n| match n {
            2 => "two\n".to_string(),
            25 => "twentyfive\n".to_string(),
            _ => format!("{n}\n"),
        })
        .collect();
    fx.write("a.txt", &edited);
    take(&fx);

    let hunks = only(&fx, "a.txt").hunks.expect("hunks");
    assert_eq!(hunks.len(), 2, "30 lines apart is two hunks");
    assert!(hunks[0].old_start < hunks[1].old_start);
}

/// A file with no trailing newline earns git's marker on the version that
/// lacks it — the difference between a patch that applies and one that
/// quietly appends a byte.
#[test]
fn a_missing_trailing_newline_is_marked() {
    let fx = Fixture::new();
    fx.write("a.txt", "one\ntwo");
    fx.commit("init");
    fx.write("a.txt", "one\ntwo\nthree\n");
    take(&fx);

    let hunks = only(&fx, "a.txt").hunks.expect("hunks");
    let marked: Vec<&str> = hunks[0]
        .lines
        .iter()
        .filter(|l| l.no_newline)
        .map(|l| l.text.as_str())
        .collect();
    assert_eq!(
        marked,
        vec!["two"],
        "the old last line is the one without a newline"
    );
    // And it is a *change*, not context: the two versions of that line
    // genuinely differ, one byte apart.
    assert!(
        hunks[0]
            .lines
            .iter()
            .any(|l| l.kind == LineKind::Delete && l.text == "two")
    );
}

/// Binary is asked-and-empty, not never-asked: the empty vec is what tells a
/// renderer to print git's `Binary files … differ` rather than nothing.
#[test]
fn a_binary_file_yields_no_hunks() {
    let fx = Fixture::new();
    fx.write("kept.txt", "x\n");
    fx.commit("init");
    std::fs::write(fx.path().join("blob.bin"), [0u8, 1, 2, 0, 3, 4]).expect("write binary");
    take(&fx);

    let file = only(&fx, "blob.bin");
    assert!(file.binary, "NUL bytes make it binary");
    assert_eq!(
        file.hunks.as_deref(),
        Some(&[][..]),
        "asked for, and there is no text to show"
    );
}

/// Paths filter by the rule `ff restore` already speaks: a file path, or a
/// directory prefix. No globs — that is fufu's pathspec, in one place.
#[test]
fn paths_select_files_and_directory_prefixes() {
    let fx = Fixture::new();
    fx.write("root.txt", "a\n");
    fx.write("src/one.txt", "a\n");
    fx.write("src/two.txt", "a\n");
    fx.commit("init");
    fx.write("root.txt", "b\n");
    fx.write("src/one.txt", "b\n");
    fx.write("src/two.txt", "b\n");
    take(&fx);

    let all: Vec<String> = patched(&fx, &[]).into_iter().map(|f| f.path).collect();
    assert_eq!(all.len(), 3);

    let one: Vec<String> = patched(&fx, &["src/one.txt"])
        .into_iter()
        .map(|f| f.path)
        .collect();
    assert_eq!(one, vec!["src/one.txt".to_string()]);

    let dir: Vec<String> = patched(&fx, &["src"]).into_iter().map(|f| f.path).collect();
    assert_eq!(
        dir,
        vec!["src/one.txt".to_string(), "src/two.txt".to_string()]
    );

    // The trailing slash is the same selector, not a different one.
    let slashed: Vec<String> = patched(&fx, &["src/"])
        .into_iter()
        .map(|f| f.path)
        .collect();
    assert_eq!(slashed, dir);
}

/// A rename carries both paths, so the header can name the source and the
/// destination rather than inventing a delete and an add.
#[test]
fn a_rename_keeps_its_source_path() {
    let body: String = (1..=20).map(|n| format!("line {n}\n")).collect();
    let fx = Fixture::new();
    fx.write("old.txt", &body);
    fx.commit("init");
    fx.remove("old.txt");
    fx.write("new.txt", &body);
    take(&fx);

    let file = only(&fx, "new.txt");
    assert_eq!(file.kind, ff_core::ChangeKind::Renamed);
    assert_eq!(file.from.as_deref(), Some("old.txt"));
    assert_eq!(file.old_mode.as_deref(), Some("100644"));
    assert_eq!(file.new_mode.as_deref(), Some("100644"));
}

/// The executable bit is part of the header, because a patch that drops it
/// restores a file that will not run.
#[test]
fn the_executable_bit_reaches_the_header() {
    let fx = Fixture::new();
    fx.write("kept.txt", "x\n");
    fx.commit("init");
    fx.write("run.sh", "#!/bin/sh\necho hi\n");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(
            fx.path().join("run.sh"),
            std::fs::Permissions::from_mode(0o755),
        )
        .expect("chmod");
    }
    take(&fx);

    let file = only(&fx, "run.sh");
    let expected = if cfg!(unix) { "100755" } else { "100644" };
    assert_eq!(file.new_mode.as_deref(), Some(expected));
}
