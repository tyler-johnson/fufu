//! `ff undo` reverses a worktree. Undoing an add retires the bay — capture
//! first, so the uncommitted work outlives the directory — and redoing it
//! puts the bay back on that capture. Undoing a removal restores what the
//! removal's capture took in. The id rides the round trip, because the
//! chain is keyed by it, and a busy bay refuses the undo rather than being
//! destroyed behind a refusal.

use std::path::PathBuf;

use ff_testsupport::Fixture;

fn prov() -> ff_core::Provenance {
    ff_core::Provenance::new("manual", None)
}

fn opts() -> ff_core::RewindOptions {
    ff_core::RewindOptions::default()
}

/// A repository with one commit, a `side` branch, and a `bay` worktree
/// standing on it. Returns the checkout path and the worktree's id.
fn bay_on_side(fx: &Fixture) -> (PathBuf, String) {
    fx.write("a.txt", "a\n");
    fx.commit("init");
    fx.git(&["branch", "side"]);

    let repo = fx.repo();
    let bay = fx.root().join("bay");
    let (report, _ctx) =
        ff_core::add_worktree(&repo, &bay, Some("side"), &prov(), None, Vec::new()).unwrap();
    assert!(bay.is_dir(), "the add checked the bay out");
    (bay, report.id)
}

/// Undoing an add takes the worktree away: the checkout is gone, and the
/// undo is the thing that may delete it because it captured first.
#[test]
fn undoing_an_add_takes_the_worktree_away() {
    let fx = Fixture::new();
    let (bay, _id) = bay_on_side(&fx);

    let repo = fx.repo();
    let (report, _ctx) = ff_core::undo(&repo, &opts(), &prov()).unwrap();
    assert!(report.warnings.is_empty(), "{:?} ", report.warnings);

    assert!(!bay.exists(), "the checkout survived the undo");
}

/// Undoing an add keeps the work: the bay's uncommitted file is captured
/// into the bay's own chain before the directory goes, and the capture
/// stays addressable after the teardown. This is the promise the whole
/// design rests on — undo captured before it destroyed.
#[test]
fn undoing_an_add_keeps_the_work() {
    let fx = Fixture::new();
    let (bay, id) = bay_on_side(&fx);
    std::fs::write(bay.join("wip.txt"), "the bay's uncommitted work\n").unwrap();

    let repo = fx.repo();
    let (report, _ctx) = ff_core::undo(&repo, &opts(), &prov()).unwrap();
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    assert!(!bay.exists(), "the checkout survived the undo");

    let chain = ff_core::ops::ops_ref(&id);
    assert!(
        repo.find_reference(chain.as_str()).is_ok(),
        "the chain {chain} did not survive the teardown"
    );
    let tip = repo
        .find_reference(chain.as_str())
        .unwrap()
        .target()
        .try_id()
        .expect("the chain points at an operation")
        .to_owned();
    let op = ff_core::ops::walk::decode(&repo, tip).unwrap();
    assert!(
        op.is_capture(),
        "the chain's newest op is the capture undo took"
    );

    let tree = repo.find_tree(op.tree()).unwrap();
    let entry = tree
        .lookup_entry_by_path("wip.txt")
        .unwrap()
        .expect("the captured tree holds the bay's file");
    let blob = repo.find_blob(entry.id().detach()).unwrap();
    assert_eq!(blob.data.as_slice(), b"the bay's uncommitted work\n");
}

