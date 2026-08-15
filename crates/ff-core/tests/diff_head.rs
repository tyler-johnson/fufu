//! Head-state differential tests: unborn, detached, slash branches, packed
//! refs, bare repos, linked worktrees, and operation-in-progress states.
//! Operation state is not in porcelain v2, so its contract is construction-
//! based: real git drives the repo into the state, fufu must read it back.

use ff_core::{HeadState, InProgress};
use ff_testsupport::Fixture;
use ff_testsupport::porcelain::assert_status_matches;

#[test]
fn unborn() {
    let fx = Fixture::new();
    let head = ff_core::head_state(&fx.repo()).unwrap();
    assert_eq!(
        head,
        HeadState::Unborn {
            r#ref: "refs/heads/main".into()
        }
    );
    assert_status_matches(&fx);
}

#[test]
fn branch_with_commit() {
    let fx = Fixture::new();
    fx.write("a.txt", "a");
    let id = fx.commit("one");
    let head = ff_core::head_state(&fx.repo()).unwrap();
    assert_eq!(
        head,
        HeadState::Branch {
            name: "main".into(),
            r#ref: "refs/heads/main".into(),
            commit: id,
        }
    );
    assert_status_matches(&fx);
}

#[test]
fn detached() {
    let fx = Fixture::new();
    fx.write("a.txt", "a");
    let first = fx.commit("one");
    fx.write("b.txt", "b");
    fx.commit("two");
    fx.git(&["checkout", "-q", &first]);
    let head = ff_core::head_state(&fx.repo()).unwrap();
    assert_eq!(head, HeadState::Detached { commit: first });
    assert_status_matches(&fx);
}

#[test]
fn slash_branch_name() {
    let fx = Fixture::new();
    fx.write("a.txt", "a");
    let id = fx.commit("one");
    fx.git(&["checkout", "-q", "-b", "feat/x/y"]);
    let head = ff_core::head_state(&fx.repo()).unwrap();
    assert_eq!(
        head,
        HeadState::Branch {
            name: "feat/x/y".into(),
            r#ref: "refs/heads/feat/x/y".into(),
            commit: id,
        }
    );
    assert_status_matches(&fx);
}

#[test]
fn packed_refs() {
    let fx = Fixture::new();
    fx.write("a.txt", "a");
    let id = fx.commit("one");
    fx.git(&["pack-refs", "--all"]);
    assert!(
        !fx.path().join(".git/refs/heads/main").exists(),
        "ref should be packed"
    );
    let head = ff_core::head_state(&fx.repo()).unwrap();
    assert_eq!(
        head,
        HeadState::Branch {
            name: "main".into(),
            r#ref: "refs/heads/main".into(),
            commit: id,
        }
    );
    assert_status_matches(&fx);
}

#[test]
fn bare_repository() {
    let fx = Fixture::new_bare();
    let repo = ff_core::discover_isolated(fx.path()).unwrap();
    // Head still readable (unborn), but status must error cleanly.
    let head = ff_core::head_state(&repo).unwrap();
    assert!(matches!(head, HeadState::Unborn { .. }));
    let err = ff_core::status(&repo).unwrap_err();
    assert!(
        err.to_string().contains("bare"),
        "bare status error should say so: {err}"
    );
}

#[test]
fn linked_worktree() {
    let fx = Fixture::new();
    fx.write("a.txt", "a");
    fx.commit("one");
    let wt = fx.root().join("wt");
    fx.git(&[
        "worktree",
        "add",
        "-q",
        wt.to_str().unwrap(),
        "-b",
        "wt-branch",
    ]);
    let repo = ff_core::discover_isolated(&wt).unwrap();
    let head = ff_core::head_state(&repo).unwrap();
    assert!(
        matches!(head, HeadState::Branch { ref name, .. } if name == "wt-branch"),
        "worktree head: {head:?}"
    );
    ff_testsupport::porcelain::assert_status_matches_at(&fx, &wt);
}

fn diverging_file_branches(fx: &Fixture) -> (String, String) {
    fx.write("conflict.txt", "base\n");
    fx.commit("base");
    fx.git(&["checkout", "-q", "-b", "other"]);
    fx.write("conflict.txt", "theirs\n");
    let theirs = fx.commit("theirs");
    fx.git(&["checkout", "-q", "main"]);
    fx.write("conflict.txt", "ours\n");
    let ours = fx.commit("ours");
    (ours, theirs)
}

