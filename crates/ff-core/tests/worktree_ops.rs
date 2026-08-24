//! Making and taking a linked worktree are operations: the add records
//! itself and lays the new chain's floor, and the removal captures the tree
//! into its own chain before it destroys it, leaving the chain addressable.

use ff_core::gix;
use ff_testsupport::Fixture;

fn prov() -> ff_core::Provenance {
    ff_core::Provenance::new("manual", None)
}

/// The newest operation's record, read through the public reader.
fn tip_record(repo: &gix::Repository) -> ff_core::ops::OpRecord {
    let log = ff_core::ops::OpLog::open(repo).unwrap();
    let op = log.get(log.tip().unwrap().unwrap()).unwrap();
    op.record()
        .unwrap()
        .cloned()
        .expect("a verb op has a record")
}

/// Adding a worktree lands an operation on the calling worktree's chain, and
/// that record is what names the worktree fufu made.
#[test]
fn adding_a_worktree_is_an_operation() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    fx.git(&["branch", "side"]);

    let repo = fx.repo();
    let (report, _ctx) = ff_core::add_worktree(
        &repo,
        &fx.root().join("bay"),
        Some("side"),
        &prov(),
        None,
        Vec::new(),
    )
    .unwrap();

    assert_eq!(report.id, "bay");
    assert_eq!(report.branch, "side");
    assert!(!report.created_branch);
    assert!(report.path.is_dir(), "the checkout is not there");

    let record = tip_record(&repo);
    assert_eq!(record.worktree.len(), 1, "the record names one worktree");
    match &record.worktree[0] {
        ff_core::ops::record::WorktreeEffect::Add { id, path, branch } => {
            assert_eq!(id, "bay");
            assert_eq!(path, &report.path.display().to_string());
            assert_eq!(branch, "side");
        }
        other => panic!("expected an Add effect, got {other:?}"),
    }
}

/// A new worktree's chain ref exists the moment the add returns: the floor is
/// laid by the operation, not by the bay's first command.
#[test]
fn a_new_worktree_gets_its_floor() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    fx.git(&["branch", "side"]);

    let repo = fx.repo();
    let (report, _ctx) = ff_core::add_worktree(
        &repo,
        &fx.root().join("bay"),
        Some("side"),
        &prov(),
        None,
        Vec::new(),
    )
    .unwrap();

    let chain = ff_core::ops::ops_ref(&report.id);
    assert!(
        repo.find_reference(chain.as_str()).is_ok(),
        "the new chain {chain} does not exist"
    );
}

/// An unnamed worktree takes its directory's name as the branch, and the
/// report says the branch was made.
#[test]
fn an_unnamed_worktree_takes_the_directorys_name() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");

    let repo = fx.repo();
    let (report, _ctx) = ff_core::add_worktree(
        &repo,
        &fx.root().join("spike"),
        None,
        &prov(),
        None,
        Vec::new(),
    )
    .unwrap();

    assert_eq!(report.branch, "spike");
    assert!(report.created_branch);
    assert_eq!(
        report.head,
        fx.git(&["rev-parse", "refs/heads/spike"]).trim(),
        "the branch does not stand at the head the report names"
    );
}

/// When the directory's name is already a branch, the add mints a different
/// name rather than failing.
#[test]
fn an_unnamed_worktree_falls_back_to_a_petname() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    fx.git(&["branch", "spike"]);

    let repo = fx.repo();
    let (report, _ctx) = ff_core::add_worktree(
        &repo,
        &fx.root().join("spike"),
        None,
        &prov(),
        None,
        Vec::new(),
    )
    .unwrap();

    assert_ne!(
        report.branch, "spike",
        "the taken name must be minted around"
    );
    assert!(report.created_branch);
}

