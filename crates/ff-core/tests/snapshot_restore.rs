//! Restore contract: worktree-only writes, a mandatory pre-restore capture,
//! the target grammar, round-trips, and refusals.

use ff_core::{CaptureOutcome, Provenance, RestoreOptions, TakeOptions, TrimOptions};
use ff_testsupport::Fixture;

fn take_created(fx: &Fixture) -> String {
    let repo = fx.repo();
    match ff_core::capture(&repo, &Provenance::new("manual", None)).expect("take") {
        CaptureOutcome::Created { id, .. } => id.hex(),
        other => panic!("expected Created, got {other:?}"),
    }
}

fn restore_all(fx: &Fixture, target: Option<&str>) -> ff_core::RestoreReport {
    let repo = fx.repo();
    ff_core::restore(
        &repo,
        &RestoreOptions {
            target: target.map(String::from),
            paths: Vec::new(),
            all: true,
            now: None,
        },
        &Provenance::new("pre", Some("ff restore --all".into())),
    )
    .expect("restore")
}

/// Restore --all to a target, then take: the take must be a no-op against
/// the target's tree — the worktree matches the snapshot exactly.
#[test]
fn round_trip_restore_then_take_is_target_tree() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.write("dir/b.txt", "b\n");
    fx.commit("init");
    fx.write("a.txt", "captured state\n");
    fx.write("dir/new.txt", "captured untracked\n");
    let snap = take_created(&fx);

    // Mutate: change, add, delete.
    fx.write("a.txt", "diverged\n");
    fx.write("extra.txt", "should disappear\n");
    fx.remove("dir/new.txt");

    let report = restore_all(&fx, Some(&snap[..7]));
    assert_eq!(report.target.id, snap);
    assert!(report.pre_op.is_some(), "pre-restore snapshot happened");

    // Worktree content matches the snapshot.
    let a = std::fs::read_to_string(fx.path().join("a.txt")).unwrap();
    assert_eq!(a, "captured state\n");
    let new = std::fs::read_to_string(fx.path().join("dir/new.txt")).unwrap();
    assert_eq!(new, "captured untracked\n");
    assert!(!fx.path().join("extra.txt").exists(), "extra file deleted");

    // A fresh take now equals the target's tree.
    let repo = fx.repo();
    let outcome = ff_core::capture(&repo, &Provenance::new("manual", None)).expect("take");
    let tree_of = |id: &str| {
        fx.git(&["rev-parse", &format!("{id}^{{tree}}")])
            .trim()
            .to_string()
    };
    match outcome {
        CaptureOutcome::Created { id, .. } => assert_eq!(tree_of(&id.hex()), tree_of(&snap)),
        CaptureOutcome::NoOp { tip: Some(tip), .. } => {
            assert_eq!(tree_of(&tip.hex()), tree_of(&snap))
        }
        other => panic!("unexpected outcome {other:?}"),
    }
}

#[test]
fn index_and_head_untouched() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    fx.write("a.txt", "captured\n");
    fx.write("staged.txt", "staged\n");
    fx.git(&["add", "staged.txt"]);
    let snap = take_created(&fx);
    fx.write("a.txt", "diverged\n");

    let index_before = fx.index_bytes();
    let head_before = fx.git(&["rev-parse", "HEAD"]);
    restore_all(&fx, Some(&snap));
    assert_eq!(fx.index_bytes(), index_before, "index byte-identical");
    assert_eq!(
        fx.git(&["rev-parse", "HEAD"]),
        head_before,
        "HEAD untouched"
    );
    let branch = fx.git(&["symbolic-ref", "HEAD"]);
    assert_eq!(branch.trim(), "refs/heads/main");
}

#[test]
fn ignored_files_untouched() {
    let fx = Fixture::new();
    fx.write(".gitignore", "cache/\n");
    fx.commit("init");
    fx.write("tracked.txt", "captured\n");
    let snap = take_created(&fx);
    fx.write("tracked.txt", "diverged\n");
    fx.write("cache/scratch.bin", "ignored bytes");

    restore_all(&fx, Some(&snap));
    assert!(
        fx.path().join("cache/scratch.bin").exists(),
        "ignored files are invisible to capture, so restore never deletes them"
    );
    assert_eq!(
        std::fs::read_to_string(fx.path().join("tracked.txt")).unwrap(),
        "captured\n"
    );
}

#[test]
fn emptied_directories_are_pruned() {
    let fx = Fixture::new();
    fx.write("keep.txt", "k\n");
    fx.commit("init");
    let snap = take_created_after(&fx, || fx.write("keep.txt", "captured\n"));
    fx.write("deep/nested/only.txt", "temporary\n");
    restore_all(&fx, Some(&snap));
    assert!(
        !fx.path().join("deep").exists(),
        "emptied parent dirs pruned bottom-up"
    );
}

