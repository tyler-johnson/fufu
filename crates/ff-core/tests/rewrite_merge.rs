//! Contract for a merge inside a rewritten range: its parents are mapped
//! through the rewrite, re-merged, its own change laid over the result, and
//! a parent left beneath another dropped. A merge of trunk collapses into a
//! straight line when the branch moves onto newer trunk; a merge of a side
//! branch stays a merge with both sides mapped; a merge that resolved a
//! conflict, or carried an edit, keeps what it did.

use std::collections::HashMap;

use ff_core::Provenance;
use ff_core::futures::At;
use ff_core::gix;
use ff_core::rewrite::{Change, DropReason, Resolution, chain, conflict, plan, plan_with, regions};
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

/// The tree id of a commit, straight from git.
fn tree_of(fx: &Fixture, commit: &str) -> String {
    fx.git(&["rev-parse", &format!("{commit}^{{tree}}")])
        .trim()
        .to_string()
}

/// The parents of a commit, in order.
fn parents_of(fx: &Fixture, commit: &str) -> Vec<String> {
    fx.git(&["rev-list", "--parents", "-n1", commit])
        .split_whitespace()
        .skip(1)
        .map(str::to_string)
        .collect()
}

/// The file names at the top of a commit's tree, sorted.
fn files_of(fx: &Fixture, commit: &str) -> Vec<String> {
    let mut names: Vec<String> = fx
        .git(&["ls-tree", "--name-only", commit])
        .lines()
        .map(str::to_string)
        .collect();
    names.sort();
    names
}

fn blob(fx: &Fixture, commit: &str, path: &str) -> String {
    fx.git(&["show", &format!("{commit}:{path}")])
}

/// Every loose object on disk: `plan` moves no refs, so only the store can
/// say whether a refusal wrote anything.
fn loose_objects(fx: &Fixture) -> usize {
    fx.git(&["count-objects", "-v"])
        .lines()
        .find_map(|line| line.strip_prefix("count: "))
        .and_then(|n| n.trim().parse::<usize>().ok())
        .expect("count-objects reports a loose count")
}

fn ancestry(fx: &Fixture, tip: &str) -> Vec<String> {
    fx.git(&["rev-list", tip])
        .lines()
        .map(str::to_string)
        .collect()
}

/// A commit closed by fufu, so it carries a `change-id` header — the
/// identity the remap reads. `fx.commit` is raw git and carries none.
fn close(fx: &Fixture, msg: &str) -> String {
    let repo = fx.repo();
    ff_core::close(
        &repo,
        &ff_core::CloseOptions {
            message: Some(msg.into()),
            now: Some(NOW),
            argv: vec!["ff".into(), "commit".into()],
            ..Default::default()
        },
        &Provenance::new("pre", Some("ff commit".into())),
    )
    .unwrap();
    fx.git(&["rev-parse", "HEAD"]).trim().to_string()
}

/// The open change folded into `target` — the way a fufu commit's content
/// changes without losing its header, where `git commit --amend` drops it.
fn absorb_into(fx: &Fixture, target: &str) -> String {
    let repo = fx.repo();
    let into = gix::ObjectId::from_hex(target.as_bytes()).unwrap();
    ff_core::absorb::move_change(
        &repo,
        &ff_core::absorb::MoveOptions {
            verb: ff_core::absorb::MoveVerb::Absorb,
            from: None,
            into: Some(ff_core::absorb::Endpoint::Commit(into)),
            paths: Vec::new(),
            message: None,
            verify: ff_core::Verify::Run,
            now: Some(NOW),
            argv: vec!["ff".into(), "absorb".into()],
        },
        &Provenance::new("pre", Some("ff absorb".into())),
    )
    .unwrap();
    fx.git(&["rev-parse", "HEAD"]).trim().to_string()
}

/// The shas of the trunk-merge shape.
struct Shape {
    f1: String,
    f2: String,
    t1: String,
    m: String,
    f3: String,
    t2: String,
}