/// A removal captures the tree before it destroys it: the report names the
/// capture, the checkout is gone, the chain survives, and the captured tree
/// holds the bay's uncommitted file with its content.
#[test]
fn removing_a_worktree_captures_it_first() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    fx.git(&["branch", "side"]);

    let repo = fx.repo();
    let bay = fx.root().join("bay");
    let (add, _ctx) =
        ff_core::add_worktree(&repo, &bay, Some("side"), &prov(), None, Vec::new()).unwrap();

    // Uncommitted work in the bay: the case `git worktree remove` loses.
    std::fs::write(bay.join("wip.txt"), "the bay's uncommitted work\n").unwrap();

    let (report, _ctx) =
        ff_core::remove_worktree(&repo, &add.id, &prov(), None, Vec::new()).unwrap();
    assert!(
        report.capture.is_some(),
        "the removal must have captured the tree"
    );
    assert!(!bay.exists(), "the checkout survived");

    let chain = ff_core::ops::ops_ref(&add.id);
    assert!(
        repo.find_reference(chain.as_str()).is_ok(),
        "the chain did not survive"
    );

    // The capture is the tip of the removed chain, and its tree carries the
    // bay's file with its bytes.
    let tip = repo
        .find_reference(chain.as_str())
        .unwrap()
        .target()
        .try_id()
        .expect("the chain points at an operation")
        .to_owned();
    let op = ff_core::ops::walk::decode(&repo, tip).unwrap();
    assert!(op.is_capture(), "the chain's newest op is the capture");
    assert_eq!(
        report.capture.as_deref(),
        Some(op.id().to_string().as_str())
    );

    let tree = repo.find_tree(op.tree()).unwrap();
    let entry = tree
        .lookup_entry_by_path("wip.txt")
        .unwrap()
        .expect("the captured tree holds the bay's file");
    let blob = repo.find_blob(entry.id().detach()).unwrap();
    assert_eq!(blob.data.as_slice(), b"the bay's uncommitted work\n");
}

/// The worktree a command runs in is not a target: removal refuses it before
/// touching anything.
#[test]
fn the_worktree_you_are_in_is_not_removable() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    fx.git(&["branch", "side"]);

    let repo = fx.repo();
    let bay = fx.root().join("bay");
    let (add, _ctx) =
        ff_core::add_worktree(&repo, &bay, Some("side"), &prov(), None, Vec::new()).unwrap();

    let wt = ff_core::discover_isolated(&bay).expect("open the bay");
    let err = ff_core::remove_worktree(&wt, &add.id, &prov(), None, Vec::new()).unwrap_err();
    assert_eq!(err.id(), "worktree/is-current");
}

/// After the removal, the removed id is still a chain: it appears in the
/// orphan list, where a reader finds a deleted bay's work.
#[test]
fn the_removal_leaves_the_chain_addressable() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    fx.git(&["branch", "side"]);

    let repo = fx.repo();
    let (add, _ctx) = ff_core::add_worktree(
        &repo,
        &fx.root().join("bay"),
        Some("side"),
        &prov(),
        None,
        Vec::new(),
    )
    .unwrap();
    let _ = ff_core::remove_worktree(&repo, &add.id, &prov(), None, Vec::new()).unwrap();

    let orphans = ff_core::linked::orphan_chains(&repo).unwrap();
    assert!(
        orphans.contains(&add.id),
        "the removed chain is not listed: {orphans:?}"
    );
}

/// The branch a creation makes belongs to the new worktree, so the operation
/// does not record it in this worktree's ref table. Recording it would claim
/// a ref this tree does not own, and the next reconcile would find it absent
/// from the world and report a deletion nobody performed — which is exactly
/// what `ff worktree add` then `ff worktree remove` used to print.
#[test]
fn a_creation_does_not_claim_the_branch_it_made() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");

    let repo = fx.repo();
    let bay = fx.root().join("bay");
    let (report, _) =
        ff_core::add_worktree(&repo, &bay, None, &prov(), None, Vec::new()).expect("add");
    assert!(report.created_branch, "the branch should have been made");

    let full = format!("refs/heads/{}", report.branch);
    let log = ff_core::ops::OpLog::open(&repo).expect("open the log");
    let tip = log.tip().expect("read the tip").expect("a tip");
    let op = log.get(tip).expect("read the op");
    let table = op
        .refs()
        .expect("read the table")
        .expect("a verb op carries one");
    assert!(
        !table.refs.contains_key(&full),
        "{full} was recorded, but the new worktree owns it: {:?}",
        table.refs.keys().collect::<Vec<_>>()
    );

    // And the reconcile that follows sees nothing foreign.
    let report = ff_core::ops::reconcile(&repo, 0).expect("reconcile");
    assert!(
        format!("{report:?}").matches(&full).count() == 0,
        "reconcile reported {full}: {report:?}"
    );
}
