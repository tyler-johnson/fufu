//! Contract for the chain: a held rewrite replayed all the way through with
//! its conflicts carried as literal marker content, and the trees that come
//! back out when the marks are resolved.

use ff_core::gix;
use ff_core::rewrite::{Chain, Change, Resolution, chain, plan, plan_with, regions};
use ff_testsupport::Fixture;

const NOW: i64 = 1_799_999_999;

/// `plan` reads the committer identity from the repo config, which the
/// fixture's hermetic env does not set; git itself gets its identity from
/// env vars, so this is only for the gix side.
fn ident(fx: &Fixture) {
    fx.set_config("user.name", "Fixture Committer");
    fx.set_config("user.email", "committer@fixture.test");
}

fn oid(hex: &str) -> gix::ObjectId {
    gix::ObjectId::from_hex(hex.trim().as_bytes()).unwrap()
}

/// The text of the blob at `path` in `tree`. Nearly every test needs to read a
/// result blob out of a tree, so this is the one shared reader.
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

/// The shared stack for the `Onto` conflict cases:
///
/// ```text
///        base ────────── main      (main: line 3 -> MAIN)
///          └─ f1 ─ f2    (feature, forked from base)
/// ```
///
/// Returns `(base, main, f1, f2)`; `f2` is `None` when the stack is one deep.
/// `feat2_blob` is the second commit's whole file, so each test chooses
/// whether it edits line 5 alongside step 1's conflict or rewrites line 3
/// over it. Leaves the working tree on `feature`.
fn onto_stack(
    fx: &Fixture,
    feat1_line: &str,
    feat2_blob: Option<&str>,
) -> (String, String, String, Option<String>) {
    fx.write("f.txt", "one\ntwo\nthree\nfour\nfive\n");
    let base = fx.commit("base");

    fx.git(&["switch", "-q", "-c", "feature", &base]);
    fx.write(
        "f.txt",
        format!("one\ntwo\n{feat1_line}\nfour\nfive\n").as_str(),
    );
    let f1 = fx.commit("add feature");
    let f2 = feat2_blob.map(|blob| {
        fx.write("f.txt", blob);
        fx.commit("add feature two")
    });

    fx.git(&["switch", "-q", "main"]);
    fx.write("f.txt", "one\ntwo\nMAIN\nfour\nfive\n");
    let main = fx.commit("main");
    fx.git(&["switch", "-q", "feature"]);

    (base, main, f1, f2)
}

#[test]
fn a_conflict_becomes_markers_instead_of_a_refusal() {
    let fx = Fixture::new();
    ident(&fx);
    let (_base, main, f1, _f2) = onto_stack(&fx, "FEAT", None);

    let repo = fx.repo();
    let chain: Chain = chain(&repo, oid(&f1), oid(&f1), &Change::Onto(oid(&main)), &[])
        .expect("a conflict must become markers, not a refusal");

    assert_eq!(chain.steps.len(), 1, "one feature commit, one step");
    assert_eq!(chain.steps[0].paths, vec!["f.txt".to_string()]);
    assert!(
        chain.tangled.is_none(),
        "a single conflict is clean, not a tangle"
    );

    let blob = tree_blob(&repo, chain.tree, "f.txt");
    assert!(
        blob.contains("<<<<<<< the rewrite so far"),
        "the opener: {blob}"
    );
    assert!(blob.contains("FEAT"), "the replayed side: {blob}");
    assert!(blob.contains("MAIN"), "the base side: {blob}");
    assert!(
        blob.lines()
            .any(|l| l == ">>>>>>> rebasing \"add feature\" (1/1)"),
        "the closer line is exact, subject and all: {blob}"
    );
}

#[test]
fn a_marked_region_is_carried_into_the_steps_above_it() {
    let fx = Fixture::new();
    ident(&fx);
    // Step 2's commit edits line 5, away from step 1's conflict on line 3.
    let (_base, main, f1, f2) = onto_stack(&fx, "FEAT", Some("one\ntwo\nFEAT\nfour\nFEAT5\n"));
    let f2 = f2.expect("the two-deep stack has a second commit");

    let repo = fx.repo();
    let chain = chain(&repo, oid(&f1), oid(&f2), &Change::Onto(oid(&main)), &[])
        .expect("the second step replays cleanly over the marker");

    assert_eq!(chain.steps.len(), 2, "both commits are steps");
    assert!(
        chain.steps[1].paths.is_empty(),
        "the second step touches only line 5, so it reports nothing: {:?}",
        chain.steps[1].paths
    );

    // Step 1's block must survive, byte-identical, into the final tree — that
    // is the property the whole attribution design rests on.
    let found = regions(&repo, &chain).expect("regions resolve");
    let region = found
        .iter()
        .find(|r| r.step == 0)
        .expect("step 1's region is still standing");
    let step1_blob = tree_blob(&repo, chain.steps[0].tree, "f.txt");
    let final_blob = tree_blob(&repo, chain.tree, "f.txt");
    assert!(
        step1_blob.contains(&region.block),
        "the block is byte-identical in step 1's tree"
    );
    assert!(
        final_blob.contains(&region.block),
        "the block is carried, byte-identical, into the final tree"
    );
    assert!(
        final_blob.contains("FEAT5"),
        "the second commit's change lands: {final_blob}"
    );
}

