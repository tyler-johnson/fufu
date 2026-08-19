//! Contract for `attribute`: given a held rewrite's chain, the marker tree it
//! laid down, and the tree the reader made of it, say which step each edit
//! belongs to. The answer feeds straight back into `chain`.

use ff_core::gix;
use ff_core::rewrite::{Attribution, Change, Region, Resolution, attribute, chain, regions};
use ff_testsupport::Fixture;

/// `chain`/`attribute` read the committer identity from the repo config, which
/// the fixture's hermetic env does not set; git gets its identity from env
/// vars, so this is only for the gix side.
fn ident(fx: &Fixture) {
    fx.set_config("user.name", "Fixture Committer");
    fx.set_config("user.email", "committer@fixture.test");
}

fn oid(hex: &str) -> gix::ObjectId {
    gix::ObjectId::from_hex(hex.trim().as_bytes()).unwrap()
}

/// The text of the blob at `path` in `tree`.
fn tree_blob(repo: &gix::Repository, tree: gix::ObjectId, path: &str) -> String {
    let tree = repo.find_tree(tree).expect("the tree exists");
    let entry = tree
        .lookup_entry_by_path(path)
        .expect("the path lookup succeeds")
        .expect("the path is in the tree");
    let blob = repo
        .find_blob(entry.id().detach())
        .expect("the blob exists");
    String::from_utf8_lossy(&blob.data).into_owned()
}

/// The tree of a commit, straight through the repository handle.
fn commit_tree(repo: &gix::Repository, commit: gix::ObjectId) -> gix::ObjectId {
    repo.find_object(commit)
        .expect("the commit exists")
        .into_commit()
        .tree_id()
        .expect("the commit has a tree")
        .detach()
}

/// The shared two-commit stack: step 0 (f1) conflicts on line 3, step 1 (f2)
/// lands clear of it on line 5. `main` takes line 3 the other way. Leaves the
/// working tree on `feature`.
fn stack(fx: &Fixture) -> (String, String, String, String) {
    fx.write("f.txt", "one\ntwo\nthree\nfour\nfive\n");
    let base = fx.commit("base");

    fx.git(&["switch", "-q", "-c", "feature", &base]);
    fx.write("f.txt", "one\ntwo\nFEAT\nfour\nfive\n");
    let f1 = fx.commit("add feature");
    fx.write("f.txt", "one\ntwo\nFEAT\nfour\nFEAT5\n");
    let f2 = fx.commit("add feature two");

    fx.git(&["switch", "-q", "main"]);
    fx.write("f.txt", "one\ntwo\nMAIN\nfour\nfive\n");
    let main = fx.commit("main");
    fx.git(&["switch", "-q", "feature"]);

    (base, main, f1, f2)
}

/// Run the unresolved chain over the shared stack.
fn run_chain(repo: &gix::Repository, f1: &str, f2: &str, main: &str) -> ff_core::rewrite::Chain {
    chain(repo, oid(f1), oid(f2), &Change::Onto(oid(main)), &[]).expect("the chain runs")
}

/// The marker tree the shared stack lays down, with its single region.
fn marker_region(repo: &gix::Repository, chain: &ff_core::rewrite::Chain) -> Region {
    regions(repo, chain)
        .expect("regions resolve")
        .into_iter()
        .next()
        .expect("the shared stack leaves one region")
}

/// Write the reader's resolved working tree, commit it, and return that
/// commit's tree — the simplest way to a real resolved tree oid.
fn resolved_tree(fx: &Fixture, repo: &gix::Repository, content: &str) -> gix::ObjectId {
    fx.write("f.txt", content);
    commit_tree(repo, oid(&fx.commit("resolved")))
}