/// A feature branch that merged trunk once, with trunk moved on since:
///
/// ```text
/// T0 ─ T1 ──────── T2          (main)
///  └─ f1 ─ f2 ─ M ─ f3         (feature; M merges T1)
/// ```
///
/// `conflict` makes f2 and T1 edit the same line, which M resolves to
/// `resolved`. `extra` gives M an edit of its own — `extra.txt` written
/// before the merge commit — and T2 a different `extra.txt`, so carrying M
/// onto T2 conflicts in the merge's own change. HEAD ends on `feature`.
fn trunk_merge(fx: &Fixture, conflict: bool, extra: bool) -> Shape {
    fx.write("f.txt", "one\ntwo\nthree\n");
    fx.write("main.txt", "main\n");
    fx.commit("T0");

    fx.git(&["switch", "-q", "-c", "feature"]);
    fx.write("a.txt", "a\n");
    let f1 = fx.commit("f1");
    if conflict {
        fx.write("f.txt", "one\nfeat\nthree\n");
    } else {
        fx.write("b.txt", "b\n");
    }
    let f2 = fx.commit("f2");

    fx.git(&["switch", "-q", "main"]);
    if conflict {
        fx.write("f.txt", "one\ntrunk\nthree\n");
    } else {
        fx.write("t1.txt", "t1\n");
    }
    let t1 = fx.commit("T1");

    fx.git(&["switch", "-q", "feature"]);
    let merged = fx.try_git(&["merge", "-q", "--no-commit", "main"]);
    assert_eq!(
        merged.status.success(),
        !conflict,
        "the merge conflicts exactly when the fixture says so"
    );
    if conflict {
        fx.write("f.txt", "one\nresolved\nthree\n");
    }
    if extra {
        fx.write("extra.txt", "ours\n");
    }
    let m = fx.commit("M: merge main");
    assert_eq!(parents_of(fx, &m), vec![f2.clone(), t1.clone()]);
    fx.write("c.txt", "c\n");
    let f3 = fx.commit("f3");

    fx.git(&["switch", "-q", "main"]);
    fx.write("t2.txt", "t2\n");
    if extra {
        fx.write("extra.txt", "theirs\n");
    }
    let t2 = fx.commit("T2");
    fx.git(&["switch", "-q", "feature"]);

    Shape {
        f1,
        f2,
        t1,
        m,
        f3,
        t2,
    }
}

fn olds(rewrites: &[ff_core::rewrite::Rewrite]) -> Vec<&str> {
    rewrites.iter().map(|r| r.old.as_str()).collect()
}

fn new_of(rewrites: &[ff_core::rewrite::Rewrite], old: &str) -> String {
    rewrites
        .iter()
        .find(|r| r.old == old)
        .unwrap_or_else(|| panic!("{old} was rewritten"))
        .new
        .clone()
}

fn dropped_as(dropped: &[ff_core::rewrite::Dropped]) -> Vec<(&str, DropReason)> {
    dropped.iter().map(|d| (d.old.as_str(), d.reason)).collect()
}

#[test]
fn a_trunk_merge_flattens_into_a_straight_line() {
    let fx = Fixture::new();
    ident(&fx);
    let s = trunk_merge(&fx, false, false);
    let repo = fx.repo();

    let rewritten = plan(
        &repo,
        oid(&s.f1),
        oid(&s.f3),
        &Change::Onto(oid(&s.t2)),
        NOW,
    )
    .expect("the merge is carried");

    assert_eq!(olds(&rewritten.rewrites), vec![&s.f1, &s.f2, &s.f3]);
    assert_eq!(
        dropped_as(&rewritten.dropped),
        vec![(s.m.as_str(), DropReason::Empty)],
        "the merge said nothing once T1 lay beneath f2'"
    );
    assert!(rewritten.flattened.is_empty(), "{:?}", rewritten.flattened);
    let f3n = rewritten.new_tip.to_string();
    let f2n = new_of(&rewritten.rewrites, &s.f2);
    let f1n = new_of(&rewritten.rewrites, &s.f1);
    assert_eq!(f3n, new_of(&rewritten.rewrites, &s.f3));
    assert_eq!(parents_of(&fx, &f3n), vec![f2n.clone()]);
    assert_eq!(parents_of(&fx, &f2n), vec![f1n.clone()]);
    assert_eq!(parents_of(&fx, &f1n), vec![s.t2.clone()]);
    assert_eq!(
        files_of(&fx, &f3n),
        [
            "a.txt", "b.txt", "c.txt", "f.txt", "main.txt", "t1.txt", "t2.txt"
        ]
    );
}

