//! Trim contract: trash-first, byte-preserved survivors, honest relinking,
//! truthful reflog replay, and gc-proofness of everything still referenced.
//!
//! Every fixture here carries one operation the old chain tests did not have:
//! the log's floor, laid by the first capture in the repository and dated with
//! it. It ages out on the same cutoff as everything else, which is why the
//! drop counts below are one higher than the number of captures taken.

use ff_core::{CaptureOutcome, Provenance, TakeOptions, TrimOptions};
use ff_testsupport::Fixture;

/// A synthetic "now" all trim tests share; snapshot times hang off it.
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

fn trim(fx: &Fixture, dry_run: bool, gone: bool) -> ff_core::TrimReport {
    let repo = fx.repo();
    ff_core::trim(
        &repo,
        &TrimOptions {
            now: Some(NOW),
            dry_run,
            gone,
            keep_secs: None, // fufu.keep default: 90 days
        },
    )
    .expect("trim")
}

struct Raw {
    tree: String,
    parents: Vec<String>,
    author: String,
    committer: String,
    message: String,
}

fn cat(fx: &Fixture, id: &str) -> Raw {
    let raw = fx.git(&["cat-file", "commit", id]);
    let (header, message) = raw.split_once("\n\n").expect("commit has message");
    let mut tree = String::new();
    let mut parents = Vec::new();
    let mut author = String::new();
    let mut committer = String::new();
    for line in header.lines() {
        if let Some(rest) = line.strip_prefix("tree ") {
            tree = rest.into();
        } else if let Some(rest) = line.strip_prefix("parent ") {
            parents.push(rest.to_string());
        } else if let Some(rest) = line.strip_prefix("author ") {
            author = rest.into();
        } else if let Some(rest) = line.strip_prefix("committer ") {
            committer = rest.into();
        }
    }
    Raw {
        tree,
        parents,
        author,
        committer,
        message: message.to_string(),
    }
}

/// Standard fixture: 4 captures at 100/95/10/5 days old — the 90d default
/// cutoff drops the two oldest, and the floor underneath them.
fn aged_chain(fx: &Fixture) -> Vec<String> {
    fx.write("a.txt", "a\n");
    fx.commit("init");
    let mut snaps = Vec::new();
    for (i, days) in [100i64, 95, 10, 5].into_iter().enumerate() {
        fx.write("a.txt", &format!("state {i}\n"));
        snaps.push(snap_at(fx, days));
    }
    snaps
}

#[test]
fn dry_run_writes_nothing() {
    let fx = Fixture::new();
    let snaps = aged_chain(&fx);
    let report = trim(&fx, true, false);
    assert!(report.dry_run);
    assert_eq!(report.pointers.len(), 1);
    assert_eq!(report.pointers[0].dropped, 3, "two captures and the floor");
    assert_eq!(report.pointers[0].kept, 2);
    let log = report.log.expect("the log is reported too");
    assert_eq!(log.dropped, 3);
    assert_eq!(log.trash_ref, None, "a dry run parks nothing");
    let tip = fx.git(&["rev-parse", "refs/fufu/snap/main"]);
    assert_eq!(tip.trim(), snaps[3], "the log is untouched");
    let trash = fx.try_git(&[
        "rev-parse",
        "--verify",
        "--quiet",
        "refs/fufu/wt/main/trash/@ops",
    ]);
    assert!(!trash.status.success(), "no trash written on dry run");
}

