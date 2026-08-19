//! Differential contract for the resolution path: a held restack, resolved
//! through fufu's hold → resolve → fix → `done`, must land byte-identical
//! commits to `git rebase` stopped at the same conflict, fixed with the same
//! text and continued. Same full shas, same trees, same messages — measured
//! tip-down, so the fix lands in the commit git puts it in, not just in the
//! tip.

use ff_testsupport::Fixture;

const NOW: i64 = 1_799_999_999;

/// The fix both sides write over the markers, in the working tree.
const RESOLUTION: &str = "1\n2\n3-merged\n4\n5\n6\n";

/// fufu's side must be configured to the same identity real git will use for
/// the oracle's rebase (`GIT_COMMITTER_NAME`/`GIT_COMMITTER_EMAIL` from
/// `Fixture`'s hermetic env) — otherwise the objects can never match
/// byte-for-byte.
fn ident(fx: &Fixture) {
    fx.set_config("user.name", "Fixture Committer");
    fx.set_config("user.email", "committer@fixture.test");
}

fn prov() -> ff_core::Provenance {
    ff_core::Provenance::new("pre", Some("ff restack".into()))
}

/// The standard conflicting stack, shared by every test so the two sides of
/// a differential cannot drift apart in setup:
///
/// ```text
/// c0 ─ m1            (main)
///  └─ f1 ─ f2        (feature)
/// ```
///
/// `f1` and `m1` disagree on line 3 of `f.txt`, so the replay of `f1`
/// onto main's tip conflicts. `f2` adds a file the conflict never touches,
/// so it replays clean above a fixed `f1` — and, because the one fix text
/// both sides write must be complete for both, the clean commit owns no
/// line of the conflicting file.
fn conflict_stack(fx: &Fixture) -> String {
    fx.write("f.txt", "1\n2\n3\n4\n5\n6\n");
    let c0 = fx.commit("c0");
    fx.git(&["switch", "-q", "-c", "feature"]);
    fx.write("f.txt", "1\n2\n3-feature\n4\n5\n6\n");
    let _f1 = fx.commit("f1");
    fx.write("g.txt", "g-two\n");
    let _f2 = fx.commit("f2");
    fx.git(&["switch", "-q", "main"]);
    fx.write("f.txt", "1\n2\n3-main\n4\n5\n6\n");
    let _m1 = fx.commit("m1");
    c0
}

/// The fufu side of the flow: the restack holds, the resolution opens, the
/// same fix goes into the working tree, and `done` lands it.
fn resolve_flow(fx: &Fixture) {
    // Prime the op log: the first fufu call on a fixture bootstraps it from
    // observed state, which would otherwise masquerade as the hold's own
    // capture.
    ff_core::ops::reconcile(&fx.repo(), NOW).unwrap();

    let (outcome, _ctx) = restack_call(fx);
    match outcome {
        ff_core::RestackOutcome::Held(_) => {}
        other => panic!("the conflicting restack must hold, got {other:?}"),
    }

    let (outcome, _ctx) = resolve_call(fx);
    match outcome {
        ff_core::ResolveOutcome::Opened(report) => {
            assert_eq!(
                report.files,
                vec!["f.txt".to_string()],
                "the markers stand in the one conflicting file"
            );
        }
        other => panic!("the resolution must open, got {other:?}"),
    }

    // The same fix the git oracle writes, in the working tree.
    fx.write("f.txt", RESOLUTION);

    let (outcome, _ctx) = done_call(fx);
    match outcome {
        ff_core::DoneOutcome::Resolved(report) => {
            assert_eq!(report.branch, "feature");
            assert_eq!(report.verb, "restack");
        }
        other => panic!("the resolution must land, got {other:?}"),
    }
}

fn restack_call(fx: &Fixture) -> (ff_core::RestackOutcome, ff_core::ops::VerbContext) {
    let repo = fx.repo();
    ff_core::restack::restack(
        &repo,
        Some("feature".into()),
        None,
        &prov(),
        Some(NOW),
        vec![],
    )
    .unwrap()
}

fn resolve_call(fx: &Fixture) -> (ff_core::ResolveOutcome, ff_core::ops::VerbContext) {
    let repo = fx.repo();
    ff_core::resolve::resolve(&repo, false, &prov(), Some(NOW), vec![]).unwrap()
}

fn done_call(fx: &Fixture) -> (ff_core::DoneOutcome, ff_core::ops::VerbContext) {
    let repo = fx.repo();
    ff_core::done::done(&repo, false, &prov(), Some(NOW), vec![]).unwrap()
}

/// The user-visible history of `feature`, tip down, the way the other
/// differentials read it: full sha, tree, identities, dates, subject.
fn history_of(fx: &Fixture) -> String {
    fx.git(&[
        "log",
        "--format=%H|%T|%an|%ad|%cn|%cd|%s",
        "--date=raw",
        "feature",
    ])
}