#[test]
fn only_the_conflicting_step_marks_anything() {
    let fx = Fixture::new();
    ident(&fx);

    fx.write("f.txt", "one\ntwo\nthree\nfour\nfive\n");
    let base = fx.commit("base");

    fx.git(&["switch", "-q", "-c", "feature", &base]);
    fx.write("a.txt", "a\n");
    let f1 = fx.commit("a file main never touched");
    fx.write("f.txt", "one\ntwo\nFEAT\nfour\nfive\n");
    let _f2 = fx.commit("the one that conflicts");
    fx.write("f.txt", "one\ntwo\nFEAT\nfour\nFEAT5\n");
    let f3 = fx.commit("well clear of the marks");

    fx.git(&["switch", "-q", "main"]);
    fx.write("f.txt", "one\ntwo\nMAIN\nfour\nfive\n");
    let main = fx.commit("main");
    fx.git(&["switch", "-q", "feature"]);

    let repo = fx.repo();
    let chain = chain(&repo, oid(&f1), oid(&f3), &Change::Onto(oid(&main)), &[])
        .expect("a conflict in the middle stops nothing");

    assert_eq!(chain.steps.len(), 3, "every commit is a step");
    assert!(
        chain.steps[0].paths.is_empty(),
        "the step before the conflict marks nothing"
    );
    assert_eq!(
        chain.steps[1].paths,
        vec!["f.txt".to_string()],
        "the conflicting step marks the one path"
    );
    assert!(
        chain.steps[2].paths.is_empty(),
        "the step after it marks nothing of its own"
    );
    assert!(
        chain.tangled.is_none(),
        "a later commit clear of the marks is not a tangle"
    );

    let found = regions(&repo, &chain).expect("regions resolve");
    assert_eq!(
        found.len(),
        1,
        "one region, from the one step that wrote it"
    );
    assert_eq!(found[0].step, 1, "tagged with the step that wrote it");
    assert!(
        tree_blob(&repo, chain.tree, "f.txt").contains("FEAT5"),
        "the step after the conflict still lands its own change"
    );
}

#[test]
fn two_conflicts_on_one_region_stop_the_chain() {
    let fx = Fixture::new();
    ident(&fx);
    // Both feature commits change line 3, to values that conflict with main.
    let (_base, main, f1, f2) = onto_stack(&fx, "FEAT1", Some("one\ntwo\nFEAT2\nfour\nfive\n"));
    let f2 = f2.expect("the two-deep stack has a second commit");

    let repo = fx.repo();
    let chain = chain(&repo, oid(&f1), oid(&f2), &Change::Onto(oid(&main)), &[])
        .expect("the chain runs and reports a tangle rather than erroring");

    assert_eq!(
        chain.steps.len(),
        1,
        "the chain stops before the second commit"
    );
    let tangle = chain.tangled.expect("two conflicts on one region tangle");
    assert_eq!(
        tangle.old, f2,
        "the tangle names the commit it stopped before"
    );
    assert_eq!(tangle.path, "f.txt");
    assert_eq!(
        chain.tree, chain.steps[0].tree,
        "the tree stays the last clean step's"
    );

    // The whole point of stopping: no nested opener in the final tree.
    let blob = tree_blob(&repo, chain.tree, "f.txt");
    let openers = blob
        .lines()
        .filter(|l| {
            l.trim_end_matches('\n')
                .starts_with("<<<<<<< the rewrite so far")
        })
        .count();
    assert_eq!(openers, 1, "exactly one opener, no nesting: {blob}");
}

