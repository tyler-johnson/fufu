use super::*;
use crate::ops::message::SegmentLink;
use crate::ops::{CaptureOutcome, capture_with};
use crate::snapshot::{Provenance, TakeOptions};
use crate::trim::{self, TrimOptions};
use ff_testsupport::Fixture;

fn take_created(fx: &Fixture) -> String {
    let repo = fx.repo();
    match capture_with(
        &repo,
        &Provenance::new("manual", None),
        &TakeOptions::default(),
    )
    .expect("capture")
    {
        CaptureOutcome::Created { id, .. } => id.hex(),
        other => panic!("expected Created, got {other:?}"),
    }
}

/// A capture backdated `days_ago` from `now`, for trim fixtures.
fn snap_at(fx: &Fixture, now: i64, days_ago: i64) -> String {
    let repo = fx.repo();
    match capture_with(
        &repo,
        &Provenance::new("manual", None),
        &TakeOptions {
            now: Some(now - days_ago * 86_400),
            max_file_size: None,
        },
    )
    .expect("capture")
    {
        CaptureOutcome::Created { id, .. } => id.hex(),
        other => panic!("expected Created, got {other:?}"),
    }
}

fn branch_name(repo: &gix::Repository) -> String {
    chain::chain_name(&crate::head::head_state(repo).unwrap())
}

fn decode(repo: &gix::Repository, id: &str) -> SnapDecode {
    snap_entry(repo, gix::ObjectId::from_hex(id.as_bytes()).unwrap())
        .unwrap()
        .expect("id decodes as an operation")
}

/// The pre-skip-link algorithm, kept here only as a correctness oracle: a full
/// linear walk with no hopping and no cap, exactly what `segment_anchors` did
/// before the segment pointer existed. The new walk must agree with this on
/// every id it manages to reach within its scan budget — this is the property
/// under test throughout this module.
fn full_sweep(repo: &gix::Repository, commit_ids: &[String]) -> HashMap<String, String> {
    let mut result = HashMap::new();
    if commit_ids.is_empty() {
        return result;
    }
    let mut wanted: HashMap<(Option<String>, gix::ObjectId), Vec<&str>> = HashMap::new();
    let mut floor: Option<i64> = Some(i64::MAX);
    for id in commit_ids {
        let oid = gix::ObjectId::from_hex(id.as_bytes()).unwrap();
        let commit = repo.find_commit(oid).unwrap();
        let parent = commit.parent_ids().next().map(|p| p.detach());
        let tree = commit.tree_id().unwrap().detach();
        let base_time = parent
            .and_then(|p| repo.find_commit(p).ok())
            .and_then(|base| base.time().ok())
            .map(|time| time.seconds);
        floor = match base_time {
            Some(seconds) => floor.map(|f| f.min(seconds)),
            None => None,
        };
        wanted
            .entry((parent.map(|p| p.to_string()), tree))
            .or_default()
            .push(id.as_str());
    }
    let floor = floor.unwrap_or(i64::MIN);

    let mut anchor = |decoded: SnapDecode| {
        if let Some(ids) = wanted.get(&(decoded.entry.base.clone(), decoded.tree)) {
            for id in ids {
                result
                    .entry((*id).to_string())
                    .or_insert_with(|| decoded.entry.id.clone());
            }
        }
        result.len() < commit_ids.len() && decoded.entry.time >= floor
    };
    walk_captures(repo, &branch_name(repo), &mut anchor).unwrap();
    result
}

#[test]
fn skip_link_matches_full_sweep_across_segments_and_a_root() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    let c0 = fx.commit("init"); // root: no base at all
    fx.write("a.txt", "one\n");
    take_created(&fx);
    fx.write("a.txt", "two\n");
    let s2 = take_created(&fx); // segment 1 (base c0): s1, s2
    let c1 = fx.commit("second"); // tree == s2's tree, parent == c0
    fx.write("a.txt", "three\n");
    take_created(&fx);
    fx.write("a.txt", "four\n");
    let s4 = take_created(&fx); // segment 2 (base c1): s3, s4
    let c2 = fx.commit("third"); // tree == s4's tree, parent == c1
    fx.write("a.txt", "five\n");
    let s5 = take_created(&fx); // segment 3 (base c2): s5 alone
    let c3 = fx.commit("fourth"); // tree == s5's tree, parent == c2

    let repo = fx.repo();
    let ids = vec![c0.clone(), c1.clone(), c2.clone(), c3.clone()];
    let fast = segment_anchors(&repo, &ids).unwrap();
    let slow = full_sweep(&repo, &ids);
    assert_eq!(fast, slow, "skip-link walk must agree with the full sweep");

    assert_eq!(fast.get(&c1), Some(&s2));
    assert_eq!(fast.get(&c2), Some(&s4));
    assert_eq!(fast.get(&c3), Some(&s5));
    assert!(
        !fast.contains_key(&c0),
        "a root commit has no base to match against"
    );
}

