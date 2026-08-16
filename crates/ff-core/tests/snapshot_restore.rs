//! Restore contract: worktree-only writes, a mandatory pre-restore capture,
//! one kind per source flag, round-trips, and refusals.

use ff_core::{
    CaptureOutcome, Provenance, RestoreOptions, RestoreSource, TakeOptions, TrimOptions,
};
use ff_testsupport::Fixture;

fn take_created(fx: &Fixture) -> String {
    let repo = fx.repo();
    match ff_core::capture(&repo, &Provenance::new("manual", None)).expect("take") {
        CaptureOutcome::Created { id, .. } => id.hex(),
        other => panic!("expected Created, got {other:?}"),
    }
}

/// The letters spelling of a hex id, which is the only form an operation
/// address is accepted in.
fn op(hex: &str) -> String {
    ff_core::snapid::encode(hex)
}

fn restore(fx: &Fixture, source: RestoreSource, paths: Vec<String>) -> ff_core::RestoreReport {
    let repo = fx.repo();
    let all = paths.is_empty();
    ff_core::restore(
        &repo,
        &RestoreOptions {
            source,
            paths,
            all,
            now: None,
        },
        &Provenance::new("pre", Some("ff restore".into())),
    )
    .expect("restore")
}

fn restore_at_op(fx: &Fixture, spec: &str) -> ff_core::RestoreReport {
    restore(fx, RestoreSource::Op(spec.to_string()), Vec::new())
}

/// Restore --all to an operation, then take: the take must be a no-op against
/// that operation's tree — the worktree matches it exactly.
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

    let report = restore_at_op(&fx, &op(&snap)[..8]);
    assert_eq!(report.origin.id, snap);
    assert_eq!(report.origin.space, "operation");
    assert!(report.pre_op.is_some(), "pre-restore capture happened");

    // Worktree content matches the operation.
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

/// The behavior change worth naming: bare `ff restore <paths>` pulls from the
/// commit under the open change, which is what `git restore <path>` and
/// `jj restore <paths>` both mean. It used to restore from the newest
/// capture, which is nearly always a no-op — the everyday "discard my edits
/// to this file" had no spelling at all.
#[test]
fn bare_restore_comes_from_the_commit_under_the_change() {
    let fx = Fixture::new();
    fx.write("a.txt", "committed\n");
    fx.write("b.txt", "also committed\n");
    fx.commit("init");
    fx.write("a.txt", "edited\n");
    fx.write("b.txt", "edited too\n");
    // A capture of the edits: the old default would have restored from here,
    // leaving both files exactly as they are.
    take_created(&fx);

    let report = restore(&fx, RestoreSource::Open, vec!["a.txt".into()]);
    assert_eq!(report.origin.space, "commit");
    assert_eq!(
        std::fs::read_to_string(fx.path().join("a.txt")).unwrap(),
        "committed\n",
        "the edits to the named path are discarded"
    );
    assert_eq!(
        std::fs::read_to_string(fx.path().join("b.txt")).unwrap(),
        "edited too\n",
        "and nothing else is touched"
    );
}

/// `--from` goes through the one revset resolver, so anything that names a
/// revision works — and `@` is refused, because the open change is where the
/// files already are.
#[test]
fn from_takes_a_revision_through_the_revset() {
    let fx = Fixture::new();
    fx.write("a.txt", "one\n");
    fx.commit("one");
    fx.write("a.txt", "two\n");
    fx.commit("two");
    fx.write("a.txt", "working\n");

    let report = restore(
        &fx,
        RestoreSource::Rev("HEAD~".into()),
        vec!["a.txt".into()],
    );
    assert_eq!(report.origin.space, "commit");
    assert_eq!(
        std::fs::read_to_string(fx.path().join("a.txt")).unwrap(),
        "one\n"
    );

    let repo = fx.repo();
    let err = ff_core::restore(
        &repo,
        &RestoreOptions {
            source: RestoreSource::Rev("@".into()),
            paths: vec!["a.txt".into()],
            all: false,
            now: None,
        },
        &Provenance::new("pre", Some("ff restore".into())),
    )
    .unwrap_err();
    assert_eq!(err.id(), "target/unresolvable", "{err}");
}

