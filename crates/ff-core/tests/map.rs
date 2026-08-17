//! The map: the skeleton rule (one branch is a tip and a frontier), runs of
//! one shown while longer runs contract, merges and forks staying visible,
//! the cap and the forced branches, the depth cap, and the unborn and
//! detached heads.

use ff_core::map::{MapNode, MapOptions};
use ff_testsupport::Fixture;

fn map(fx: &Fixture, opts: &MapOptions) -> ff_core::Map {
    ff_core::map::map(&fx.repo(), opts).unwrap()
}

fn kinds(rows: &[ff_core::MapRow]) -> Vec<&'static str> {
    rows.iter()
        .map(|r| match &r.node {
            MapNode::Open { .. } => "open",
            MapNode::Commit { .. } => "commit",
            MapNode::Elided { .. } => "elided",
        })
        .collect()
}

fn subject_at(rows: &[ff_core::MapRow], row: usize) -> Option<&str> {
    match &rows[row].node {
        MapNode::Commit { subject, .. } => Some(subject),
        _ => None,
    }
}

fn row_with_subject(rows: &[ff_core::MapRow], subject: &str) -> Option<usize> {
    rows.iter().position(|r| match &r.node {
        MapNode::Commit { subject: s, .. } => s == subject,
        _ => false,
    })
}

/// The branch names on one row's refs.
fn ref_names_at(rows: &[ff_core::MapRow], row: usize) -> Vec<String> {
    match &rows[row].node {
        MapNode::Commit { refs, .. } => refs.iter().map(|r| r.name.clone()).collect(),
        _ => Vec::new(),
    }
}

/// Every branch name that appears on any commit row.
fn all_ref_names(rows: &[ff_core::MapRow]) -> Vec<String> {
    rows.iter()
        .enumerate()
        .flat_map(|(i, _)| ref_names_at(rows, i))
        .collect()
}

/// Elision counts across the rows; `None` is a frontier marker.
fn elisions(rows: &[ff_core::MapRow]) -> Vec<Option<usize>> {
    rows.iter()
        .filter_map(|r| match &r.node {
            MapNode::Elided { count } => Some(*count),
            _ => None,
        })
        .collect()
}

/// Whether `from` reaches `to` through parent links.
fn reaches(rows: &[ff_core::MapRow], from: usize, to: usize) -> bool {
    let mut seen = std::collections::HashSet::new();
    let mut stack = vec![from];
    while let Some(n) = stack.pop() {
        if n == to {
            return true;
        }
        if !seen.insert(n) {
            continue;
        }
        stack.extend(rows[n].parents.iter().copied());
    }
    false
}

fn open_branch(rows: &[ff_core::MapRow]) -> &str {
    match &rows[0].node {
        MapNode::Open { branch, .. } => branch,
        _ => panic!("row 0 is not the open change"),
    }
}

#[test]
fn one_branch_is_tip_and_frontier() {
    let fx = Fixture::new();
    for i in 1..=5 {
        fx.commit(&format!("c{i}"));
    }
    let m = map(&fx, &MapOptions::default());
    assert!(!m.truncated);
    assert_eq!(kinds(&m.rows), vec!["open", "commit", "elided"]);
    assert_eq!(m.rows[0].parents, vec![1]);
    assert_eq!(m.rows[1].parents, vec![2]);
    assert_eq!(m.rows[2].parents, Vec::<usize>::new());
    assert_eq!(subject_at(&m.rows, 1), Some("c5"));
    assert_eq!(elisions(&m.rows), vec![None]);
}

