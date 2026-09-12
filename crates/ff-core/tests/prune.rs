//! Contract for `prune`: what gone means, how a gone branch is classified,
//! how the branches stacked on a delete are re-aimed, and that `branch -d`
//! records the pointer move the prune shares with it.

use ff_core::model::Kept;
use ff_core::{Provenance, branch, branchmeta, prune};
use ff_testsupport::Fixture;

const NOW: i64 = 1_799_999_999;

fn prov() -> Provenance {
    Provenance::new("pre", Some("ff branch".into()))
}

/// A repository on `main` with origin configured and one commit.
fn repo() -> Fixture {
    let fx = Fixture::new();
    fx.set_config("user.name", "Fixture Committer");
    fx.set_config("user.email", "committer@fixture.test");
    fx.set_config("remote.origin.url", "/nonexistent/remote.git");
    fx.set_config("remote.origin.fetch", "+refs/heads/*:refs/remotes/origin/*");
    fx.write("root.txt", "root\n");
    fx.commit("root");
    fx
}

/// A branch at `main` with an upstream of its own name, its tracking ref
/// absent, and no record yet.
fn configured(fx: &Fixture, name: &str) {
    fx.git(&["branch", name]);
    fx.set_config(&format!("branch.{name}.remote"), "origin");
    fx.set_config(
        &format!("branch.{name}.merge"),
        &format!("refs/heads/{name}"),
    );
}

fn seen(fx: &Fixture, name: &str, at: &str) {
    fx.git(&["update-ref", &format!("refs/fufu/seen/{name}"), at]);
}

fn published(fx: &Fixture, name: &str, at: &str) {
    fx.git(&["update-ref", &format!("refs/fufu/published/{name}"), at]);
}

fn tip(fx: &Fixture, rev: &str) -> String {
    fx.git(&["rev-parse", rev]).trim().to_string()
}

#[test]
fn gone_needs_all_three_parts() {
    let fx = repo();
    let main = tip(&fx, "main");
    // No upstream at all.
    fx.git(&["branch", "plain"]);
    seen(&fx, "plain", &main);
    assert!(!prune::is_gone(&fx.repo(), "plain").unwrap());

    // An upstream and no record: the fresh clone's shape.
    configured(&fx, "fresh");
    assert!(!prune::is_gone(&fx.repo(), "fresh").unwrap());

    // An upstream, a record, and the tracking ref standing: not gone.
    configured(&fx, "standing");
    seen(&fx, "standing", &main);
    fx.git(&["update-ref", "refs/remotes/origin/standing", &main]);
    assert!(!prune::is_gone(&fx.repo(), "standing").unwrap());

    // All three, through seen, and through published alone.
    configured(&fx, "gone");
    seen(&fx, "gone", &main);
    assert!(prune::is_gone(&fx.repo(), "gone").unwrap());
    configured(&fx, "sent");
    published(&fx, "sent", &main);
    assert!(prune::is_gone(&fx.repo(), "sent").unwrap());
}

/// An upstream under another branch's name is the base the branch was cut
/// from, not a copy: never gone, whatever the record says.
#[test]
fn an_upstream_that_is_a_base_is_not_gone() {
    let fx = repo();
    let main = tip(&fx, "main");
    fx.git(&["branch", "cut"]);
    fx.set_config("branch.cut.remote", "origin");
    fx.set_config("branch.cut.merge", "refs/heads/main");
    seen(&fx, "cut", &main);
    assert!(!prune::is_gone(&fx.repo(), "cut").unwrap());
}

