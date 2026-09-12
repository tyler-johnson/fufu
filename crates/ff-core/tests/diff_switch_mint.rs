//! `ff switch`'s mint arm — `ff start`'s spelling of it: a target that names
//! no branch here mints a fresh one, forking at trunk (bare) or at a
//! resolved revision, and carries the open change across the fork only
//! under `@`, as a copy. Plus `ff describe` pending-description round trips,
//! unrelated to the mint but hosted here alongside the rest of the
//! composition tests.

use ff_core::SwitchOptions;
use ff_core::gix;
use ff_testsupport::Fixture;

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
    fx.set_config("user.name", "New User");
    fx.set_config("user.email", "new@test");
}

fn prov() -> ff_core::Provenance {
    ff_core::Provenance::new("pre", Some("ff start".into()))
}

fn run_start(fx: &Fixture, opts: SwitchOptions) -> ff_core::SwitchReport {
    let repo = fx.repo();
    let (report, _ctx) = ff_core::switch(&repo, &opts, &prov()).unwrap();
    report
}

/// What a mint reports of itself; a continue reports none.
fn minted(report: &ff_core::SwitchReport) -> &ff_core::Minted {
    report.minted.as_ref().expect("the switch minted")
}

/// The fork source a mint reports: a branch name, or a short sha.
fn forked_from(report: &ff_core::SwitchReport) -> &str {
    minted(report)
        .forked_from
        .as_deref()
        .expect("a fork names what it forked from")
}

fn commit_count(fx: &Fixture, rev: &str) -> String {
    fx.git(&["rev-list", "--count", rev]).trim().to_string()
}

#[test]
fn bare_forks_trunk_and_opens_clean() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    fx.git(&["branch", "feature"]);
    fx.write("a.txt", "b\n");
    fx.commit("second"); // main advances past feature
    fx.git(&["checkout", "-q", "feature"]);
    ident(&fx);
    let main_tip = fx.git(&["rev-parse", "main"]).trim().to_string();
    let feature_tip = fx.git(&["rev-parse", "feature"]).trim().to_string();
    assert_ne!(
        main_tip, feature_tip,
        "test fixture: trunk must have moved on"
    );

    fx.write("a.txt", "dirty on feature\n");
    let report = run_start(
        &fx,
        SwitchOptions {
            now: Some(NOW),
            ..Default::default()
        },
    );

    assert!(report.to.starts_with("ff/"), "{}", report.to);
    assert_eq!(forked_from(&report), "main");
    assert_eq!(
        fx.git(&["rev-parse", &format!("refs/heads/{}", report.to)])
            .trim(),
        main_tip,
        "forked at trunk's tip, not the branch it was standing on"
    );
    assert_eq!(fx.git(&["status", "--porcelain=v2"]), "", "opens clean");
    assert!(report.parked.is_some());
}

/// The park line names the branch the change was open on, which is not the
/// branch the fork came from: standing on `feature` and forking trunk parks
/// feature's work. `main` in the assertion is what the old code said.
#[test]
fn the_park_names_the_branch_the_change_was_on() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    fx.git(&["branch", "feature"]);
    fx.git(&["checkout", "-q", "feature"]);
    ident(&fx);
    fx.write("a.txt", "dirty on feature\n");

    let report = run_start(
        &fx,
        SwitchOptions {
            now: Some(NOW),
            ..Default::default()
        },
    );

    assert!(report.parked.is_some(), "a dirty tree parks");
    assert_eq!(forked_from(&report), "main", "the fork came from trunk");
    assert_eq!(
        Some(report.from.as_str()),
        Some("feature"),
        "the park was of feature's work, whatever the fork source was"
    );
}

#[test]
fn bare_parks_the_open_change_retrievably() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    fx.git(&["branch", "feature"]);
    fx.git(&["checkout", "-q", "feature"]);
    ident(&fx);
    fx.write("a.txt", "parked work\n");

    let report = run_start(
        &fx,
        SwitchOptions {
            now: Some(NOW),
            ..Default::default()
        },
    );
    assert!(report.parked.is_some());

    let repo = fx.repo();
    let (switch_report, _ctx) = ff_core::switch(
        &repo,
        &SwitchOptions {
            target: Some("feature".into()),
            now: Some(NOW + 10),
            argv: Vec::new(),
            ..Default::default()
        },
        &prov(),
    )
    .unwrap();
    assert!(
        matches!(
            switch_report.arrival,
            ff_core::ArrivalReport::Restored { .. }
        ),
        "{:?}",
        switch_report.arrival
    );
    assert_eq!(
        std::fs::read_to_string(fx.path().join("a.txt")).unwrap(),
        "parked work\n"
    );
}