/// `--at` takes a time and only a time. Nothing here has to out-guess an id,
/// which is why splitting the flag retired an entire class of shadowing:
/// `123d` is a duration now, not an object prefix that happens to be hex.
#[test]
fn at_takes_a_clock_and_nothing_else() {
    let now = 1_700_000_000;
    for (raw, want) in [
        ("30m", now - 1800),
        ("2h", now - 7200),
        ("3d", now - 3 * 86_400),
        ("123d", now - 123 * 86_400),
        ("1w", now - 7 * 86_400),
        ("90s", now - 90),
    ] {
        assert_eq!(
            ff_core::restore_time(raw, now).unwrap(),
            want,
            "--at {raw:?}"
        );
    }
    // A spelling that is neither an age nor a date is refused by name.
    let err = ff_core::restore_time("kqzm", now).unwrap_err();
    assert_eq!(err.id(), "usage/bad-restore-target", "{err}");
}

/// `--at` lands on the operation current at that moment, over the whole log.
#[test]
fn at_lands_on_the_operation_current_then() {
    const NOW: i64 = 1_700_000_000;
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");

    let capture_at = |body: &str, secs_ago: i64| {
        fx.write("a.txt", body);
        let repo = fx.repo();
        match ff_core::capture_with(
            &repo,
            &Provenance::new("manual", None),
            &TakeOptions {
                now: Some(NOW - secs_ago),
                max_file_size: None,
            },
        )
        .expect("take")
        {
            CaptureOutcome::Created { id, .. } => id.hex(),
            other => panic!("expected Created, got {other:?}"),
        }
    };
    let old = capture_at("three hours ago\n", 3 * 3600);
    capture_at("just now\n", 1);

    let repo = fx.repo();
    let report = ff_core::restore(
        &repo,
        &RestoreOptions {
            source: RestoreSource::Time("2h".into()),
            paths: Vec::new(),
            all: true,
            now: Some(NOW),
        },
        &Provenance::new("pre", Some("ff restore".into())),
    )
    .expect("restore");
    assert_eq!(report.origin.id, old, "the operation current two hours ago");
    assert_eq!(
        std::fs::read_to_string(fx.path().join("a.txt")).unwrap(),
        "three hours ago\n"
    );
}

/// The address-space leak this plan closed: `--at-op` takes letters, and raw
/// hex in an operation-typed position is refused rather than resolved.
#[test]
fn at_op_refuses_hex() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    fx.write("a.txt", "captured\n");
    let snap = take_created(&fx);
    fx.write("a.txt", "diverged\n");

    let repo = fx.repo();
    let err = ff_core::restore(
        &repo,
        &RestoreOptions {
            source: RestoreSource::Op(snap[..8].to_string()),
            paths: Vec::new(),
            all: true,
            now: None,
        },
        &Provenance::new("pre", Some("ff restore".into())),
    )
    .unwrap_err();
    assert_eq!(err.id(), "op/not-found", "{err}");
    assert_eq!(
        std::fs::read_to_string(fx.path().join("a.txt")).unwrap(),
        "diverged\n",
        "nothing was written"
    );

    // The same operation, spelled the one way it is spelled, works.
    let report = restore_at_op(&fx, &op(&snap)[..8]);
    assert_eq!(report.origin.id, snap);
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
    restore_at_op(&fx, &op(&snap));
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

    restore_at_op(&fx, &op(&snap));
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
    fx.write("keep.txt", "captured\n");
    let snap = take_created(&fx);
    fx.write("deep/nested/only.txt", "temporary\n");
    restore_at_op(&fx, &op(&snap));
    assert!(
        !fx.path().join("deep").exists(),
        "emptied parent dirs pruned bottom-up"
    );
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

    let report = restore(&fx, RestoreSource::Op(op(&snap)), vec!["dir".into()]);
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

/// A letters-spelled id drives a real restore end to end.
#[test]
fn letters_id_restores() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    fx.write("a.txt", "captured\n");
    let snap = take_created(&fx);
    fx.write("a.txt", "diverged\n");

    let report = restore_at_op(&fx, &op(&snap)[..8]);
    assert_eq!(report.origin.id, snap);
    assert_eq!(
        std::fs::read_to_string(fx.path().join("a.txt")).unwrap(),
        "captured\n"
    );
}

