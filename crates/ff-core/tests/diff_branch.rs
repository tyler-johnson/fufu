//! Branch verbs: the naming rename must preserve the timeline byte-for-byte
//! (modulo the name), match `git branch -m`'s observable reflog semantics,
//! carry the open commit and metadata, and guard worktrees. Delete trashes
//! the chain and leaves the open commit pinned — never loses work.

use ff_core::SwitchOptions;
use ff_core::gix;
use ff_testsupport::Fixture;

/// Park `branch`'s dirty tree by switching to `away` and back to nowhere:
/// the open commit is the park. HEAD ends on `away`.
fn park_by_switching(fx: &Fixture, away: &str, now: i64) -> String {
    let repo = fx.repo();
    let (report, _) = ff_core::switch(
        &repo,
        &SwitchOptions {
            target: Some(away.into()),
            now: Some(now),
            argv: Vec::new(),
            ..Default::default()
        },
        &prov(),
    )
    .unwrap();
    report.parked.expect("a dirty tree parks")
}

/// The newest operation's record, read through the public reader.
fn tip_record(repo: &gix::Repository) -> ff_core::ops::OpRecord {
    let log = ff_core::ops::OpLog::open(repo).unwrap();
    let op = log.get(log.tip().unwrap().unwrap()).unwrap();
    op.record()
        .unwrap()
        .cloned()
        .expect("a verb op has a record")
}

const NOW: i64 = 1_700_000_000;

fn ident(fx: &Fixture) {
    fx.set_config("user.name", "Branch User");
    fx.set_config("user.email", "branch@test");
}

fn prov() -> ff_core::Provenance {
    ff_core::Provenance::new("pre", Some("ff describe".into()))
}

#[test]
fn rename_matches_git_branch_m_reflog_and_target() {
    let make = || {
        let fx = Fixture::new();
        fx.write("a.txt", "a\n");
        fx.commit("one");
        fx.write("a.txt", "b\n");
        fx.commit("two");
        fx.git(&["checkout", "-q", "-b", "side"]);
        fx.write("a.txt", "c\n");
        fx.commit("three");
        fx.git(&["checkout", "-q", "main"]);
        ident(&fx);
        fx
    };
    let ours = make();
    let control = make();

    let repo = ours.repo();
    ff_core::branch::rename(&repo, "side", "renamed", NOW).unwrap();
    control.git(&["branch", "-m", "side", "renamed"]);

    assert_eq!(
        ours.git(&["rev-parse", "refs/heads/renamed"]),
        control.git(&["rev-parse", "refs/heads/renamed"]),
        "same target"
    );
    assert!(
        !ours
            .try_git(&["rev-parse", "--verify", "refs/heads/side"])
            .status
            .success(),
        "old name gone"
    );
    // Reflog entry values match git's (messages and shas); git appends a
    // "Branch: renamed" line whose old == new — dropped by our transaction
    // machinery, an accepted divergence — so compare the shared prefix.
    let ours_log = ours.git(&["reflog", "renamed", "--format=%H %gs"]);
    let control_log = control.git(&["reflog", "renamed", "--format=%H %gs"]);
    let ours_lines: Vec<&str> = ours_log.lines().collect();
    let mut control_lines: Vec<&str> = control_log.lines().collect();
    control_lines.retain(|l| !l.contains("Branch: renamed"));
    assert_eq!(ours_lines, control_lines, "replayed reflog matches git's");
}