#[test]
fn segment_pointer_assignment_follows_the_rule() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    fx.write("a.txt", "one\n");
    let s1 = take_created(&fx);
    fx.write("a.txt", "two\n");
    let s2 = take_created(&fx);
    fx.commit("second"); // base moves: next capture opens a new segment
    fx.write("a.txt", "three\n");
    let s3 = take_created(&fx);
    fx.write("a.txt", "four\n");
    let s4 = take_created(&fx);

    let repo = fx.repo();
    assert_eq!(
        decode(&repo, &s1).segment_prev,
        Some(SegmentLink::ChainStart),
        "the first operation of a log declares itself chain start"
    );
    assert_eq!(
        decode(&repo, &s2).segment_prev,
        Some(SegmentLink::ChainStart),
        "same segment as s1: copies its ChainStart verbatim"
    );
    let s3_decoded = decode(&repo, &s3);
    assert_eq!(
        s3_decoded.segment_prev,
        Some(SegmentLink::At(
            gix::ObjectId::from_hex(s2.as_bytes()).unwrap()
        )),
        "a fresh segment points at prev itself"
    );
    let s4_decoded = decode(&repo, &s4);
    assert_eq!(
        s4_decoded.segment_prev, s3_decoded.segment_prev,
        "same segment as s3: copies its pointer verbatim"
    );
}

#[test]
fn raw_git_commit_without_a_capture_has_no_anchor() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    fx.write("a.txt", "one\n");
    let s1 = take_created(&fx);
    let c1 = fx.commit("second"); // tree == s1's tree, parent == c0: has an anchor
    fx.write("a.txt", "two\n");
    let c2 = fx.commit("third"); // straight through git: no capture matches it

    let repo = fx.repo();
    let ids = vec![c1.clone(), c2.clone()];
    let fast = segment_anchors(&repo, &ids).unwrap();
    let slow = full_sweep(&repo, &ids);
    assert_eq!(fast, slow);
    assert_eq!(fast.get(&c1), Some(&s1));
    assert!(
        !fast.contains_key(&c2),
        "no capture ever recorded c2's content"
    );
}

/// Rewrite the oldest `n` operations on a branch to drop their segment pointer
/// trailer — simulating a log whose earliest history predates this feature —
/// relinking `fufu-prev`, `fufu-prev-branch` and parent 1 through every
/// operation above them so the log stays one valid history. Returns the
/// original id -> rewritten id map for every operation, since content
/// addressing means a relinked parent changes a commit's sha even where its
/// own message does not.
fn strip_pointers_from_oldest(
    repo: &gix::Repository,
    branch: &str,
    n: usize,
) -> HashMap<String, gix::ObjectId> {
    let newest_first = ref_ids(repo, branch).unwrap();
    let mut mapping = HashMap::new();
    let mut prev_new: Option<gix::ObjectId> = None;
    for (i, old_hex) in newest_first.iter().rev().enumerate() {
        let old_id = gix::ObjectId::from_hex(old_hex.as_bytes()).unwrap();
        let obj = repo.find_object(old_id).unwrap();
        let commit_ref = gix::objs::CommitRef::from_bytes(&obj.data).unwrap();
        let mut commit: gix::objs::Commit = commit_ref.into();
        drop(obj);
        if let Some(p) = prev_new
            && !commit.parents.is_empty()
        {
            commit.parents[0] = p;
        }
        let text = String::from_utf8_lossy(commit.message.as_ref()).into_owned();
        let mut skeleton = crate::ops::message::parse(&text).unwrap();
        skeleton.prev = prev_new;
        skeleton.prev_on_branch = prev_new;
        let mut msg = crate::ops::message::rebuild(&text, &skeleton);
        if i < n {
            // A message with no `fufu-prev-segment` key at all: absent means
            // "written before the link existed", which is what this simulates.
            msg = msg
                .lines()
                .filter(|line| !line.starts_with("fufu-prev-segment:"))
                .map(|line| format!("{line}\n"))
                .collect();
        }
        commit.message = msg.into();
        let new_id = repo.write_object(&commit).unwrap().detach();
        mapping.insert(old_hex.clone(), new_id);
        prev_new = Some(new_id);
    }
    let tip = prev_new.expect("the log is non-empty");
    for name in [
        crate::ops::OPS_REF.to_string(),
        format!("{}{branch}", crate::ops::BRANCH_PREFIX),
    ] {
        crate::refs::write_ref(
            repo,
            &name,
            tip,
            gix::refs::transaction::PreviousValue::Any,
            0,
            "test: simulate a pre-pointer log",
        )
        .unwrap();
    }
    mapping
}