/// The guard is `is_op_commit` and not "does it bear the fufu identity" — a
/// record commit bears the identity too, and restoring from one would wipe
/// the working tree and write three metadata files in its place. Resolution
/// runs through `OpLog::resolve`, which applies it to every candidate.
#[test]
fn refuses_targets_that_are_not_operations() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    let head = fx.commit("init");
    fx.write("a.txt", "dirty\n");
    take_created(&fx);

    let repo = fx.repo();
    let err = ff_core::restore(
        &repo,
        &RestoreOptions {
            source: RestoreSource::Op(op(&head)),
            paths: Vec::new(),
            all: true,
            now: None,
        },
        &Provenance::new("pre", Some("ff restore".into())),
    )
    .unwrap_err();
    assert_eq!(err.id(), "op/not-found", "{err}");
}

#[test]
fn contended_pre_capture_aborts_before_writing() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    fx.write("a.txt", "captured\n");
    let snap = take_created(&fx);
    fx.write("a.txt", "diverged\n");

    // Hold the branch pointer's lock: the mandatory pre-capture must hit
    // Contended.
    let lock = fx.path().join(".git/refs/fufu/snap/main.lock");
    std::fs::create_dir_all(lock.parent().unwrap()).unwrap();
    std::fs::write(&lock, "held").unwrap();

    let repo = fx.repo();
    let err = ff_core::restore(
        &repo,
        &RestoreOptions {
            source: RestoreSource::Op(op(&snap)),
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

/// An unborn branch has no commit under the open change, so the bare form has
/// nothing to offer and says which flags do.
#[test]
fn an_unborn_branch_has_nothing_under_the_change() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    let repo = fx.repo();
    let err = ff_core::restore(
        &repo,
        &RestoreOptions {
            source: RestoreSource::Open,
            paths: Vec::new(),
            all: true,
            now: None,
        },
        &Provenance::new("pre", Some("ff restore --all".into())),
    )
    .unwrap_err();
    assert_eq!(err.id(), "op/not-found", "{err}");
}

#[test]
fn untracked_files_survive_because_diff_is_target_vs_snapshot() {
    // The load-bearing subtlety: the write diff is source ↔ fresh capture,
    // never source ↔ worktree. An untracked file present in both states
    // simply isn't in the diff.
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    fx.write("a.txt", "captured\n");
    fx.write("untracked.txt", "present in the operation too\n");
    let snap = take_created(&fx);
    fx.write("a.txt", "diverged\n");

    restore_at_op(&fx, &op(&snap));
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
    restore_at_op(&fx, &op(&snap));

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
/// instead: keep taking captures until two ids share a 4+ character prefix,
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
    // turns up most runs but not every run, and forcing it would cost
    // thousands of captures. When it does not, assert the other half of the
    // same code path instead — that the shortest prefix the index calls
    // unique really does resolve to exactly one id — so this test always
    // exercises index-backed resolution rather than passing vacuously.
    let Some((a, b, prefix)) = collision else {
        let newest = ids.last().expect("at least one capture").clone();
        let repo = fx.repo();
        let lens = ff_core::ops::index::prefix_lens(&repo, std::slice::from_ref(&newest))
            .expect("prefix_lens");
        // Never below git's four-character minimum: below it the index has
        // nothing to say and the resolver refuses on length alone.
        let len = lens[&newest].max(4);
        drop(repo);
        let report = restore_at_op(&fx, &op(&newest)[..len]);
        assert_eq!(
            report.origin.id, newest,
            "the index's own unique prefix must resolve to its own id"
        );
        return;
    };

    let repo = fx.repo();
    let err = ff_core::restore(
        &repo,
        &RestoreOptions {
            source: RestoreSource::Op(op(&prefix)),
            paths: Vec::new(),
            all: true,
            now: None,
        },
        &Provenance::new("pre", Some("ff restore".into())),
    )
    .unwrap_err();
    let msg = err.to_string();
    assert_eq!(err.id(), "op/ambiguous", "{msg}");
    // Candidates are listed in the letters alphabet, because that is the
    // spelling the user has to type back.
    for hex in [&a, &b] {
        let letters = ff_core::snapid::encode(hex);
        assert!(msg.contains(&letters[..12]), "missing {letters} in {msg}");
    }
}

/// Trim moves the pre-cutoff suffix of the log to `refs/fufu/trash/@ops` —
/// those ids no longer live on the live log at all. Prefix resolution must
/// still find them there.
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
            keep_secs: Some(10 * 86_400), // drops the 100-day-old op, keeps the 5-day one
        },
    )
    .expect("trim");

    // Now only reachable via refs/fufu/trash/@ops, not the live log.
    let report = restore_at_op(&fx, &op(&old_id)[..8]);
    assert_eq!(report.origin.id, old_id);
}