#[test]
fn claim_carries_chain_open_commit_and_metadata() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    ident(&fx);
    fx.git(&["checkout", "-q", "-b", "ff/misty-owl"]);

    // A snap chain, an open change, and a pending description.
    fx.write("a.txt", "wip\n");
    let repo = fx.repo();
    // The description first, so the capture's open commit carries it and
    // the claim has nothing to rewrite.
    let mut meta = ff_core::branchmeta::read(&repo, "ff/misty-owl").unwrap();
    meta.pending_description = Some("the plan".into());
    ff_core::branchmeta::write(&repo, "ff/misty-owl", &meta).unwrap();
    ff_core::capture(&repo, &ff_core::Provenance::new("manual", None)).unwrap();
    // First-parent only: that walk IS the timeline. A full `git log` also
    // surfaces every commit the log pins (the open commit, among others),
    // which says nothing about whether the rename carried the pointer.
    let timeline_before = fx.git(&[
        "log",
        "--first-parent",
        "--format=%H %s",
        "refs/fufu/snap/ff/misty-owl",
    ]);
    let open_before = fx.git(&["rev-parse", "refs/fufu/open/ff/misty-owl"]);

    let repo = fx.repo();
    let (report, _ctx) = ff_core::branch::rename_current(
        &repo,
        "real-work",
        &prov(),
        Some(NOW + 1),
        vec![
            "ff".into(),
            "describe".into(),
            "-b".into(),
            "real-work".into(),
        ],
    )
    .unwrap();
    assert_eq!(report.from, "ff/misty-owl");
    assert_eq!(report.to, "real-work");

    // Timeline byte-equal modulo the name; parked ref and metadata carried.
    // (The claim's own preamble may have grown the log — compare the suffix
    // that existed before the claim.)
    let timeline_after = fx.git(&[
        "log",
        "--first-parent",
        "--format=%H %s",
        "refs/fufu/snap/real-work",
    ]);
    assert!(
        timeline_after.ends_with(&timeline_before),
        "timeline preserved:\nbefore:\n{timeline_before}\nafter:\n{timeline_after}"
    );
    assert_eq!(
        fx.git(&["rev-parse", "refs/fufu/open/real-work"]),
        open_before,
        "before:\n{}\nafter:\n{}",
        fx.git(&["cat-file", "commit", open_before.trim()]),
        fx.git(&["cat-file", "commit", "refs/fufu/open/real-work"])
    );
    let meta = ff_core::branchmeta::read(&fx.repo(), "real-work").unwrap();
    assert_eq!(meta.pending_description.as_deref(), Some("the plan"));
    assert!(
        ff_core::branchmeta::read(&fx.repo(), "ff/misty-owl")
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        fx.git(&["symbolic-ref", "HEAD"]).trim(),
        "refs/heads/real-work"
    );
    // Recorded, and clean after.
    let record = tip_record(&fx.repo());
    assert_eq!(record.verb, "describe", "naming a branch is describe's act");
    let after = ff_core::ops::reconcile(&fx.repo(), NOW + 5).unwrap();
    assert!(after.is_quiet(), "{after:?}");
}

/// Naming a proper name is allowed — that is `ff describe -b`'s whole
/// difference from the claim it replaced — while landing on a name someone
/// else's work already holds is the one guess worth refusing.
#[test]
fn naming_allows_proper_names_and_refuses_taken_ones() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    ident(&fx);
    let repo = fx.repo();
    // On main (a proper name): renaming is the same act as claiming.
    let (report, _ctx) =
        ff_core::branch::rename_current(&repo, "other", &prov(), Some(NOW), Vec::new()).unwrap();
    assert_eq!(report.from, "main");
    assert_eq!(report.to, "other");
    // On an anonymous branch, but the name is taken.
    fx.git(&["branch", "taken"]);
    fx.git(&["checkout", "-q", "-b", "ff/bold-crane"]);
    let repo = fx.repo();
    let err = ff_core::branch::rename_current(&repo, "taken", &prov(), Some(NOW + 1), Vec::new())
        .unwrap_err();
    assert_eq!(err.id(), "branch/exists");
}

#[test]
fn rename_guards_branches_checked_out_in_other_worktrees() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    fx.git(&["branch", "elsewhere"]);
    let wt = fx.root().join("linked-wt");
    fx.git(&["worktree", "add", "-q", wt.to_str().unwrap(), "elsewhere"]);
    ident(&fx);

    let repo = fx.repo();
    let err = ff_core::branch::rename(&repo, "elsewhere", "moved", NOW);
    assert!(err.is_err(), "rename must refuse: {err:?}");
    let err = ff_core::branch::delete(&repo, "elsewhere", &prov(), Some(NOW), Vec::new());
    assert!(err.is_err(), "delete must refuse");
}

