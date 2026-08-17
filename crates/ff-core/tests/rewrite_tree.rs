//! Contract for the replay half of [`ff_core::rewrite::plan`]: a message
//! change moves no tree and must not; a tree change replays descendants by
//! three-way merge; a conflict refuses the rewrite and writes nothing.

use ff_core::gix;
use ff_core::rewrite::{Change, RewritePlan, plan};
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

/// Every loose object on disk. Reachability proves nothing here — `plan`
/// moves no refs, so a commit it wrote would be unreachable whether or not
/// the refusal held. Only the object store itself can answer.
fn loose_objects(fx: &Fixture) -> usize {
    fx.git(&["count-objects", "-v"])
        .lines()
        .find_map(|line| line.strip_prefix("count: "))
        .and_then(|n| n.trim().parse::<usize>().ok())
        .expect("count-objects reports a loose count")
}

/// The tree id of a commit, straight from git.
fn tree_of(fx: &Fixture, commit: &str) -> String {
    fx.git(&["rev-parse", &format!("{commit}^{{tree}}")])
        .trim()
        .to_string()
}

#[test]
fn message_change_leaves_descendant_trees_untouched() {
    let fx = Fixture::new();
    ident(&fx);
    fx.write("a.txt", "one\n");
    let c1 = fx.commit("one");
    fx.write("b.txt", "b\n");
    let c2 = fx.commit("two");
    fx.write("a.txt", "three\n");
    let c3 = fx.commit("three");
    fx.write("c.txt", "c\n");
    let c4 = fx.commit("four");

    let original_trees: Vec<(&str, String)> = [c1.as_str(), c2.as_str(), c3.as_str(), c4.as_str()]
        .into_iter()
        .map(|c| (c, tree_of(&fx, c)))
        .collect();

    let repo = fx.repo();
    let rewritten: RewritePlan = plan(
        &repo,
        oid(&c2),
        oid(&c4),
        &Change::Message("reworded".into()),
        NOW,
    )
    .unwrap();

    assert_eq!(
        rewritten.rewrites.len(),
        3,
        "the target and its two descendants are rewritten; c1 is below the target"
    );
    for r in &rewritten.rewrites {
        let original = original_trees
            .iter()
            .find(|(c, _)| *c == r.old)
            .map(|(_, t)| t.clone())
            .expect("every rewrite was part of the stack");
        assert_eq!(
            tree_of(&fx, &r.new),
            original,
            "a message change must not move the tree of {}",
            r.old
        );
    }
}

#[test]
fn tree_change_replays_descendants() {
    let fx = Fixture::new();
    ident(&fx);
    fx.write("a.txt", "original\n");
    let c1 = fx.commit("one");
    fx.write("b.txt", "b\n");
    let c2 = fx.commit("two");

    // The target's new tree: a.txt modified, no b.txt. The scratch commit
    // exists only to give that tree a sha to hand to `Change::Tree`.
    fx.write("a.txt", "modified\n");
    std::fs::remove_file(fx.path().join("b.txt")).unwrap();
    let scratch = fx.commit("scratch");
    let new_tree = tree_of(&fx, &scratch);

    let repo = fx.repo();
    let rewritten = plan(
        &repo,
        oid(&c1),
        oid(&c2),
        &Change::Tree(oid(&new_tree)),
        NOW,
    )
    .unwrap();

    let c2_new = rewritten
        .rewrites
        .iter()
        .find(|r| r.old == c2)
        .expect("c2 was replayed");
    let c2_tree = tree_of(&fx, &c2_new.new);

    let a = fx.git(&["show", &format!("{c2_tree}:a.txt")]);
    assert_eq!(a.trim(), "modified", "c2' must carry the new a.txt: {a}");
    let b = fx.git(&["show", &format!("{c2_tree}:b.txt")]);
    assert_eq!(b.trim(), "b", "c2' must carry its own b.txt: {b}");
}

#[test]
fn tree_change_conflict_refuses_and_writes_nothing() {
    let fx = Fixture::new();
    ident(&fx);
    fx.write("f.txt", "one\n");
    let c1 = fx.commit("one");
    fx.write("f.txt", "two\n");
    let c2 = fx.commit("two");

    // c1' changes the same line c2 changed, differently: replaying c2 over
    // it must conflict.
    fx.write("f.txt", "one-prime\n");
    let scratch = fx.commit("scratch");
    let new_tree = tree_of(&fx, &scratch);

    let loose_before = loose_objects(&fx);

    let repo = fx.repo();
    let err = plan(
        &repo,
        oid(&c1),
        oid(&c2),
        &Change::Tree(oid(&new_tree)),
        NOW,
    )
    .expect_err("replaying a conflicting commit must refuse");

    assert_eq!(err.id(), "held/rewrite-conflict", "{err}");
    assert!(
        err.to_string().contains("f.txt"),
        "the message must name the path: {err}"
    );
    // The dry run against an in-memory object store is what makes this true:
    // the conflict is raised before the real pass writes anything, so a
    // refused rewrite costs the object store nothing at all.
    assert_eq!(
        loose_objects(&fx),
        loose_before,
        "a refused rewrite must leave no object on disk"
    );
}

