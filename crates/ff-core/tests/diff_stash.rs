//! The legacy park, read for the fold. Before the open commit was the park,
//! fufu parked as `git stash push -u -m "fufu: wip on <branch>"` plus
//! `refs/fufu/parked/<branch>`; nothing writes that shape now, and what is
//! left of the reader is the drop that spends an entry the way `git reflog
//! delete --rewrite` would, and the demotion of a ref whose entry was
//! dropped outside fufu. The fold itself is in `park.rs`.

use ff_core::gix;
use ff_core::stash;
use ff_core::{ArrivalReport, SwitchOptions};
use ff_testsupport::{Fixture, legacy_park};

const NOW: i64 = 1_700_000_000;

fn ident(fx: &Fixture) {
    fx.set_config("user.name", "Stash User");
    fx.set_config("user.email", "stash@test");
}

fn switch_to(fx: &Fixture, target: &str, now: i64) -> ff_core::SwitchReport {
    let repo = fx.repo();
    ff_core::switch(
        &repo,
        &SwitchOptions {
            target: Some(target.into()),
            now: Some(now),
            argv: vec!["ff".into(), "switch".into(), target.into()],
            ..Default::default()
        },
        &ff_core::Provenance::new("pre", Some(format!("ff switch {target}"))),
    )
    .unwrap()
    .0
}

#[test]
fn drop_by_identity_matches_git_reflog_delete_rewrite() {
    let make = || {
        let fx = Fixture::new();
        fx.write("a.txt", "base\n");
        fx.commit("init");
        ident(&fx);
        for n in 1..=3 {
            fx.write("a.txt", &format!("change {n}\n"));
            fx.git(&["stash", "push", "-u", "-m", &format!("entry {n}")]);
        }
        fx
    };

    let ours = make();
    let control = make();

    // Drop the middle entry (stash@{1} = "entry 2") by identity on ours,
    // by position on the control.
    let middle = ours.git(&["rev-parse", "stash@{1}"]).trim().to_string();
    let repo = ours.repo();
    stash::drop_stash_entry(&repo, gix::ObjectId::from_hex(middle.as_bytes()).unwrap()).unwrap();
    control.git(&["reflog", "delete", "--rewrite", "--updateref", "stash@{1}"]);

    assert_eq!(
        ours.git(&["stash", "list", "--format=%gs %H"]),
        control.git(&["stash", "list", "--format=%gs %H"]),
        "stash list identical after drop"
    );
    assert_eq!(
        ours.git(&["rev-parse", "refs/stash"]),
        control.git(&["rev-parse", "refs/stash"]),
        "refs/stash target identical"
    );
    let ours_log = std::fs::read_to_string(ours.path().join(".git/logs/refs/stash")).unwrap();
    let control_log = std::fs::read_to_string(control.path().join(".git/logs/refs/stash")).unwrap();
    assert_eq!(ours_log, control_log, "reflog bytes identical after drop");

    // Dropping the last two empties the stack and removes the ref.
    let repo = ours.repo();
    for rev in ["stash@{0}", "stash@{0}"] {
        let sha = ours.git(&["rev-parse", rev]).trim().to_string();
        stash::drop_stash_entry(&repo, gix::ObjectId::from_hex(sha.as_bytes()).unwrap()).unwrap();
    }
    let out = ours.try_git(&["rev-parse", "--verify", "refs/stash"]);
    assert!(!out.status.success(), "refs/stash gone once empty");
}

#[test]
fn externally_dropped_stash_demotes_the_parked_ref() {
    let fx = Fixture::new();
    fx.write("a.txt", "base\n");
    fx.commit("init");
    fx.git(&["branch", "other"]);
    ident(&fx);
    fx.write("a.txt", "parked\n");
    legacy_park(&fx, "main");

    // The user pops it with real git behind fufu's back.
    fx.git(&["stash", "pop", "-q"]);
    fx.git(&["checkout", "-q", "--", "."]); // and discards, for a clean tree

    switch_to(&fx, "other", NOW);
    let back = switch_to(&fx, "main", NOW + 1);
    assert!(
        matches!(back.arrival, ArrivalReport::Invalidated { .. }),
        "{:?}",
        back.arrival
    );
    assert!(stash::parked_entry(&fx.repo(), "main").unwrap().is_none());
}