#[test]
fn delete_trashes_chain_leaves_the_open_commit_and_journals() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    ident(&fx);
    fx.git(&["checkout", "-q", "-b", "doomed"]);
    fx.write("a.txt", "doomed wip\n");
    let parked = park_by_switching(&fx, "main", NOW);

    let repo = fx.repo();
    let (report, _ctx) = ff_core::branch::delete(
        &repo,
        "doomed",
        &prov(),
        Some(NOW + 1),
        vec!["ff".into(), "branch".into(), "-d".into(), "doomed".into()],
    )
    .unwrap();
    assert_eq!(report.name, "doomed");
    assert_eq!(report.trash_ref.as_deref(), Some("refs/fufu/trash/doomed"));
    assert_eq!(report.open_left.as_deref(), Some(parked.as_str()));

    assert!(
        !fx.try_git(&["rev-parse", "--verify", "refs/heads/doomed"])
            .status
            .success()
    );
    assert!(
        !fx.try_git(&["rev-parse", "--verify", "refs/fufu/snap/doomed"])
            .status
            .success()
    );
    assert!(
        !fx.try_git(&["rev-parse", "--verify", "refs/fufu/open/doomed"])
            .status
            .success(),
        "the open ref goes with the branch"
    );
    fx.git(&["rev-parse", "refs/fufu/trash/doomed"]);
    // The open commit survives, pinned; nothing is on the stash list.
    assert!(fx.git(&["stash", "list"]).is_empty());
    fx.git(&["cat-file", "-e", &parked]);

    // The deleted tip stays pinned through gc via the journal.
    fx.git(&["reflog", "expire", "--expire=all", "--all"]);
    fx.git(&["gc", "--prune=now", "--quiet"]);
    let repo = fx.repo();
    let tip_id = gix::ObjectId::from_hex(report.tip.as_bytes()).unwrap();
    assert!(
        repo.try_find_object(tip_id).unwrap().is_some(),
        "tip pinned"
    );

    let after = ff_core::ops::reconcile(&repo, NOW + 5).unwrap();
    assert!(after.is_quiet(), "{after:?}");
}

fn open_ref(fx: &Fixture, branch: &str) -> Option<String> {
    let out = fx.git(&[
        "for-each-ref",
        "--format=%(objectname)",
        &format!("refs/fufu/open/{branch}"),
    ]);
    let out = out.trim();
    (!out.is_empty()).then(|| out.to_string())
}

fn rewind_opts(now: i64) -> ff_core::RewindOptions {
    ff_core::RewindOptions {
        force: false,
        now: Some(now),
        argv: Vec::new(),
    }
}

