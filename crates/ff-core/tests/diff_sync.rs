//! Differential contract for `sync::sync`: each axis is a replay, and a
//! replay has an oracle — `git rebase --update-refs --no-keep-empty --onto`.
//! `ff sync` is two such replays (remote axis, then base axis), and this file
//! proves each lands the **same shas** the git recipe would, and that the two
//! axes run in the same order a person would run those two rebases in.
//!
//! Both sides write identical commit objects, which requires identical
//! committer identity and committer date. fufu's side takes `now: Some(NOW)`
//! and reads its identity from repo config (`ident`); the oracle gets
//! `GIT_COMMITTER_DATE=@{NOW} +0000` and its identity from `Fixture`'s
//! hermetic env. That is why this file drives `sync()` directly rather than
//! the `ff` binary, which has no way to pin its clock.

use ff_core::gix;
use ff_core::sync::SyncOptions;
use ff_core::{BaseAxis, Provenance, RemoteAxis, RestackOutcome, SyncReport};
use ff_testsupport::Fixture;

const NOW: i64 = 1_799_999_999;

/// fufu's side must be configured to the same identity real git will use for
/// the oracle's rebase (`GIT_COMMITTER_NAME`/`GIT_COMMITTER_EMAIL` from
/// `Fixture`'s hermetic env) — otherwise the objects can never match
/// byte-for-byte.
fn ident(fx: &Fixture) {
    fx.set_config("user.name", "Fixture Committer");
    fx.set_config("user.email", "committer@fixture.test");
}

fn prov() -> Provenance {
    Provenance::new("pre", Some("ff sync".into()))
}

fn sync_run(
    repo: &gix::Repository,
    pre: &ff_core::sync::Preflight,
    push: Option<bool>,
    fetched: bool,
    after: Option<&str>,
) -> SyncReport {
    let after = after.map(|h| gix::ObjectId::from_hex(h.as_bytes()).unwrap());
    ff_core::sync::sync(
        repo,
        pre,
        SyncOptions {
            push,
            fetched,
            tracking_after: after,
            now: Some(NOW),
            argv: vec!["ff".into(), "sync".into()],
        },
        &prov(),
    )
    .unwrap()
    .0
}

/// The user-visible history over the given refs. Two repositories agree when
/// `log_of` over the same refs returns the same string: same shas, same
/// trees, same identities, same dates, same subjects.
fn log_of(fx: &Fixture, refs: &[&str]) -> String {
    let mut args = vec!["log", "--format=%H|%T|%an|%ad|%cn|%cd|%s", "--date=raw"];
    args.extend_from_slice(refs);
    fx.git(&args)
}

/// Run the git oracle: `--onto <onto> <old_base> <branch>` names the range's
/// floor explicitly instead of letting git infer it, which is what fufu does.
/// `--no-keep-empty` brings the oracle onto fufu's rule: no empty commit
/// survives a replay.
fn git_oracle_restack(fx: &Fixture, onto: &str, old_base: &str, branch: &str, now: i64) {
    fx.git_env_in(
        &fx.path(),
        &[
            "rebase",
            "--update-refs",
            "--no-keep-empty",
            "--onto",
            onto,
            old_base,
            branch,
        ],
        &[("GIT_COMMITTER_DATE", &format!("@{now} +0000"))],
    );
}

