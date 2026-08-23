//! Id-index integration tests: the materialized operation-id domain must
//! reproduce the walk-based answer in every case, and self-heal when corrupted.
//!
//! The domain is one log now rather than one chain per branch, so the walk the
//! index has to reproduce is the whole of `refs/fufu/wt/main/ops` plus whatever trim
//! parked at `refs/fufu/wt/main/trash/@ops`.

use std::collections::HashMap;
use std::thread::scope;

use ff_core::ops::index::{self, Kind};
use ff_core::{CaptureOutcome, Provenance, TakeOptions};
use ff_testsupport::Fixture;

const OPS_REF: &str = "refs/fufu/wt/main/ops";

/// Which of the two domains to walk.
#[derive(Clone, Copy)]
enum Domain {
    Live,
    Trash,
}

/// Every operation id in a domain, newest first, following the stated link.
fn log_ids(repo: &gix::Repository, domain: Domain) -> Vec<String> {
    let log = ff_core::ops::OpLog::open(repo).expect("open");
    let start = match domain {
        Domain::Live => log.tip(),
        Domain::Trash => log.trash_tip(),
    };
    let mut out = Vec::new();
    let mut cur = start.expect("tip");
    while let Some(id) = cur {
        let Ok(op) = log.get(id) else { break };
        out.push(op.id().hex());
        cur = op.prev();
    }
    out
}

/// The answer the whole domain gives when walked: sort the live+trash ids and
/// give each one a character more than its longest common prefix with either
/// neighbour. This is what the index must reproduce without the walk.
fn reference(repo: &gix::Repository) -> HashMap<String, usize> {
    let mut all_ids = log_ids(repo, Domain::Live);
    all_ids.extend(log_ids(repo, Domain::Trash));
    let mut sorted: Vec<&str> = all_ids.iter().map(String::as_str).collect();
    sorted.sort_unstable();
    sorted.dedup();
    let common = |a: &str, b: &str| a.bytes().zip(b.bytes()).take_while(|(x, y)| x == y).count();
    let mut lens = HashMap::new();
    for (i, id) in sorted.iter().enumerate() {
        let prev = if i > 0 { common(sorted[i - 1], id) } else { 0 };
        let next = if i + 1 < sorted.len() {
            common(id, sorted[i + 1])
        } else {
            0
        };
        lens.insert(id.to_string(), (prev.max(next) + 1).min(id.len().max(1)));
    }
    lens
}

fn take_created(fx: &Fixture) -> String {
    let repo = fx.repo();
    match ff_core::capture(&repo, &Provenance::new("manual", None)).expect("capture") {
        CaptureOutcome::Created { id, .. } => id.hex(),
        other => panic!("expected Created, got {other:?}"),
    }
}

/// Build a fixture with N captures and return their ids.
fn build_log(fx: &Fixture, n: usize) -> Vec<String> {
    let mut ids = Vec::new();
    for i in 0..n {
        fx.write(&format!("file_{i}.txt"), &format!("content {i}"));
        ids.push(take_created(fx));
    }
    ids
}

fn tip(repo: &gix::Repository) -> gix::ObjectId {
    ff_core::ops::OpLog::open(repo)
        .expect("open")
        .tip()
        .expect("tip")
        .expect("tip exists")
        .object_id()
}

#[test]
fn builds_on_first_read_and_matches_the_walk() {
    let fx = Fixture::new();
    let ids = build_log(&fx, 12);

    let repo = fx.repo();
    let lens = index::prefix_lens(&repo, &ids).expect("prefix_lens");
    let ref_lens = reference(&repo);
    for id in &ids {
        assert_eq!(
            lens.get(id),
            ref_lens.get(id),
            "prefix_lens must match the walk-based reference for {id}"
        );
    }

    // The index file now exists and has a legal layout.
    let idx_path = index::path(&repo, Kind::Live);
    assert!(
        idx_path.exists(),
        "live index file must exist after first read"
    );
    let len = std::fs::metadata(&idx_path).expect("metadata").len() as usize;
    assert!(
        (len - 50).is_multiple_of(41),
        "file length {len} must satisfy (len - 50) % 41 == 0"
    );
}