#[test]
fn mixed_log_pointerless_prefix_falls_back_and_still_finds_anchors() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    fx.write("a.txt", "one\n");
    take_created(&fx);
    fx.write("a.txt", "two\n");
    let s2 = take_created(&fx); // segment 1 (base c0)
    let c1 = fx.commit("second"); // tree == s2's tree
    fx.write("a.txt", "three\n");
    take_created(&fx);
    fx.write("a.txt", "four\n");
    let s4 = take_created(&fx); // segment 2 (base c1)
    let c2 = fx.commit("third"); // tree == s4's tree

    let repo = fx.repo();
    let branch = branch_name(&repo);

    // Strip every capture so far — the whole of segments 1 and 2 now looks
    // like it was written before this feature existed.
    let mapping = strip_pointers_from_oldest(&repo, &branch, 4);
    let new_s2 = mapping[&s2];
    let new_s4 = mapping[&s4];

    // Capture more, normally, on top of the rewritten prefix.
    fx.write("a.txt", "five\n");
    take_created(&fx);
    fx.write("a.txt", "six\n");
    let s6 = take_created(&fx); // segment 3 (base c2), pointer -> new_s4
    let c3 = fx.commit("fourth"); // tree == s6's tree

    let repo = fx.repo();
    let ids = vec![c1.clone(), c2.clone(), c3.clone()];
    let fast = segment_anchors(&repo, &ids).unwrap();
    let slow = full_sweep(&repo, &ids);
    assert_eq!(fast, slow, "must agree even where the walk falls back");
    assert_eq!(fast.get(&c1), Some(&new_s2.to_string()));
    assert_eq!(fast.get(&c2), Some(&new_s4.to_string()));
    assert_eq!(fast.get(&c3), Some(&s6));

    // The newer half still carries a pointer; the rewritten half does not —
    // a genuine mixed log, healing from the tip down.
    assert!(decode(&repo, &s6).segment_prev.is_some());
    assert_eq!(decode(&repo, &new_s4.to_string()).segment_prev, None);
    assert_eq!(decode(&repo, &new_s2.to_string()).segment_prev, None);
}

#[test]
fn anchors_after_trim_relink_or_drop_the_pointer() {
    const NOW: i64 = 1_700_000_000;
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    fx.write("a.txt", "one\n");
    snap_at(&fx, NOW, 100); // base c0 — will be trimmed away (> 90d)
    let c1 = fx.commit("second"); // tree == "one"
    fx.write("a.txt", "two\n");
    snap_at(&fx, NOW, 5); // base c1 — kept; boundary trailer -> the 100d op
    let c2 = fx.commit("third"); // tree == "two"
    fx.write("a.txt", "three\n");
    snap_at(&fx, NOW, 4); // base c2 — kept; boundary trailer -> the 5d op
    let c3 = fx.commit("fourth"); // tree == "three"

    let repo = fx.repo();
    trim::trim(
        &repo,
        &TrimOptions {
            now: Some(NOW),
            dry_run: false,
            gone: false,
            keep_secs: None, // fufu.keep default: 90 days
        },
    )
    .expect("trim");

    let branch = branch_name(&repo);
    let survivors = ref_ids(&repo, &branch).unwrap(); // newest first
    assert_eq!(survivors.len(), 2, "the 100d capture alone was dropped");
    let new_top = survivors[0].clone(); // was the 4d capture
    let new_mid = survivors[1].clone(); // was the 5d capture

    let ids = vec![c1.clone(), c2.clone(), c3.clone()];
    let fast = segment_anchors(&repo, &ids).unwrap();
    let slow = full_sweep(&repo, &ids);
    assert_eq!(fast, slow);

    assert!(
        !fast.contains_key(&c1),
        "its only anchor was trimmed off the live log"
    );
    assert_eq!(fast.get(&c2), Some(&new_mid));
    assert_eq!(fast.get(&c3), Some(&new_top));

    // The surviving mid operation's trailer named the now-gone 100d one:
    // dropped to ChainStart, not left dangling. The surviving top one's
    // trailer named the mid one, which did survive: relinked.
    assert_eq!(
        decode(&repo, &new_mid).segment_prev,
        Some(SegmentLink::ChainStart)
    );
    assert_eq!(
        decode(&repo, &new_top).segment_prev,
        Some(SegmentLink::At(
            gix::ObjectId::from_hex(new_mid.as_bytes()).unwrap()
        ))
    );
}