/// `ff branch <name> @` parks a copy of the open change on the new branch:
/// the same open commit and metadata under the new name, recorded on the
/// new name so the copy goes with the mint under one undo, while the branch
/// underfoot keeps its own and nothing moves.
#[test]
fn create_at_the_open_change_carries_a_copy_and_undo_takes_it_back() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    let init = fx.commit("init");
    ident(&fx);
    fx.write("a.txt", "dirty\n");
    let repo = fx.repo();
    ff_core::describe::set_pending(
        &repo,
        Some("the plan".into()),
        &prov(),
        Some(NOW - 10),
        Vec::new(),
    )
    .unwrap();
    let main_meta = ff_core::branchmeta::read(&repo, "main").unwrap();
    let main_open = open_ref(&fx, "main").expect("main's open commit");
    let status_before = fx.git(&["status", "--porcelain=v2"]);

    let (report, _ctx) =
        ff_core::branch::create(&repo, "spike", Some("@"), &prov(), Some(NOW), Vec::new()).unwrap();
    assert_eq!(report.at, init);
    assert_eq!(report.carried.as_deref(), Some(main_open.as_str()));
    assert_eq!(open_ref(&fx, "spike").as_deref(), Some(main_open.as_str()));
    let meta = ff_core::branchmeta::read(&repo, "spike").unwrap();
    assert_eq!(meta.change_id, main_meta.change_id);
    assert_eq!(meta.change_born, main_meta.change_born);
    assert_eq!(meta.pending_description.as_deref(), Some("the plan"));
    assert_eq!(
        meta.forked_from.as_deref(),
        Some(report.forked_from.as_str())
    );

    // Nothing moved here.
    assert_eq!(fx.git(&["symbolic-ref", "HEAD"]).trim(), "refs/heads/main");
    assert_eq!(fx.git(&["status", "--porcelain=v2"]), status_before);
    assert_eq!(open_ref(&fx, "main").as_deref(), Some(main_open.as_str()));
    assert_eq!(ff_core::branchmeta::read(&repo, "main").unwrap(), main_meta);

    // Recorded on the new name, with the copy's transitions.
    let record = tip_record(&repo);
    assert_eq!(record.verb, "branch");
    assert!(
        record.summary.contains("carrying the open change"),
        "{}",
        record.summary
    );
    assert_eq!(record.head, None, "nothing moved");
    assert_eq!(record.description.as_ref().unwrap().branch, "spike");
    assert_eq!(record.change_id.as_ref().unwrap().branch, "spike");
    assert_eq!(record.change_id.as_ref().unwrap().new, main_meta.change_id);
    let log = ff_core::ops::OpLog::open(&repo).unwrap();
    let tip = log.get(log.tip().unwrap().unwrap()).unwrap();
    assert_eq!(tip.branch(), Some("spike"));

    ff_core::undo(&repo, &rewind_opts(NOW + 10), &prov()).unwrap();
    assert!(
        fx.git(&["for-each-ref", "refs/heads/spike"])
            .trim()
            .is_empty(),
        "the mint is gone"
    );
    assert_eq!(open_ref(&fx, "spike"), None, "and the copy with it");
    let meta = ff_core::branchmeta::read(&repo, "spike").unwrap();
    assert_eq!(meta.change_id, None);
    assert_eq!(meta.pending_description, None);
    assert_eq!(open_ref(&fx, "main").as_deref(), Some(main_open.as_str()));
    assert_eq!(fx.git(&["status", "--porcelain=v2"]), status_before);

    ff_core::redo(&repo, &rewind_opts(NOW + 20), &prov()).unwrap();
    assert_eq!(fx.git(&["rev-parse", "spike"]).trim(), init);
    assert_eq!(open_ref(&fx, "spike").as_deref(), Some(main_open.as_str()));
    let meta = ff_core::branchmeta::read(&repo, "spike").unwrap();
    assert_eq!(meta.change_id, main_meta.change_id);
    assert_eq!(meta.pending_description.as_deref(), Some("the plan"));
    assert_eq!(open_ref(&fx, "main").as_deref(), Some(main_open.as_str()));
}

/// A clean tree has nothing to copy: `@` is the commit under HEAD, and the
/// mint is recorded on the branch it runs on, as any other target's is.
#[test]
fn create_at_the_open_change_with_a_clean_tree_copies_nothing() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    let init = fx.commit("init");
    ident(&fx);
    let repo = fx.repo();

    let (report, _ctx) =
        ff_core::branch::create(&repo, "spike", Some("@"), &prov(), Some(NOW), Vec::new()).unwrap();
    assert_eq!(report.at, init);
    assert_eq!(report.carried, None);
    assert_eq!(open_ref(&fx, "spike"), None);
    let meta = ff_core::branchmeta::read(&repo, "spike").unwrap();
    assert_eq!(meta.change_id, None);
    assert_eq!(meta.pending_description, None);
    let record = tip_record(&repo);
    assert_eq!(record.description, None);
    assert_eq!(record.change_id, None);
    let log = ff_core::ops::OpLog::open(&repo).unwrap();
    let tip = log.get(log.tip().unwrap().unwrap()).unwrap();
    assert_eq!(tip.branch(), Some("main"));
}

#[test]
fn delete_refuses_current_and_unknown() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    ident(&fx);
    let repo = fx.repo();
    assert!(ff_core::branch::delete(&repo, "main", &prov(), Some(NOW), Vec::new()).is_err());
    assert!(ff_core::branch::delete(&repo, "ghost", &prov(), Some(NOW), Vec::new()).is_err());
}