#[test]
fn a_fix_inside_a_region_belongs_to_its_step() {
    let fx = Fixture::new();
    ident(&fx);
    let (_base, main, f1, f2) = stack(&fx);
    let repo = fx.repo();

    let chain = run_chain(&repo, &f1, &f2, &main);
    let region = marker_region(&repo, &chain);
    assert_eq!(region.step, 0, "the conflicting step is the first one");

    // The reader replaces the marker block with a single line.
    let resolved = resolved_tree(&fx, &repo, "one\ntwo\nRESOLVED\nfour\nFEAT5\n");
    let at: Attribution = attribute(&repo, &chain, resolved).expect("attribute runs");

    assert_eq!(at.resolutions.len(), 1, "one region, one resolution");
    let res = &at.resolutions[0];
    assert_eq!(res.step, 0, "it belongs to the conflicting step");
    assert_eq!(res.path, "f.txt");
    assert_eq!(res.block, region.block, "the block verbatim");
    assert!(
        res.block.contains("<<<<<<< the rewrite so far"),
        "the block is the marker block: {}",
        res.block
    );
    assert_eq!(res.with, "RESOLVED\n", "the replacement text");
    assert!(at.unresolved.is_empty(), "nothing left alone");
}

#[test]
fn a_region_left_alone_comes_back_unresolved() {
    let fx = Fixture::new();
    ident(&fx);
    let (_base, main, f1, f2) = stack(&fx);
    let repo = fx.repo();

    let chain = run_chain(&repo, &f1, &f2, &main);
    let region = marker_region(&repo, &chain);

    // The reader left the tree exactly as `ff resolve` laid it: the resolved
    // tree is the marker tree itself.
    let at: Attribution = attribute(&repo, &chain, chain.tree).expect("attribute runs");

    assert!(at.resolutions.is_empty(), "nobody fixed it");
    assert_eq!(at.unresolved.len(), 1, "the region still stands");
    assert_eq!(at.unresolved[0].step, region.step);
    assert_eq!(at.unresolved[0].path, "f.txt");
    assert_eq!(at.unresolved[0].block, region.block);
}

#[test]
fn a_half_fixed_region_is_still_unresolved() {
    let fx = Fixture::new();
    ident(&fx);
    let (_base, main, f1, f2) = stack(&fx);
    let repo = fx.repo();

    let chain = run_chain(&repo, &f1, &f2, &main);

    // Delete only the separator and the closer, leaving the opener: the
    // region was not finished.
    let resolved = resolved_tree(
        &fx,
        &repo,
        "one\ntwo\n<<<<<<< the rewrite so far\nFEAT\nMAIN\nfour\nFEAT5\n",
    );
    let at: Attribution = attribute(&repo, &chain, resolved).expect("attribute runs");

    assert!(at.resolutions.is_empty(), "a surviving opener is not a fix");
    assert_eq!(at.unresolved.len(), 1, "it still comes back unresolved");
    assert_eq!(at.unresolved[0].step, 0);
    assert_eq!(at.unresolved[0].path, "f.txt");
}

#[test]
fn an_edit_beyond_the_markers_is_still_that_regions_edit() {
    let fx = Fixture::new();
    ident(&fx);
    let (_base, main, f1, f2) = stack(&fx);
    let repo = fx.repo();

    let chain = run_chain(&repo, &f1, &f2, &main);

    // Resolve the block and change the line immediately after it in the same
    // hunk: the `four` line becomes `FOUR`.
    let resolved = resolved_tree(&fx, &repo, "one\ntwo\nRESOLVED\nFOUR\nFEAT5\n");
    let at: Attribution = attribute(&repo, &chain, resolved).expect("attribute runs");

    assert_eq!(at.resolutions.len(), 1, "one region, one resolution");
    assert_eq!(at.resolutions[0].step, 0);
    assert_eq!(
        at.resolutions[0].with, "RESOLVED\nFOUR\n",
        "the resolution covers the block and the edit that spilled past it"
    );
    assert!(at.unresolved.is_empty());
}

#[test]
fn an_edit_far_from_every_region_is_not_attributed() {
    let fx = Fixture::new();
    ident(&fx);
    let (_base, main, f1, f2) = stack(&fx);
    let repo = fx.repo();

    let chain = run_chain(&repo, &f1, &f2, &main);

    // Resolve the block, and separately change a far line (FEAT5 -> FAR) with
    // untouched lines between. The far edit belongs to the last step and is
    // therefore not returned.
    let resolved = resolved_tree(&fx, &repo, "one\ntwo\nRESOLVED\nfour\nFAR\n");
    let at: Attribution = attribute(&repo, &chain, resolved).expect("attribute runs");

    assert_eq!(at.resolutions.len(), 1, "only the region is attributed");
    assert_eq!(at.resolutions[0].step, 0);
    assert_eq!(
        at.resolutions[0].with, "RESOLVED\n",
        "covering only the region"
    );
    assert!(at.unresolved.is_empty());
}