#[test]
fn a_merge_that_resolved_a_conflict_holds_beneath_and_lands_the_resolution() {
    let fx = Fixture::new();
    ident(&fx);
    let s = trunk_merge(&fx, true, false);
    let repo = fx.repo();
    let change = Change::Onto(oid(&s.t2));

    // The hold is at f2, where the branch's edit meets trunk's — and the
    // stack is three deep as far as the chain got: M's own change lands on
    // f2's marked region, so the chain stops before it.
    let held = conflict(&repo, oid(&s.f1), oid(&s.f3), &change)
        .expect("the question is answered")
        .expect("the carry holds");
    assert_eq!(
        held.at,
        At::Commit {
            id: s.f2.clone(),
            subject: "f2".into(),
        }
    );
    assert_eq!(held.paths, vec!["f.txt"]);
    assert_eq!(held.of, 3);
    let first = chain(&repo, oid(&s.f1), oid(&s.f3), &change, &[]).expect("the chain runs");
    assert_eq!(
        first.tangled.as_ref().map(|t| t.old.as_str()),
        Some(s.m.as_str()),
        "the merge's resolution meets f2's marker"
    );

    // Resolve f2's region the way the merge did; the merge then agrees with
    // what stands beneath it and says nothing.
    let region = regions(&repo, &first)
        .expect("regions resolve")
        .into_iter()
        .next()
        .expect("one region");
    assert_eq!((region.step, region.path.as_str()), (1, "f.txt"));
    let resolution = Resolution {
        step: 1,
        path: "f.txt".into(),
        block: region.block,
        with: "resolved\n".into(),
    };
    let resolved =
        chain(&repo, oid(&s.f1), oid(&s.f3), &change, &[resolution]).expect("the chain runs");
    assert!(resolved.tangled.is_none(), "{:?}", resolved.tangled);
    assert_eq!(resolved.steps.len(), 4, "f1, f2, M, f3");
    assert!(
        regions(&repo, &resolved).expect("regions").is_empty(),
        "nothing is left marked"
    );

    let trees: HashMap<gix::ObjectId, gix::ObjectId> = resolved
        .steps
        .iter()
        .map(|step| (oid(&step.old), step.tree))
        .collect();
    let landed = plan_with(&repo, oid(&s.f1), oid(&s.f3), &change, NOW, &trees).expect("lands");
    assert_eq!(olds(&landed.rewrites), vec![&s.f1, &s.f2, &s.f3]);
    assert_eq!(
        dropped_as(&landed.dropped),
        vec![(s.m.as_str(), DropReason::Empty)]
    );
    let f2n = new_of(&landed.rewrites, &s.f2);
    assert_eq!(blob(&fx, &f2n, "f.txt"), "one\nresolved\nthree\n");
    assert_eq!(parents_of(&fx, &landed.new_tip.to_string()), vec![f2n]);
}

#[test]
fn a_fix_beneath_a_merge_carries_it_with_both_parents() {
    let fx = Fixture::new();
    ident(&fx);
    let s = trunk_merge(&fx, false, false);
    // f1's tree with a.txt fixed, built on a detached side commit so the
    // range itself is untouched.
    fx.git(&["switch", "-q", "--detach", &s.f1]);
    fx.write("a.txt", "a, fixed\n");
    let fixed = fx.commit("f1, fixed");
    fx.git(&["switch", "-q", "feature"]);
    let repo = fx.repo();

    let rewritten = plan(
        &repo,
        oid(&s.f1),
        oid(&s.f3),
        &Change::Tree {
            tree: oid(&tree_of(&fx, &fixed)),
            message: None,
        },
        NOW,
    )
    .expect("the merge is carried");

    assert_eq!(olds(&rewritten.rewrites), vec![&s.f1, &s.f2, &s.m, &s.f3]);
    assert!(rewritten.dropped.is_empty(), "{:?}", rewritten.dropped);
    assert!(rewritten.flattened.is_empty(), "{:?}", rewritten.flattened);
    let f2n = new_of(&rewritten.rewrites, &s.f2);
    let mn = new_of(&rewritten.rewrites, &s.m);
    assert_eq!(
        parents_of(&fx, &mn),
        vec![f2n.clone(), s.t1.clone()],
        "the branch side is mapped, the trunk side stays"
    );
    assert_eq!(
        tree_of(&fx, &mn),
        fx.git(&["merge-tree", "--write-tree", &f2n, &s.t1]).trim(),
        "the merge's tree is the auto-merge of its new parents"
    );
    assert_eq!(blob(&fx, &mn, "a.txt"), "a, fixed\n");
    assert_eq!(parents_of(&fx, &rewritten.new_tip.to_string()), vec![mn]);
}