#[test]
fn delete_report_names_the_shared_copy() {
    // Pins that a plain delete reports the tracking ref it leaves behind —
    // fully named — and that the tracking ref survives the delete.
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    let sha = fx.commit("one");
    ident(&fx);
    fx.git(&["branch", "shared"]);
    fx.set_config("remote.origin.url", "file:///nonexistent");
    fx.set_config("remote.origin.fetch", "+refs/heads/*:refs/remotes/origin/*");
    fx.set_config("branch.shared.remote", "origin");
    fx.set_config("branch.shared.merge", "refs/heads/shared");
    fx.git(&["update-ref", "refs/remotes/origin/shared", &sha]);

    let (report, _ctx) =
        ff_core::branch::delete(&fx.repo(), "shared", &prov(), Some(NOW), Vec::new()).unwrap();
    let shared = report.shared.expect("shared must be named in the report");
    assert_eq!(shared.remote, "origin");
    assert_eq!(shared.name, "origin/shared");
    assert_eq!(shared.r#ref, "refs/remotes/origin/shared");
    assert_eq!(shared.remote_branch, "shared");
    assert_eq!(shared.tip, sha);

    // The reported copy is left standing: it is reported, not removed.
    assert_eq!(
        fx.git(&["rev-parse", "refs/remotes/origin/shared"]).trim(),
        sha
    );
}

#[test]
fn delete_without_an_upstream_reports_no_shared_copy() {
    // Pins that a branch answering to nothing reports `shared: None`.
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("one");
    ident(&fx);
    fx.git(&["branch", "shared"]);

    let (report, _ctx) =
        ff_core::branch::delete(&fx.repo(), "shared", &prov(), Some(NOW), Vec::new()).unwrap();
    assert!(report.shared.is_none());
}

#[test]
fn delete_of_a_branch_tracking_another_name_reports_no_shared_copy() {
    // Pins that a tracking ref wearing another local branch's name is not
    // the branch's copy: it is the base the branch was cut from, so the
    // report names no shared copy and the tracking ref is left standing.
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    let sha = fx.commit("one");
    ident(&fx);
    fx.git(&["branch", "other"]);
    fx.git(&["branch", "shared"]);
    fx.set_config("remote.origin.url", "file:///nonexistent");
    fx.set_config("remote.origin.fetch", "+refs/heads/*:refs/remotes/origin/*");
    fx.set_config("branch.shared.remote", "origin");
    fx.set_config("branch.shared.merge", "refs/heads/other");
    fx.git(&["update-ref", "refs/remotes/origin/other", &sha]);

    let (report, _ctx) =
        ff_core::branch::delete(&fx.repo(), "shared", &prov(), Some(NOW), Vec::new()).unwrap();
    assert!(report.shared.is_none(), "{:?}", report.shared);
    assert_eq!(
        fx.git(&["rev-parse", "refs/remotes/origin/other"]).trim(),
        sha
    );
}

#[test]
fn list_segregates_and_annotates() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    ident(&fx);
    fx.git(&["branch", "ff/hidden-brook"]);
    fx.git(&["branch", "named"]);
    let repo = fx.repo();
    ff_core::branchmeta::write(
        &repo,
        "named",
        &ff_core::branchmeta::BranchMeta {
            pending_description: Some("todo".into()),
            change_id: None,
            change_born: None,
            forked_from: None,
            parent: None,
            session: None,
            held: None,
            resolving: None,
        },
    )
    .unwrap();
    // Park something on main so the row is annotated: a switch away.
    fx.write("a.txt", "wip\n");
    park_by_switching(&fx, "named", NOW);

    let list = ff_core::branch::list(&fx.repo(), &ff_core::BranchListOptions::default()).unwrap();
    let names: Vec<&str> = list.named.iter().map(|b| b.name.as_str()).collect();
    assert_eq!(names, vec!["main", "named"]);
    let anon: Vec<&str> = list.anonymous.iter().map(|b| b.name.as_str()).collect();
    assert_eq!(anon, vec!["ff/hidden-brook"]);
    let main = &list.named[0];
    assert!(!main.current);
    assert!(main.parked, "parked annotation");
    let named = &list.named[1];
    assert_eq!(named.pending_description.as_deref(), Some("todo"));
}