/// A merge commit over `c2` and `c3`, built object-level with
/// `commit-tree` so no ref moves — `plan` takes the tip as an id, so the
/// merge need not sit at any head.
fn merge_commit(fx: &Fixture, c2: &str, c3: &str) -> String {
    let tree = tree_of(fx, c3);
    fx.git(&["commit-tree", &tree, "-p", c2, "-p", c3, "-m", "merge"])
        .trim()
        .to_string()
}

#[test]
fn merge_commit_refuses_under_tree_change() {
    let fx = Fixture::new();
    ident(&fx);
    fx.write("a.txt", "one\n");
    let c1 = fx.commit("one");
    fx.write("b.txt", "b\n");
    let c2 = fx.commit("two");
    fx.write("c.txt", "c\n");
    let c3 = fx.commit("three");
    let merge = merge_commit(&fx, &c2, &c3);

    let repo = fx.repo();
    let err = plan(
        &repo,
        oid(&c1),
        oid(&merge),
        &Change::Tree(oid(&tree_of(&fx, &c3))),
        NOW,
    )
    .expect_err("a merge in the range must refuse under a tree change");

    assert_eq!(err.id(), "rewrite/merge-in-range", "{err}");
    assert!(err.to_string().contains("is a merge"), "{err}");
}

#[test]
fn merge_commit_still_allowed_under_message_change() {
    let fx = Fixture::new();
    ident(&fx);
    fx.write("a.txt", "one\n");
    let c1 = fx.commit("one");
    fx.write("b.txt", "b\n");
    let c2 = fx.commit("two");
    fx.write("c.txt", "c\n");
    let c3 = fx.commit("three");
    let merge = merge_commit(&fx, &c2, &c3);

    let repo = fx.repo();
    let rewritten = plan(
        &repo,
        oid(&c1),
        oid(&merge),
        &Change::Message("reworded".into()),
        NOW,
    )
    .expect("re-parenting a merge stays allowed under a message change");

    assert!(
        rewritten.rewrites.iter().any(|r| r.old == merge),
        "the merge commit itself is re-parented"
    );
}

#[test]
fn target_not_in_history_still_refuses() {
    let fx = Fixture::new();
    ident(&fx);
    fx.write("a.txt", "one\n");
    let c1 = fx.commit("one");
    fx.write("b.txt", "b\n");
    let c2 = fx.commit("two");

    // A side commit: a sibling of c2, not in c2's history.
    let side = fx
        .git(&["commit-tree", &tree_of(&fx, &c1), "-p", &c1, "-m", "side"])
        .trim()
        .to_string();

    let repo = fx.repo();
    let err = plan(
        &repo,
        oid(&side),
        oid(&c2),
        &Change::Tree(oid(&tree_of(&fx, &c1))),
        NOW,
    )
    .expect_err("a target outside the tip's history must refuse");

    assert_eq!(err.id(), "rewrite/not-in-history", "{err}");
}

/// The shared stack:
///
/// c0 ─ c1 ─────────── m2        (main)
///       └─ f1 ─ f2 ─ f3         (feature, with `mid` at f2)
///
/// Different files throughout, so the replay is clean. Leaves the fixture
/// standing on `feature`.
fn onto_stack(fx: &Fixture) -> (String, String, String, String, String, String) {
    fx.write("root.txt", "root\n");
    let c0 = fx.commit("root");
    fx.write("m.txt", "m\n");
    let c1 = fx.commit("m");

    fx.git(&["switch", "-q", "-c", "feature"]);
    fx.write("a.txt", "a\n");
    let f1 = fx.commit("f1");
    fx.write("b.txt", "b\n");
    let f2 = fx.commit("f2");
    fx.write("c.txt", "c\n");
    let f3 = fx.commit("f3");
    fx.git(&["branch", "mid", &f2]);

    fx.git(&["switch", "-q", "main"]);
    fx.write("d.txt", "d\n");
    let m2 = fx.commit("m2");

    fx.git(&["switch", "-q", "feature"]);
    (c0, c1, f1, f2, f3, m2)
}

#[test]
fn onto_replays_the_target_onto_the_new_base() {
    let fx = Fixture::new();
    ident(&fx);
    let (_c0, _c1, f1, f2, f3, m2) = onto_stack(&fx);

    let repo = fx.repo();
    let rewritten = plan(&repo, oid(&f1), oid(&f3), &Change::Onto(oid(&m2)), NOW).unwrap();

    assert_eq!(
        rewritten.rewrites.len(),
        3,
        "the target and both descendants are rewritten"
    );

    let new = |old: &str| {
        rewritten
            .rewrites
            .iter()
            .find(|r| r.old == *old)
            .expect("the commit was rewritten")
            .new
            .clone()
    };
    let new_f1 = new(&f1);
    let new_f2 = new(&f2);
    let new_f3 = new(&f3);

    assert_eq!(
        fx.git(&["rev-parse", &format!("{new_f1}^")]).trim(),
        m2,
        "the new target hangs from the new base"
    );
    assert_eq!(
        rewritten.new_tip.to_string(),
        new_f3,
        "the new tip is the rewrite of f3"
    );

    // The target's replay is the three-way merge git itself produces,
    // because merge-base(m2, f1) is exactly the target's old first parent.
    let expected = fx
        .git(&["merge-tree", "--write-tree", &m2, &f1])
        .trim()
        .to_string();
    assert_eq!(
        tree_of(&fx, &new_f1),
        expected,
        "the target's new tree is git's own merge"
    );

    // The descendants are replayed over the rewritten parent: every
    // rewritten tree carries d.txt from the new base and keeps its own file.
    for (sha, own) in [
        (new_f1.as_str(), "a.txt"),
        (new_f2.as_str(), "b.txt"),
        (new_f3.as_str(), "c.txt"),
    ] {
        let listing = fx.git(&["ls-tree", "-r", "--name-only", sha]);
        let paths: Vec<&str> = listing.lines().collect();
        assert!(
            paths.contains(&"d.txt"),
            "{sha} must carry d.txt from the new base"
        );
        assert!(paths.contains(&own), "{sha} must keep its own {own}");
    }
}