fn take_created_after(fx: &Fixture, mutate: impl FnOnce()) -> String {
    mutate();
    take_created(fx)
}

#[test]
fn path_scoped_restore_leaves_the_rest() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.write("dir/b.txt", "b\n");
    fx.commit("init");
    fx.write("a.txt", "captured a\n");
    fx.write("dir/b.txt", "captured b\n");
    let snap = take_created(&fx);
    fx.write("a.txt", "diverged a\n");
    fx.write("dir/b.txt", "diverged b\n");

    let repo = fx.repo();
    let report = ff_core::restore(
        &repo,
        &RestoreOptions {
            target: Some(snap.clone()),
            paths: vec!["dir".into()],
            all: false,
            now: None,
        },
        &Provenance::new("pre", Some("ff restore dir".into())),
    )
    .expect("restore");
    assert_eq!(report.restored, vec!["dir/b.txt".to_string()]);
    assert_eq!(
        std::fs::read_to_string(fx.path().join("dir/b.txt")).unwrap(),
        "captured b\n"
    );
    assert_eq!(
        std::fs::read_to_string(fx.path().join("a.txt")).unwrap(),
        "diverged a\n",
        "unselected paths stay diverged"
    );
}

/// `d` is a duration unit and a hex digit both, so the two readings of a
/// target overlap and the order of the checks decides the winner. Git's
/// four-character prefix minimum is what separates them: at or above it a
/// hex-shaped target is an id, below it there is no id it could be.
#[test]
fn hex_shaped_targets_are_ids_not_durations() {
    let now = 1_700_000_000;
    let id = |s: &str| ff_core::RestoreTarget::Id(s.to_string());
    let age = |secs: i64| ff_core::RestoreTarget::AtTime(now - secs);

    let table: &[(&str, ff_core::RestoreTarget)] = &[
        // Four hex characters or more: an object prefix, whatever it spells.
        ("123d", id("123d")),
        ("1234d", id("1234d")),
        ("beed", id("beed")),
        ("0000", id("0000")),
        ("123D", id("123d")),
        ("abcdef1", id("abcdef1")),
        // Too short to be a prefix, so the duration reading is the only one.
        ("3d", age(3 * 86_400)),
        ("10d", age(10 * 86_400)),
        ("9s", age(9)),
        // Never hex-shaped at any length: the unit is not a hex digit.
        ("30m", age(1800)),
        ("2h", age(7200)),
        ("1w", age(7 * 86_400)),
        ("123w", age(123 * 7 * 86_400)),
        ("90s", age(90)),
    ];

    for (raw, want) in table {
        assert_eq!(
            &ff_core::parse_target(Some(raw), now).unwrap(),
            want,
            "target {raw:?}"
        );
    }
}

#[test]
fn target_grammar_resolves() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    fx.write("a.txt", "one\n");
    let snap1 = take_created(&fx);
    fx.write("a.txt", "two\n");
    let snap2 = take_created(&fx);

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;

    // Default: newest.
    use ff_core::RestoreTarget;
    assert_eq!(
        ff_core::parse_target(None, now).unwrap(),
        RestoreTarget::Newest
    );
    // Hex prefix.
    assert_eq!(
        ff_core::parse_target(Some(&snap1[..7]), now).unwrap(),
        RestoreTarget::Id(snap1[..7].to_string())
    );
    // @{n}.
    assert_eq!(
        ff_core::parse_target(Some("@{1}"), now).unwrap(),
        RestoreTarget::Back(1)
    );
    // Compact ages.
    assert_eq!(
        ff_core::parse_target(Some("30m"), now).unwrap(),
        RestoreTarget::AtTime(now - 1800)
    );
    assert_eq!(
        ff_core::parse_target(Some("2h"), now).unwrap(),
        RestoreTarget::AtTime(now - 7200)
    );

    // Letters-spelled prefix (jj-style reverse hex) decodes to the hex id.
    let letters = ff_core::snapid::encode(&snap1[..7]);
    assert_eq!(
        ff_core::parse_target(Some(&letters), now).unwrap(),
        RestoreTarget::Id(snap1[..7].to_string())
    );
    // Uppercase letters are accepted.
    assert_eq!(
        ff_core::parse_target(Some(&letters.to_ascii_uppercase()), now).unwrap(),
        RestoreTarget::Id(snap1[..7].to_string())
    );
    // `noon` is all-alphabet: the id branch shadows the date word by design.
    assert_eq!(
        ff_core::parse_target(Some("noon"), now).unwrap(),
        RestoreTarget::Id("cbbc".to_string())
    );

    // @{1} restores the previous snapshot's state.
    fx.write("a.txt", "diverged\n");
    let report = restore_all(&fx, Some("@{1}"));
    assert_eq!(report.target.id, snap1);
    assert_eq!(
        std::fs::read_to_string(fx.path().join("a.txt")).unwrap(),
        "one\n"
    );
    let _ = snap2;
}