#[test]
fn list_shows_branches_that_exist_only_on_the_remote() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    let sha = fx.commit("one");
    ident(&fx);
    fx.set_config("remote.origin.url", "file:///nonexistent");
    fx.set_config("remote.origin.fetch", "+refs/heads/*:refs/remotes/origin/*");
    fx.set_config("branch.main.remote", "origin");
    fx.set_config("branch.main.merge", "refs/heads/main");
    fx.git(&["update-ref", "refs/remotes/origin/main", &sha]);
    fx.git(&["update-ref", "refs/remotes/origin/spike", &sha]);
    fx.git(&["update-ref", "refs/remotes/origin/other", &sha]);

    let list = ff_core::branch::list(&fx.repo(), &ff_core::BranchListOptions::default()).unwrap();
    // origin/main is tracked, so the bucket holds the other two only.
    let names: Vec<&str> = list.remote_only.iter().map(|b| b.name.as_str()).collect();
    assert_eq!(names, vec!["origin/other", "origin/spike"]);
    for row in &list.remote_only {
        assert_eq!(row.remote, "origin");
        assert_eq!(row.tip, sha);
    }
    assert_eq!(list.remote_more, 0);
}

#[test]
fn remote_head_is_not_a_branch() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    let sha = fx.commit("one");
    ident(&fx);
    fx.set_config("remote.origin.url", "file:///nonexistent");
    fx.set_config("remote.origin.fetch", "+refs/heads/*:refs/remotes/origin/*");
    fx.git(&["update-ref", "refs/remotes/origin/main", &sha]);
    fx.git(&[
        "symbolic-ref",
        "refs/remotes/origin/HEAD",
        "refs/remotes/origin/main",
    ]);

    // A symbolic <remote>/HEAD must be skipped, not raised on — the listing
    // comes back Ok and without a row for it.
    let list = ff_core::branch::list(&fx.repo(), &ff_core::BranchListOptions::default())
        .expect("a symbolic origin/HEAD must not fail the listing");
    assert!(!list.remote_only.iter().any(|b| b.name == "origin/HEAD"));
}

#[test]
fn a_local_branch_hides_the_ref_it_tracks_whatever_it_is_called() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    let sha = fx.commit("one");
    ident(&fx);
    fx.git(&["checkout", "-q", "-b", "mine"]);
    fx.set_config("remote.origin.url", "file:///nonexistent");
    fx.set_config("remote.origin.fetch", "+refs/heads/*:refs/remotes/origin/*");
    fx.set_config("branch.mine.remote", "origin");
    fx.set_config("branch.mine.merge", "refs/heads/other");
    fx.git(&["update-ref", "refs/remotes/origin/other", &sha]);
    fx.git(&["update-ref", "refs/remotes/origin/spike", &sha]);

    // mine and other differ in name, so only the tracking ref subtracts.
    let list = ff_core::branch::list(&fx.repo(), &ff_core::BranchListOptions::default()).unwrap();
    let names: Vec<&str> = list.remote_only.iter().map(|b| b.name.as_str()).collect();
    assert_eq!(names, vec!["origin/spike"]);
}

#[test]
fn the_bound_reports_what_it_left_out() {
    let fx = Fixture::new();
    fx.write("a.txt", "one\n");
    let one = fx.commit("one");
    fx.write("a.txt", "two\n");
    let two = fx.commit("two");
    fx.write("a.txt", "three\n");
    let three = fx.commit("three");
    ident(&fx);
    fx.set_config("remote.origin.url", "file:///nonexistent");
    fx.set_config("remote.origin.fetch", "+refs/heads/*:refs/remotes/origin/*");
    fx.git(&["update-ref", "refs/remotes/origin/a", &one]);
    fx.git(&["update-ref", "refs/remotes/origin/b", &two]);
    fx.git(&["update-ref", "refs/remotes/origin/c", &three]);

    let opts = ff_core::BranchListOptions {
        remote_limit: Some(2),
    };
    let list = ff_core::branch::list(&fx.repo(), &opts).unwrap();
    assert_eq!(list.remote_only.len(), 2);
    assert_eq!(list.remote_more, 1);
    let names: Vec<&str> = list.remote_only.iter().map(|b| b.name.as_str()).collect();
    assert_eq!(names, vec!["origin/c", "origin/b"]);
}