#[test]
fn bare_works_with_remote_only_trunk() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    fx.git(&["branch", "other"]);
    fx.git(&["checkout", "-q", "other"]);
    fx.git(&["branch", "-D", "main"]);
    let sha = fx.git(&["rev-parse", "other"]).trim().to_string();
    fx.git(&["update-ref", "refs/remotes/origin/main", &sha]);
    fx.git(&[
        "symbolic-ref",
        "refs/remotes/origin/HEAD",
        "refs/remotes/origin/main",
    ]);
    ident(&fx);
    fx.write("a.txt", "dirty on other\n");

    let report = run_start(
        &fx,
        SwitchOptions {
            now: Some(NOW),
            ..Default::default()
        },
    );
    assert_eq!(forked_from(&report), "main");
    assert_eq!(
        fx.git(&["rev-parse", &format!("refs/heads/{}", report.to)])
            .trim(),
        sha
    );
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

/// The verb operations since `since`'s tip, newest first, read through the
/// public reader — captures and notes left out, so the count is the count
/// of decisions.
fn verb_ops_since(repo: &gix::Repository, since: &Option<ff_core::ops::OpId>) -> Vec<String> {
    let log = ff_core::ops::OpLog::open(repo).unwrap();
    let mut out = Vec::new();
    let mut cursor = log.tip().unwrap();
    while let Some(id) = cursor {
        if Some(id) == *since {
            break;
        }
        let op = log.get(id).unwrap();
        if op.kind() == ff_core::ops::OpKind::Op {
            out.push(
                op.record()
                    .unwrap()
                    .map(|r| r.verb.clone())
                    .unwrap_or_default(),
            );
        }
        cursor = op.prev();
    }
    out
}

/// `@` is the one target that carries: the fork lands under the open change
/// and the new branch receives a copy of it — the same open commit, id,
/// birth, and description — while main keeps its own, parked. All of it is
/// one operation, and one undo takes the mint, the copy, and the switch back.
#[test]
fn at_forks_under_the_open_change_and_carries_a_copy() {
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
    let before = ff_core::ops::OpLog::open(&repo).unwrap().tip().unwrap();

    let report = run_start(
        &fx,
        SwitchOptions {
            target: Some("@".into()),
            branch: Some(Some("spike".into())),
            now: Some(NOW),
            ..Default::default()
        },
    );

    assert_eq!(report.to, "spike");
    assert_eq!(
        fx.git(&["rev-parse", "spike"]).trim(),
        init,
        "forked under the open change"
    );
    assert_eq!(
        fx.git(&["symbolic-ref", "HEAD"]).trim(),
        "refs/heads/spike",
        "and switched there"
    );
    assert_eq!(
        std::fs::read_to_string(fx.path().join("a.txt")).unwrap(),
        "dirty\n",
        "the copy is the working tree"
    );
    let parked = report.parked.clone().expect("main's change parked");
    let carried = minted(&report)
        .carried
        .clone()
        .expect("the copy is reported");
    assert_eq!(carried, parked, "one sha on both branches");
    assert_eq!(open_ref(&fx, "main").as_deref(), Some(parked.as_str()));
    assert_eq!(open_ref(&fx, "spike").as_deref(), Some(carried.as_str()));
    assert_eq!(Some(report.from.as_str()), Some("main"));
    let meta = ff_core::branchmeta::read(&repo, "spike").unwrap();
    assert_eq!(meta.change_id, main_meta.change_id, "same change");
    assert_eq!(meta.change_born, main_meta.change_born, "same birth");
    assert_eq!(meta.pending_description.as_deref(), Some("the plan"));
    assert_eq!(
        ff_core::branchmeta::read(&repo, "main").unwrap(),
        main_meta,
        "main keeps its own"
    );

    // One operation, recorded on the new branch, carrying every transition.
    assert_eq!(verb_ops_since(&repo, &before), vec!["switch".to_string()]);
    let record = tip_record(&repo);
    assert_eq!(record.verb, "switch");
    assert!(record.head.is_some(), "the switch rides the op");
    assert_eq!(record.refs.len(), 1);
    assert_eq!(record.refs[0].name, "refs/heads/spike");
    assert_eq!(record.description.as_ref().unwrap().branch, "spike");
    assert_eq!(
        record.description.as_ref().unwrap().new.as_deref(),
        Some("the plan")
    );
    assert_eq!(record.change_id.as_ref().unwrap().branch, "spike");
    assert_eq!(record.change_id.as_ref().unwrap().new, main_meta.change_id);

    // One undo takes it all back; redo brings it all back.
    ff_core::undo(&repo, &rewind_opts(NOW + 10), &prov()).unwrap();
    assert_eq!(fx.git(&["symbolic-ref", "HEAD"]).trim(), "refs/heads/main");
    assert_eq!(
        std::fs::read_to_string(fx.path().join("a.txt")).unwrap(),
        "dirty\n",
        "main's change is back underfoot"
    );
    assert!(
        fx.git(&["for-each-ref", "refs/heads/spike"])
            .trim()
            .is_empty(),
        "the mint is gone"
    );
    assert_eq!(open_ref(&fx, "spike"), None, "and the copy with it");
    assert_eq!(open_ref(&fx, "main").as_deref(), Some(parked.as_str()));

    ff_core::redo(&repo, &rewind_opts(NOW + 20), &prov()).unwrap();
    assert_eq!(fx.git(&["symbolic-ref", "HEAD"]).trim(), "refs/heads/spike");
    assert_eq!(fx.git(&["rev-parse", "spike"]).trim(), init);
    assert_eq!(open_ref(&fx, "spike").as_deref(), Some(carried.as_str()));
    assert_eq!(open_ref(&fx, "main").as_deref(), Some(parked.as_str()));
    assert_eq!(
        std::fs::read_to_string(fx.path().join("a.txt")).unwrap(),
        "dirty\n"
    );
}