#[test]
fn a_merge_of_a_side_branch_stays_a_merge_with_both_parents_mapped() {
    let fx = Fixture::new();
    ident(&fx);
    fx.write("main.txt", "main\n");
    fx.commit("T0");
    fx.git(&["switch", "-q", "-c", "feature"]);
    fx.write("a.txt", "a\n");
    let f1 = fx.commit("f1");
    fx.git(&["switch", "-q", "-c", "side"]);
    fx.write("s.txt", "s\n");
    let s1 = fx.commit("s1");
    fx.git(&["switch", "-q", "feature"]);
    fx.write("b.txt", "b\n");
    let f2 = fx.commit("f2");
    fx.git(&["merge", "-q", "--no-commit", "side"]);
    let m = fx.commit("M: merge side");
    assert_eq!(parents_of(&fx, &m), vec![f2.clone(), s1.clone()]);
    fx.write("c.txt", "c\n");
    let f3 = fx.commit("f3");
    fx.git(&["switch", "-q", "main"]);
    fx.write("t2.txt", "t2\n");
    let t2 = fx.commit("T2");
    fx.git(&["switch", "-q", "feature"]);
    let repo = fx.repo();

    let rewritten = plan(&repo, oid(&f1), oid(&f3), &Change::Onto(oid(&t2)), NOW)
        .expect("the merge is carried");

    let mut rewritten_olds = olds(&rewritten.rewrites);
    rewritten_olds.sort();
    let mut expected = vec![
        f1.as_str(),
        s1.as_str(),
        f2.as_str(),
        m.as_str(),
        f3.as_str(),
    ];
    expected.sort();
    assert_eq!(rewritten_olds, expected);
    assert!(rewritten.dropped.is_empty(), "{:?}", rewritten.dropped);
    assert!(rewritten.flattened.is_empty(), "{:?}", rewritten.flattened);
    let mn = new_of(&rewritten.rewrites, &m);
    assert_eq!(
        parents_of(&fx, &mn),
        vec![
            new_of(&rewritten.rewrites, &f2),
            new_of(&rewritten.rewrites, &s1)
        ]
    );
    assert_eq!(
        files_of(&fx, &mn),
        ["a.txt", "b.txt", "main.txt", "s.txt", "t2.txt"]
    );
}

#[test]
fn a_conflicting_carry_holds_at_the_merge_and_resolves() {
    let fx = Fixture::new();
    ident(&fx);
    let s = trunk_merge(&fx, false, true);
    let repo = fx.repo();
    let change = Change::Onto(oid(&s.t2));
    let short = &s.m[..7];

    // The merge's own edit of extra.txt meets T2's: the rewrite refuses at
    // the merge, and writes nothing.
    let before = loose_objects(&fx);
    let err = plan(&repo, oid(&s.f1), oid(&s.f3), &change, NOW).expect_err("the carry conflicts");
    assert_eq!(err.id(), "held/rewrite-conflict", "{err}");
    let text = err.to_string();
    assert!(text.contains(short), "{text}");
    assert!(text.contains("M: merge main"), "{text}");
    assert!(text.contains("extra.txt"), "{text}");
    assert_eq!(loose_objects(&fx), before, "a refusal writes nothing");

    let held = conflict(&repo, oid(&s.f1), oid(&s.f3), &change)
        .expect("answered")
        .expect("holds");
    assert_eq!(
        held.at,
        At::Commit {
            id: s.m.clone(),
            subject: "M: merge main".into(),
        }
    );
    assert_eq!(held.paths, vec!["extra.txt"]);
    assert_eq!(held.of, 4);

    // Resolve the merge's region: it lands as an ordinary commit holding the
    // resolution, and the plan says so.
    let first = chain(&repo, oid(&s.f1), oid(&s.f3), &change, &[]).expect("the chain runs");
    assert!(first.tangled.is_none(), "{:?}", first.tangled);
    let region = regions(&repo, &first)
        .expect("regions resolve")
        .into_iter()
        .next()
        .expect("one region");
    assert_eq!((region.step, region.path.as_str()), (2, "extra.txt"));
    let resolution = Resolution {
        step: 2,
        path: "extra.txt".into(),
        block: region.block,
        with: "both\n".into(),
    };
    let resolved =
        chain(&repo, oid(&s.f1), oid(&s.f3), &change, &[resolution]).expect("the chain runs");
    assert!(resolved.tangled.is_none());
    let trees: HashMap<gix::ObjectId, gix::ObjectId> = resolved
        .steps
        .iter()
        .map(|step| (oid(&step.old), step.tree))
        .collect();
    let landed = plan_with(&repo, oid(&s.f1), oid(&s.f3), &change, NOW, &trees).expect("lands");
    assert_eq!(olds(&landed.rewrites), vec![&s.f1, &s.f2, &s.m, &s.f3]);
    assert!(landed.dropped.is_empty(), "{:?}", landed.dropped);
    let mn = new_of(&landed.rewrites, &s.m);
    assert_eq!(
        parents_of(&fx, &mn),
        vec![new_of(&landed.rewrites, &s.f2)],
        "T1 lies beneath f2', so the merge is an ordinary commit"
    );
    assert_eq!(blob(&fx, &mn, "extra.txt"), "both\n");
    assert_eq!(
        landed
            .flattened
            .iter()
            .map(|f| (f.old.as_str(), f.new.as_str(), f.subject.as_str()))
            .collect::<Vec<_>>(),
        vec![(s.m.as_str(), mn.as_str(), "M: merge main")]
    );
}