/// A later commit whose change lands *inside* a standing mark, and lands
/// there cleanly because the rest of the marked text is context it never
/// touches, is a second conflict on that region wearing a disguise. The merge
/// folds it in, the block's text drifts away from the tree of the step that
/// owns it, and no resolution can be aimed at that step any more — so the
/// chain has to stop, exactly as it does when two conflicts interleave.
#[test]
fn a_later_commit_folding_into_a_mark_stops_the_chain() {
    let fx = Fixture::new();
    ident(&fx);
    // `f1` writes two lines and `main` rewrites the file, so step 1 marks the
    // whole of it. `f2` then changes only the second of `f1`'s lines, leaving
    // the first as context — small enough for the merge to apply it inside
    // the block instead of fighting it.
    fx.write("f.env", "");
    let base = fx.commit("base");
    fx.git(&["switch", "-q", "-c", "feature", &base]);
    fx.write("f.env", "alpha\n\n");
    let f1 = fx.commit("feature 1");
    fx.write("f.env", "alpha\nbeta\n");
    let f2 = fx.commit("feature 2");
    fx.git(&["switch", "-q", "main"]);
    fx.write("f.env", "gamma\n\ndelta");
    let main = fx.commit("main advances");
    fx.git(&["switch", "-q", "feature"]);

    let repo = fx.repo();
    let chain = chain(&repo, oid(&f1), oid(&f2), &Change::Onto(oid(&main)), &[])
        .expect("the chain runs and reports a tangle rather than erroring");

    assert_eq!(
        chain.steps.len(),
        1,
        "the chain stops before the commit that would fold into the mark"
    );
    // The mark that survives is the one step 1 wrote: `f2`'s "beta" is not in
    // it, and the block stands byte-identical in step 1's own tree, which is
    // the property attribution rests on.
    let found = regions(&repo, &chain).expect("regions resolve");
    let tangle = chain.tangled.expect("a fold into a mark is a tangle");
    assert_eq!(
        tangle.old, f2,
        "the tangle names the commit it stopped before"
    );
    assert_eq!(tangle.path, "f.env");
    assert_eq!(found.len(), 1, "one region, step 1's: {found:?}");
    assert_eq!(found[0].step, 0);
    assert!(
        !found[0].block.contains("beta"),
        "the later commit's change did not fold into the mark: {}",
        found[0].block
    );
    assert!(
        tree_blob(&repo, chain.steps[0].tree, "f.env").contains(&found[0].block),
        "the block is byte-identical in the tree of the step that owns it"
    );
}

#[test]
fn a_resolution_lands_in_the_step_that_owned_it() {
    let fx = Fixture::new();
    ident(&fx);
    // Case 2's fixture: step 2 edits line 5, away from the conflict.
    let (_base, main, f1, f2) = onto_stack(&fx, "FEAT", Some("one\ntwo\nFEAT\nfour\nFEAT5\n"));
    let f2 = f2.expect("the two-deep stack has a second commit");
    let repo = fx.repo();

    // First, find the one region the unresolved run leaves behind.
    let first =
        chain(&repo, oid(&f1), oid(&f2), &Change::Onto(oid(&main)), &[]).expect("runs clean");
    let region = regions(&repo, &first)
        .expect("regions resolve")
        .into_iter()
        .next()
        .expect("the unresolved run leaves one region");

    // Then resolve it, folded into the step that owned it.
    let resolution = Resolution {
        step: region.step,
        path: region.path,
        block: region.block,
        with: "RESOLVED\n".to_string(),
    };
    let second = chain(
        &repo,
        oid(&f1),
        oid(&f2),
        &Change::Onto(oid(&main)),
        &[resolution],
    )
    .expect("the resolution lands");

    assert!(second.tangled.is_none(), "a resolved stack tangles nothing");
    assert_eq!(
        tree_blob(&repo, second.steps[0].tree, "f.txt"),
        "one\ntwo\nRESOLVED\nfour\nfive\n",
        "step 1's block is replaced where it was owned"
    );
    assert_eq!(
        tree_blob(&repo, second.tree, "f.txt"),
        "one\ntwo\nRESOLVED\nfour\nFEAT5\n",
        "the whole stack lands clean, step 2's change on top"
    );
}

#[test]
fn a_resolution_that_does_not_match_is_an_error() {
    let fx = Fixture::new();
    ident(&fx);
    let (_base, main, f1, f2) = onto_stack(&fx, "FEAT", Some("one\ntwo\nFEAT\nfour\nFEAT5\n"));
    let f2 = f2.expect("the two-deep stack has a second commit");
    let repo = fx.repo();

    let resolution = Resolution {
        step: 0,
        path: "f.txt".to_string(),
        block: "a block that is not in the tree".to_string(),
        with: "X".to_string(),
    };
    let err = chain(
        &repo,
        oid(&f1),
        oid(&f2),
        &Change::Onto(oid(&main)),
        &[resolution],
    )
    .expect_err("a resolution the engine cannot honor must be an error");
    assert!(
        err.to_string().contains("f.txt"),
        "the message names the path: {err}"
    );
}