#[test]
fn an_edit_in_a_file_with_no_regions_is_not_attributed() {
    let fx = Fixture::new();
    ident(&fx);
    // A clean stack: no step marks anything, so the file the reader touches
    // carries no region.
    fx.write("f.txt", "a\nb\n");
    fx.write("g.txt", "g1\ng2\n");
    let base = fx.commit("base");
    fx.git(&["switch", "-q", "-c", "feature", &base]);
    fx.write("h.txt", "h\n");
    let f1 = fx.commit("a file main never touched");
    fx.git(&["switch", "-q", "main"]);
    fx.write("f.txt", "a\nB\n");
    let main = fx.commit("main");
    fx.git(&["switch", "-q", "feature"]);

    let repo = fx.repo();
    let chain = chain(&repo, oid(&f1), oid(&f1), &Change::Onto(oid(&main)), &[])
        .expect("a clean replay runs");
    assert!(
        chain.steps.iter().all(|s| s.paths.is_empty()),
        "no step marks anything: {:?}",
        chain.steps
    );

    // The reader touches g.txt, a file no step marked.
    let resolved = {
        fx.write("g.txt", "g1\nG2\n");
        commit_tree(&repo, oid(&fx.commit("resolved")))
    };
    let at: Attribution = attribute(&repo, &chain, resolved).expect("attribute runs");

    assert!(at.resolutions.is_empty(), "nothing to attribute");
    assert!(at.unresolved.is_empty(), "nothing left alone");
}

#[test]
fn the_last_steps_own_region_is_not_returned() {
    let fx = Fixture::new();
    ident(&fx);
    // The last commit is the one that conflicts: step 0 is clean, step 1
    // (the last) marks f.txt.
    fx.write("f.txt", "one\ntwo\nthree\nfour\nfive\n");
    let base = fx.commit("base");
    fx.git(&["switch", "-q", "-c", "feature", &base]);
    fx.write("f.txt", "one\ntwo\nthree\nfour\nF1\n");
    let f1 = fx.commit("clean step");
    fx.write("f.txt", "one\ntwo\nFEAT\nfour\nF1\n");
    let f2 = fx.commit("the one that conflicts");
    fx.git(&["switch", "-q", "main"]);
    fx.write("f.txt", "one\ntwo\nMAIN\nfour\nfive\n");
    let main = fx.commit("main");
    fx.git(&["switch", "-q", "feature"]);

    let repo = fx.repo();
    let chain =
        chain(&repo, oid(&f1), oid(&f2), &Change::Onto(oid(&main)), &[]).expect("the chain runs");
    let region = regions(&repo, &chain)
        .expect("regions resolve")
        .into_iter()
        .next()
        .expect("the last step leaves one region");
    assert_eq!(region.step, 1, "the region belongs to the last step");

    // Resolve its markers: the last step's tree is the resolved tree itself,
    // so the resolution is dropped on the floor.
    let resolved = resolved_tree(&fx, &repo, "one\ntwo\nRESOLVED\nfour\nF1\n");
    let at: Attribution = attribute(&repo, &chain, resolved).expect("attribute runs");

    assert!(
        at.resolutions.is_empty(),
        "the last step's own region is not returned"
    );
    assert!(at.unresolved.is_empty());
}

#[test]
fn a_deleted_file_attributes_nothing() {
    let fx = Fixture::new();
    ident(&fx);
    let (_base, main, f1, f2) = stack(&fx);
    let repo = fx.repo();

    let chain = run_chain(&repo, &f1, &f2, &main);
    let _region = marker_region(&repo, &chain);

    // Resolve by deleting the conflicted file outright.
    let resolved = {
        fx.remove("f.txt");
        commit_tree(&repo, oid(&fx.commit("resolved")))
    };
    let at: Attribution = attribute(&repo, &chain, resolved).expect("attribute runs");

    assert!(
        at.resolutions.is_empty(),
        "a deleted file attributes no resolution"
    );
    assert!(
        at.unresolved.is_empty(),
        "a deleted file attributes no unresolved region"
    );
}

