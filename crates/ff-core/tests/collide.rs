//! Collide: the sideways axis — would two branches, neither beneath the
//! other, collide with each other — and the invariant that the probe
//! writes nothing to the object database.

use ff_core::Pairing;
use ff_core::futures::UnknownReason;
use ff_core::gix;
use ff_testsupport::Fixture;

fn oid(hex: &str) -> gix::ObjectId {
    gix::ObjectId::from_hex(hex.trim().as_bytes()).unwrap()
}

fn tip(fx: &Fixture, rev: &str) -> gix::ObjectId {
    oid(&fx.git(&["rev-parse", rev]))
}

/// Loose objects on disk, counted as files by a stack walk over the odb.
/// Loose objects, and strictly those: a two-hex fanout directory holding a
/// thirty-eight-hex name.
///
/// Counting every file under `.git/objects` instead is what this used to do,
/// and it made the assertion measure more than it claimed. `pack/`, `info/`,
/// a `commit-graph`, a `.lock` and git's `tmp_obj_*` staging files all live
/// there, none of them is a loose object, and any of them appearing or being
/// cleaned up between the two counts moves the number for reasons the probe
/// had nothing to do with. That is not hypothetical: CI caught it once, and
/// the count had gone *down* by one -- the opposite of the leak the message
/// names, and impossible for the thing being tested to cause.
fn loose_count(fx: &Fixture) -> usize {
    let mut count = 0;
    let objects = fx.path().join(".git/objects");
    for entry in std::fs::read_dir(objects).unwrap() {
        let entry = entry.unwrap();
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        if name.len() != 2 || !name.chars().all(|c| c.is_ascii_hexdigit()) {
            continue;
        }
        for object in std::fs::read_dir(entry.path()).unwrap() {
            let object = object.unwrap();
            let name = object.file_name();
            let Some(name) = name.to_str() else { continue };
            if name.len() == 38 && name.chars().all(|c| c.is_ascii_hexdigit()) {
                count += 1;
            }
        }
    }
    count
}

/// Three branches off one base: `feat-a` and `feat-b` both edit the same
/// line of `shared.txt` differently; `fix-c` adds only its own new file.
fn three_branches() -> Fixture {
    let fx = Fixture::new();
    fx.write("shared.txt", "alpha\nbeta\ngamma\n");
    fx.commit("base");
    fx.git(&["switch", "-c", "feat-a"]);
    fx.write("shared.txt", "alpha\nBETA-A\ngamma\n");
    fx.commit("feat-a edits the middle line");
    fx.git(&["switch", "main"]);
    fx.git(&["switch", "-c", "feat-b"]);
    fx.write("shared.txt", "alpha\nBETA-B\ngamma\n");
    fx.commit("feat-b edits the middle line");
    fx.git(&["switch", "main"]);
    fx.git(&["switch", "-c", "fix-c"]);
    fx.write("c.txt", "c\n");
    fx.commit("fix-c adds its own file");
    fx
}

#[test]
fn two_branches_touching_the_same_line_collide() {
    let fx = three_branches();
    let repo = fx.repo();
    let out = ff_core::collide(&repo, "feat-a", "feat-b").unwrap();
    assert_eq!(out.a.name, "feat-a");
    assert_eq!(out.a.tip, tip(&fx, "feat-a").to_string());
    assert_eq!(out.b.name, "feat-b");
    match &out.pairing {
        Pairing::Collide { paths } => {
            assert!(paths.contains(&"shared.txt".to_string()), "{paths:?}")
        }
        other => panic!("the same line edited two ways must collide: {other:?}"),
    }
}

#[test]
fn two_branches_touching_different_files_are_clear() {
    let fx = three_branches();
    let repo = fx.repo();
    let out = ff_core::collide(&repo, "feat-a", "fix-c").unwrap();
    assert_eq!(out.pairing, Pairing::Clear);
}

/// The merge does not know which side is "ours", so neither does the verb:
/// the same two names in either order answer the same way.
#[test]
fn the_verdict_does_not_depend_on_which_side_is_named_first() {
    let fx = three_branches();
    let repo = fx.repo();
    let forward = ff_core::collide(&repo, "feat-a", "feat-b").unwrap();
    let backward = ff_core::collide(&repo, "feat-b", "feat-a").unwrap();
    assert_eq!(forward.pairing, backward.pairing);
    assert_eq!(forward.a.name, backward.b.name);
    assert_eq!(forward.b.name, backward.a.name);
}

#[test]
fn unrelated_histories_are_an_honest_unknown() {
    let fx = three_branches();
    fx.git(&["checkout", "--orphan", "other"]);
    fx.write("o.txt", "other\n");
    fx.commit("a second root");
    let repo = fx.repo();
    let out = ff_core::collide(&repo, "other", "main").unwrap();
    assert_eq!(
        out.pairing,
        Pairing::Unknown {
            reason: UnknownReason::UnrelatedHistories
        },
        "no base to merge against is refused, not guessed"
    );
}

#[test]
fn an_uncommitted_edit_shows_up_as_a_collision() {
    let fx = three_branches();
    fx.git(&["switch", "feat-b"]);
    // Not committed: the open change still holds the line, and it differs
    // from both the tip's text and feat-a's.
    fx.write("shared.txt", "alpha\nBETA-B2\ngamma\n");
    let repo = fx.repo();
    // capture writes real objects, so any loose_count baseline here would
    // measure the capture, not the probe. None is taken.
    ff_core::capture(&repo, &ff_core::Provenance::new("manual", None)).unwrap();
    let out = ff_core::collide(&repo, "feat-a", "feat-b").unwrap();
    match &out.pairing {
        Pairing::Collide { paths } => {
            assert!(paths.contains(&"shared.txt".to_string()), "{paths:?}")
        }
        other => panic!("the uncommitted edit must still collide: {other:?}"),
    }
    assert!(
        out.b.open,
        "the answer includes work that has not been committed"
    );
    assert!(!out.a.open, "feat-a has nothing open");
}

#[test]
fn collide_writes_nothing_to_the_object_database() {
    let fx = three_branches();
    let repo = fx.repo();
    let before = loose_count(&fx);
    // The colliding pair, deliberately: a conflicted merge is the case most
    // likely to leak a blob.
    let out = ff_core::collide(&repo, "feat-a", "feat-b").unwrap();
    assert!(
        matches!(out.pairing, Pairing::Collide { .. }),
        "{:?}",
        out.pairing
    );
    assert_eq!(
        loose_count(&fx),
        before,
        "a probe must write nothing to the object database"
    );
}

#[test]
fn an_unknown_branch_name_is_refused() {
    let fx = three_branches();
    let repo = fx.repo();
    let err = ff_core::collide(&repo, "nope", "feat-a").unwrap_err();
    assert_eq!(err.id(), "branch/not-found", "{err}");
    let err = ff_core::collide(&repo, "feat-a", "nope").unwrap_err();
    assert_eq!(err.id(), "branch/not-found", "{err}");
}

#[test]
fn one_branch_cannot_collide_with_itself() {
    let fx = three_branches();
    let repo = fx.repo();
    let err = ff_core::collide(&repo, "feat-a", "feat-a").unwrap_err();
    assert_eq!(err.id(), "usage/collide-same-branch", "{err}");
}