#[test]
fn petnames_mint_unused_names() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    let repo = fx.repo();
    let name = ff_core::petname::mint(&repo).unwrap();
    assert!(name.starts_with("ff/"), "{name}");
    ff_core::branch::validate_name(&name).unwrap();
    // Minting again after taking the name yields something else.
    let head = fx.git(&["rev-parse", "HEAD"]).trim().to_string();
    fx.git(&["branch", &name]);
    let repo = fx.repo();
    let second = ff_core::petname::mint(&repo).unwrap();
    assert_ne!(name, second);
    let _ = head;
}

#[test]
fn rename_carries_the_upstream_like_git_branch_m() {
    let make = || {
        let fx = Fixture::new();
        fx.write("a.txt", "a\n");
        fx.commit("one");
        fx.git(&["checkout", "-q", "-b", "side"]);
        ident(&fx);
        fx.set_config("branch.side.remote", "origin");
        fx.set_config("branch.side.merge", "refs/heads/side");
        fx
    };
    let ours = make();
    let control = make();

    let repo = ours.repo();
    ff_core::branch::rename(&repo, "side", "renamed", NOW).unwrap();
    control.git(&["branch", "-m", "side", "renamed"]);

    // fufu appends the renamed section where git renames it in place, so
    // file order may differ — this is about the content carried, not the
    // layout; compare sorted.
    let lines = |fx: &Fixture| -> Vec<String> {
        let mut lines: Vec<String> = fx
            .git(&["config", "--get-regexp", r"^branch\."])
            .lines()
            .map(str::to_string)
            .collect();
        lines.sort();
        lines
    };
    assert_eq!(
        lines(&ours),
        lines(&control),
        "same upstream carried across the rename"
    );
    let ours_lines = lines(&ours);
    assert!(ours_lines.contains(&"branch.renamed.remote origin".into()));
    assert!(
        ours_lines.contains(&"branch.renamed.merge refs/heads/side".into()),
        "merge names the remote side, which this rename did not touch"
    );
    assert!(
        !ours_lines.iter().any(|l| l.ends_with("refs/heads/renamed")),
        "merge not rewritten to the new name"
    );
    assert!(
        !ours_lines.iter().any(|l| l.starts_with("branch.side.")),
        "no stale section under the old name"
    );
}

#[test]
fn rename_replaces_a_stale_section_under_the_new_name() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("one");
    fx.git(&["checkout", "-q", "-b", "side"]);
    ident(&fx);
    fx.set_config("branch.side.remote", "origin");
    fx.set_config("branch.side.merge", "refs/heads/side");
    // A leftover section under the new name: rename already refused a
    // branch of that name, so it can only be stale config.
    fx.set_config("branch.renamed.remote", "ghost");

    let repo = fx.repo();
    ff_core::branch::rename(&repo, "side", "renamed", NOW).unwrap();

    let remotes = fx.git(&["config", "--get-all", "branch.renamed.remote"]);
    let lines: Vec<&str> = remotes.lines().collect();
    assert_eq!(
        lines,
        vec!["origin"],
        "stale section replaced, no duplicate key"
    );
}

#[test]
fn rename_without_an_upstream_leaves_the_config_untouched() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("one");
    fx.git(&["checkout", "-q", "-b", "side"]);
    ident(&fx);
    let config = fx.path().join(".git/config");
    let before = std::fs::read(&config).unwrap();

    let repo = fx.repo();
    ff_core::branch::rename(&repo, "side", "renamed", NOW).unwrap();

    assert_eq!(
        std::fs::read(&config).unwrap(),
        before,
        "no upstream, no write"
    );
}