/// The tree of the commit with that subject on `feature` — the assertion
/// that the fix landed in the commit that owned the conflict, on either side.
fn tree_of_subject(fx: &Fixture, subject: &str) -> String {
    let out = fx.git(&["log", "--format=%T%x09%s", "feature"]);
    out.lines()
        .filter_map(|line| line.split_once('\t'))
        .find(|(_, s)| *s == subject)
        .map(|(tree, _)| tree.to_string())
        .expect("a commit with that subject on feature")
}

/// The git oracle: `git rebase` stops at `f1`'s conflict, the test writes
/// the resolution over the markers, and `rebase --continue` lands `f1` and
/// replays `f2`.
fn git_oracle_resolve(fx: &Fixture, old_base: &str) {
    let stopped = fx.try_git(&[
        "rebase",
        "--update-refs",
        "--no-keep-empty",
        "--onto",
        "main",
        old_base,
        "feature",
    ]);
    assert!(
        !stopped.status.success(),
        "the rebase must stop on f1's conflict: {}",
        String::from_utf8_lossy(&stopped.stderr)
    );
    let markers = std::fs::read_to_string(fx.path().join("f.txt")).unwrap();
    assert!(
        markers.contains("<<<<<<<"),
        "the stop must leave a conflicted f.txt: {markers}"
    );

    fx.write("f.txt", RESOLUTION);
    fx.git(&["add", "-A"]);
    fx.git_env_in(
        &fx.path(),
        &["rebase", "--continue"],
        &[
            ("GIT_COMMITTER_DATE", &format!("@{NOW} +0000")),
            ("GIT_EDITOR", "true"),
        ],
    );
}

/// The whole point: same shas, same trees, same messages, tip down.
#[test]
fn a_resolved_restack_matches_git_rebase() {
    let fx_ff = Fixture::new();
    ident(&fx_ff);
    let c0_ff = conflict_stack(&fx_ff);

    let fx_git = Fixture::new();
    ident(&fx_git);
    let c0_git = conflict_stack(&fx_git);

    assert_eq!(c0_ff, c0_git, "setup must be lockstep before any rewrite");

    fx_ff.git(&["switch", "-q", "feature"]);
    resolve_flow(&fx_ff);

    git_oracle_resolve(&fx_git, &c0_git);

    assert_eq!(
        fx_ff.git(&["rev-parse", "feature"]).trim(),
        fx_git.git(&["rev-parse", "feature"]).trim(),
        "feature diverged between fufu and git"
    );

    let history_ff = history_of(&fx_ff);
    let history_git = history_of(&fx_git);
    assert_eq!(
        history_ff, history_git,
        "every commit must be byte-identical, tip down\nfufu:\n{history_ff}\ngit:\n{history_git}"
    );
}

/// Resolving once at the end and resolving at git's stop have to agree
/// about which commit owns the fix, or the feature is a squash wearing a
/// disguise: the *conflicting* commit's tree must match git's, not just
/// the tip's.
#[test]
fn the_fix_lands_in_the_same_commit_git_puts_it_in() {
    let fx_ff = Fixture::new();
    ident(&fx_ff);
    let c0_ff = conflict_stack(&fx_ff);

    let fx_git = Fixture::new();
    ident(&fx_git);
    let c0_git = conflict_stack(&fx_git);

    assert_eq!(c0_ff, c0_git, "setup must be lockstep before any rewrite");

    fx_ff.git(&["switch", "-q", "feature"]);
    resolve_flow(&fx_ff);

    git_oracle_resolve(&fx_git, &c0_git);

    assert_eq!(
        tree_of_subject(&fx_ff, "f1"),
        tree_of_subject(&fx_git, "f1"),
        "the fix must land in f1 — the commit the conflict lived in — on both sides"
    );
    assert_ne!(
        tree_of_subject(&fx_ff, "f1"),
        tree_of_subject(&fx_ff, "f2"),
        "and f2's tree must still be its own"
    );
}

/// The commit above the conflict replays clean, and both sides must agree
/// on what it lands with: its own file, over the fixed one.
#[test]
fn a_clean_commit_above_a_conflict_is_untouched_either_way() {
    let fx_ff = Fixture::new();
    ident(&fx_ff);
    let c0_ff = conflict_stack(&fx_ff);

    let fx_git = Fixture::new();
    ident(&fx_git);
    let c0_git = conflict_stack(&fx_git);

    assert_eq!(c0_ff, c0_git, "setup must be lockstep before any rewrite");

    fx_ff.git(&["switch", "-q", "feature"]);
    resolve_flow(&fx_ff);

    git_oracle_resolve(&fx_git, &c0_git);

    assert_eq!(
        tree_of_subject(&fx_ff, "f2"),
        tree_of_subject(&fx_git, "f2"),
        "f2 replays its own file above the fix, on both sides"
    );
}