/// A clean tree has nothing to copy: `@` forks under HEAD and opens clean.
#[test]
fn at_with_a_clean_tree_carries_nothing() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    let init = fx.commit("init");
    ident(&fx);

    let report = run_start(
        &fx,
        SwitchOptions {
            target: Some("@".into()),
            now: Some(NOW),
            ..Default::default()
        },
    );
    assert_eq!(
        fx.git(&["rev-parse", &format!("refs/heads/{}", report.to)])
            .trim(),
        init
    );
    assert_eq!(report.parked, None);
    assert_eq!(minted(&report).carried, None);
    assert_eq!(open_ref(&fx, &report.to), None);
    assert_eq!(fx.git(&["status", "--porcelain=v2"]), "", "opens clean");
    let meta = ff_core::branchmeta::read(&fx.repo(), &report.to).unwrap();
    assert_eq!(meta.change_id, None);
    assert_eq!(meta.pending_description, None);
}

/// An unborn branch has no commit under its open change to fork at.
#[test]
fn at_on_an_unborn_branch_is_refused() {
    let fx = Fixture::new();
    ident(&fx);
    fx.write("a.txt", "dirty\n");
    let repo = fx.repo();
    let err = ff_core::switch(
        &repo,
        &SwitchOptions {
            target: Some("@".into()),
            now: Some(NOW),
            ..Default::default()
        },
        &prov(),
    )
    .expect_err("must refuse");
    assert_eq!(err.id(), "target/unresolvable");
    assert_eq!(err.to_string(), "@ has no commit under it yet");
    assert!(
        fx.git(&["for-each-ref", "refs/heads/"]).trim().is_empty(),
        "no branch minted"
    );
}

/// The mint and the switch are one operation, whatever the target: one undo
/// from a bare start lands back on the branch it left with its tree dirty.
#[test]
fn start_is_one_operation() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    ident(&fx);
    fx.write("a.txt", "dirty\n");
    let repo = fx.repo();
    let before = ff_core::ops::OpLog::open(&repo).unwrap().tip().unwrap();

    let report = run_start(
        &fx,
        SwitchOptions {
            now: Some(NOW),
            ..Default::default()
        },
    );
    assert_eq!(verb_ops_since(&repo, &before), vec!["switch".to_string()]);
    let record = tip_record(&repo);
    assert_eq!(record.verb, "switch");
    assert!(record.head.is_some());
    assert_eq!(fx.git(&["status", "--porcelain=v2"]), "", "opens clean");

    ff_core::undo(&repo, &rewind_opts(NOW + 10), &prov()).unwrap();
    assert_eq!(fx.git(&["symbolic-ref", "HEAD"]).trim(), "refs/heads/main");
    assert_eq!(
        std::fs::read_to_string(fx.path().join("a.txt")).unwrap(),
        "dirty\n"
    );
    assert!(
        fx.git(&["for-each-ref", &format!("refs/heads/{}", report.to)])
            .trim()
            .is_empty(),
        "the mint is gone"
    );
}

