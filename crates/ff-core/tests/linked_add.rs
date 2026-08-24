//! The write half of linked worktrees: fufu writes the layout git would
//! have written, and real git accepts what fufu writes.

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

use ff_testsupport::Fixture;

/// Real git accepts the worktree fufu wrote: it lists the bay, the checkout
/// is clean, the bay stands on the branch, and its git dir is the one git
/// files under `worktrees/bay`.
#[test]
fn fufu_writes_a_worktree_real_git_accepts() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    fx.git(&["branch", "side"]);

    let bay = fx.root().join("bay");
    let repo = fx.repo();
    let created = ff_core::linked::add::create(&repo, &bay, "side", 0).expect("create");
    assert_eq!(created.id, "bay");
    assert_eq!(created.branch, "side");

    let list = fx.git(&["worktree", "list"]);
    assert!(
        list.contains(&created.path.to_string_lossy().into_owned()),
        "git worktree list does not name the bay:\n{list}"
    );
    assert!(
        fx.git_in(&bay, &["status", "--porcelain"])
            .trim()
            .is_empty(),
        "git considers the checkout dirty"
    );
    assert_eq!(
        fx.git_in(&bay, &["rev-parse", "--abbrev-ref", "HEAD"])
            .trim(),
        "side"
    );
    assert!(
        fx.git_in(&bay, &["rev-parse", "--absolute-git-dir"])
            .trim()
            .ends_with("worktrees/bay")
    );
}

/// The files a branch holds arrive in the bay with their bytes, and a file
/// the branch marks executable is executable there.
#[test]
fn the_content_arrives() {
    let fx = Fixture::new();
    fx.write("doc.txt", "the doc\n");
    fx.write("run.sh", "#!/bin/sh\necho run\n");
    #[cfg(unix)]
    std::fs::set_permissions(
        fx.path().join("run.sh"),
        std::fs::Permissions::from_mode(0o755),
    )
    .expect("set the exec bit");
    fx.commit("add files");
    fx.git(&["branch", "side"]);

    let bay = fx.root().join("content");
    ff_core::linked::add::create(&fx.repo(), &bay, "side", 0).expect("create");

    assert_eq!(
        std::fs::read(bay.join("doc.txt")).expect("read the doc"),
        b"the doc\n"
    );
    #[cfg(unix)]
    {
        let mode = std::fs::metadata(bay.join("run.sh"))
            .expect("read the script")
            .permissions()
            .mode();
        assert!(
            mode & 0o111 != 0,
            "run.sh is not executable in the bay: {mode:o}"
        );
    }
}

/// Two checkouts whose directories share a basename take distinct ids: the
/// first keeps the name, the next takes the smallest free number.
#[test]
fn an_id_collision_takes_the_next_number() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    fx.git(&["branch", "side"]);
    fx.git(&["branch", "other"]);

    let repo = fx.repo();
    let first =
        ff_core::linked::add::create(&repo, &fx.root().join("alpha").join("bay"), "side", 0)
            .expect("create the first");
    let second =
        ff_core::linked::add::create(&repo, &fx.root().join("beta").join("bay"), "other", 0)
            .expect("create the second");

    assert_eq!(first.id, "bay");
    assert_eq!(second.id, "bay1");
    assert!(repo.common_dir().join("worktrees").join("bay1").is_dir());
}

/// A checkout whose basename is `main` does not take the main worktree's
/// chain: it is filed under `main1` and reads back as `main1`.
#[test]
fn a_worktree_named_main_does_not_take_mains_chain() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    fx.git(&["branch", "side"]);

    let path = fx.root().join("main");
    let created = ff_core::linked::add::create(&fx.repo(), &path, "side", 0).expect("create");
    assert_eq!(created.id, "main1");

    let wt = ff_core::discover_isolated(&path).expect("open the new worktree");
    assert_eq!(ff_core::linked::id(&wt), "main1");
}

/// A destination that already holds something is refused, and nothing is
/// written under the main worktree's `worktrees/`.
#[test]
fn a_nonempty_destination_is_refused() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    fx.git(&["branch", "side"]);

    let dest = fx.root().join("taken");
    std::fs::create_dir_all(&dest).expect("make the destination");
    std::fs::write(dest.join("keep.txt"), "mine\n").expect("put something in it");

    let repo = fx.repo();
    let err = ff_core::linked::add::create(&repo, &dest, "side", 0).unwrap_err();
    assert_eq!(err.id(), "worktree/exists");

    let filed = std::fs::read_dir(repo.common_dir().join("worktrees"))
        .map(|entries| entries.flatten().count())
        .unwrap_or(0);
    assert_eq!(filed, 0, "an administrative entry was written");
}

/// Teardown takes both the checkout and the administrative directory, and
/// git no longer lists the bay.
#[test]
fn teardown_takes_both_halves() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    fx.git(&["branch", "side"]);

    let bay = fx.root().join("bay");
    let repo = fx.repo();
    let created = ff_core::linked::add::create(&repo, &bay, "side", 0).expect("create");

    let removed = ff_core::linked::remove::teardown(&repo, &created.id).expect("teardown");
    assert_eq!(removed.id, created.id);
    assert_eq!(removed.path.as_deref(), Some(created.path.as_path()));
    assert!(!bay.exists(), "the checkout directory survived");
    assert!(
        !repo
            .common_dir()
            .join("worktrees")
            .join(&created.id)
            .exists(),
        "the administrative directory survived"
    );
    let list = fx.git(&["worktree", "list"]);
    assert!(
        !list.contains(&created.path.to_string_lossy().into_owned()),
        "git still lists the bay:\n{list}"
    );
}

/// The main worktree is not a linked worktree: teardown refuses it by name
/// and takes nothing.
#[test]
fn the_main_worktree_is_not_removable() {
    let fx = Fixture::new();
    let err = ff_core::linked::remove::teardown(&fx.repo(), "main").unwrap_err();
    assert_eq!(err.id(), "worktree/is-main");
}

/// The `.git` file points at an absolute path. git resolves a relative
/// `gitdir:` against the worktree directory rather than the repository, so a
/// relative one yields `<dest>/./.git/worktrees/<id>` and every git command
/// run inside the new worktree fails. `repo.common_dir()` is relative
/// whenever the repository was discovered by a relative path, which is what
/// a shell sitting in the repository does — and what this fixture, which
/// discovers by an absolute path, would otherwise never catch.
#[test]
fn the_gitdir_pointer_is_absolute() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    fx.git(&["branch", "side"]);

    let bay = fx.root().join("bay");
    let created = ff_core::linked::add::create(&fx.repo(), &bay, "side", 0).expect("create");

    let pointer = std::fs::read_to_string(created.path.join(".git")).expect("read .git");
    let target = pointer
        .trim()
        .strip_prefix("gitdir:")
        .expect("a gitdir pointer")
        .trim();
    assert!(
        std::path::Path::new(target).is_absolute(),
        "the gitdir pointer is relative: {target}"
    );
    assert!(
        !target.contains("/./"),
        "the gitdir pointer is unresolved: {target}"
    );
}