#[test]
fn a_clean_replay_runs_every_step_and_marks_nothing() {
    let fx = Fixture::new();
    ident(&fx);
    // main and feature touch different files: the replay is clean.
    fx.write("a.txt", "root\n");
    let base = fx.commit("base");
    fx.git(&["switch", "-q", "-c", "feature", &base]);
    fx.write("c.txt", "f1\n");
    let f1 = fx.commit("f1");
    fx.write("d.txt", "f2\n");
    let f2 = fx.commit("f2");
    fx.git(&["switch", "-q", "main"]);
    fx.write("b.txt", "main\n");
    let main = fx.commit("main");
    fx.git(&["switch", "-q", "feature"]);

    let repo = fx.repo();
    let chain = chain(&repo, oid(&f1), oid(&f2), &Change::Onto(oid(&main)), &[])
        .expect("a clean replay runs");

    assert_eq!(chain.steps.len(), 2, "every step is present");
    for step in &chain.steps {
        assert!(
            step.paths.is_empty(),
            "a clean step marks nothing: {:?}",
            step.paths
        );
    }
    assert!(chain.tangled.is_none(), "a clean replay tangles nothing");

    let plan = plan(&repo, oid(&f1), oid(&f2), &Change::Onto(oid(&main)), NOW)
        .expect("plan succeeds on a clean replay");
    let plan_tree = commit_tree(&repo, plan.new_tip);
    assert_eq!(
        chain.tree, plan_tree,
        "the clean chain lands exactly where plan does"
    );
}

#[test]
fn a_reword_chain_moves_no_tree() {
    let fx = Fixture::new();
    ident(&fx);
    fx.write("a.txt", "root\n");
    let _base = fx.commit("base");
    fx.write("b.txt", "b\n");
    let f1 = fx.commit("f1");
    fx.write("c.txt", "c\n");
    let f2 = fx.commit("f2");

    let repo = fx.repo();
    let chain = chain(
        &repo,
        oid(&f1),
        oid(&f2),
        &Change::Message("reworded".into()),
        &[],
    )
    .expect("a reword runs");

    assert_eq!(chain.steps.len(), 2);
    for (step, commit) in chain.steps.iter().zip([f1.as_str(), f2.as_str()]) {
        assert_eq!(
            step.tree,
            commit_tree(&repo, oid(commit)),
            "a reword carries the commit's own tree"
        );
        assert!(step.paths.is_empty(), "a reword marks nothing");
    }
    assert!(chain.tangled.is_none(), "a reword tangles nothing");
}

#[test]
fn planning_with_a_tree_given_skips_the_merge() {
    let fx = Fixture::new();
    ident(&fx);
    let (_base, main, f1, _f2) = onto_stack(&fx, "FEAT", None);

    let repo = fx.repo();
    // Supply the target's own tree — one that would otherwise conflict with
    // main — so the merge is skipped.
    let supplied = commit_tree(&repo, oid(&f1));
    let trees = std::collections::HashMap::from([(oid(&f1), supplied)]);
    let plan = plan_with(
        &repo,
        oid(&f1),
        oid(&f1),
        &Change::Onto(oid(&main)),
        NOW,
        &trees,
    )
    .expect("a supplied tree skips the merge, no conflict");

    let new = plan
        .rewrites
        .first()
        .expect("the target is rewritten")
        .new
        .clone();
    assert_eq!(
        commit_tree(&repo, oid(&new)),
        supplied,
        "the new commit's tree is exactly the one supplied"
    );
}

#[test]
fn planning_with_an_empty_map_is_planning() {
    let fx = Fixture::new();
    ident(&fx);
    // A clean stack, so both paths return a plan rather than refusing.
    fx.write("a.txt", "root\n");
    let base = fx.commit("base");
    fx.git(&["switch", "-q", "-c", "feature", &base]);
    fx.write("c.txt", "f1\n");
    let f1 = fx.commit("f1");
    fx.write("d.txt", "f2\n");
    let f2 = fx.commit("f2");
    fx.git(&["switch", "-q", "main"]);
    fx.write("b.txt", "main\n");
    let main = fx.commit("main");

    let repo = fx.repo();
    let empty = std::collections::HashMap::<gix::ObjectId, gix::ObjectId>::new();
    let a = plan(&repo, oid(&f1), oid(&f2), &Change::Onto(oid(&main)), NOW).expect("plan succeeds");
    let b = plan_with(
        &repo,
        oid(&f1),
        oid(&f2),
        &Change::Onto(oid(&main)),
        NOW,
        &empty,
    )
    .expect("plan_with with an empty map succeeds");

    assert_eq!(
        a.rewrites, b.rewrites,
        "the delegation pins: rewrites agree"
    );
    assert_eq!(a.dropped, b.dropped, "the delegation pins: dropped agree");
    assert_eq!(a.new_tip, b.new_tip, "the delegation pins: new_tip agrees");
}