/// `-m` on a carry describes the copy: the same change, under a different
/// description, so a different sha — and `carried` names the one the new
/// branch holds.
#[test]
fn the_copy_wears_the_dash_m_message() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
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

    let report = run_start(
        &fx,
        SwitchOptions {
            target: Some("@".into()),
            message: Some("the spike".into()),
            branch: Some(Some("spike".into())),
            now: Some(NOW),
            ..Default::default()
        },
    );
    let parked = report.parked.clone().expect("parked");
    let carried = minted(&report).carried.clone().expect("carried");
    assert_ne!(
        carried, parked,
        "a different description is a different sha"
    );
    assert_eq!(open_ref(&fx, "spike").as_deref(), Some(carried.as_str()));
    assert_eq!(open_ref(&fx, "main").as_deref(), Some(parked.as_str()));
    let meta = ff_core::branchmeta::read(&repo, "spike").unwrap();
    assert_eq!(meta.change_id, main_meta.change_id, "same change");
    assert_eq!(meta.pending_description.as_deref(), Some("the spike"));
    assert_eq!(
        fx.git(&["log", "-1", "--format=%s", &carried]).trim(),
        "the spike"
    );
    assert_eq!(
        fx.git(&["log", "-1", "--format=%s", &parked]).trim(),
        "the plan",
        "the original keeps its own"
    );
    assert_eq!(
        ff_core::branchmeta::read(&repo, "main")
            .unwrap()
            .pending_description
            .as_deref(),
        Some("the plan")
    );
}

/// The copy is the same change: closed on either branch, it lands as the
/// same commit — the open commit's sha, which is what both branches hold.
#[test]
fn closing_on_both_branches_lands_one_sha() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
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

    let report = run_start(
        &fx,
        SwitchOptions {
            target: Some("@".into()),
            branch: Some(Some("spike".into())),
            now: Some(NOW),
            ..Default::default()
        },
    );
    let carried = minted(&report).carried.clone().expect("carried");

    let close = |now: i64| {
        let (outcome, _) = ff_core::close(
            &repo,
            &ff_core::CloseOptions {
                now: Some(now),
                ..Default::default()
            },
            &prov(),
        )
        .unwrap();
        let ff_core::CommitOutcome::Closed { id, .. } = outcome;
        id
    };
    let on_spike = close(NOW + 10);
    assert_eq!(on_spike, carried);

    let (switch_report, _) = ff_core::switch(
        &repo,
        &SwitchOptions {
            target: Some("main".into()),
            now: Some(NOW + 20),
            argv: Vec::new(),
            ..Default::default()
        },
        &prov(),
    )
    .unwrap();
    assert!(
        matches!(
            switch_report.arrival,
            ff_core::ArrivalReport::Restored { .. }
        ),
        "{:?}",
        switch_report.arrival
    );
    assert_eq!(
        std::fs::read_to_string(fx.path().join("a.txt")).unwrap(),
        "dirty\n"
    );
    let on_main = close(NOW + 30);
    assert_eq!(on_main, on_spike, "one change, one sha, on both branches");
}

#[test]
fn rev_target_forks_and_parks() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    let first = fx.commit("first");
    fx.write("a.txt", "b\n");
    fx.commit("second");
    ident(&fx);
    fx.write("a.txt", "dirty on main\n");

    let report = run_start(
        &fx,
        SwitchOptions {
            target: Some(first.clone()),
            now: Some(NOW),
            ..Default::default()
        },
    );

    assert!(report.to.starts_with("ff/"), "{}", report.to);
    assert_eq!(
        fx.git(&["rev-parse", &format!("refs/heads/{}", report.to)])
            .trim(),
        first
    );
    assert!(report.parked.is_some());
    assert_eq!(fx.git(&["status", "--porcelain=v2"]), "", "opens clean");
    assert_eq!(
        std::fs::read_to_string(fx.path().join("a.txt")).unwrap(),
        "a\n",
        "worktree moved to the fork point"
    );
    assert!(
        first.starts_with(forked_from(&report)),
        "forked_from ({}) should be a short sha of {first}",
        forked_from(&report)
    );
    let meta = ff_core::branchmeta::read(&fx.repo(), &report.to).unwrap();
    assert_eq!(meta.forked_from.as_deref(), Some(forked_from(&report)));
}

