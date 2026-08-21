//! The path axis of the log walk: the touch rule, rename following, and the
//! contrast with `-r`, each held by one test.

use ff_testsupport::Fixture;

fn ids(fx: &Fixture, paths: &[&str], revs: Option<&str>) -> Vec<String> {
    let mut repo = fx.repo();
    let opts = ff_core::LogOptions {
        limit: None,
        revs: revs.map(|r| ff_core::revset::Revset::parse(r).expect("revset")),
        paths: paths.iter().map(|p| p.to_string()).collect(),
    };
    ff_core::log(&mut repo, &opts)
        .expect("log")
        .entries
        .map(|e| e.expect("entry").id)
        .collect()
}

/// The same walk with `-n` — the guard for limit-after-filter ordering.
fn ids_limited(fx: &Fixture, paths: &[&str], limit: usize) -> Vec<String> {
    let mut repo = fx.repo();
    let opts = ff_core::LogOptions {
        limit: Some(limit),
        revs: None,
        paths: paths.iter().map(|p| p.to_string()).collect(),
    };
    ff_core::log(&mut repo, &opts)
        .expect("log")
        .entries
        .map(|e| e.expect("entry").id)
        .collect()
}

/// A selector's rows are exactly the commits whose tree entry for it differs
/// from the first parent's, newest first — a change, an appearance, nothing
/// else.
#[test]
fn a_file_row_appears_only_where_it_changed() {
    let fx = Fixture::new();
    fx.write("a.txt", "one\n");
    fx.write("b.txt", "x\n");
    let first = fx.commit("first");
    fx.write("a.txt", "one\ntwo\n");
    let second = fx.commit("second");
    fx.write("b.txt", "x\ny\n");
    let third = fx.commit("third");

    assert_eq!(
        ids(&fx, &["a.txt"], None),
        vec![second.clone(), first.clone()]
    );
    assert_eq!(ids(&fx, &["b.txt"], None), vec![third, first]);
}

/// A file's last row is the commit that deletes it: present in the first
/// parent's tree, absent in the commit's.
#[test]
fn a_deletion_is_a_row() {
    let fx = Fixture::new();
    fx.write("a.txt", "one\n");
    let first = fx.commit("first");
    fx.remove("a.txt");
    let second = fx.commit("drop a");

    assert_eq!(ids(&fx, &["a.txt"], None), vec![second, first]);
}

/// A directory is a tree entry, so it selects exactly what is under it — and
/// `src` and `src/` are one selector.
#[test]
fn a_directory_prefix_selects_everything_under_it() {
    let fx = Fixture::new();
    fx.write("src/one.rs", "one\n");
    let one = fx.commit("one");
    fx.write("src/two.rs", "two\n");
    let two = fx.commit("two");
    fx.write("docs/x.md", "x\n");
    let docs = fx.commit("docs");

    let src = ids(&fx, &["src"], None);
    assert_eq!(src, ids(&fx, &["src/"], None));
    assert_eq!(src, vec![two, one]);
    assert!(!src.contains(&docs));
}

/// `-n` bounds the matching rows, not the rows walked: with only the two
/// oldest of six commits matching, one row comes back and it is the newer
/// of the two.
#[test]
fn the_limit_counts_matching_rows_not_walked_ones() {
    let fx = Fixture::new();
    fx.write("a.txt", "one\n");
    fx.commit("a one");
    fx.write("b.txt", "b\n");
    fx.commit("b one");
    fx.write("a.txt", "one\ntwo\n");
    let a2 = fx.commit("a two");
    fx.write("c.txt", "c\n");
    fx.commit("c one");
    fx.write("d.txt", "d\n");
    fx.commit("d one");
    fx.write("e.txt", "e\n");
    fx.commit("e one");

    assert_eq!(ids_limited(&fx, &["a.txt"], 1), vec![a2]);
}

/// A selector that survives a rename keeps following the file across it, and
/// its rows are exactly git's own `--follow` rows.
#[test]
fn following_a_rename_matches_git_follow() {
    let fx = Fixture::new();
    fx.write("a.txt", "one\n");
    fx.write("b.txt", "x\n");
    fx.commit("first");
    fx.write("a.txt", "one\ntwo\n");
    fx.commit("second");
    fx.git(&["mv", "a.txt", "c.txt"]);
    fx.commit("rename a to c");
    fx.write("c.txt", "one\ntwo\nthree\n");
    fx.commit("fourth");

    let oracle: Vec<String> = fx
        .git(&["log", "--follow", "--format=%H", "--", "c.txt"])
        .lines()
        .map(str::to_string)
        .collect();
    assert_eq!(oracle.len(), 4);
    assert_eq!(ids(&fx, &["c.txt"], None), oracle);
    assert_eq!(ids(&fx, &["b.txt"], None).len(), 1);
}