/// A letters-spelled id drives a real restore end to end.
#[test]
fn letters_id_restores() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    fx.write("a.txt", "captured\n");
    let snap = take_created(&fx);
    fx.write("a.txt", "diverged\n");

    let letters = ff_core::snapid::encode(&snap[..8]);
    let report = restore_all(&fx, Some(&letters));
    assert_eq!(report.target.id, snap);
    assert_eq!(
        std::fs::read_to_string(fx.path().join("a.txt")).unwrap(),
        "captured\n"
    );
}

#[test]
fn refuses_non_fufu_targets() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    let head = fx.commit("init");
    fx.write("a.txt", "dirty\n");
    take_created(&fx);

    let repo = fx.repo();
    let err = ff_core::restore(
        &repo,
        &RestoreOptions {
            target: Some(head.clone()),
            paths: Vec::new(),
            all: true,
            now: None,
        },
        &Provenance::new("pre", Some("ff restore".into())),
    )
    .unwrap_err();
    // The guard is `is_op_commit`, not "does it bear the fufu identity" — a
    // record commit bears the identity too, and restoring from one would wipe
    // the working tree and write three metadata files in its place.
    assert_eq!(err.id(), "op/not-found", "{err}");
    assert!(
        err.to_string().contains("not a fufu operation")
            || err.to_string().contains("no operation matches"),
        "refusal names the problem: {err}"
    );
}

#[test]
fn contended_pre_capture_aborts_before_writing() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    fx.write("a.txt", "captured\n");
    let snap = take_created(&fx);
    fx.write("a.txt", "diverged\n");

    // Hold the chain lock: the mandatory pre-snapshot must hit Contended.
    let lock = fx.path().join(".git/refs/fufu/snap/main.lock");
    std::fs::create_dir_all(lock.parent().unwrap()).unwrap();
    std::fs::write(&lock, "held").unwrap();

    let repo = fx.repo();
    let err = ff_core::restore(
        &repo,
        &RestoreOptions {
            target: Some(snap),
            paths: Vec::new(),
            all: true,
            now: None,
        },
        &Provenance::new("pre", Some("ff restore --all".into())),
    )
    .unwrap_err();
    assert!(
        err.to_string().contains("concurrent"),
        "contended abort: {err}"
    );
    assert_eq!(
        std::fs::read_to_string(fx.path().join("a.txt")).unwrap(),
        "diverged\n",
        "nothing was written"
    );
}

#[test]
fn no_chain_means_nothing_to_restore() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    let repo = fx.repo();
    let err = ff_core::restore(
        &repo,
        &RestoreOptions {
            target: None,
            paths: Vec::new(),
            all: true,
            now: None,
        },
        &Provenance::new("pre", Some("ff restore --all".into())),
    )
    .unwrap_err();
    assert_eq!(err.id(), "op/not-found", "{err}");
    assert!(err.to_string().contains("no operations on main"), "{err}");
}

#[test]
fn untracked_files_survive_because_diff_is_target_vs_snapshot() {
    // The load-bearing subtlety: the write diff is target ↔ fresh snapshot,
    // never target ↔ worktree. An untracked file present in both states
    // simply isn't in the diff.
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    fx.write("a.txt", "captured\n");
    fx.write("untracked.txt", "present in the snapshot too\n");
    let snap = take_created(&fx);
    fx.write("a.txt", "diverged\n");

    restore_all(&fx, Some(&snap));
    assert!(
        fx.path().join("untracked.txt").exists(),
        "untracked-but-captured file untouched"
    );
}

// unix-only: exec bits and freely creatable symlinks don't exist on Windows.
#[cfg(unix)]
#[test]
fn symlink_and_exec_bit_restore() {
    use std::os::unix::fs::PermissionsExt;
    let fx = Fixture::new();
    fx.write("target.txt", "t\n");
    fx.commit("init");
    fx.write("run.sh", "#!/bin/sh\n");
    let sh = fx.path().join("run.sh");
    let mut perms = std::fs::metadata(&sh).unwrap().permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&sh, perms).unwrap();
    std::os::unix::fs::symlink("target.txt", fx.path().join("link")).unwrap();
    let snap = take_created(&fx);

    fx.remove("run.sh");
    fx.remove("link");
    restore_all(&fx, Some(&snap));

    let md = std::fs::metadata(fx.path().join("run.sh")).unwrap();
    assert_eq!(md.permissions().mode() & 0o111, 0o111, "exec bit restored");
    let link_md = std::fs::symlink_metadata(fx.path().join("link")).unwrap();
    assert!(link_md.is_symlink(), "symlink recreated as a symlink");
    assert_eq!(
        std::fs::read_link(fx.path().join("link")).unwrap(),
        std::path::PathBuf::from("target.txt")
    );
}

