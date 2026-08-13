//! Trim contract: trash-first, byte-preserved survivors, honest relinking,
//! truthful reflog replay, and gc-proofness of everything still referenced.

use ff_core::{Provenance, SnapOutcome, TakeOptions, TrimOptions};
use ff_testsupport::Fixture;

/// A synthetic "now" all trim tests share; snapshot times hang off it.
const NOW: i64 = 1_700_000_000;

fn snap_at(fx: &Fixture, days_ago: i64) -> String {
    let repo = fx.repo();
    match ff_core::take_with(
        &repo,
        &Provenance::new("manual", Some(format!("{days_ago}d ago"))),
        &TakeOptions {
            now: Some(NOW - days_ago * 86_400),
            max_file_size: None,
        },
    )
    .expect("take")
    {
        SnapOutcome::Created { id, .. } => id,
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

/// Standard fixture: 4 snapshots at 100/95/10/5 days old — the 90d default
/// cutoff drops the two oldest.
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
    assert_eq!(report.chains.len(), 1);
    assert_eq!(report.chains[0].dropped, 2);
    assert_eq!(report.chains[0].kept, 2);
    let tip = fx.git(&["rev-parse", "refs/fufu/snap/main"]);
    assert_eq!(tip.trim(), snaps[3], "chain untouched");
    let trash = fx.try_git(&["rev-parse", "--verify", "--quiet", "refs/fufu/trash/main"]);
    assert!(!trash.status.success(), "no trash written on dry run");
}

#[test]
fn trim_drops_suffix_preserving_survivors() {
    let fx = Fixture::new();
    let snaps = aged_chain(&fx);
    let originals: Vec<Raw> = snaps.iter().map(|id| cat(&fx, id)).collect();

    let report = trim(&fx, false, false);
    assert_eq!(report.chains[0].dropped, 2);
    assert_eq!(report.chains[0].kept, 2);
    assert!(!report.chains[0].deleted);

    // Trash = the pre-trim tip, written before anything moved.
    let trash = fx.git(&["rev-parse", "refs/fufu/trash/main"]);
    assert_eq!(trash.trim(), snaps[3]);

    // The rebuilt chain: two survivors, oldest relinked to its base edge.
    let new_tip = fx
        .git(&["rev-parse", "refs/fufu/snap/main"])
        .trim()
        .to_string();
    let new_top = cat(&fx, &new_tip);
    let new_oldest_id = new_top.parents[0].clone();
    let new_oldest = cat(&fx, &new_oldest_id);

    // Survivor content is byte-preserved: tree, message, identities, dates.
    for (rebuilt, original) in [(&new_top, &originals[3]), (&new_oldest, &originals[2])] {
        assert_eq!(rebuilt.tree, original.tree, "tree byte-preserved");
        assert_eq!(rebuilt.message, original.message, "message byte-preserved");
        assert_eq!(
            rebuilt.author, original.author,
            "author + date byte-preserved"
        );
        assert_eq!(rebuilt.committer, original.committer);
    }
    // Relink rules: the oldest survivor's parents are its base edge verbatim
    // (prev slot dropped); the newer survivor keeps its base in slot 2.
    assert_eq!(new_oldest.parents, originals[2].parents[1..].to_vec());
    assert_eq!(new_top.parents[1..], originals[3].parents[1..]);

    // Dropped snapshots remain reachable through trash (one-deep undo).
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

    // At that moment the tip was the (rebuilt) 10d-old snapshot: the new
    // tip's first parent.
    let new_tip = fx
        .git(&["rev-parse", "refs/fufu/snap/main"])
        .trim()
        .to_string();
    let expected = cat(&fx, &new_tip).parents[0].clone();
    assert_eq!(resolved, expected, "@{{time}} truthful after replay");

    let entries = fx.git(&["reflog", "show", "refs/fufu/snap/main"]);
    assert_eq!(entries.lines().count(), 2, "one reflog line per survivor");
}

#[test]
fn all_dropped_moves_chain_to_trash_and_deletes_ref() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    fx.write("a.txt", "old\n");
    let old_snap = snap_at(&fx, 200);
    let report = trim(&fx, false, false);
    assert_eq!(report.chains[0].dropped, 1);
    assert!(report.chains[0].deleted);

    let gone = fx.try_git(&["rev-parse", "--verify", "--quiet", "refs/fufu/snap/main"]);
    assert!(!gone.status.success(), "chain ref deleted");
    let trash = fx.git(&["rev-parse", "refs/fufu/trash/main"]);
    assert_eq!(trash.trim(), old_snap, "trash holds the whole chain");
}

#[test]
fn gone_branches_drop_entire_chains() {
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
    let doomed = report.chains.iter().find(|c| c.branch == "doomed").unwrap();
    assert_eq!(doomed.dropped, 0);

    let report = trim(&fx, false, true);
    let doomed = report.chains.iter().find(|c| c.branch == "doomed").unwrap();
    assert!(doomed.deleted);
    let gone = fx.try_git(&["rev-parse", "--verify", "--quiet", "refs/fufu/snap/doomed"]);
    assert!(!gone.status.success());
    let trash = fx.git(&["rev-parse", "refs/fufu/trash/doomed"]);
    assert_eq!(trash.trim(), doomed_snap);
}

/// The gc proof: aggressive reflog expiry plus gc --prune=now must not
/// collect anything the chain or trash still references.
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
            "{id} was collected — the chain is not gc-proof"
        );
    }
    // And the snapshot content is still restorable.
    let tree = fx.try_git(&["rev-parse", "--verify", &format!("{new_tip}^{{tree}}")]);
    assert!(tree.status.success());
}

#[test]
fn nothing_to_drop_reports_and_leaves_chain_alone() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    fx.write("a.txt", "fresh\n");
    let snap = snap_at(&fx, 1);
    let report = trim(&fx, false, false);
    assert_eq!(report.chains[0].dropped, 0);
    assert_eq!(report.chains[0].kept, 1);
    let tip = fx.git(&["rev-parse", "refs/fufu/snap/main"]);
    assert_eq!(tip.trim(), snap, "untouched chains keep their exact shas");
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
        report.chains[0].dropped, 1,
        "7d cutoff drops a 10d snapshot"
    );
}