/// A branch target with no `-b` is the branch itself: the switch continues
/// it, and mints nothing.
#[test]
fn a_branch_target_continues() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    fx.git(&["branch", "feature"]);
    fx.git(&["checkout", "-q", "feature"]);
    ident(&fx);
    let main_tip = fx.git(&["rev-parse", "main"]).trim().to_string();

    let report = run_start(
        &fx,
        SwitchOptions {
            target: Some("main".into()),
            now: Some(NOW),
            ..Default::default()
        },
    );

    assert_eq!(report.to, "main");
    assert_eq!(report.from, "feature");
    assert_eq!(report.minted, None, "nothing minted");
    assert_eq!(fx.git(&["symbolic-ref", "HEAD"]).trim(), "refs/heads/main");
    assert_eq!(fx.git(&["rev-parse", "main"]).trim(), main_tip);
    assert_eq!(
        fx.git(&["for-each-ref", "refs/heads/"]).lines().count(),
        2,
        "no branch was added"
    );
}

/// `-b` on a branch target forks it: a new branch at the target's tip,
/// with the target recorded as its parent, and the target untouched.
#[test]
fn a_branch_target_with_dash_b_forks() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    ident(&fx);
    let main_tip = fx.git(&["rev-parse", "main"]).trim().to_string();

    let report = run_start(
        &fx,
        SwitchOptions {
            target: Some("main".into()),
            branch: Some(None),
            now: Some(NOW),
            ..Default::default()
        },
    );

    assert_ne!(report.to, "main", "a new branch is minted, not main itself");
    assert!(report.to.starts_with("ff/"), "bare -b mints a petname");
    assert_eq!(forked_from(&report), "main");
    assert_eq!(minted(&report).parent.as_deref(), Some("main"));
    assert_eq!(minted(&report).tracking, None);
    assert_eq!(
        fx.git(&["rev-parse", &format!("refs/heads/{}", report.to)])
            .trim(),
        main_tip
    );
    assert_eq!(
        fx.git(&["rev-parse", "main"]).trim(),
        main_tip,
        "main itself is untouched"
    );
    assert_eq!(
        fx.git(&["symbolic-ref", "HEAD"]).trim(),
        format!("refs/heads/{}", report.to)
    );
    let meta = ff_core::branchmeta::read(&fx.repo(), &report.to).unwrap();
    assert_eq!(meta.parent.as_deref(), Some("main"));
}

#[test]
fn dash_b_names_the_mint() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    ident(&fx);

    let report = run_start(
        &fx,
        SwitchOptions {
            branch: Some(Some("hotfix".into())),
            now: Some(NOW),
            ..Default::default()
        },
    );
    assert_eq!(report.to, "hotfix");
}

#[test]
fn dash_b_on_an_existing_name_errors() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    fx.git(&["branch", "existing"]);
    ident(&fx);

    let repo = fx.repo();
    let Err(err) = ff_core::switch(
        &repo,
        &SwitchOptions {
            branch: Some(Some("existing".into())),
            now: Some(NOW),
            ..Default::default()
        },
        &prov(),
    ) else {
        panic!("expected an error");
    };
    assert_eq!(err.to_string(), "a branch named existing already exists");
}

#[test]
fn every_start_mints() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    ident(&fx);

    let one = run_start(
        &fx,
        SwitchOptions {
            now: Some(NOW),
            ..Default::default()
        },
    );
    let two = run_start(
        &fx,
        SwitchOptions {
            now: Some(NOW + 10),
            ..Default::default()
        },
    );
    let three = run_start(
        &fx,
        SwitchOptions {
            now: Some(NOW + 20),
            ..Default::default()
        },
    );

    assert_ne!(one.to, two.to);
    assert_ne!(two.to, three.to);
    assert_ne!(one.to, three.to);
}