#[test]
fn trim_drops_suffix_preserving_survivors() {
    let fx = Fixture::new();
    let snaps = aged_chain(&fx);
    let originals: Vec<Raw> = snaps.iter().map(|id| cat(&fx, id)).collect();

    let report = trim(&fx, false, false);
    assert_eq!(report.pointers[0].dropped, 3);
    assert_eq!(report.pointers[0].kept, 2);
    assert!(!report.pointers[0].deleted);

    // Trash = the pre-trim log tip, written before anything moved.
    let trash = fx.git(&["rev-parse", "refs/fufu/wt/main/trash/@ops"]);
    assert_eq!(trash.trim(), snaps[3]);

    // The rebuilt log: two survivors under the trim's own note.
    let note = fx
        .git(&["rev-parse", "refs/fufu/snap/main"])
        .trim()
        .to_string();
    let new_tip = cat(&fx, &note).parents[0].clone();
    let new_top = cat(&fx, &new_tip);
    let new_oldest_id = new_top.parents[0].clone();
    let new_oldest = cat(&fx, &new_oldest_id);

    // Survivor content is byte-preserved: tree, subject, identities, dates.
    // What may differ is exactly what describes the log's SHAPE — the four
    // stated links — because the shape is the thing the trim changed.
    let shape_keys = [
        "fufu-prev:",
        "fufu-prev-branch:",
        "fufu-prev-segment:",
        "fufu-prev-verb:",
    ];
    let without_links = |msg: &str| -> Vec<String> {
        msg.lines()
            .filter(|l| !shape_keys.iter().any(|k| l.starts_with(k)))
            .map(str::to_string)
            .collect()
    };
    for (rebuilt, original) in [(&new_top, &originals[3]), (&new_oldest, &originals[2])] {
        assert_eq!(rebuilt.tree, original.tree, "tree byte-preserved");
        assert_eq!(
            without_links(&rebuilt.message),
            without_links(&original.message),
            "everything but the shape links is byte-preserved"
        );
        assert_eq!(
            rebuilt.author, original.author,
            "author + date byte-preserved"
        );
        assert_eq!(rebuilt.committer, original.committer);
    }
    // And the links really did relink rather than being left dangling.
    assert!(
        new_oldest.message.contains("fufu-prev: none"),
        "the new root states that nothing precedes it: {:?}",
        new_oldest.message
    );
    // Relink rules. The oldest survivor becomes the log's new root, so it
    // loses both leading slots: the prev it no longer has, and the base that
    // would otherwise slide up into slot 1 and send `git log --first-parent`
    // out through the user's own history. A capture has nothing after slot 2,
    // so what remains is nothing at all.
    assert!(
        new_oldest.parents.is_empty(),
        "the new root keeps no leading slots: {:?}",
        new_oldest.parents
    );
    assert_eq!(originals[2].parents.len(), 2, "it had both before the trim");
    // The newer survivor keeps its base in slot 2, verbatim.
    assert_eq!(new_top.parents[1..], originals[3].parents[1..]);

    // Dropped operations remain reachable through trash (one-deep undo).
    for dropped in &snaps[..2] {
        let out = fx.git(&["cat-file", "-e", dropped]);
        drop(out);
        let reachable = fx.git(&["merge-base", "--is-ancestor", dropped, snaps[3].as_str()]);
        drop(reachable);
    }
}

#[test]
fn reflog_replay_keeps_time_queries_truthful() {
    let fx = Fixture::new();
    let _snaps = aged_chain(&fx);
    trim(&fx, false, false);

    // The replayed reflog carries the original snapshot dates, so
    // `ref@{<time>}` still answers with the chain as of that moment.
    let real_now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;
    // A moment between the two survivors (10d and 5d before NOW).
    let target = NOW - 7 * 86_400;
    let ago = real_now - target;
    let spec = format!("refs/fufu/snap/main@{{{ago} seconds ago}}");
    let resolved = fx.git(&["rev-parse", &spec]).trim().to_string();

    // At that moment the pointer was on the (rebuilt) 10d-old capture. The
    // pointer's tip is now the trim's own note; step past it and past the 5d
    // survivor to reach it.
    let note = fx
        .git(&["rev-parse", "refs/fufu/snap/main"])
        .trim()
        .to_string();
    let newest_survivor = cat(&fx, &note).parents[0].clone();
    let expected = cat(&fx, &newest_survivor).parents[0].clone();
    assert_eq!(resolved, expected, "@{{time}} truthful after replay");

    let entries = fx.git(&["reflog", "show", "refs/fufu/snap/main"]);
    assert_eq!(
        entries.lines().count(),
        3,
        "one line per survivor, plus the trim note that landed on top"
    );
}

#[test]
fn all_dropped_moves_the_log_to_trash_and_deletes_every_ref() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    fx.write("a.txt", "old\n");
    let old_snap = snap_at(&fx, 200);
    let report = trim(&fx, false, false);
    assert_eq!(report.pointers[0].dropped, 2, "the capture and the floor");
    assert!(report.pointers[0].deleted);
    assert!(
        !report.pointers[0].gone,
        "main exists throughout — a deleted pointer does not make the branch gone"
    );
    assert!(report.log.as_ref().unwrap().deleted);

    for r in ["refs/fufu/snap/main", "refs/fufu/wt/main/ops"] {
        let gone = fx.try_git(&["rev-parse", "--verify", "--quiet", r]);
        assert!(!gone.status.success(), "{r} deleted");
    }
    let trash = fx.git(&["rev-parse", "refs/fufu/wt/main/trash/@ops"]);
    assert_eq!(trash.trim(), old_snap, "trash holds the whole log");
}

