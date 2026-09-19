//! What a commit is measured against: its parent, or for a merge the
//! auto-merge of its parents, with the first parent standing in when the
//! auto-merge cannot be made.

use ff_core::gix::ObjectId;
use ff_core::{Against, DiffOptions, Fallback};
use ff_testsupport::Fixture;

fn oid(sha: &str) -> ObjectId {
    ObjectId::from_hex(sha.trim().as_bytes()).expect("a sha")
}

fn head(fx: &Fixture) -> ObjectId {
    oid(&fx.git(&["rev-parse", "HEAD"]))
}

fn tree_of(fx: &Fixture, rev: &str) -> ObjectId {
    oid(&fx.git(&["rev-parse", &format!("{rev}^{{tree}}")]))
}

fn object_count(fx: &Fixture) -> String {
    fx.git(&["count-objects", "-v"])
        .lines()
        .find(|l| l.starts_with("count:"))
        .expect("a count line")
        .to_string()
}

/// A base, then `side` adding its own file and `main` adding its own: the
/// two are mergeable cleanly. Leaves HEAD on `main` after `main work`.
fn fork(fx: &Fixture) {
    fx.write("base.txt", "base\n");
    fx.commit("base");
    fx.git(&["switch", "-c", "side", "-q"]);
    fx.write("side.txt", "side\n");
    fx.commit("side work");
    fx.git(&["switch", "main", "-q"]);
    fx.write("main.txt", "main\n");
    fx.commit("main work");
}

fn clean_merge() -> Fixture {
    let fx = Fixture::new();
    fork(&fx);
    fx.git(&["merge", "--no-ff", "-q", "-m", "merge side", "side"]);
    fx
}

/// A clean merge whose commit also touched a third file.
fn extra_edit_merge() -> Fixture {
    let fx = Fixture::new();
    fork(&fx);
    fx.git(&["merge", "--no-ff", "--no-commit", "-q", "side"]);
    fx.write("extra.txt", "carried\n");
    fx.commit("merge side with an extra edit");
    fx
}

/// Both sides edit line 2 of `a.txt`; the merge resolves it to `resolved`.
fn resolved_conflict() -> Fixture {
    let fx = Fixture::new();
    fx.write("a.txt", "1\n2\n3\n");
    fx.commit("base");
    fx.git(&["switch", "-c", "side", "-q"]);
    fx.write("a.txt", "1\nside\n3\n");
    fx.commit("side work");
    fx.git(&["switch", "main", "-q"]);
    fx.write("a.txt", "1\nmain\n3\n");
    fx.commit("main work");
    let out = fx.try_git(&["merge", "side"]);
    assert!(!out.status.success(), "the merge conflicts");
    fx.write("a.txt", "1\nresolved\n3\n");
    fx.git(&["commit", "-qam", "resolve"]);
    fx
}

/// `main` and an orphan branch, merged with `--allow-unrelated-histories`.
fn unrelated_histories() -> Fixture {
    let fx = Fixture::new();
    fx.write("main.txt", "main\n");
    fx.commit("main root");
    fx.git(&["switch", "--orphan", "other", "-q"]);
    fx.write("other.txt", "other\n");
    fx.commit("other root");
    fx.git(&["switch", "main", "-q"]);
    fx.git(&[
        "merge",
        "--allow-unrelated-histories",
        "-q",
        "-m",
        "merge other",
        "other",
    ]);
    fx
}

#[test]
fn a_plain_commit_is_measured_against_its_parent() {
    let fx = Fixture::new();
    fx.write("a.txt", "one\n");
    let first = fx.commit("one");
    fx.write("a.txt", "two\n");
    let second = fx.commit("two");

    let repo = fx.repo();
    let m = ff_core::measure(&repo, oid(&second)).expect("measure");
    assert_eq!(m.against, Against::Parent);
    assert_eq!(m.commit, Some(oid(&first)));
    assert_eq!(m.tree, tree_of(&fx, &first));

    let root = ff_core::measure(&repo, oid(&first)).expect("measure");
    assert_eq!(root.against, Against::Parent);
    assert_eq!(root.commit, None);
    assert_eq!(root.tree, ObjectId::empty_tree(repo.object_hash()));
}

/// The auto-merge of a clean merge's parents is the merge's own tree, so
/// nothing lies between them, and through a memory handle the auto-merge
/// tree reaches no store.
#[test]
fn a_clean_merge_measures_nothing_beyond_the_auto_merge() {
    let fx = clean_merge();
    let before = object_count(&fx);
    let repo = fx.repo();
    let memory = repo.clone().with_object_memory();
    let m = ff_core::measure(&memory, head(&fx)).expect("measure");
    assert_eq!(m.against, Against::AutoMerge);
    assert_eq!(m.commit, None);
    let stat = ff_core::tree_diff(
        &memory,
        m.tree,
        tree_of(&fx, "HEAD"),
        &DiffOptions::default(),
    )
    .expect("tree_diff through the memory handle");
    assert!(stat.files.is_empty(), "{stat:?}");
    assert_eq!(object_count(&fx), before, "no auto-merge tree was written");
}

#[test]
fn an_extra_edit_is_the_merges_own_change() {
    let fx = extra_edit_merge();
    let repo = fx.repo();
    let memory = repo.clone().with_object_memory();
    let m = ff_core::measure(&memory, head(&fx)).expect("measure");
    assert_eq!(m.against, Against::AutoMerge);
    let stat = ff_core::tree_diff(
        &memory,
        m.tree,
        tree_of(&fx, "HEAD"),
        &DiffOptions::default(),
    )
    .expect("tree_diff");
    let paths: Vec<&str> = stat.files.iter().map(|f| f.path.as_str()).collect();
    assert_eq!(paths, vec!["extra.txt"]);
}

#[test]
fn a_conflicting_auto_merge_falls_back_to_the_first_parent() {
    let fx = resolved_conflict();
    let p1 = oid(&fx.git(&["rev-parse", "HEAD^1"]));
    let repo = fx.repo();
    let m = ff_core::measure(&repo, head(&fx)).expect("measure");
    assert_eq!(
        m.against,
        Against::FirstParent(Fallback::Conflicts(vec!["a.txt".into()]))
    );
    assert_eq!(m.commit, Some(p1));
    assert_eq!(m.tree, tree_of(&fx, "HEAD^1"));
}

#[test]
fn no_merge_base_falls_back_to_the_first_parent() {
    let fx = unrelated_histories();
    let p1 = oid(&fx.git(&["rev-parse", "HEAD^1"]));
    let repo = fx.repo();
    let m = ff_core::measure(&repo, head(&fx)).expect("measure");
    assert_eq!(m.against, Against::FirstParent(Fallback::NoBase));
    assert_eq!(m.commit, Some(p1));
    assert_eq!(m.tree, tree_of(&fx, "HEAD^1"));
}

/// `commit_diff` spends the measure and reports which one applied.
#[test]
fn commit_diff_carries_the_measure() {
    let fx = resolved_conflict();
    let diff = ff_core::commit_diff(
        &fx.repo(),
        head(&fx),
        &DiffOptions {
            hunks: true,
            ..Default::default()
        },
    )
    .expect("commit_diff");
    assert!(matches!(diff.against, Against::FirstParent(_)));
    assert_eq!(diff.stat.files.len(), 1);
    assert_eq!(diff.stat.files[0].path, "a.txt");
}