#[test]
fn two_branches_off_one_fork() {
    let fx = Fixture::new();
    fx.commit("f0");
    fx.commit("f");
    fx.git(&["branch", "feature"]);
    fx.commit("m1");
    fx.commit("m2");
    fx.git(&["switch", "feature"]);
    fx.commit("x1");
    fx.commit("x2");
    fx.git(&["switch", "main"]);
    let m = map(&fx, &MapOptions::default());
    let rows = &m.rows;

    let feature_rows = rows
        .iter()
        .enumerate()
        .filter_map(|(i, r)| match &r.node {
            MapNode::Commit { refs, .. } if refs.iter().any(|r| r.name == "feature") => Some(i),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(feature_rows.len(), 1);
    assert!(!all_ref_names(rows).iter().any(|n| n == "main"));

    let fork = row_with_subject(rows, "f").unwrap();
    let m2 = row_with_subject(rows, "m2").unwrap();
    let x2 = row_with_subject(rows, "x2").unwrap();
    assert!(reaches(rows, m2, fork));
    assert!(reaches(rows, x2, fork));
    assert!(!elisions(rows).iter().any(|c| c.is_some()));
}

#[test]
fn a_run_of_one_is_shown_not_elided() {
    let fx = Fixture::new();
    fx.commit("f");
    fx.git(&["branch", "feature"]);
    fx.commit("m1");
    fx.commit("m2");
    fx.git(&["switch", "feature"]);
    fx.commit("x1");
    fx.git(&["switch", "main"]);
    let m = map(&fx, &MapOptions::default());
    assert!(
        !m.rows
            .iter()
            .any(|r| matches!(&r.node, MapNode::Elided { count: Some(1) }))
    );
    assert!(row_with_subject(&m.rows, "m1").is_some());
}

#[test]
fn a_run_of_five_elides_with_its_count() {
    let fx = Fixture::new();
    fx.commit("f");
    fx.git(&["branch", "feature"]);
    for i in 1..=6 {
        fx.commit(&format!("m{i}"));
    }
    fx.git(&["switch", "feature"]);
    fx.commit("x1");
    fx.git(&["switch", "main"]);
    let m = map(&fx, &MapOptions::default());
    assert_eq!(
        elisions(&m.rows).iter().filter(|c| **c == Some(5)).count(),
        1
    );
    assert!(row_with_subject(&m.rows, "m3").is_none());
}

#[test]
fn a_merge_commit_is_visible() {
    let fx = Fixture::new();
    fx.commit("f");
    fx.git(&["branch", "feature"]);
    fx.commit("m1");
    fx.git(&["switch", "feature"]);
    fx.commit("x1");
    fx.git(&["switch", "main"]);
    fx.git(&["merge", "--no-ff", "-m", "merged", "feature"]);
    let m = map(&fx, &MapOptions::default());
    let merged = row_with_subject(&m.rows, "merged").unwrap();
    let parents = &m.rows[merged].parents;
    assert_eq!(parents.len(), 2);
    assert_ne!(parents[0], parents[1]);
}

/// A base commit, then four branches each taking one commit off it, ending
/// back on `main` with `b4` the newest tip and `main` the oldest.
fn four_branches_off_one_base() -> Fixture {
    let fx = Fixture::new();
    fx.commit("base");
    for b in ["b1", "b2", "b3", "b4"] {
        fx.git(&["switch", "-c", b, "main"]);
        fx.commit(&format!("{b} tip"));
        fx.git(&["switch", "main"]);
    }
    fx
}

#[test]
fn n_caps_the_branches_by_tip_time() {
    let fx = four_branches_off_one_base();
    let capped = map(&fx, &MapOptions { branches: Some(2) });
    let names = all_ref_names(&capped.rows);
    assert!(names.iter().any(|n| n == "b4"));
    assert!(names.iter().any(|n| n == "b3"));
    assert!(!names.iter().any(|n| n == "b1"));

    let full = map(&fx, &MapOptions { branches: None });
    let names = all_ref_names(&full.rows);
    for b in ["b1", "b2", "b3", "b4"] {
        assert!(names.iter().any(|n| n == b), "{b} missing");
    }
}

#[test]
fn trunk_and_current_are_forced_in() {
    let fx = four_branches_off_one_base();
    fx.git(&["switch", "b4"]);
    let m = map(&fx, &MapOptions { branches: Some(1) });
    assert!(all_ref_names(&m.rows).iter().any(|n| n == "main"));
    assert_eq!(open_branch(&m.rows), "b4");
}

#[test]
fn the_depth_cap_sets_truncated() {
    let fx = Fixture::new();
    fx.commit("base");
    fx.git(&["switch", "-c", "side"]);
    for i in 1..=12 {
        fx.commit(&format!("s{i}"));
    }
    fx.git(&["switch", "main"]);
    for i in 1..=12 {
        fx.commit(&format!("m{i}"));
    }
    // The same fixture, cap unset, converges.
    assert!(!map(&fx, &MapOptions::default()).truncated);

    fx.set_config("fufu.mapDepth", "3");
    let capped = map(&fx, &MapOptions::default());
    assert!(capped.truncated);
    assert!(matches!(
        capped.rows.last().unwrap().node,
        MapNode::Elided { count: None }
    ));
}

#[test]
fn unborn_head_yields_only_the_open_row() {
    let fx = Fixture::new();
    let m = map(&fx, &MapOptions::default());
    assert_eq!(m.rows.len(), 1);
    match &m.rows[0].node {
        MapNode::Open { born, .. } => assert!(!born),
        _ => panic!("row 0 is not the open change"),
    }
    assert_eq!(m.rows[0].parents, Vec::<usize>::new());
}

#[test]
fn detached_head_still_has_a_parent() {
    let fx = Fixture::new();
    fx.commit("c1");
    fx.commit("c2");
    fx.commit("c3");
    fx.git(&["checkout", "--detach", "HEAD~1"]);
    let m = map(&fx, &MapOptions::default());
    assert_eq!(open_branch(&m.rows), "@detached");
    assert_eq!(m.rows[0].parents.len(), 1);
    assert!(!all_ref_names(&m.rows).iter().any(|n| n == "@detached"));
}
