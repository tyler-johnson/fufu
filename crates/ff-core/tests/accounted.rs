//! Contract for `accounted_for`: a commit is accounted for when fufu's own
//! log names it as the `old` side of a rewrite or records a replay dropping
//! it as empty, and every failure mode — a log-less repo, an unreadable sha
//! — fails toward "not accounted for", never the other way.

use ff_core::Provenance;
use ff_testsupport::Fixture;

const NOW: i64 = 1_799_999_999;

/// `restack` reads the committer identity from the repo config, which the
/// fixture's hermetic env does not set; git itself gets its identity from
/// env vars, so this is only for the gix side.
fn ident(fx: &Fixture) {
    fx.set_config("user.name", "Fixture Committer");
    fx.set_config("user.email", "committer@fixture.test");
}

fn prov() -> Provenance {
    Provenance::new("pre", Some("ff test".into()))
}

/// A feature branch one commit past the fork, restacked onto a moved main so
/// the feature commit is a recorded rewrite. Returns the shas of the root
/// and of the feature commit before the restack.
fn restacked(fx: &Fixture) -> (String, String) {
    ident(fx);
    fx.write("m.txt", "main one\n");
    let root = fx.commit("main one");

    fx.git(&["switch", "-q", "-c", "feature", &root]);
    fx.write("f.txt", "feature\n");
    let feat = fx.commit("feature work");

    fx.git(&["switch", "-q", "main"]);
    fx.write("m.txt", "main two\n");
    fx.commit("main two");

    fx.git(&["switch", "-q", "feature"]);
    let repo = fx.repo();
    ff_core::restack::restack(
        &repo,
        None,
        None,
        &prov(),
        Some(NOW),
        vec!["ff".into(), "restack".into()],
    )
    .expect("the restack lands");

    (root, feat)
}

/// The same file, with the same contents, committed separately on each side
/// of a fork, main then moved on. Restacking makes the feature-side commit
/// introduce nothing over the new base, so the replay drops it. Returns its
/// sha.
fn dropped_commit(fx: &Fixture) -> String {
    ident(fx);
    fx.write("m.txt", "main one\n");
    let root = fx.commit("main one");

    fx.write("dup.txt", "shared\n");
    fx.commit("main writes the shared file");

    fx.git(&["switch", "-q", "-c", "feature", &root]);
    fx.write("dup.txt", "shared\n");
    let feat_dup = fx.commit("feature writes the same file");

    fx.git(&["switch", "-q", "main"]);
    fx.write("m.txt", "main two\n");
    fx.commit("main two");

    fx.git(&["switch", "-q", "feature"]);
    let repo = fx.repo();
    ff_core::restack::restack(
        &repo,
        None,
        None,
        &prov(),
        Some(NOW),
        vec!["ff".into(), "restack".into()],
    )
    .expect("the restack lands");

    feat_dup
}

#[test]
fn a_rewritten_commit_is_accounted_for() {
    let fx = Fixture::new();
    let (_, feat) = restacked(&fx);

    let found = ff_core::accounted_for(&fx.repo(), std::slice::from_ref(&feat))
        .expect("the lookup succeeds");
    assert_eq!(
        found,
        std::collections::HashSet::from([feat]),
        "the rewritten commit comes back accounted for"
    );
}

#[test]
fn a_stranger_commit_is_not_accounted_for() {
    let fx = Fixture::new();
    let (root, feat) = restacked(&fx);

    let alone = ff_core::accounted_for(&fx.repo(), std::slice::from_ref(&root))
        .expect("the lookup succeeds");
    assert!(
        alone.is_empty(),
        "a commit the log never rewrote comes back empty"
    );

    // One call, both directions: the rewritten one in, the stranger out.
    let both = ff_core::accounted_for(&fx.repo(), &[feat.clone(), root.clone()])
        .expect("the lookup succeeds");
    assert!(
        both.contains(&feat),
        "the rewritten commit is accounted for"
    );
    assert!(
        !both.contains(&root),
        "the stranger commit is not accounted for"
    );
    assert_eq!(both.len(), 1, "the set holds exactly the accounted member");
}

#[test]
fn a_dropped_commit_is_accounted_for() {
    let fx = Fixture::new();
    let feat_dup = dropped_commit(&fx);

    let found = ff_core::accounted_for(&fx.repo(), std::slice::from_ref(&feat_dup))
        .expect("the lookup succeeds");
    assert_eq!(
        found,
        std::collections::HashSet::from([feat_dup]),
        "the dropped commit comes back accounted for"
    );
}

#[test]
fn an_empty_log_accounts_for_nothing() {
    let fx = Fixture::new();
    fx.write("m.txt", "main one\n");
    let sha = fx.commit("main one");

    // A repo with no fufu log at all is an ordinary state, not a failure.
    let found = ff_core::accounted_for(&fx.repo(), std::slice::from_ref(&sha))
        .expect("a log-less repo answers, it does not error");
    assert!(
        found.is_empty(),
        "with no operations, nothing is accounted for"
    );
}

#[test]
fn an_unreadable_sha_drops_the_floor() {
    let fx = Fixture::new();
    let (_, feat) = restacked(&fx);
    let ghost = "a".repeat(40);

    // The ghost has no committer time, so the floor drops away and the
    // walk is unbounded — which must not stop the real sha being found.
    let found = ff_core::accounted_for(&fx.repo(), &[ghost.clone(), feat.clone()])
        .expect("an unreadable sha drops the floor, it does not fail the call");
    assert!(
        found.contains(&feat),
        "the real sha is found with the floor dropped"
    );
    assert!(
        !found.contains(&ghost),
        "the unreadable sha is not accounted for"
    );
}