#[test]
fn operation_merge() {
    let fx = Fixture::new();
    diverging_file_branches(&fx);
    let out = fx.try_git(&["merge", "other"]);
    assert!(!out.status.success(), "merge should conflict");
    assert_eq!(ff_core::operation(&fx.repo()), Some(InProgress::Merge));
    assert_status_matches(&fx);
}

#[test]
fn operation_rebase_apply_backend() {
    let fx = Fixture::new();
    diverging_file_branches(&fx);
    let out = fx.try_git(&["-c", "rebase.backend=apply", "rebase", "other"]);
    assert!(!out.status.success(), "rebase should conflict");
    assert_eq!(ff_core::operation(&fx.repo()), Some(InProgress::Rebase));
    assert_status_matches(&fx);
}

#[test]
fn operation_rebase_merge_backend() {
    let fx = Fixture::new();
    diverging_file_branches(&fx);
    let out = fx.try_git(&["rebase", "other"]);
    assert!(!out.status.success(), "rebase should conflict");
    // git's default merge backend writes rebase-merge/interactive even for a
    // plain rebase, so the on-disk state legitimately reads as interactive.
    assert_eq!(
        ff_core::operation(&fx.repo()),
        Some(InProgress::RebaseInteractive)
    );
    assert_status_matches(&fx);
}

#[test]
fn operation_rebase_interactive() {
    let fx = Fixture::new();
    diverging_file_branches(&fx);
    let out = fx.try_git(&["-c", "sequence.editor=true", "rebase", "-i", "other"]);
    assert!(!out.status.success(), "interactive rebase should conflict");
    assert_eq!(
        ff_core::operation(&fx.repo()),
        Some(InProgress::RebaseInteractive)
    );
    assert_status_matches(&fx);
}

#[test]
fn operation_cherry_pick() {
    let fx = Fixture::new();
    let (_, theirs) = diverging_file_branches(&fx);
    let out = fx.try_git(&["cherry-pick", &theirs]);
    assert!(!out.status.success(), "cherry-pick should conflict");
    assert_eq!(ff_core::operation(&fx.repo()), Some(InProgress::CherryPick));
    assert_status_matches(&fx);
}

#[test]
fn operation_revert() {
    let fx = Fixture::new();
    fx.write("conflict.txt", "one\n");
    fx.commit("one");
    fx.write("conflict.txt", "two\n");
    let second = fx.commit("two");
    fx.write("conflict.txt", "three\n");
    fx.commit("three");
    let out = fx.try_git(&["revert", "--no-edit", &second]);
    assert!(!out.status.success(), "revert should conflict");
    assert_eq!(ff_core::operation(&fx.repo()), Some(InProgress::Revert));
    assert_status_matches(&fx);
}

#[test]
fn operation_bisect() {
    let fx = Fixture::new();
    let mut first = String::new();
    for i in 0..4 {
        fx.write("f.txt", &format!("{i}\n"));
        let id = fx.commit(&format!("c{i}"));
        if i == 0 {
            first = id;
        }
    }
    fx.git(&["bisect", "start"]);
    fx.git(&["bisect", "bad", "HEAD"]);
    fx.git(&["bisect", "good", &first]);
    assert_eq!(ff_core::operation(&fx.repo()), Some(InProgress::Bisect));
    assert_status_matches(&fx);
    fx.git(&["bisect", "reset"]);
    assert_eq!(ff_core::operation(&fx.repo()), None);
}

#[test]
fn no_operation_when_clean() {
    let fx = Fixture::new();
    fx.write("a.txt", "a");
    fx.commit("one");
    assert_eq!(ff_core::operation(&fx.repo()), None);
}

/// Smoke test for the production (non-isolated) discovery path.
#[test]
fn production_discover_smoke() {
    let fx = Fixture::new();
    fx.write("a.txt", "a");
    fx.commit("one");
    let repo = ff_core::discover(fx.path()).unwrap();
    let head = ff_core::head_state(&repo).unwrap();
    assert!(matches!(head, HeadState::Branch { .. }));
}