/// Every worktree file as (repo-relative path, bytes), sorted by path.
fn worktree_files(fx: &Fixture) -> Vec<(String, Vec<u8>)> {
    let root = fx.path();
    let mut out = Vec::new();
    let mut dirs = vec![root.clone()];
    while let Some(dir) = dirs.pop() {
        for entry in std::fs::read_dir(&dir).unwrap() {
            let entry = entry.unwrap();
            let name = entry.file_name();
            if name == ".git" {
                continue;
            }
            let path = entry.path();
            if path.is_dir() {
                dirs.push(path);
            } else {
                let rel = path
                    .strip_prefix(&root)
                    .unwrap()
                    .to_string_lossy()
                    .replace(std::path::MAIN_SEPARATOR, "/");
                out.push((rel, std::fs::read(&path).unwrap()));
            }
        }
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

/// The tracking shape a fetch would have left behind at `tip`. The URL is
/// never contacted — the remote only has to be *configured* so fufu sees it.
fn track(fx: &Fixture, tip: &str) {
    fx.git(&["update-ref", "refs/remotes/origin/feature", tip]);
    fx.set_config("branch.feature.remote", "origin");
    fx.set_config("branch.feature.merge", "refs/heads/feature");
    fx.set_config("remote.origin.url", "/nonexistent/remote.git");
    fx.set_config("remote.origin.fetch", "+refs/heads/*:refs/remotes/origin/*");
}

/// No remote. `main` two commits ahead of where `feature` forked, `feature`
/// two commits of its own, standing on `feature` with a clean tree. Returns
/// the fork point — the range's floor for the oracle.
fn base_shape(fx: &Fixture) -> String {
    fx.write("root.txt", "root\n");
    let fork = fx.commit("c0");
    fx.git(&["switch", "-q", "-c", "feature"]);
    fx.write("a.txt", "a\n");
    let _f1 = fx.commit("f1");
    fx.write("b.txt", "b\n");
    let _f2 = fx.commit("f2");
    fx.git(&["switch", "-q", "main"]);
    fx.write("m1.txt", "m1\n");
    let _m1 = fx.commit("m1");
    fx.write("m2.txt", "m2\n");
    let _m2 = fx.commit("m2");
    fx.git(&["switch", "-q", "feature"]);
    fork
}

/// `feature` forked from `main` with one commit (f1) and was pushed once
/// (tracking at f1). The collaborator moved `origin/feature` to a divergent
/// line two commits up — the fetch will bring it to the returned tip. Leaves
/// the fixture standing on `feature` with tracking at the *before* position.
fn remote_shape(fx: &Fixture) -> String {
    fx.write("root.txt", "root\n");
    let _c0 = fx.commit("c0");
    fx.git(&["switch", "-q", "-c", "feature"]);
    fx.write("f.txt", "f\n");
    let before = fx.commit("f1");
    // The collaborator's line: two commits off the fork (main at c0),
    // divergent from feature's f1 — the fetch brings it in as `origin/feature`.
    fx.git(&["switch", "-q", "main"]);
    fx.git(&["switch", "-q", "-c", "collab"]);
    fx.write("c1.txt", "c1\n");
    let _c1 = fx.commit("c1");
    fx.write("c2.txt", "c2\n");
    let after = fx.commit("c2");
    fx.git(&["switch", "-q", "feature"]);
    track(fx, &before);
    after
}

/// `main` moved ahead **and** `origin/feature` moved ahead, independently,
/// with `feature` behind both. `feature` forked from `main` with one commit
/// (f1) and was pushed once (tracking at f1); `main` then added a commit and
/// the collaborator pushed a divergent line to `origin/feature`. Returns the
/// tip the fetch will bring; leaves tracking at the *before* position (f1)
/// and the fixture standing on `feature`.
fn both_shape(fx: &Fixture) -> String {
    fx.write("root.txt", "root\n");
    let _c0 = fx.commit("c0");
    fx.git(&["switch", "-q", "-c", "feature"]);
    fx.write("f.txt", "f\n");
    let before = fx.commit("f1");
    // The collaborator's line: off the fork (main at c0), divergent from
    // feature's f1 — the fetch brings it in as `origin/feature`.
    fx.git(&["switch", "-q", "main"]);
    fx.git(&["switch", "-q", "-c", "collab"]);
    fx.write("cb.txt", "cb\n");
    let after = fx.commit("cb");
    // main moves ahead of the fork.
    fx.git(&["switch", "-q", "main"]);
    fx.write("m1.txt", "m1\n");
    let _m1 = fx.commit("m1");
    fx.git(&["switch", "-q", "feature"]);
    track(fx, &before);
    after
}

/// A remote that moved and conflicts with the local commit (both rewrite the
/// same line of the same file), plus a `main` that also moved so the base
/// axis would otherwise have work. Leaves an uncommitted edit in a third,
/// unrelated file so the working tree is genuinely dirty. Returns the tip the
/// fetch will bring; leaves tracking at the *before* position (f1) and the
/// fixture standing on `feature`.
fn held_shape(fx: &Fixture) -> String {
    fx.write("root.txt", "root\n");
    let _c0 = fx.commit("root");
    fx.git(&["switch", "-q", "-c", "feature"]);
    fx.write("shared.txt", "base\n");
    let before = fx.commit("f1");
    track(fx, &before);
    // Both sides rewrite the same line of the same file...
    fx.git(&["switch", "-q", "-c", "collab"]);
    fx.write("shared.txt", "theirs\n");
    let after = fx.commit("collab");
    fx.git(&["switch", "-q", "feature"]);
    fx.write("shared.txt", "mine\n");
    let _local = fx.commit("local");
    // ...and main also moves, so the base axis has work it never gets.
    fx.git(&["switch", "-q", "main"]);
    fx.write("m1.txt", "m1\n");
    let _m1 = fx.commit("m1");
    fx.git(&["switch", "-q", "feature"]);
    // An uncommitted edit in a third, unrelated file: a genuinely dirty tree.
    fx.write("notes.txt", "wip\n");
    after
}

#[test]
fn the_base_axis_lands_gits_shas() {
    let fufu = Fixture::new();
    ident(&fufu);
    let _fork = base_shape(&fufu);
    let report = {
        let repo = fufu.repo();
        let pre = ff_core::sync::preflight(&repo).unwrap();
        sync_run(&repo, &pre, Some(false), false, None)
    };

    let oracle = Fixture::new();
    ident(&oracle);
    let fork = base_shape(&oracle);
    git_oracle_restack(&oracle, "main", &fork, "feature", NOW);

    assert_eq!(
        log_of(&fufu, &["feature", "main"]),
        log_of(&oracle, &["feature", "main"]),
        "sync's base axis must land the shas git's rebase would"
    );
    match report.base {
        BaseAxis::Ran { name, outcome, .. } => {
            assert_eq!(name, "main");
            match outcome {
                RestackOutcome::Restacked(_) => {}
                other => panic!("the base axis must replay, got {other:?}"),
            }
        }
        other => panic!("the base axis must act, got {other:?}"),
    }
}

#[test]
fn the_remote_axis_lands_gits_shas() {
    let fufu = Fixture::new();
    ident(&fufu);
    let after = remote_shape(&fufu);
    let report = {
        // preflight records where the tracking ref stood before; the fetch
        // moves it to the collaborator's divergent tip, which is handed in.
        let repo = fufu.repo();
        let pre = ff_core::sync::preflight(&repo).unwrap();
        fufu.git(&["update-ref", "refs/remotes/origin/feature", &after]);
        sync_run(&repo, &pre, None, true, Some(&after))
    };

    let oracle = Fixture::new();
    ident(&oracle);
    let after = remote_shape(&oracle);
    // The fetch, mirrored: the tracking ref stands at the collaborator's tip.
    oracle.git(&["update-ref", "refs/remotes/origin/feature", &after]);
    let mb = oracle
        .git(&["merge-base", "feature", "origin/feature"])
        .trim()
        .to_string();
    git_oracle_restack(&oracle, "origin/feature", &mb, "feature", NOW);

    assert_eq!(
        log_of(&fufu, &["feature"]),
        log_of(&oracle, &["feature"]),
        "sync's remote axis must land the shas git's rebase would"
    );
    match report.remote {
        RemoteAxis::Ran { name, outcome, .. } => {
            assert_eq!(name, "origin/feature");
            match outcome {
                RestackOutcome::Restacked(_) => {}
                other => panic!("the remote axis must replay, got {other:?}"),
            }
        }
        other => panic!("the remote axis must act, got {other:?}"),
    }
}

#[test]
fn both_axes_land_gits_two_rebases_in_that_order() {
    let fufu = Fixture::new();
    ident(&fufu);
    let after = both_shape(&fufu);
    let report = {
        let repo = fufu.repo();
        let pre = ff_core::sync::preflight(&repo).unwrap();
        fufu.git(&["update-ref", "refs/remotes/origin/feature", &after]);
        sync_run(&repo, &pre, None, true, Some(&after))
    };

    let oracle = Fixture::new();
    ident(&oracle);
    let after = both_shape(&oracle);
    // The fetch, mirrored: the tracking ref stands at the collaborator's tip.
    oracle.git(&["update-ref", "refs/remotes/origin/feature", &after]);
    // Sync's own order: the remote axis first...
    let mb1 = oracle
        .git(&["merge-base", "feature", "origin/feature"])
        .trim()
        .to_string();
    git_oracle_restack(&oracle, "origin/feature", &mb1, "feature", NOW);
    // ...then the base axis. The first rebase moved `feature`, so the merge
    // base with main is recomputed before the second rebase.
    let mb2 = oracle
        .git(&["merge-base", "feature", "main"])
        .trim()
        .to_string();
    git_oracle_restack(&oracle, "main", &mb2, "feature", NOW);

    assert_eq!(
        log_of(&fufu, &["feature", "main"]),
        log_of(&oracle, &["feature", "main"]),
        "both axes in sync's order must land the shas the two rebases would"
    );
    match report.remote {
        RemoteAxis::Ran { outcome, .. } => match outcome {
            RestackOutcome::Restacked(_) => {}
            other => panic!("the remote axis must replay, got {other:?}"),
        },
        other => panic!("the remote axis must act, got {other:?}"),
    }
    match report.base {
        BaseAxis::Ran { outcome, .. } => match outcome {
            RestackOutcome::Restacked(_) => {}
            other => panic!("the base axis must replay, got {other:?}"),
        },
        other => panic!("the base axis must act, got {other:?}"),
    }
}

#[test]
fn a_sync_that_holds_moves_nothing() {
    let fx = Fixture::new();
    ident(&fx);
    let after = held_shape(&fx);

    let repo = fx.repo();
    let pre = ff_core::sync::preflight(&repo).unwrap();
    // The fetch, simulated: the tracking ref moves to the collaborator's tip.
    fx.git(&["update-ref", "refs/remotes/origin/feature", &after]);

    // Capture, before the sync, every branch and remote ref's target and the
    // working tree. (Fufu's own `refs/fufu/*` bookkeeping is not part of the
    // user's repository — a hold is expected to record itself in the op log.)
    let refs_before = fx.git(&[
        "for-each-ref",
        "--format=%(refname) %(objectname)",
        "refs/heads",
        "refs/remotes",
    ]);
    let tree_before = worktree_files(&fx);

    let report = sync_run(&repo, &pre, None, true, Some(&after));

    match report.remote {
        RemoteAxis::Ran { outcome, .. } => match outcome {
            RestackOutcome::Held(_) => {}
            other => panic!("the replay must hold, got {other:?}"),
        },
        other => panic!("the remote axis must act, got {other:?}"),
    }
    match report.base {
        BaseAxis::Skipped => {}
        other => panic!("a hold stops the run, got {other:?}"),
    }
    assert_eq!(
        fx.git(&[
            "for-each-ref",
            "--format=%(refname) %(objectname)",
            "refs/heads",
            "refs/remotes",
        ]),
        refs_before,
        "a hold moved no branch or remote ref"
    );
    assert_eq!(
        worktree_files(&fx),
        tree_before,
        "a hold left the working tree exactly as it found it"
    );
}