#[test]
fn onto_carries_a_branch_inside_the_range() {
    let fx = Fixture::new();
    ident(&fx);
    let (_c0, _c1, f1, f2, f3, m2) = onto_stack(&fx);

    let repo = fx.repo();
    let rewritten = plan(&repo, oid(&f1), oid(&f3), &Change::Onto(oid(&m2)), NOW).unwrap();

    let new_f2 = rewritten
        .rewrites
        .iter()
        .find(|r| r.old == f2)
        .expect("mid's commit was rewritten")
        .new
        .clone();
    let mid = rewritten
        .carried
        .iter()
        .find(|c| c.name == "refs/heads/mid")
        .expect("mid sits inside the rewritten range");
    assert_eq!(mid.old.as_deref(), Some(f2.as_str()), "mid's old end is f2");
    assert_eq!(
        mid.new.as_deref(),
        Some(new_f2.as_str()),
        "mid's new end is f2's rewrite"
    );

    assert!(
        rewritten
            .carried
            .iter()
            .any(|c| c.name == "refs/heads/feature"),
        "feature is the range's tip and must carry too"
    );
}

/// Guards against seeding the rewrite map: the target's new parent must not
/// be published into it, because `carried` is built by looking every local
/// head up in that map and a seeded entry would move a branch nobody named.
#[test]
fn onto_leaves_a_branch_at_the_old_fork_point_alone() {
    let fx = Fixture::new();
    ident(&fx);
    let (_c0, c1, f1, _f2, f3, m2) = onto_stack(&fx);
    fx.git(&["branch", "stay", &c1]);

    let repo = fx.repo();
    let rewritten = plan(&repo, oid(&f1), oid(&f3), &Change::Onto(oid(&m2)), NOW).unwrap();

    assert!(
        rewritten
            .carried
            .iter()
            .all(|c| c.name != "refs/heads/stay"),
        "stay sits at the old fork point and nobody named it: {:?}",
        rewritten.carried
    );
    assert_eq!(
        fx.git(&["rev-parse", "stay"]).trim(),
        c1,
        "stay still reads the old fork point"
    );
}

#[test]
fn onto_conflict_refuses_and_writes_nothing() {
    let fx = Fixture::new();
    ident(&fx);
    fx.write("f.txt", "one\n");
    fx.commit("base");
    // main and feature edit the same line of the same file from the same
    // base, so replaying f1 over m2 must conflict.
    fx.git(&["switch", "-q", "-c", "feature"]);
    fx.write("f.txt", "two\n");
    let f1 = fx.commit("f1");
    fx.git(&["switch", "-q", "main"]);
    fx.write("f.txt", "three\n");
    let m2 = fx.commit("m2");
    fx.git(&["switch", "-q", "feature"]);

    let loose_before = loose_objects(&fx);

    let repo = fx.repo();
    let err = plan(&repo, oid(&f1), oid(&f1), &Change::Onto(oid(&m2)), NOW)
        .expect_err("replaying a conflicting target must refuse");

    assert_eq!(err.id(), "held/rewrite-conflict", "{err}");
    assert!(
        err.to_string().contains("f.txt"),
        "the message must name the path: {err}"
    );
    assert_eq!(
        loose_objects(&fx),
        loose_before,
        "a refused rewrite must leave no object on disk"
    );
}

#[test]
fn onto_refuses_a_merge_target() {
    let fx = Fixture::new();
    ident(&fx);
    fx.write("a.txt", "one\n");
    let c1 = fx.commit("one");
    fx.write("b.txt", "b\n");
    let c2 = fx.commit("two");
    fx.write("c.txt", "c\n");
    let c3 = fx.commit("three");
    let merge = merge_commit(&fx, &c2, &c3);

    let repo = fx.repo();
    let err = plan(
        &repo,
        oid(&merge),
        oid(&merge),
        &Change::Onto(oid(&c1)),
        NOW,
    )
    .expect_err("a merge target must refuse under Onto");

    assert_eq!(err.id(), "rewrite/merge-in-range", "{err}");
    assert!(err.to_string().contains("is a merge"), "{err}");
}