#[test]
fn plan_classifies_the_kept_kinds() {
    let fx = repo();
    let main = tip(&fx, "main");

    // Current: gone and underfoot.
    fx.set_config("branch.main.remote", "origin");
    fx.set_config("branch.main.merge", "refs/heads/main");
    seen(&fx, "main", &main);

    // Ahead: one commit past the record.
    fx.git(&["switch", "-q", "-c", "ahead"]);
    fx.write("ahead.txt", "ahead\n");
    let ahead_tip = fx.commit("ahead");
    fx.git(&["switch", "-q", "main"]);
    fx.set_config("branch.ahead.remote", "origin");
    fx.set_config("branch.ahead.merge", "refs/heads/ahead");
    seen(&fx, "ahead", &main);
    let _ = ahead_tip;

    // Held: a rewrite waits on it.
    configured(&fx, "held");
    seen(&fx, "held", &main);
    ff_core::held::set(
        &fx.repo(),
        "held",
        Some(ff_core::held::Held {
            intent: ff_core::held::Intent::Restack {
                branch: "held".into(),
                onto: "refs/heads/main".into(),
            },
            at: ff_core::futures::At::OpenChange,
            paths: vec!["a.txt".into()],
            time: NOW,
        }),
    )
    .unwrap();

    // Level: gone and level with the record, so it goes.
    configured(&fx, "level");
    seen(&fx, "level", &main);

    // A record whose commit is unreadable: kept, never deleted on a guess.
    // git refuses to write such a ref, so the loose file is written by hand.
    configured(&fx, "lost");
    let lost = fx.path().join(".git/refs/fufu/seen");
    std::fs::create_dir_all(&lost).unwrap();
    std::fs::write(lost.join("lost"), format!("{}\n", "d".repeat(40))).unwrap();

    let plan = prune::plan(&fx.repo(), "main").unwrap();
    let deleted: Vec<&str> = plan.deletes.iter().map(|d| d.name.as_str()).collect();
    assert_eq!(deleted, vec!["level"]);
    let kept: Vec<(&str, &Kept)> = plan
        .kept
        .iter()
        .map(|k| (k.name.as_str(), &k.reason))
        .collect();
    assert_eq!(
        kept,
        vec![
            ("ahead", &Kept::Ahead { count: 1 }),
            (
                "held",
                &Kept::Held {
                    verb: "restack".into()
                }
            ),
            ("lost", &Kept::Ahead { count: 1 }),
            ("main", &Kept::Current),
        ]
    );
}

/// `gamma` on `beta` on `alpha`, all gone, `delta` on `gamma`: delta lands
/// on what alpha sat on, and no re-aim is recorded for a branch that is
/// itself deleted.
#[test]
fn plan_reaims_through_a_chain_of_deletes() {
    let fx = repo();
    let main = tip(&fx, "main");
    fx.git(&["branch", "base"]);
    for name in ["alpha", "beta", "gamma"] {
        configured(&fx, name);
        seen(&fx, name, &main);
    }
    fx.git(&["branch", "delta"]);
    // Opened after the config was written: the snapshot is read once.
    let repo = fx.repo();
    let parent = |name: &str, parent: &str| {
        let mut meta = branchmeta::read(&repo, name).unwrap();
        meta.parent = Some(parent.into());
        branchmeta::write(&repo, name, &meta).unwrap();
    };
    parent("alpha", "base");
    parent("beta", "alpha");
    parent("gamma", "beta");
    parent("delta", "gamma");

    let plan = prune::plan(&repo, "main").unwrap();
    let deleted: Vec<&str> = plan.deletes.iter().map(|d| d.name.as_str()).collect();
    assert_eq!(deleted, vec!["alpha", "beta", "gamma"]);
    assert_eq!(plan.reaims.len(), 1, "{:?}", plan.reaims);
    assert_eq!(plan.reaims[0].branch, "delta");
    assert_eq!(plan.reaims[0].old.as_deref(), Some("gamma"));
    assert_eq!(plan.reaims[0].new.as_deref(), Some("base"));
    assert_eq!(
        plan.reaimed(&repo, "gamma"),
        vec![ff_core::Reaim {
            branch: "delta".into(),
            onto: Some("base".into()),
        }]
    );
}

