//! Id-index integration tests: the materialized snapshot-id domain must
//! reproduce the walk-based answer in every case, and self-heal when corrupted.

use std::collections::HashMap;
use std::thread::scope;

use ff_core::{Provenance, SnapOutcome, TakeOptions};
use ff_testsupport::Fixture;

/// The answer the whole chain gives when walked: sort the live+trash ids and
/// give each one a character more than its longest common prefix with either
/// neighbour. This is `render::unique_prefix_lens` over `ref_ids`, and it is
/// what the index must reproduce without the walk.
fn reference(repo: &gix::Repository, chain: &str) -> HashMap<String, usize> {
    let live_ref = format!("refs/fufu/snap/{chain}");
    let trash_ref = format!("refs/fufu/trash/{chain}");
    let mut all_ids = Vec::new();
    if let Ok(ids) = ff_core::ref_ids(repo, &live_ref) {
        all_ids.extend(ids);
    }
    if let Ok(ids) = ff_core::ref_ids(repo, &trash_ref) {
        all_ids.extend(ids);
    }
    let mut sorted: Vec<&str> = all_ids.iter().map(String::as_str).collect();
    sorted.sort_unstable();
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

fn take_snapshot(fx: &Fixture) -> SnapOutcome {
    let repo = fx.repo();
    ff_core::take(&repo, &Provenance::new("manual", None)).expect("take")
}

fn take_created(fx: &Fixture) -> String {
    match take_snapshot(fx) {
        SnapOutcome::Created { id, .. } => id,
        other => panic!("expected Created, got {other:?}"),
    }
}

/// Helper: build a fixture with N snapshots and return the live ids.
fn build_chain(fx: &Fixture, n: usize) -> Vec<String> {
    let mut ids = Vec::new();
    for i in 0..n {
        fx.write(&format!("file_{i}.txt"), &format!("content {i}"));
        ids.push(take_created(fx));
    }
    ids
}

#[test]
fn builds_on_first_read_and_matches_the_walk() {
    let fx = Fixture::new();
    let chain = "main";
    let ids = build_chain(&fx, 12);

    let repo = fx.repo();
    let lens = ff_core::idindex::prefix_lens(&repo, chain, &ids).expect("prefix_lens");
    let ref_lens = reference(&repo, chain);

    assert_eq!(
        lens, ref_lens,
        "prefix_lens must match the walk-based reference"
    );

    // The index file now exists and has a legal layout.
    let idx_path = ff_core::idindex::path(&repo, chain, ff_core::idindex::Kind::Live);
    assert!(
        idx_path.exists(),
        "live index file must exist after first read"
    );
    let len = std::fs::metadata(&idx_path).expect("metadata").len() as usize;
    assert!(
        (len - 50).is_multiple_of(41),
        "file length {len} must satisfy (len - 50) %% 41 == 0"
    );
}

#[test]
fn append_extends_the_index() {
    let fx = Fixture::new();
    let chain = "main";
    let ids = build_chain(&fx, 5);

    let repo = fx.repo();

    // First read builds the index.
    let _lens = ff_core::idindex::prefix_lens(&repo, chain, &ids).expect("prefix_lens");
    let idx_path = ff_core::idindex::path(&repo, chain, ff_core::idindex::Kind::Live);
    let len_before = std::fs::metadata(&idx_path).expect("metadata").len();

    // Get the current tip and take a new snapshot.
    let tip_before = ff_core::snapshot::chain::tip(&repo, &format!("refs/fufu/snap/{chain}"))
        .expect("tip")
        .expect("tip exists");
    fx.write("new_file.txt", "new content");
    let new_id = take_created(&fx);
    let tip_after = ff_core::snapshot::chain::tip(&repo, &format!("refs/fufu/snap/{chain}"))
        .expect("tip")
        .expect("tip exists");

    // Append via record.
    ff_core::idindex::record(&repo, chain, Some(tip_before), tip_after);

    // File grew by exactly 41 bytes.
    let len_after = std::fs::metadata(&idx_path).expect("metadata").len();
    assert_eq!(
        len_after - len_before,
        41,
        "file must grow by exactly RECORD_LEN (41) bytes"
    );

    // Header tip equals new tip.
    let contents = std::fs::read(&idx_path).expect("read index");
    let header_tip = String::from_utf8_lossy(&contents[0..40]).to_string();
    assert_eq!(header_tip, new_id, "header tip must equal new snapshot id");

    // prefix_lens still matches reference.
    let all_ids: Vec<String> = ids
        .iter()
        .chain(std::iter::once(&new_id))
        .cloned()
        .collect();
    let lens = ff_core::idindex::prefix_lens(&repo, chain, &all_ids).expect("prefix_lens");
    let ref_lens = reference(&repo, chain);
    assert_eq!(
        lens, ref_lens,
        "prefix_lens after append must match reference"
    );
}

#[test]
fn a_stale_header_is_never_extended() {
    let fx = Fixture::new();
    let chain = "main";
    let ids = build_chain(&fx, 3);

    let repo = fx.repo();
    let _lens = ff_core::idindex::prefix_lens(&repo, chain, &ids).expect("prefix_lens");

    let idx_path = ff_core::idindex::path(&repo, chain, ff_core::idindex::Kind::Live);
    let mut contents = std::fs::read(&idx_path).expect("read index");
    let _original_contents = contents.clone();

    // Corrupt the header tip by flipping one hex character.
    contents[0] = if contents[0] == b'0' { b'1' } else { b'0' };
    std::fs::write(&idx_path, &contents).expect("write corrupted header");

    // Take a new snapshot to get prev/new ids.
    fx.write("stale_test.txt", "stale");
    let prev_tip = ff_core::snapshot::chain::tip(&repo, &format!("refs/fufu/snap/{chain}"))
        .expect("tip")
        .expect("tip exists");
    let new_id = take_created(&fx);
    let new_tip = ff_core::snapshot::chain::tip(&repo, &format!("refs/fufu/snap/{chain}"))
        .expect("tip")
        .expect("tip exists");

    // record must decline to append to a stale file.
    ff_core::idindex::record(&repo, chain, Some(prev_tip), new_tip);

    // File is byte-for-byte unchanged (the append declined).
    let after_contents = std::fs::read(&idx_path).expect("read index");
    assert_eq!(
        contents, after_contents,
        "record must not modify a stale index file"
    );

    // prefix_lens rebuilds and still matches reference.
    let all_ids: Vec<String> = ids
        .iter()
        .chain(std::iter::once(&new_id))
        .cloned()
        .collect();
    let lens = ff_core::idindex::prefix_lens(&repo, chain, &all_ids).expect("prefix_lens");
    let ref_lens = reference(&repo, chain);
    assert_eq!(
        lens, ref_lens,
        "prefix_lens after rebuild must match reference"
    );

    // Header now carries the real tip.
    let rebuilt = std::fs::read(&idx_path).expect("read rebuilt index");
    let header_tip = String::from_utf8_lossy(&rebuilt[0..40]).to_string();
    assert_eq!(header_tip, new_id, "rebuilt header must carry current tip");
}

#[test]
fn the_tail_merges_once_it_is_long() {
    let fx = Fixture::new();
    let chain = "main";
    let ids = build_chain(&fx, 3);

    let repo = fx.repo();
    let tip_id = ff_core::snapshot::chain::tip(&repo, &format!("refs/fufu/snap/{chain}"))
        .expect("tip")
        .expect("tip exists");
    let tip_str = tip_id.to_string();

    // Hand-write an index file with base=0 and 600 synthetic records.
    let idx_path = ff_core::idindex::path(&repo, chain, ff_core::idindex::Kind::Live);
    let parent = idx_path.parent().unwrap();
    std::fs::create_dir_all(parent).expect("create dir");

    let mut body = String::new();
    body.push_str(&tip_str);
    body.push(' ');
    body.push_str(&format!("{:08x}", 0usize)); // base = 0
    body.push('\n');

    // 600 synthetic 40-hex records (unsorted to test merge sorting).
    for i in (0..600).rev() {
        body.push_str(&format!("{:040x}", i));
        body.push('\n');
    }

    std::fs::write(&idx_path, &body).expect("write synthetic index");

    // Call prefix_lens — this triggers merge_if_due (600 > 512).
    let _lens = ff_core::idindex::prefix_lens(&repo, chain, &ids).expect("prefix_lens");

    // Check the on-disk header: base count now equals total.
    let contents = std::fs::read(&idx_path).expect("read index");
    let total = (contents.len() - 50) / 41;
    let base_hex = String::from_utf8_lossy(&contents[41..49]);
    let base = usize::from_str_radix(&base_hex, 16).expect("parse base");
    assert_eq!(base, total, "base must equal total after merge");

    // Records are sorted ascending.
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
    let chain = "main";
    let ids = build_chain(&fx, 5);

    let repo = fx.repo();
    let _lens = ff_core::idindex::prefix_lens(&repo, chain, &ids).expect("prefix_lens");
    let idx_path = ff_core::idindex::path(&repo, chain, ff_core::idindex::Kind::Live);

    // Sub-case 1: truncate the file by one byte.
    {
        let contents = std::fs::read(&idx_path).expect("read");
        std::fs::write(&idx_path, &contents[..contents.len() - 1]).expect("truncate");
        let lens = ff_core::idindex::prefix_lens(&repo, chain, &ids).expect("prefix_lens");
        let ref_lens = reference(&repo, chain);
        assert_eq!(lens, ref_lens, "truncate: prefix_lens must match reference");
        let len = std::fs::metadata(&idx_path).expect("metadata").len() as usize;
        assert!(
            (len - 50).is_multiple_of(41),
            "truncate: file layout must be legal"
        );
    }

    // Sub-case 2: corrupt the header tip.
    {
        let contents = std::fs::read(&idx_path).expect("read");
        let mut corrupted = contents;
        corrupted[5] = if corrupted[5] == b'0' { b'1' } else { b'0' };
        std::fs::write(&idx_path, &corrupted).expect("corrupt");
        let lens = ff_core::idindex::prefix_lens(&repo, chain, &ids).expect("prefix_lens");
        let ref_lens = reference(&repo, chain);
        assert_eq!(
            lens, ref_lens,
            "corrupt tip: prefix_lens must match reference"
        );
        let rebuilt = std::fs::read(&idx_path).expect("read rebuilt");
        let tip_str = String::from_utf8_lossy(&rebuilt[0..40]).to_string();
        let current_tip = ff_core::snapshot::chain::tip(&repo, &format!("refs/fufu/snap/{chain}"))
            .expect("tip")
            .expect("tip exists")
            .to_string();
        assert_eq!(
            tip_str, current_tip,
            "rebuilt header tip must match current tip"
        );
    }

    // Sub-case 3: delete the file.
    {
        std::fs::remove_file(&idx_path).expect("delete");
        let lens = ff_core::idindex::prefix_lens(&repo, chain, &ids).expect("prefix_lens");
        let ref_lens = reference(&repo, chain);
        assert_eq!(lens, ref_lens, "delete: prefix_lens must match reference");
        assert!(idx_path.exists(), "file must be recreated");
        let len = std::fs::metadata(&idx_path).expect("metadata").len() as usize;
        assert!(
            (len - 50).is_multiple_of(41),
            "delete: file layout must be legal"
        );
    }

    // Sub-case 4: move the chain ref by hand to an older snapshot.
    {
        // ids are newest-first, so the last one is oldest.
        let older_id = &ids[ids.len() - 1];
        fx.git(&["update-ref", &format!("refs/fufu/snap/{chain}"), older_id]);
        let new_ids = ff_core::ref_ids(&repo, &format!("refs/fufu/snap/{chain}")).expect("ref_ids");
        let lens = ff_core::idindex::prefix_lens(&repo, chain, &new_ids).expect("prefix_lens");
        let ref_lens = reference(&repo, chain);
        assert_eq!(
            lens, ref_lens,
            "ref move: prefix_lens must match reference for current chain"
        );
        let contents = std::fs::read(&idx_path).expect("read");
        let tip_str = String::from_utf8_lossy(&contents[0..40]).to_string();
        assert_eq!(tip_str, *older_id, "header tip must match the moved ref");
        let len = std::fs::read(&idx_path).expect("read").len();
        assert!(
            (len - 50).is_multiple_of(41),
            "file layout must be legal after ref move"
        );
    }
}

#[test]
fn survives_a_trim_rewrite() {
    let fx = Fixture::new();
    let chain = "main";

    // Take snapshots with explicit timestamps so some fall outside the keep window.
    let repo = fx.repo();
    let base_time = 1_700_000_000i64; // ~Nov 2023
    let mut snap_ids = Vec::new();

    for i in 0..8 {
        fx.write(&format!("trim_file_{i}.txt"), &format!("trim content {i}"));
        let now = base_time + i * 86400 * 5; // 5 days apart
        let outcome = ff_core::take_with(
            &repo,
            &Provenance::new("manual", None),
            &TakeOptions {
                now: Some(now),
                ..TakeOptions::default()
            },
        )
        .expect("take_with");
        if let SnapOutcome::Created { id, .. } = outcome {
            snap_ids.push(id);
        } else {
            panic!("expected Created, got {outcome:?}");
        }
    }

    // Trim with keep_secs that drops the oldest few (30 days = drops ~5 of 8).
    let trim_opts = ff_core::TrimOptions {
        now: Some(base_time + 86400 * 5 * 7), // time of last snapshot
        keep_secs: Some(30 * 86400),
        ..ff_core::TrimOptions::default()
    };
    let report = ff_core::trim(&repo, &trim_opts).expect("trim");
    assert!(
        report.chains.iter().any(|c| c.dropped > 0),
        "trim should have dropped some snapshots"
    );

    // Get the current live ids after trim.
    let live_ids =
        ff_core::ref_ids(&repo, &format!("refs/fufu/snap/{chain}")).expect("live ref_ids");
    assert!(
        live_ids.len() >= 2,
        "at least two snapshots must survive trim"
    );

    // At least one trash id exists.
    let trash_ids =
        ff_core::ref_ids(&repo, &format!("refs/fufu/trash/{chain}")).expect("trash ref_ids");
    assert!(
        !trash_ids.is_empty(),
        "trash chain must have ids after trim"
    );

    // Collect all ids from both chains and compute prefix_lens.
    let all_ids: Vec<String> = live_ids.iter().chain(trash_ids.iter()).cloned().collect();
    let lens = ff_core::idindex::prefix_lens(&repo, chain, &all_ids).expect("prefix_lens");

    // Build the reference over the exact same sorted id set (deduplicated).
    let mut unique_ids: Vec<String> = all_ids.to_vec();
    unique_ids.sort_unstable();
    unique_ids.dedup();
    let sorted: Vec<&str> = unique_ids.iter().map(String::as_str).collect();
    let common = |a: &str, b: &str| a.bytes().zip(b.bytes()).take_while(|(x, y)| x == y).count();
    let mut ref_lens = HashMap::new();
    for (i, id) in sorted.iter().enumerate() {
        let prev = if i > 0 { common(sorted[i - 1], id) } else { 0 };
        let next = if i + 1 < sorted.len() {
            common(id, sorted[i + 1])
        } else {
            0
        };
        ref_lens.insert(id.to_string(), (prev.max(next) + 1).min(id.len().max(1)));
    }

    for id in &unique_ids {
        let idx_len = lens
            .get(id)
            .unwrap_or_else(|| panic!("index missed id {id}"));
        let ref_len = ref_lens
            .get(id)
            .unwrap_or_else(|| panic!("reference missed id {id}"));
        assert_eq!(
            idx_len, ref_len,
            "prefix length mismatch for {id}: index={}, ref={}",
            idx_len, ref_len
        );
    }

    // Both live and trash index files exist after prefix_lens creates them.
    let live_path = ff_core::idindex::path(&repo, chain, ff_core::idindex::Kind::Live);
    let trash_path = ff_core::idindex::path(&repo, chain, ff_core::idindex::Kind::Trash);
    assert!(
        live_path.exists(),
        "live index file must exist after prefix_lens"
    );
    assert!(
        trash_path.exists(),
        "trash index file must exist after prefix_lens"
    );

    // At least one trash id must have a prefix length.
    assert!(
        trash_ids.iter().any(|id| lens.contains_key(id)),
        "at least one trash id must have a prefix length"
    );
}

#[test]
fn prefix_matches_agrees_with_the_walk() {
    let fx = Fixture::new();
    let chain = "main";
    let _live_ids = build_chain(&fx, 8);

    let repo = fx.repo();

    // Build reference: walk both live and trash, collect ids matching each prefix.
    let live_walk = ff_core::ref_ids(&repo, &format!("refs/fufu/snap/{chain}")).expect("live walk");
    let trash_walk =
        ff_core::ref_ids(&repo, &format!("refs/fufu/trash/{chain}")).unwrap_or_default();

    // Test each id's first 6 hex chars as a prefix.
    let all_walk_ids: Vec<String> = live_walk.iter().chain(trash_walk.iter()).cloned().collect();
    for id in &all_walk_ids {
        let prefix = &id[..6];
        let matches = ff_core::idindex::prefix_matches(&repo, chain, prefix)
            .expect("prefix_matches")
            .expect("must return Some");
        let match_strs: Vec<String> = matches.iter().map(|o| o.to_string()).collect();

        // Walk-based: collect all ids starting with prefix.
        let walk_matches: Vec<String> = all_walk_ids
            .iter()
            .filter(|i| i.starts_with(prefix))
            .cloned()
            .collect();

        assert_eq!(
            match_strs, walk_matches,
            "prefix_matches for {:?} must agree with walk",
            prefix
        );
    }

    // Test a prefix that matches nothing.
    let empty = ff_core::idindex::prefix_matches(&repo, chain, "zzzzzz")
        .expect("prefix_matches")
        .expect("must return Some");
    assert!(
        empty.is_empty(),
        "prefix matching nothing must return empty vec"
    );
}

#[test]
fn racing_captures_leave_a_usable_index() {
    let fx = Fixture::new();
    let chain = "main";

    // Build an initial chain so the index file exists.
    let _ids = build_chain(&fx, 3);

    let repo_path = fx.path();

    // Two threads, each doing ~5 rounds of capture.
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
                    let _ = ff_core::take(
                        &thread_repo,
                        &Provenance::new("racer", Some(format!("thread-{thread_id}"))),
                    );
                }
            });
        }
    });

    // After contention, the index must still be usable and match the reference.
    let final_repo = fx.repo();
    let live_ids =
        ff_core::ref_ids(&final_repo, &format!("refs/fufu/snap/{chain}")).expect("ref_ids");
    let lens = ff_core::idindex::prefix_lens(&final_repo, chain, &live_ids).expect("prefix_lens");
    let ref_lens = reference(&final_repo, chain);
    assert_eq!(
        lens, ref_lens,
        "prefix_lens after racing captures must match reference"
    );
}