#[test]
fn append_extends_the_index() {
    let fx = Fixture::new();
    let ids = build_log(&fx, 5);

    let repo = fx.repo();

    // First read builds the index.
    let _lens = index::prefix_lens(&repo, &ids).expect("prefix_lens");
    let idx_path = index::path(&repo, Kind::Live);
    let len_before = std::fs::metadata(&idx_path).expect("metadata").len();

    let tip_before = tip(&repo);
    fx.write("new_file.txt", "new content");
    let new_id = take_created(&fx);
    let tip_after = tip(&repo);

    // The capture already recorded; do it again explicitly and confirm the
    // stale-header guard leaves the file alone rather than double-appending.
    let len_after_capture = std::fs::metadata(&idx_path).expect("metadata").len();
    assert_eq!(
        len_after_capture - len_before,
        41,
        "the capture's own record must grow the file by exactly RECORD_LEN"
    );
    index::record(&repo, Some(tip_before), tip_after);
    assert_eq!(
        std::fs::metadata(&idx_path).expect("metadata").len(),
        len_after_capture,
        "a second record against a header that already moved must decline"
    );

    // Header tip equals the new tip.
    let contents = std::fs::read(&idx_path).expect("read index");
    let header_tip = String::from_utf8_lossy(&contents[0..40]).to_string();
    assert_eq!(header_tip, new_id, "header tip must equal the new op id");

    // prefix_lens still matches the reference.
    let all_ids: Vec<String> = ids
        .iter()
        .chain(std::iter::once(&new_id))
        .cloned()
        .collect();
    let lens = index::prefix_lens(&repo, &all_ids).expect("prefix_lens");
    let ref_lens = reference(&repo);
    for id in &all_ids {
        assert_eq!(lens.get(id), ref_lens.get(id), "after append, {id}");
    }
}

#[test]
fn a_stale_header_is_never_extended() {
    let fx = Fixture::new();
    let ids = build_log(&fx, 3);

    let repo = fx.repo();
    let _lens = index::prefix_lens(&repo, &ids).expect("prefix_lens");

    let idx_path = index::path(&repo, Kind::Live);
    let mut contents = std::fs::read(&idx_path).expect("read index");

    // Corrupt the header tip by flipping one hex character.
    contents[0] = if contents[0] == b'0' { b'1' } else { b'0' };
    std::fs::write(&idx_path, &contents).expect("write corrupted header");

    let prev_tip = tip(&repo);
    fx.write("stale_test.txt", "stale");
    let new_id = take_created(&fx);
    let new_tip = tip(&repo);

    // record must decline to append to a stale file.
    index::record(&repo, Some(prev_tip), new_tip);
    let after_contents = std::fs::read(&idx_path).expect("read index");
    assert_eq!(
        contents, after_contents,
        "record must not modify a stale index file"
    );

    // prefix_lens rebuilds and still matches the reference.
    let all_ids: Vec<String> = ids
        .iter()
        .chain(std::iter::once(&new_id))
        .cloned()
        .collect();
    let lens = index::prefix_lens(&repo, &all_ids).expect("prefix_lens");
    let ref_lens = reference(&repo);
    for id in &all_ids {
        assert_eq!(lens.get(id), ref_lens.get(id), "after rebuild, {id}");
    }

    // Header now carries the real tip.
    let rebuilt = std::fs::read(&idx_path).expect("read rebuilt index");
    let header_tip = String::from_utf8_lossy(&rebuilt[0..40]).to_string();
    assert_eq!(header_tip, new_id, "rebuilt header must carry current tip");
}

#[test]
fn the_tail_merges_once_it_is_long() {
    let fx = Fixture::new();
    let ids = build_log(&fx, 3);

    let repo = fx.repo();
    let tip_str = tip(&repo).to_string();

    // Hand-write an index file with base=0 and 600 synthetic records.
    let idx_path = index::path(&repo, Kind::Live);
    let parent = idx_path.parent().unwrap();
    std::fs::create_dir_all(parent).expect("create dir");

    let mut body = String::new();
    body.push_str(&tip_str);
    body.push(' ');
    body.push_str(&format!("{:08x}", 0usize)); // base = 0
    body.push('\n');
    for i in (0..600).rev() {
        body.push_str(&format!("{i:040x}"));
        body.push('\n');
    }
    std::fs::write(&idx_path, &body).expect("write synthetic index");

    // A read plus one append: the merge lives on the write path, so the read
    // must leave the tail alone and the next record must fold it in.
    let _lens = index::prefix_lens(&repo, &ids).expect("prefix_lens");
    fx.write("merge_trigger.txt", "trigger");
    take_created(&fx);

    let contents = std::fs::read(&idx_path).expect("read index");
    let total = (contents.len() - 50) / 41;
    let base_hex = String::from_utf8_lossy(&contents[41..49]);
    let base = usize::from_str_radix(&base_hex, 16).expect("parse base");
    assert_eq!(base, total, "base must equal total after merge");

    let body_str = std::str::from_utf8(&contents[50..]).expect("index body is utf-8");
    let records: Vec<&str> = body_str.lines().filter(|s| !s.is_empty()).collect();
    let mut sorted = records.clone();
    sorted.sort_unstable();
    assert_eq!(
        records, sorted,
        "records must be sorted ascending after merge"
    );
}