/// A new name that is a creation, not a rename, is not followed: the file's
/// rows start at its creating commit and do not bleed into the other file's.
#[test]
fn a_created_file_is_not_a_rename() {
    let fx = Fixture::new();
    fx.write("a.txt", "one\n");
    let first = fx.commit("first");
    fx.write("c.txt", "fresh\n");
    let second = fx.commit("create c");
    fx.write("a.txt", "one\ntwo\n");
    let third = fx.commit("touch a");

    assert_eq!(ids(&fx, &["c.txt"], None), vec![second]);
    assert_eq!(ids(&fx, &["a.txt"], None), vec![third, first]);
}

/// A merge is measured against its first parent, and the filtered walk stays
/// topologically sound: no parent is ever emitted before its children.
#[test]
fn a_merge_is_measured_against_its_first_parent() {
    let fx = Fixture::new();
    fx.write("s.txt", "s0\n");
    fx.commit("base");
    fx.git(&["checkout", "-q", "-b", "side"]);
    fx.write("s.txt", "s0\ns1\n");
    fx.commit("side");
    fx.git(&["checkout", "-q", "main"]);
    fx.write("m.txt", "m\n");
    fx.commit("main");
    fx.git(&["merge", "-q", "--no-edit", "side"]);

    let rows = ids(&fx, &["s.txt"], None);

    // Every returned commit's first parent, when also returned, sits at a
    // later index: the walk never shows a parent before its children.
    for (i, id) in rows.iter().enumerate() {
        let first_parent = fx
            .git(&["rev-list", "--parents", "-n", "1", id])
            .split_whitespace()
            .nth(1)
            .map(str::to_string);
        if let Some(parent) = first_parent
            && let Some(j) = rows.iter().position(|r| r == &parent)
        {
            assert!(j > i, "parent {parent} emitted before its child {id}");
        }
    }

    // The oracle is git's *first-parent* reading, not its default one. git's
    // default applies history simplification: a merge TREESAME to some parent
    // is dropped and only that parent is followed, so a clean merge never
    // appears however much it changed against the first parent. fufu measures
    // a commit against its first parent everywhere — it is what `ff show`
    // prints — so the merge is a row here, and `--diff-merges=first-parent` is
    // the spelling of that same question in git.
    let oracle: Vec<String> = fx
        // `--no-patch`: asking for first-parent merge diffs turns the patch
        // itself on, and only the ids are the oracle here.
        .git(&[
            "log",
            "--diff-merges=first-parent",
            "--no-patch",
            "--format=%H",
            "--",
            "s.txt",
        ])
        .lines()
        .map(str::to_string)
        .collect();
    assert_eq!(rows, oracle);

    // The contrast, so the choice above is asserted rather than assumed: the
    // default reading really does drop the merge.
    let simplified: Vec<String> = fx
        .git(&["log", "--format=%H", "--", "s.txt"])
        .lines()
        .map(str::to_string)
        .collect();
    assert_eq!(simplified.len(), rows.len() - 1);
    assert!(!simplified.contains(&rows[0]));
}

/// A revset names a set with no line of descent: it filters by the touch
/// rule only, so the pre-rename commits are out of reach — the contrast the
/// no-`-r` walk, which follows, does not have.
#[test]
fn revsets_filter_but_do_not_follow() {
    let fx = Fixture::new();
    fx.write("a.txt", "one\n");
    fx.write("b.txt", "x\n");
    fx.commit("first");
    fx.write("a.txt", "one\ntwo\n");
    fx.commit("second");
    fx.git(&["mv", "a.txt", "c.txt"]);
    let third = fx.commit("rename a to c");
    fx.write("c.txt", "one\ntwo\nthree\n");
    let fourth = fx.commit("fourth");

    assert_eq!(ids(&fx, &["c.txt"], Some("::@")), vec![fourth, third]);
    assert_eq!(ids(&fx, &["c.txt"], None).len(), 4);
}