/// Redoing an add puts the bay back with the work: the checkout returns on
/// the same branch, and the uncommitted file is restored from the capture
/// undo took when it retired the bay.
#[test]
fn redoing_an_add_puts_it_back_with_the_work() {
    let fx = Fixture::new();
    let (bay, id) = bay_on_side(&fx);
    std::fs::write(bay.join("wip.txt"), "the bay's uncommitted work\n").unwrap();

    let repo = fx.repo();
    ff_core::undo(&repo, &opts(), &prov()).unwrap();
    assert!(!bay.exists(), "the checkout survived the undo");

    let (report, _ctx) = ff_core::redo(&repo, &opts(), &prov()).unwrap();
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);

    assert!(bay.is_dir(), "the checkout did not come back");
    let file = bay.join("wip.txt");
    assert_eq!(
        std::fs::read_to_string(&file).unwrap(),
        "the bay's uncommitted work\n",
        "the captured file did not come back with the bay"
    );

    let wt = ff_core::discover_isolated(&bay).expect("open the revived bay");
    assert_eq!(
        std::fs::read_to_string(wt.git_dir().join("HEAD"))
            .unwrap()
            .trim(),
        "ref: refs/heads/side",
        "the bay is not back on its branch"
    );
    assert_eq!(
        ff_core::linked::id(&wt),
        id,
        "a fresh id would orphan the chain"
    );
}

/// Undoing a removal puts the worktree back: the removal captured the bay's
/// file with its bytes, and the revival stands on that capture, so the
/// checkout returns with the work the removal took in.
#[test]
fn undoing_a_remove_puts_the_worktree_back() {
    let fx = Fixture::new();
    let (bay, id) = bay_on_side(&fx);
    std::fs::write(bay.join("wip.txt"), "the bay's uncommitted work\n").unwrap();

    let repo = fx.repo();
    let (removed, _ctx) = ff_core::remove_worktree(&repo, &id, &prov(), None, Vec::new()).unwrap();
    assert!(
        removed.capture.is_some(),
        "the removal must have captured the tree"
    );
    assert!(!bay.exists(), "the removal left the checkout behind");

    let (report, _ctx) = ff_core::undo(&repo, &opts(), &prov()).unwrap();
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);

    assert!(bay.is_dir(), "the checkout did not come back");
    assert_eq!(
        std::fs::read_to_string(bay.join("wip.txt")).unwrap(),
        "the bay's uncommitted work\n",
        "the captured file did not come back with the bay"
    );
}

/// The id survives the round trip: a revived worktree is the same
/// worktree, and a fresh id would orphan the chain the capture lives in.
#[test]
fn the_id_survives_the_round_trip() {
    let fx = Fixture::new();
    let (bay, id) = bay_on_side(&fx);
    std::fs::write(bay.join("wip.txt"), "wip\n").unwrap();

    let repo = fx.repo();
    ff_core::undo(&repo, &opts(), &prov()).unwrap();
    ff_core::redo(&repo, &opts(), &prov()).unwrap();

    let wt = ff_core::discover_isolated(&bay).expect("open the revived bay");
    assert_eq!(ff_core::linked::id(&wt), id);
    assert!(
        repo.common_dir().join("worktrees").join(&id).is_dir(),
        "the administrative entry is not filed under the original id"
    );
}

/// A busy worktree refuses rather than destroying: undo cannot capture a
/// bay whose log is locked, so it must not tear the bay down either, and
/// it must say so with the busy id rather than step over the effect in
/// silence.
#[test]
fn a_busy_worktree_refuses_rather_than_destroying() {
    let fx = Fixture::new();
    let (bay, id) = bay_on_side(&fx);
    // Uncommitted work, so the capture actually has something to write and
    // reaches the locked append: a clean bay is a no-op and would never
    // touch the lock at all.
    std::fs::write(bay.join("wip.txt"), "wip\n").unwrap();
    let repo = fx.repo();

    // Hold the bay's chain lock the way a running fufu writer would: the
    // marker file `ops::lock` acquires, already present.
    let fufu = repo.common_dir().join("fufu");
    std::fs::create_dir_all(&fufu).unwrap();
    std::fs::write(fufu.join(format!("oplog-{id}.lock")), "held by the test").unwrap();

    let err = ff_core::undo(&repo, &opts(), &prov()).unwrap_err();
    assert_eq!(err.id(), "worktree/busy");
    assert!(bay.is_dir(), "the checkout was destroyed behind a refusal");
}