/// A hash collision can't be forced, so ambiguity is forced by prefix length
/// instead: keep taking snapshots until two ids share a 4+ character prefix,
/// then restore to exactly that shared prefix and check the error names both
/// full ids. Real collisions this short are rare, so the loop is capped —
/// hitting the cap (no ambiguity found) is an accepted outcome, not a
/// failure.
#[test]
fn ambiguous_prefix_errors_with_both_ids() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");

    let mut ids: Vec<String> = Vec::new();
    let mut collision: Option<(String, String, String)> = None;
    for i in 0..300 {
        fx.write("a.txt", &format!("v{i}\n"));
        let id = take_created(&fx);
        for existing in &ids {
            let shared = existing
                .bytes()
                .zip(id.bytes())
                .take_while(|(a, b)| a == b)
                .count();
            if shared >= 4 {
                collision = Some((existing.clone(), id.clone(), existing[..shared].to_string()));
                break;
            }
        }
        let found = collision.is_some();
        ids.push(id);
        if found {
            break;
        }
    }

    // Two ids sharing four hex characters is a birthday event: at this cap it
    // turns up most runs but not every run, and forcing it would cost thousands
    // of captures. When it does not, assert the other half of the same code
    // path instead — that the shortest prefix the index calls unique really
    // does resolve to exactly one id — so this test always exercises
    // index-backed resolution rather than passing vacuously.
    let Some((a, b, prefix)) = collision else {
        let newest = ids.last().expect("at least one capture").clone();
        let repo = fx.repo();
        let lens = ff_core::ops::index::prefix_lens(&repo, std::slice::from_ref(&newest))
            .expect("prefix_lens");
        // Never below git's four-character minimum: shorter than that is a
        // duration to the target grammar, deliberately (`3d` is three days).
        let len = lens[&newest].max(4);
        let report = restore_all(&fx, Some(&newest[..len]));
        assert_eq!(
            report.target.id, newest,
            "the index's own unique prefix must resolve to its own id"
        );
        return;
    };

    let repo = fx.repo();

    let err = ff_core::restore(
        &repo,
        &RestoreOptions {
            target: Some(prefix.clone()),
            paths: Vec::new(),
            all: true,
            now: None,
        },
        &Provenance::new("pre", Some("ff restore".into())),
    )
    .unwrap_err();
    let msg = err.to_string();
    assert_eq!(err.id(), "op/ambiguous", "{msg}");
    assert!(
        msg.starts_with(&format!("{prefix} matches ")),
        "unexpected message: {msg}"
    );
    // Candidates are listed in the letters alphabet, because that is the
    // spelling the user has to type back.
    for hex in [&a, &b] {
        let letters = ff_core::snapid::encode(hex);
        assert!(msg.contains(&letters[..12]), "missing {letters} in {msg}");
    }
}

/// Trim moves the pre-cutoff suffix of the chain to `refs/fufu/trash/<name>`
/// — those ids no longer live on the live chain at all. Restore's prefix
/// resolution must still find them there.
#[test]
fn trash_ids_still_resolve() {
    const NOW: i64 = 1_700_000_000;

    fn snap_at(fx: &Fixture, days_ago: i64) -> String {
        let repo = fx.repo();
        match ff_core::capture_with(
            &repo,
            &Provenance::new("manual", Some(format!("{days_ago}d ago"))),
            &TakeOptions {
                now: Some(NOW - days_ago * 86_400),
                max_file_size: None,
            },
        )
        .expect("take")
        {
            CaptureOutcome::Created { id, .. } => id.hex(),
            other => panic!("expected Created, got {other:?}"),
        }
    }

    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");

    fx.write("a.txt", "old\n");
    let old_id = snap_at(&fx, 100);
    fx.write("a.txt", "new\n");
    let _new_id = snap_at(&fx, 5);

    let repo = fx.repo();
    ff_core::trim(
        &repo,
        &TrimOptions {
            now: Some(NOW),
            dry_run: false,
            gone: false,
            keep_secs: Some(10 * 86_400), // drops the 100-day-old snapshot, keeps the 5-day one
        },
    )
    .expect("trim");

    // Now only reachable via refs/fufu/trash/main, not the live chain.
    let report = restore_all(&fx, Some(&old_id[..8]));
    assert_eq!(report.target.id, old_id);
}