/// The prune writes the re-aim, and one undo puts the parent link and the
/// branches back.
#[test]
fn prune_writes_the_reaim_and_undo_restores_it() {
    let fx = repo();
    let main = tip(&fx, "main");
    configured(&fx, "alpha");
    seen(&fx, "alpha", &main);
    fx.git(&["branch", "beta"]);
    let repo = fx.repo();
    let mut meta = branchmeta::read(&repo, "beta").unwrap();
    meta.parent = Some("alpha".into());
    branchmeta::write(&repo, "beta", &meta).unwrap();

    let (report, ctx) = prune::prune(
        &repo,
        prune::PruneOptions {
            dry_run: false,
            fetched: false,
        },
        &prov(),
        Some(NOW),
        vec![],
    )
    .unwrap();
    assert!(ctx.is_some());
    assert_eq!(report.pruned.len(), 1);
    assert_eq!(report.pruned[0].reaimed[0].branch, "beta");
    assert_eq!(report.pruned[0].reaimed[0].onto.as_deref(), Some("main"));
    assert_eq!(branchmeta::read(&repo, "beta").unwrap().parent, None);
    assert!(
        !fx.try_git(&["rev-parse", "--verify", "-q", "refs/heads/alpha"])
            .status
            .success()
    );

    ff_core::undo(
        &repo,
        &ff_core::RewindOptions {
            force: false,
            now: Some(NOW + 10),
            argv: vec![],
        },
        &prov(),
    )
    .unwrap();
    assert_eq!(
        branchmeta::read(&repo, "beta").unwrap().parent.as_deref(),
        Some("alpha")
    );
    assert!(
        fx.try_git(&["rev-parse", "--verify", "-q", "refs/heads/alpha"])
            .status
            .success()
    );
}

/// A dry run, and a run with nothing to delete, each write nothing.
#[test]
fn prune_writes_nothing_under_dry_run_or_with_nothing_to_delete() {
    let fx = repo();
    let main = tip(&fx, "main");
    let (report, ctx) = prune::prune(
        &fx.repo(),
        prune::PruneOptions {
            dry_run: false,
            fetched: false,
        },
        &prov(),
        Some(NOW),
        vec![],
    )
    .unwrap();
    assert!(report.pruned.is_empty() && report.kept.is_empty());
    assert!(ctx.is_some(), "a real run took its capture");

    configured(&fx, "alpha");
    seen(&fx, "alpha", &main);
    let (report, ctx) = prune::prune(
        &fx.repo(),
        prune::PruneOptions {
            dry_run: true,
            fetched: true,
        },
        &prov(),
        Some(NOW),
        vec![],
    )
    .unwrap();
    assert!(ctx.is_none());
    assert!(report.dry_run && report.fetched);
    assert_eq!(report.pruned[0].name, "alpha");
    assert!(
        fx.try_git(&["rev-parse", "--verify", "-q", "refs/heads/alpha"])
            .status
            .success()
    );
}

/// `ff branch -d` now records the pointer's move to trash, and undo brings
/// `refs/fufu/snap/<name>` back through it.
#[test]
fn delete_records_the_pointer_and_undo_restores_it() {
    let fx = repo();
    fx.git(&["switch", "-q", "-c", "side"]);
    fx.write("side.txt", "side\n");
    let repo = fx.repo();
    // A capture on `side` gives it a pointer into the log.
    ff_core::capture(&repo, &prov()).unwrap();
    let snap = tip(&fx, "refs/fufu/snap/side");
    fx.git(&["switch", "-q", "main"]);

    let (report, _) = branch::delete(&repo, "side", &prov(), Some(NOW + 1), vec![]).unwrap();
    assert_eq!(report.trash_ref.as_deref(), Some("refs/fufu/trash/side"));
    let log = ff_core::ops::OpLog::open(&repo).unwrap();
    let op = log.get(log.tip().unwrap().unwrap()).unwrap();
    let record = op
        .record()
        .unwrap()
        .cloned()
        .expect("a verb op has a record");
    assert_eq!(record.pointers.len(), 1);
    assert_eq!(record.pointers[0].from, "refs/fufu/snap/side");
    assert_eq!(record.pointers[0].to, "refs/fufu/trash/side");
    assert_eq!(record.pointers[0].tip, snap);
    assert!(
        !fx.try_git(&["rev-parse", "--verify", "-q", "refs/fufu/snap/side"])
            .status
            .success()
    );

    ff_core::undo(
        &repo,
        &ff_core::RewindOptions {
            force: false,
            now: Some(NOW + 10),
            argv: vec![],
        },
        &prov(),
    )
    .unwrap();
    assert_eq!(tip(&fx, "refs/fufu/snap/side"), snap);
    assert!(
        fx.try_git(&["rev-parse", "--verify", "-q", "refs/heads/side"])
            .status
            .success()
    );
}