#[test]
fn two_regions_far_apart_in_one_file_are_both_kept() {
    let fx = Fixture::new();
    ident(&fx);

    // Two conflicts in one file, with clean ground between them, and a third
    // commit that touches the same file again. Both blocks have to come
    // through the third step intact and separately attributable — regions
    // that do not overlap are not a tangle, however many of them a file
    // collects.
    fx.write("f.txt", "a1\na2\na3\nmid\nb1\nb2\nb3\n");
    let base = fx.commit("base");

    fx.git(&["switch", "-q", "-c", "feature", &base]);
    fx.write("f.txt", "a1\nFEAT_A\na3\nmid\nb1\nb2\nb3\n");
    let f1 = fx.commit("the first region");
    fx.write("f.txt", "a1\nFEAT_A\na3\nmid\nb1\nFEAT_B\nb3\n");
    let _f2 = fx.commit("the second region");
    fx.write("f.txt", "a1\nFEAT_A\na3\nFEAT_MID\nb1\nFEAT_B\nb3\n");
    let f3 = fx.commit("well clear of both");

    fx.git(&["switch", "-q", "main"]);
    fx.write("f.txt", "a1\nMAIN_A\na3\nmid\nb1\nMAIN_B\nb3\n");
    let main = fx.commit("main moves both");
    fx.git(&["switch", "-q", "feature"]);

    let repo = fx.repo();
    let chain = chain(&repo, oid(&f1), oid(&f3), &Change::Onto(oid(&main)), &[])
        .expect("two regions far apart do not tangle");

    assert!(chain.tangled.is_none(), "separate regions are not a tangle");
    assert_eq!(chain.steps.len(), 3, "every commit is a step");

    let blob = tree_blob(&repo, chain.tree, "f.txt");
    let openers = blob
        .lines()
        .filter(|l| {
            l.trim_end_matches('\n')
                .starts_with("<<<<<<< the rewrite so far")
        })
        .count();
    assert_eq!(openers, 2, "both regions stand in the final tree: {blob}");
    assert!(
        blob.contains("(1/3)") && blob.contains("(2/3)"),
        "each block names the step that wrote it, on both markers: {blob}"
    );
    assert!(
        blob.contains("FEAT_MID"),
        "the third commit's own change lands too: {blob}"
    );

    let found = regions(&repo, &chain).expect("regions resolve");
    assert_eq!(found.len(), 2, "both regions are attributable");
    assert_eq!(found[0].step, 0);
    assert_eq!(found[1].step, 1);
}

#[test]
fn a_tree_handed_in_already_marked_is_still_attributed() {
    let fx = Fixture::new();
    ident(&fx);

    // What an absorb whose fold conflicted hands over: the target's new tree,
    // carrying marks the chain did not write. Nothing later claims them, so
    // the chain has to notice them itself or the region stands in every tree
    // with no step owning it.
    fx.write("f.txt", "one\ntwo\nthree\n");
    let c1 = fx.commit("the target");
    fx.write("g.txt", "g\n");
    let c2 = fx.commit("a descendant");

    let repo = fx.repo();
    // A tree for c1 that already carries a block, built by hand the way a
    // conflicted fold would leave it.
    fx.write(
        "f.txt",
        "one\n<<<<<<< the rewrite so far (1/2)\ntwo\n=======\nTWO\n>>>>>>> rebasing \"the target\" (1/2)\nthree\n",
    );
    let marked = fx.commit("the fold, as it would land");
    let marked_tree = commit_tree(&repo, oid(&marked));
    fx.git(&["reset", "-q", "--hard", &c2]);

    let repo = fx.repo();
    let chain = chain(
        &repo,
        oid(&c1),
        oid(&c2),
        &Change::Tree {
            tree: marked_tree,
            message: None,
        },
        &[],
    )
    .expect("a marked tree is a tree like any other");

    assert_eq!(chain.steps.len(), 2, "both commits are steps");
    assert_eq!(
        chain.steps[0].paths,
        vec!["f.txt".to_string()],
        "the handed-in tree's own marks are the target step's"
    );

    let found = regions(&repo, &chain).expect("regions resolve");
    assert_eq!(found.len(), 1, "the fold's region is attributable");
    assert_eq!(found[0].step, 0, "it belongs to the target");
    assert_eq!(found[0].path, "f.txt");
}