#[test]
fn a_renamed_branch_keeps_its_remote_axis() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    let sha = fx.commit("one");
    fx.git(&["checkout", "-q", "-b", "side"]);
    ident(&fx);
    fx.set_config("remote.origin.url", "file:///nonexistent");
    fx.set_config("remote.origin.fetch", "+refs/heads/*:refs/remotes/origin/*");
    fx.set_config("branch.side.remote", "origin");
    fx.set_config("branch.side.merge", "refs/heads/side");
    fx.git(&["update-ref", "refs/remotes/origin/side", &sha]);

    let repo = fx.repo();
    ff_core::branch::rename(&repo, "side", "renamed", NOW).unwrap();

    // A fresh handle: gix serves the config it had at open time, so the
    // old one would not see the rename's write.
    let repo = fx.repo();
    let remote = ff_core::futures::remote_for(&repo, "renamed")
        .unwrap()
        .expect("the renamed branch kept its remote axis");
    assert_eq!(remote.r#ref, "refs/remotes/origin/side");
}

/// `branch.<n>.merge` is legitimately multi-valued — git spells an octopus
/// upstream as repeated keys — and the section move has to carry the values
/// once each. Collecting names per occurrence and values per name squares
/// them, which is what this pins.
#[test]
fn rename_carries_a_multi_valued_key_once() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("one");
    fx.git(&["checkout", "-q", "-b", "side"]);
    ident(&fx);
    fx.set_config("branch.side.remote", "origin");
    fx.set_config("branch.side.merge", "refs/heads/one");
    fx.git(&["config", "--add", "branch.side.merge", "refs/heads/two"]);

    let repo = fx.repo();
    ff_core::branch::rename(&repo, "side", "renamed", NOW).unwrap();

    let merges: Vec<String> = fx
        .git(&["config", "--get-all", "branch.renamed.merge"])
        .lines()
        .map(str::to_string)
        .collect();
    assert_eq!(merges, vec!["refs/heads/one", "refs/heads/two"]);
}

#[test]
fn setting_an_upstream_matches_git_set_upstream_to() {
    let make = || {
        let fx = Fixture::new();
        fx.write("a.txt", "a\n");
        let sha = fx.commit("one");
        fx.git(&["checkout", "-q", "-b", "side"]);
        ident(&fx);
        fx.set_config("remote.origin.url", "file:///nonexistent");
        fx.set_config("remote.origin.fetch", "+refs/heads/*:refs/remotes/origin/*");
        // git's `branch --set-upstream-to` refuses a tracking ref that is
        // not there, so the control side needs it.
        fx.git(&["update-ref", "refs/remotes/origin/side", &sha]);
        fx
    };
    let ours = make();
    let control = make();

    let repo = ours.repo();
    ff_core::snapshot::config::set_branch_upstream(&repo, "side", "origin").unwrap();
    control.git(&["branch", "--set-upstream-to", "origin/side", "side"]);

    // One appends, the other rewrites in place: the claim is the content,
    // not the layout, so compare sorted.
    let lines = |fx: &Fixture| -> Vec<String> {
        let mut lines: Vec<String> = fx
            .git(&["config", "--get-regexp", r"^branch\."])
            .lines()
            .map(str::to_string)
            .collect();
        lines.sort();
        lines
    };
    assert_eq!(
        lines(&ours),
        lines(&control),
        "the same upstream git would write"
    );
    assert_eq!(
        lines(&ours),
        vec![
            "branch.side.merge refs/heads/side".to_string(),
            "branch.side.remote origin".to_string(),
        ],
        "remote and merge, and nothing else"
    );
}

#[test]
fn setting_an_upstream_replaces_a_stale_section() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("one");
    fx.git(&["checkout", "-q", "-b", "side"]);
    ident(&fx);
    // A stale section with a different remote and a doubled merge — the
    // state an octopus upstream leaves behind.
    fx.set_config("branch.side.remote", "elsewhere");
    fx.set_config("branch.side.merge", "refs/heads/stale");
    fx.git(&["config", "--add", "branch.side.merge", "refs/heads/older"]);

    let repo = fx.repo();
    ff_core::snapshot::config::set_branch_upstream(&repo, "side", "origin").unwrap();

    let merges: Vec<String> = fx
        .git(&["config", "--get-all", "branch.side.merge"])
        .lines()
        .map(str::to_string)
        .collect();
    assert_eq!(
        merges,
        vec!["refs/heads/side"],
        "the section is replaced, not appended into"
    );
    let remote = fx.git(&["config", "--get", "branch.side.remote"]);
    assert_eq!(remote.trim(), "origin", "the stale remote is gone");
}