#[test]
fn self_heals() {
    let fx = Fixture::new();
    let ids = build_log(&fx, 5);

    let repo = fx.repo();
    let _lens = index::prefix_lens(&repo, &ids).expect("prefix_lens");
    let idx_path = index::path(&repo, Kind::Live);

    let matches_reference = |repo: &gix::Repository, ids: &[String], what: &str| {
        let lens = index::prefix_lens(repo, ids).expect("prefix_lens");
        let ref_lens = reference(repo);
        for id in ids {
            assert_eq!(lens.get(id), ref_lens.get(id), "{what}: {id}");
        }
    };

    // Sub-case 1: truncate the file by one byte.
    {
        let contents = std::fs::read(&idx_path).expect("read");
        std::fs::write(&idx_path, &contents[..contents.len() - 1]).expect("truncate");
        matches_reference(&repo, &ids, "truncate");
        let len = std::fs::metadata(&idx_path).expect("metadata").len() as usize;
        assert!(
            (len - 50).is_multiple_of(41),
            "truncate: file layout must be legal"
        );
    }

    // Sub-case 2: corrupt the header tip.
    {
        let mut corrupted = std::fs::read(&idx_path).expect("read");
        corrupted[5] = if corrupted[5] == b'0' { b'1' } else { b'0' };
        std::fs::write(&idx_path, &corrupted).expect("corrupt");
        matches_reference(&repo, &ids, "corrupt tip");
        let rebuilt = std::fs::read(&idx_path).expect("read rebuilt");
        let tip_str = String::from_utf8_lossy(&rebuilt[0..40]).to_string();
        assert_eq!(
            tip_str,
            tip(&repo).to_string(),
            "rebuilt header tip must match current tip"
        );
    }

    // Sub-case 3: delete the file.
    {
        std::fs::remove_file(&idx_path).expect("delete");
        matches_reference(&repo, &ids, "delete");
        assert!(idx_path.exists(), "file must be recreated");
        let len = std::fs::metadata(&idx_path).expect("metadata").len() as usize;
        assert!(
            (len - 50).is_multiple_of(41),
            "delete: file layout must be legal"
        );
    }

    // Sub-case 4: move the log ref by hand to an older operation.
    {
        let older_id = &ids[ids.len() - 1];
        fx.git(&["update-ref", OPS_REF, older_id]);
        let new_ids = log_ids(&repo, Domain::Live);
        matches_reference(&repo, &new_ids, "ref move");
        let contents = std::fs::read(&idx_path).expect("read");
        let tip_str = String::from_utf8_lossy(&contents[0..40]).to_string();
        assert_eq!(tip_str, *older_id, "header tip must match the moved ref");
        let len = contents.len();
        assert!(
            (len - 50).is_multiple_of(41),
            "file layout must be legal after ref move"
        );
    }
}