#[test]
fn single_segment_log_stops_at_chain_start_sentinel() {
    // A long single-segment log whose base no displayed commit wants: the walk
    // should stop at the ChainStart sentinel without burning the linear budget
    // on pointless decodes.
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");

    // More than SEGMENT_SCAN_CAP so the old behavior would hit the cap.
    for i in 0..600 {
        fx.write("a.txt", format!("v{i}\n").as_str());
        take_created(&fx);
    }

    fx.write("a.txt", "final\n");
    let c_final = fx.commit("final");
    fx.write("a.txt", "after\n");
    let _s_after = take_created(&fx);

    let repo = fx.repo();
    let ids = vec![c_final.clone()];
    let fast = segment_anchors(&repo, &ids).unwrap();
    let slow = full_sweep(&repo, &ids);
    assert_eq!(fast, slow, "anchors must match the oracle");
}

#[test]
fn mixed_log_pointerless_prefix_with_chain_start_still_degrades_safely() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    fx.write("a.txt", "one\n");
    take_created(&fx);
    fx.write("a.txt", "two\n");
    let s2 = take_created(&fx); // segment 1 (base c0), ChainStart
    let c1 = fx.commit("second"); // tree == s2's tree
    fx.write("a.txt", "three\n");
    take_created(&fx);
    fx.write("a.txt", "four\n");
    let s4 = take_created(&fx); // segment 2 (base c1), At(s2)
    let c2 = fx.commit("third"); // tree == s4's tree

    let repo = fx.repo();
    let branch = branch_name(&repo);

    let mapping = strip_pointers_from_oldest(&repo, &branch, 2);
    let new_s2 = mapping[&s2];
    let new_s4 = mapping[&s4];

    let new_s4_decoded = decode(&repo, &new_s4.to_string());
    assert!(
        matches!(new_s4_decoded.segment_prev, Some(SegmentLink::At(_))),
        "segment 2 still has an At pointer"
    );

    let repo = fx.repo();
    let ids = vec![c1.clone(), c2.clone()];
    let fast = segment_anchors(&repo, &ids).unwrap();
    let slow = full_sweep(&repo, &ids);
    assert_eq!(fast, slow, "must agree even with a pointerless prefix");
    assert_eq!(fast.get(&c1), Some(&new_s2.to_string()));
    assert_eq!(fast.get(&c2), Some(&new_s4.to_string()));
}

/// A verb operation plans exactly the state a close creates — base = the old
/// HEAD, tree = the tree about to be committed — so it matches the same key
/// the pre-close capture does, and it is newer. Only captures may answer, or
/// the anchor column would name the operation that wrote the commit down
/// instead of the moment the content existed.
#[test]
fn a_verb_operation_never_shadows_the_capture_it_followed() {
    let fx = Fixture::new();
    fx.set_config("user.name", "Anchor");
    fx.set_config("user.email", "anchor@test");
    fx.write("a.txt", "a\n");
    fx.commit("init");
    fx.write("a.txt", "landing\n");

    let repo = fx.repo();
    let (_outcome, _ctx) = crate::close::close(
        &repo,
        &crate::close::CloseOptions {
            message: Some("landed".into()),
            ..Default::default()
        },
        &Provenance::new("pre", Some("ff commit".into())),
    )
    .expect("close");
    let landed = crate::refs::ref_target(&repo, "refs/heads/main")
        .unwrap()
        .unwrap()
        .to_string();

    let anchors = segment_anchors(&repo, std::slice::from_ref(&landed)).unwrap();
    let anchor = anchors.get(&landed).expect("the close has an anchor");
    let decoded = decode(&repo, anchor);
    assert!(
        decoded.is_capture,
        "the anchor must be the capture, not the commit operation that followed it"
    );
}