#[test]
fn gone_branches_lose_their_pointer_and_age_out_on_time() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    fx.git(&["checkout", "-q", "-b", "doomed"]);
    fx.write("a.txt", "doomed work\n");
    let doomed_snap = snap_at(&fx, 1); // fresh — age alone would keep it
    fx.git(&["checkout", "-q", "main"]);
    fx.git(&["branch", "-qD", "doomed"]);

    // Without --gone the chain stays.
    let report = trim(&fx, false, false);
    let doomed = report
        .pointers
        .iter()
        .find(|c| c.branch == "doomed")
        .unwrap();
    assert_eq!(doomed.dropped, 0);
    assert!(
        doomed.gone,
        "existence is reported unconditionally; only deletion is flag-gated"
    );

    // With --gone the POINTER goes, and only the pointer. You cannot excise
    // one branch's operations from the middle of a global chain without
    // rewriting every operation after them, so the operations behind the name
    // stay on the log and age out on the same cutoff as everything else.
    let report = trim(&fx, false, true);
    let doomed = report
        .pointers
        .iter()
        .find(|c| c.branch == "doomed")
        .unwrap();
    assert!(doomed.deleted);
    assert!(doomed.gone);
    assert_eq!(doomed.dropped, 0, "--gone drops no operations, and says so");
    let gone = fx.try_git(&["rev-parse", "--verify", "--quiet", "refs/fufu/snap/doomed"]);
    assert!(!gone.status.success(), "the pointer is deleted");
    assert!(
        fx.git(&["rev-list", "refs/fufu/wt/main/ops"])
            .lines()
            .any(|l| l == doomed_snap),
        "the operation itself stays on the log"
    );
    // And nothing was rewritten: the log's tip is exactly where it was.
    assert_eq!(
        fx.git(&["rev-parse", "refs/fufu/wt/main/ops"]).trim(),
        doomed_snap,
        "--gone rebuilds nothing, so no operation changes its sha"
    );
}

/// A live branch whose every operation aged out loses its pointer, not its
/// existence: the report must say the branch is still there, so the renderer
/// never calls it gone.
#[test]
fn aged_out_live_branch_loses_its_pointer_but_not_the_branch() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    fx.git(&["checkout", "-q", "-b", "stale"]);
    fx.write("a.txt", "stale work\n");
    snap_at(&fx, 100);
    fx.git(&["checkout", "-q", "main"]);
    fx.write("a.txt", "fresh work\n");
    snap_at(&fx, 1);

    let report = trim(&fx, false, false);
    let stale = report
        .pointers
        .iter()
        .find(|c| c.branch == "stale")
        .unwrap();
    assert!(stale.deleted, "every operation aged out, the pointer goes");
    assert!(!stale.gone, "the branch itself still exists");
    let pointer = fx.try_git(&["rev-parse", "--verify", "--quiet", "refs/fufu/snap/stale"]);
    assert!(!pointer.status.success(), "the pointer is deleted");
    let head = fx.try_git(&["rev-parse", "--verify", "--quiet", "refs/heads/stale"]);
    assert!(head.status.success(), "refs/heads/stale still resolves");
}

/// The gc proof: aggressive reflog expiry plus gc --prune=now must not
/// collect anything the log or its trash still references.
#[test]
fn gc_cannot_collect_the_chain() {
    let fx = Fixture::new();
    let snaps = aged_chain(&fx);
    trim(&fx, false, false);
    let new_tip = fx
        .git(&["rev-parse", "refs/fufu/snap/main"])
        .trim()
        .to_string();

    fx.git(&["reflog", "expire", "--expire=now", "--all"]);
    fx.git(&["gc", "--prune=now", "--quiet"]);

    for id in [new_tip.as_str(), snaps[3].as_str(), snaps[0].as_str()] {
        let out = fx.try_git(&["cat-file", "-e", id]);
        assert!(
            out.status.success(),
            "{id} was collected — the log is not gc-proof"
        );
    }
    // And the snapshot content is still restorable.
    let tree = fx.try_git(&["rev-parse", "--verify", &format!("{new_tip}^{{tree}}")]);
    assert!(tree.status.success());
}

#[test]
fn nothing_to_drop_reports_and_leaves_the_log_alone() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    fx.write("a.txt", "fresh\n");
    let snap = snap_at(&fx, 1);
    let report = trim(&fx, false, false);
    assert_eq!(report.pointers[0].dropped, 0);
    assert_eq!(report.pointers[0].kept, 2, "the capture and the floor");
    let tip = fx.git(&["rev-parse", "refs/fufu/snap/main"]);
    assert_eq!(tip.trim(), snap, "an untouched log keeps its exact shas");
}

/// fufu.keep is honored when set.
#[test]
fn keep_config_overrides_default() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    fx.write("a.txt", "one\n");
    snap_at(&fx, 10);
    fx.set_config("fufu.keep", "7d");
    let report = trim(&fx, false, false);
    assert_eq!(
        report.pointers[0].dropped, 2,
        "a 7d cutoff drops a 10d capture and the floor beneath it"
    );
}