#[test]
fn survives_a_trim_rewrite() {
    let fx = Fixture::new();
    let repo = fx.repo();
    let base_time = 1_700_000_000i64; // ~Nov 2023

    for i in 0..8 {
        fx.write(&format!("trim_file_{i}.txt"), &format!("trim content {i}"));
        let now = base_time + i * 86400 * 5; // 5 days apart
        let outcome = ff_core::capture_with(
            &repo,
            &Provenance::new("manual", None),
            &TakeOptions {
                now: Some(now),
                ..TakeOptions::default()
            },
        )
        .expect("capture_with");
        assert!(
            matches!(outcome, CaptureOutcome::Created { .. }),
            "expected Created, got {outcome:?}"
        );
    }

    let report = ff_core::trim(
        &repo,
        &ff_core::TrimOptions {
            now: Some(base_time + 86400 * 5 * 7),
            keep_secs: Some(30 * 86400),
            ..ff_core::TrimOptions::default()
        },
    )
    .expect("trim");
    assert!(
        report.log.as_ref().is_some_and(|l| l.dropped > 0),
        "trim should have dropped some operations"
    );

    let live_ids = log_ids(&repo, Domain::Live);
    assert!(live_ids.len() >= 2, "at least two operations must survive");
    let trash_ids = log_ids(&repo, Domain::Trash);
    assert!(!trash_ids.is_empty(), "the trash log must have ids");

    let all_ids: Vec<String> = live_ids.iter().chain(trash_ids.iter()).cloned().collect();
    let lens = index::prefix_lens(&repo, &all_ids).expect("prefix_lens");
    let ref_lens = reference(&repo);
    for id in &all_ids {
        assert_eq!(
            lens.get(id),
            ref_lens.get(id),
            "prefix length mismatch for {id}"
        );
    }

    assert!(
        index::path(&repo, Kind::Live).exists(),
        "live index file must exist after prefix_lens"
    );
    assert!(
        index::path(&repo, Kind::Trash).exists(),
        "trash index file must exist after prefix_lens"
    );
    assert!(
        trash_ids.iter().any(|id| lens.contains_key(id)),
        "at least one trashed id must have a prefix length"
    );
}

#[test]
fn prefix_matches_agrees_with_the_walk() {
    let fx = Fixture::new();
    build_log(&fx, 8);

    let repo = fx.repo();
    let mut all_walk_ids = log_ids(&repo, Domain::Live);
    all_walk_ids.extend(log_ids(&repo, Domain::Trash));

    for id in &all_walk_ids {
        let prefix = &id[..6];
        let matches = index::prefix_matches(&repo, prefix).expect("prefix_matches");
        let mut match_strs: Vec<String> = matches.iter().map(|o| o.to_string()).collect();
        match_strs.sort_unstable();

        let mut walk_matches: Vec<String> = all_walk_ids
            .iter()
            .filter(|i| i.starts_with(prefix))
            .cloned()
            .collect();
        walk_matches.sort_unstable();
        walk_matches.dedup();

        assert_eq!(
            match_strs, walk_matches,
            "prefix_matches for {prefix:?} must agree with the walk"
        );
    }

    let empty = index::prefix_matches(&repo, "zzzzzz").expect("prefix_matches");
    assert!(
        empty.is_empty(),
        "a prefix matching nothing must return an empty vec"
    );
}

#[test]
fn racing_captures_leave_a_usable_index() {
    let fx = Fixture::new();
    build_log(&fx, 3);
    let repo_path = fx.path();

    scope(|s| {
        for thread_id in 0..2 {
            let tpath = repo_path.clone();
            s.spawn(move || {
                let thread_repo = ff_core::discover_isolated(&tpath).expect("discover");
                for round in 0..5 {
                    let filename = format!("thread{thread_id}_round{round}.txt");
                    std::fs::write(
                        tpath.join(&filename),
                        format!("thread {thread_id} round {round}"),
                    )
                    .expect("write file");
                    let _ = ff_core::capture(
                        &thread_repo,
                        &Provenance::new("racer", Some(format!("thread-{thread_id}"))),
                    );
                }
            });
        }
    });

    let final_repo = fx.repo();
    let live_ids = log_ids(&final_repo, Domain::Live);

    // The invariant underneath the index one, and the reason this test used
    // to fail about once in fifty. Every position the ref has held must be
    // on the walk from the tip: an append that reported success and then got
    // written over is an operation the reflog reaches and the log does not,
    // which is a lost write wearing a rewind's clothes. Assert it before the
    // index, because the index disagreeing is only how it happened to show.
    let reflog = fx.git(&["reflog", "show", OPS_REF, "--format=%H"]);
    let strays: Vec<&str> = reflog
        .lines()
        .filter(|line| !line.is_empty() && !live_ids.iter().any(|id| id == line))
        .collect();
    assert!(
        strays.is_empty(),
        "operations the reflog reaches and the walk does not: {strays:#?}\nreflog:\n{reflog}\nwalk:\n{}",
        live_ids.join("\n")
    );

    let lens = index::prefix_lens(&final_repo, &live_ids).expect("prefix_lens");
    let ref_lens = reference(&final_repo);
    for id in &live_ids {
        assert_eq!(
            lens.get(id),
            ref_lens.get(id),
            "after racing captures, {id}"
        );
    }
}