/// The trunk-merge shape with T1 built by `make_t1`, then trunk rewritten
/// beneath the branch by `rewrite_t1`, so the merge's trunk side is in
/// abandoned history. Returns `(f1, m, f3, t1, t1_rewritten)`.
fn rewritten_trunk(
    fx: &Fixture,
    make_t1: impl Fn(&Fixture) -> String,
    rewrite_t1: impl Fn(&Fixture, &str) -> String,
) -> (String, String, String, String, String) {
    fx.write("main.txt", "main\n");
    fx.commit("T0");
    fx.write("t1.txt", "t1\n");
    let t1 = make_t1(fx);
    fx.git(&["switch", "-q", "-c", "feature", "HEAD~1"]);
    fx.write("a.txt", "a\n");
    let f1 = fx.commit("f1");
    fx.write("b.txt", "b\n");
    fx.commit("f2");
    fx.git(&["merge", "-q", "--no-commit", "main"]);
    let m = fx.commit("M: merge main");
    fx.write("c.txt", "c\n");
    let f3 = fx.commit("f3");
    fx.git(&["switch", "-q", "main"]);
    let t1_rewritten = rewrite_t1(fx, &t1);
    assert_ne!(t1_rewritten, t1);
    fx.git(&["switch", "-q", "feature"]);
    (f1, m, f3, t1, t1_rewritten)
}

#[test]
fn a_rewritten_trunk_maps_the_merges_parent_by_identity() {
    let fx = Fixture::new();
    ident(&fx);
    let (f1, m, f3, t1, t1_new) = rewritten_trunk(
        &fx,
        |fx| close(fx, "T1"),
        |fx, t1| {
            fx.write("t1.txt", "t1, edited\n");
            absorb_into(fx, t1)
        },
    );
    let repo = fx.repo();

    let rewritten = plan(&repo, oid(&f1), oid(&f3), &Change::Onto(oid(&t1_new)), NOW)
        .expect("the merge is carried");

    assert_eq!(
        dropped_as(&rewritten.dropped),
        vec![(m.as_str(), DropReason::Empty)],
        "T1 maps to its rewrite, which lies beneath f2', and the merge says nothing"
    );
    assert_eq!(rewritten.rewrites.len(), 3);
    let tip = rewritten.new_tip.to_string();
    let history = ancestry(&fx, &tip);
    assert!(!history.contains(&t1), "abandoned trunk is not reachable");
    assert!(history.contains(&t1_new));
    for id in rewritten.rewrites.iter().map(|r| r.new.as_str()) {
        assert_eq!(parents_of(&fx, id).len(), 1, "{id} is on the straight line");
    }
}

#[test]
fn a_trunk_commit_without_an_identity_keeps_the_merge_real() {
    let fx = Fixture::new();
    ident(&fx);
    let (f1, m, f3, t1, t1_new) = rewritten_trunk(
        &fx,
        |fx| fx.commit("T1"),
        |fx, _t1| {
            fx.git(&["commit", "-q", "--amend", "-m", "T1, reworded by git"]);
            fx.git(&["rev-parse", "HEAD"]).trim().to_string()
        },
    );
    let repo = fx.repo();

    let rewritten = plan(&repo, oid(&f1), oid(&f3), &Change::Onto(oid(&t1_new)), NOW)
        .expect("the merge is carried");

    assert!(rewritten.dropped.is_empty(), "{:?}", rewritten.dropped);
    let mn = new_of(&rewritten.rewrites, &m);
    let parents = parents_of(&fx, &mn);
    assert_eq!(
        parents.len(),
        2,
        "no identity to map by: the merge is rebuilt for real"
    );
    assert_eq!(parents[1], t1);
    assert!(ancestry(&fx, &rewritten.new_tip.to_string()).contains(&t1));
}