#[test]
fn start_never_commits() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    let first = fx.commit("first");
    fx.write("a.txt", "b\n");
    fx.commit("second");
    fx.git(&["branch", "other"]); // same tip as main
    ident(&fx);
    let main_before = commit_count(&fx, "main");
    let other_before = commit_count(&fx, "other");

    // Case 1: bare, forks trunk.
    fx.write("a.txt", "dirty1\n");
    run_start(
        &fx,
        SwitchOptions {
            now: Some(NOW),
            ..Default::default()
        },
    );
    assert_eq!(commit_count(&fx, "main"), main_before);
    assert_eq!(commit_count(&fx, "other"), other_before);

    // Case 5: rev target.
    fx.write("some.txt", "dirty2\n");
    let rev_report = run_start(
        &fx,
        SwitchOptions {
            target: Some(first.clone()),
            now: Some(NOW + 10),
            ..Default::default()
        },
    );
    assert_eq!(
        commit_count(&fx, &rev_report.to),
        "1",
        "the mint carries exactly the fork commit, nothing more"
    );
    assert_eq!(commit_count(&fx, "main"), main_before);
    assert_eq!(commit_count(&fx, "other"), other_before);

    // Case 6: branch target, forked.
    fx.write("more.txt", "dirty3\n");
    run_start(
        &fx,
        SwitchOptions {
            target: Some("other".into()),
            branch: Some(None),
            now: Some(NOW + 20),
            ..Default::default()
        },
    );
    assert_eq!(commit_count(&fx, "main"), main_before);
    assert_eq!(commit_count(&fx, "other"), other_before);
}

#[test]
fn message_describes_the_opened_change() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    ident(&fx);
    let before = ff_core::ops::OpLog::open(&fx.repo())
        .unwrap()
        .tip()
        .unwrap();

    let report = run_start(
        &fx,
        SwitchOptions {
            message: Some("next: the plan".into()),
            now: Some(NOW),
            ..Default::default()
        },
    );
    let repo = fx.repo();
    let meta = ff_core::branchmeta::read(&repo, &report.to).unwrap();
    assert_eq!(meta.pending_description.as_deref(), Some("next: the plan"));
    assert_eq!(
        fx.git(&[
            "log",
            "-1",
            "--format=%s",
            &format!("refs/heads/{}", report.to)
        ])
        .trim(),
        "init",
        "the message lands as a pending description, not a commit"
    );
    // The description rides the start's one operation, and a described
    // change has an identity from the start.
    assert_eq!(verb_ops_since(&repo, &before), vec!["switch".to_string()]);
    let record = tip_record(&repo);
    assert_eq!(record.verb, "switch");
    let transition = record
        .description
        .as_ref()
        .expect("a description transition");
    assert_eq!(transition.branch, report.to);
    assert_eq!(transition.new.as_deref(), Some("next: the plan"));
    let minted = record.change_id.as_ref().expect("a minted id");
    assert_eq!(minted.branch, report.to);
    assert_eq!(minted.new, meta.change_id);
    assert_eq!(meta.change_id.as_ref().map(String::len), Some(32));
    assert_eq!(meta.change_born, Some(NOW));
}

#[test]
fn describe_round_trips_and_journals() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    ident(&fx);
    let repo = fx.repo();
    let (report, _ctx) = ff_core::describe::set_pending(
        &repo,
        Some("the plan".into()),
        &prov(),
        Some(NOW),
        Vec::new(),
    )
    .unwrap();
    assert_eq!(report.new.as_deref(), Some("the plan"));
    assert_eq!(report.old, None);

    let record = tip_record(&repo);
    assert_eq!(record.verb, "describe");
    let transition = record.description.as_ref().unwrap();
    assert_eq!(transition.new.as_deref(), Some("the plan"));
    // A described change has an identity, journaled beside the description
    // so an undo takes both back.
    let minted = record
        .change_id
        .as_ref()
        .expect("the describe minted an id");
    assert_eq!(minted.old, None);
    let letters = minted.new.clone().expect("a minted id");
    assert_eq!(letters.len(), 32, "{letters}");
    assert_eq!(
        ff_core::branchmeta::read(&repo, "main")
            .unwrap()
            .change_id
            .as_deref(),
        Some(letters.as_str())
    );

    // Clearing round-trips; indexes untouched throughout.
    let index_before = fx.index_bytes();
    let (report, _ctx) =
        ff_core::describe::set_pending(&repo, None, &prov(), Some(NOW + 1), Vec::new()).unwrap();
    assert_eq!(report.old.as_deref(), Some("the plan"));
    assert_eq!(report.new, None);
    assert_eq!(
        fx.index_bytes(),
        index_before,
        "describe never touches the index"
    );
    let meta = ff_core::branchmeta::read(&repo, "main").unwrap();
    assert_eq!(meta.pending_description, None);
    assert_eq!(
        meta.change_id.as_deref(),
        Some(letters.as_str()),
        "the identity outlives the description: the change is still open"
    );
    assert!(
        tip_record(&repo).change_id.is_none(),
        "clearing a description mints nothing"
    );
}