#[test]
fn two_regions_in_one_file_are_attributed_separately() {
    let fx = Fixture::new();
    ident(&fx);
    // Two commits each conflict on a different part of the same file, so both
    // blocks stand in the final tree. A third commit sits last, clean and on
    // a different file, so neither conflict is the last step's — both
    // resolutions are returned rather than one being dropped on the floor.
    fx.write("f.txt", "one\ntwo\nthree\nfour\nfive\nsix\n");
    let base = fx.commit("base");
    fx.git(&["switch", "-q", "-c", "feature", &base]);
    fx.write("f.txt", "one\ntwo\nF1\nfour\nfive\nsix\n");
    let f1 = fx.commit("add one");
    fx.write("f.txt", "one\ntwo\nF1\nfour\nF2\nsix\n");
    fx.commit("add two");
    fx.write("g.txt", "g\n");
    let f3 = fx.commit("a clean tail, off to the side");
    fx.git(&["switch", "-q", "main"]);
    fx.write("f.txt", "one\ntwo\nM3\nfour\nM5\nsix\n");
    let main = fx.commit("main");
    fx.git(&["switch", "-q", "feature"]);

    let repo = fx.repo();
    let chain = chain(&repo, oid(&f1), oid(&f3), &Change::Onto(oid(&main)), &[])
        .expect("both conflicts are on different regions, so no tangle");
    assert!(
        chain.tangled.is_none(),
        "two regions on one file do not tangle"
    );
    let found = regions(&repo, &chain).expect("regions resolve");
    assert_eq!(found.len(), 2, "both blocks stand in the final tree");

    // Resolve the two regions differently.
    let resolved = resolved_tree(&fx, &repo, "one\ntwo\nR1\nfour\nR2\nsix\n");
    let at: Attribution = attribute(&repo, &chain, resolved).expect("attribute runs");

    assert_eq!(at.resolutions.len(), 2, "two regions, two resolutions");
    assert_eq!(at.resolutions[0].step, 0);
    assert_eq!(
        at.resolutions[0].with, "R1\n",
        "the first region's own text"
    );
    assert_eq!(at.resolutions[1].step, 1);
    assert_eq!(
        at.resolutions[1].with, "R2\n",
        "the second region's own text"
    );
    assert!(at.unresolved.is_empty());
}

#[test]
fn what_attribute_returns_feeds_straight_back_into_chain() {
    let fx = Fixture::new();
    ident(&fx);
    let (_base, main, f1, f2) = stack(&fx);
    let repo = fx.repo();

    // Case 1: attribute the one fix into its step.
    let first = run_chain(&repo, &f1, &f2, &main);
    let _region = marker_region(&repo, &first);
    let resolved = resolved_tree(&fx, &repo, "one\ntwo\nRESOLVED\nfour\nFEAT5\n");
    let at: Attribution = attribute(&repo, &first, resolved).expect("attribute runs");
    let resolutions: Vec<Resolution> = at.resolutions.clone();
    assert_eq!(resolutions.len(), 1);

    // The round trip: hand the resolutions back into `chain`.
    let second = chain(
        &repo,
        oid(&f1),
        oid(&f2),
        &Change::Onto(oid(&main)),
        &resolutions,
    )
    .expect("the resolutions land");

    // Every step's tree is marker-free.
    for step in &second.steps {
        let blob = tree_blob(&repo, step.tree, "f.txt");
        assert!(
            !blob.contains("<<<<<<< the rewrite so far"),
            "step {}'s tree is marker-free: {blob}",
            step.old
        );
    }

    // The earlier step's blob holds the resolved text.
    assert_eq!(
        tree_blob(&repo, second.steps[0].tree, "f.txt"),
        "one\ntwo\nRESOLVED\nfour\nfive\n",
        "step 1's block is replaced where it was owned"
    );
    // The whole stack lands clean, step 2's change on top.
    assert_eq!(
        tree_blob(&repo, second.tree, "f.txt"),
        "one\ntwo\nRESOLVED\nfour\nFEAT5\n",
        "the stack lands clean"
    );
}
