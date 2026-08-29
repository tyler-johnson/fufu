//! Reconcile across worktrees: a branch another worktree holds is invisible
//! to `observe_refs`, and a baseline that simply dropped it would fabricate
//! a deletion when the branch is taken and a creation when it is released.
//! The stored table instead carries held entries forward at their last-known
//! sha — "the world as this tree last knew it" — so park/resume churn in one
//! tree stays quiet in the other.

use std::path::PathBuf;

use ff_core::SwitchOptions;
use ff_testsupport::Fixture;

const NOW: i64 = 1_700_000_000;

fn prov() -> ff_core::Provenance {
    ff_core::Provenance::new("manual", None)
}

fn switch_in(repo: &gix::Repository, target: &str) {
    ff_core::switch(
        repo,
        &SwitchOptions {
            target: target.into(),
            now: Some(NOW),
            argv: vec!["ff".into(), "switch".into(), target.into()],
        },
        &prov(),
    )
    .unwrap();
}

/// A repository with one commit, branches `side` and `topic`, and a `bay`
/// worktree standing on `side`.
fn bay_on_side(fx: &Fixture) -> PathBuf {
    fx.set_config("user.name", "Reconcile User");
    fx.set_config("user.email", "reconcile@test");
    fx.write("a.txt", "a\n");
    fx.commit("init");
    fx.git(&["branch", "side"]);
    fx.git(&["branch", "topic"]);

    let repo = fx.repo();
    let bay = fx.root().join("bay");
    ff_core::add_worktree(&repo, &bay, Some("side"), &prov(), None, Vec::new()).unwrap();
    assert!(bay.is_dir(), "the add checked the bay out");
    bay
}

/// The main tree's newest stored ref table.
fn tip_table(repo: &gix::Repository) -> ff_core::ops::RefsTable {
    let log = ff_core::ops::OpLog::open(repo).unwrap();
    let op = log.get(log.tip().unwrap().unwrap()).unwrap();
    op.refs()
        .expect("read the table")
        .expect("a verb op carries one")
        .clone()
}

/// Another worktree taking a branch is not a deletion, and releasing it
/// unchanged is not a creation. The release direction also exercises the
/// `commit_op` carry: the add-op's table was written while the bay held
/// `side`, so without the carry it would lack the entry and the bay's
/// departure from `side` would read as `side` springing into existence.
#[test]
fn holding_a_branch_elsewhere_is_not_a_deletion_and_release_is_not_a_creation() {
    let fx = Fixture::new();
    let bay = bay_on_side(&fx);

    // The bay leaves `side` for `topic`: `topic` becomes held (hidden from
    // the main tree's observation), `side` comes back unchanged.
    let bay_repo = ff_core::discover_isolated(&bay).expect("open the bay");
    switch_in(&bay_repo, "topic");

    let repo = fx.repo();
    let pending = ff_core::ops::verb::pending_foreign(&repo).unwrap();
    for change in &pending {
        assert!(
            change.name != "refs/heads/topic" && change.name != "refs/heads/side",
            "pending_foreign fabricated a transition: {change:?}"
        );
    }

    let report = ff_core::ops::reconcile(&repo, NOW + 1).unwrap();
    for change in &report.foreign {
        assert!(
            change.name != "refs/heads/topic" && change.name != "refs/heads/side",
            "reconcile fabricated a transition: {change:?}"
        );
    }

    // The stored table still knows `topic` at its real sha while the bay
    // holds it.
    let real = repo
        .find_reference("refs/heads/topic")
        .unwrap()
        .target()
        .try_id()
        .expect("topic points at a commit")
        .to_string();
    let table = tip_table(&repo);
    assert_eq!(
        table.refs.get("refs/heads/topic"),
        Some(&real),
        "the tip table lost refs/heads/topic while the bay holds it: {:?}",
        table.refs.keys().collect::<Vec<_>>()
    );
}

/// The flight repro: dirty work in the bay, a switch away (parking it) and
/// back (resuming it). The main tree's reconcile after each step must not
/// name the branch — the genuine `refs/fufu/parked/*` and `refs/stash`
/// churn the bay really made is allowed through.
#[test]
fn park_resume_churn_in_one_tree_stays_quiet_in_the_other() {
    let fx = Fixture::new();
    let bay = bay_on_side(&fx);
    let bay_repo = ff_core::discover_isolated(&bay).expect("open the bay");
    switch_in(&bay_repo, "topic");
    std::fs::write(bay.join("wip.txt"), "the bay's open work\n").unwrap();

    let repo = fx.repo();

    // Away: the bay parks `topic`'s work and takes `side`.
    switch_in(&bay_repo, "side");
    let report = ff_core::ops::reconcile(&repo, NOW + 1).unwrap();
    for change in &report.foreign {
        assert!(
            !change.name.starts_with("refs/heads/"),
            "reconcile fabricated a branch transition on park: {change:?}"
        );
    }

    // Back: the bay resumes `topic` and releases `side` unchanged.
    switch_in(&bay_repo, "topic");
    let report = ff_core::ops::reconcile(&repo, NOW + 2).unwrap();
    for change in &report.foreign {
        assert!(
            !change.name.starts_with("refs/heads/"),
            "reconcile fabricated a branch transition on resume: {change:?}"
        );
    }
}
