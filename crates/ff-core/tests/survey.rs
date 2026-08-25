//! The survey: every live worktree, and every chain whose worktree is gone.

use ff_testsupport::Fixture;

/// A repository with no linked worktrees is exactly one row: the main
/// worktree, current, on its branch, with no orphan chains.
#[test]
fn a_lone_repository_is_one_row() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");

    let survey = ff_core::survey(&fx.repo()).expect("survey");
    assert_eq!(survey.worktrees.len(), 1);
    let row = &survey.worktrees[0];
    assert_eq!(row.id, "main");
    assert!(row.current);
    let branch = fx
        .git(&["rev-parse", "--abbrev-ref", "HEAD"])
        .trim()
        .to_string();
    assert_eq!(row.branch.as_deref(), Some(branch.as_str()));
    assert!(survey.orphans.is_empty());
}

/// A bay shows up as a live row carrying its branch, its chain ref, and its
/// checkout — and it is not the current worktree.
#[test]
fn a_bay_shows_up_with_its_branch_and_chain() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    fx.git(&["branch", "side"]);

    let bay = fx.root().join("bay");
    ff_core::linked::add::create(&fx.repo(), &bay, "side", 0).expect("create");

    let survey = ff_core::survey(&fx.repo()).expect("survey");
    assert_eq!(survey.worktrees.len(), 2);
    let row = survey
        .worktrees
        .iter()
        .find(|w| {
            w.path
                .as_deref()
                .is_some_and(|p| ff_core::linked::path::same(p, &bay))
        })
        .expect("the bay row");
    assert_eq!(row.branch.as_deref(), Some("side"));
    assert_eq!(row.chain, "refs/fufu/wt/bay/ops");
    assert!(!row.current);
    assert!(survey.orphans.is_empty());
}

/// The current row is the worktree the survey runs from: from the bay, the
/// bay is current and main is not.
#[test]
fn the_current_worktree_is_the_one_you_are_in() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    fx.git(&["branch", "side"]);

    let bay = fx.root().join("bay");
    ff_core::linked::add::create(&fx.repo(), &bay, "side", 0).expect("create");

    let wt = ff_core::discover_isolated(&bay).expect("open the bay");
    let survey = ff_core::survey(&wt).expect("survey");
    let mine = survey
        .worktrees
        .iter()
        .find(|w| w.current)
        .expect("a current row");
    assert_eq!(mine.id, "bay");
    let main = survey
        .worktrees
        .iter()
        .find(|w| w.id == "main")
        .expect("the main row");
    assert!(!main.current);
}

/// A removed bay leaves the live rows and comes back as an orphan whose tip
/// is the operation `ff restore --at-op` takes.
#[test]
fn a_removed_bay_becomes_an_orphan_row() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    fx.git(&["branch", "side"]);

    let bay = fx.root().join("bay");
    let repo = fx.repo();
    let created = ff_core::linked::add::create(&repo, &bay, "side", 0).expect("create");

    // Leave work in the bay and capture it, so the chain has a floor before
    // the worktree goes away.
    std::fs::write(bay.join("note.txt"), "left behind\n").expect("leave work");
    let bay_repo = ff_core::discover_isolated(&bay).expect("open the bay");
    ff_core::capture(&bay_repo, &ff_core::Provenance::new("manual", None)).expect("capture");

    ff_core::linked::remove::teardown(&repo, &created.id).expect("teardown");

    let survey = ff_core::survey(&repo).expect("survey");
    assert!(
        survey.worktrees.iter().all(|w| w.id != created.id),
        "the bay is still listed as live"
    );
    let orphan = survey
        .orphans
        .iter()
        .find(|o| o.id == created.id)
        .expect("the orphan row");
    assert_eq!(orphan.chain, "refs/fufu/wt/bay/ops");
    assert!(
        orphan.tip.is_some(),
        "the tip is the address the restore takes"
    );
}

/// A worktree on a detached HEAD is a live row without a branch, and it is
/// not an orphan: a detached tree holds no branch and is still alive.
#[test]
fn a_detached_bay_is_listed_without_a_branch() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");

    let detached = fx.root().join("detached");
    let path = detached.display().to_string();
    fx.git(&["worktree", "add", "-q", "--detach", &path]);

    let survey = ff_core::survey(&fx.repo()).expect("survey");
    let row = survey
        .worktrees
        .iter()
        .find(|w| {
            w.path
                .as_deref()
                .is_some_and(|p| ff_core::linked::path::same(p, &detached))
        })
        .expect("the detached row");
    assert!(row.branch.is_none());
    assert!(
        survey.orphans.iter().all(|o| o.id != row.id),
        "a detached worktree is alive, not gone"
    );
}
