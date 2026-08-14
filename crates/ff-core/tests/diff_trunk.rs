//! Trunk resolution ladder: config, origin/HEAD, lone main/master, lone branch.

use ff_core::TrunkKind;
use ff_testsupport::Fixture;

fn trunk(fx: &Fixture) -> ff_core::Trunk {
    ff_core::trunk(&fx.repo()).expect("trunk resolved")
}

fn trunk_err(fx: &Fixture) -> String {
    ff_core::trunk(&fx.repo())
        .expect_err("trunk should fail")
        .to_string()
}

// --- rung 1: config ---

#[test]
fn config_wins() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    fx.git(&["branch", "develop"]);
    fx.set_config("fufu.trunk", "develop");
    let t = trunk(&fx);
    assert_eq!(t.name, "develop");
    assert_eq!(t.kind, TrunkKind::Local);
    assert_eq!(t.source, ff_core::TrunkSource::Config);
}

#[test]
fn config_naming_a_missing_branch_errors() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    fx.set_config("fufu.trunk", "nope");
    let msg = trunk_err(&fx);
    assert_eq!(msg, "ff: fufu.trunk names nope, which is not a branch here");
}

// --- rung 2: origin/HEAD ---

#[test]
fn origin_head_wins_over_lone_local() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    let sha = fx.git(&["rev-parse", "HEAD"]).trim().to_string();
    fx.git(&["update-ref", "refs/remotes/origin/trunkish", &sha]);
    fx.git(&[
        "symbolic-ref",
        "refs/remotes/origin/HEAD",
        "refs/remotes/origin/trunkish",
    ]);
    let t = trunk(&fx);
    assert_eq!(t.name, "trunkish");
    assert_eq!(
        t.kind,
        TrunkKind::Remote {
            remote: "origin".into()
        }
    );
    assert_eq!(t.source, ff_core::TrunkSource::OriginHead);
}

// --- rung 3: lone main or master ---

#[test]
fn lone_local_main() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    let t = trunk(&fx);
    assert_eq!(t.name, "main");
    assert_eq!(t.kind, TrunkKind::Local);
    assert_eq!(t.source, ff_core::TrunkSource::LoneMainOrMaster);
}

#[test]
fn lone_local_master() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    fx.git(&["branch", "-m", "master"]);
    let t = trunk(&fx);
    assert_eq!(t.name, "master");
    assert_eq!(t.kind, TrunkKind::Local);
    assert_eq!(t.source, ff_core::TrunkSource::LoneMainOrMaster);
}

// --- rung 4: lone branch ---

#[test]
fn lone_branch_of_any_name() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    fx.git(&["branch", "-m", "develop"]);
    let t = trunk(&fx);
    assert_eq!(t.name, "develop");
    assert_eq!(t.kind, TrunkKind::Local);
    assert_eq!(t.source, ff_core::TrunkSource::LoneBranch);
}

#[test]
fn main_beside_a_feature_branch() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    fx.git(&["branch", "feature-x"]);
    let t = trunk(&fx);
    assert_eq!(t.name, "main");
    assert_eq!(t.source, ff_core::TrunkSource::LoneMainOrMaster);
}

// --- ambiguity ---

#[test]
fn main_and_master_is_ambiguous() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    fx.git(&["branch", "master"]);
    let msg = trunk_err(&fx);
    assert_eq!(
        msg,
        "ff: cannot tell which branch is trunk (candidates: main, master); \
         set one with ff config trunk <branch>"
    );
}

// --- remote-only trunk ---

#[test]
fn remote_only_trunk() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    // Create a second branch so we can leave main
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
    let t = trunk(&fx);
    assert_eq!(t.name, "main");
    assert_eq!(
        t.kind,
        TrunkKind::Remote {
            remote: "origin".into()
        }
    );
    assert_eq!(t.full_ref, "refs/remotes/origin/main");
}

// --- config trunk: slashed locals ---

#[test]
fn config_trunk_accepts_a_slashed_local_branch() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    fx.git(&["branch", "feature/main"]);
    fx.set_config("fufu.trunk", "feature/main");
    let t = trunk(&fx);
    assert_eq!(t.name, "feature/main");
    assert_eq!(t.kind, TrunkKind::Local);
    assert_eq!(t.source, ff_core::TrunkSource::Config);
    assert_eq!(t.full_ref, "refs/heads/feature/main");
}

#[test]
fn config_trunk_prefers_local_over_remote_on_collision() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    // Two different commits so we can prove which ref was chosen
    fx.write("b.txt", "b\n");
    fx.commit("second");
    let local_sha = fx.git(&["rev-parse", "HEAD"]).trim().to_string();
    let remote_sha = fx.git(&["rev-parse", "HEAD~1"]).trim().to_string();
    assert_ne!(local_sha, remote_sha, "test fixture: commits must differ");
    // Create refs/heads/origin/main pointing to the newer commit
    fx.git(&["branch", "origin/main"]);
    fx.git(&["update-ref", "refs/heads/origin/main", &local_sha]);
    // Create refs/remotes/origin/main pointing to the older commit
    fx.git(&["update-ref", "refs/remotes/origin/main", &remote_sha]);
    fx.set_config("fufu.trunk", "origin/main");
    let t = trunk(&fx);
    assert_eq!(t.full_ref, "refs/heads/origin/main");
    assert_eq!(t.kind, TrunkKind::Local);
    assert_eq!(t.name, "origin/main");
    // Prove the local ref (newer commit) was chosen, not the remote
    let tip = fx
        .git(&["rev-parse", "refs/heads/origin/main"])
        .trim()
        .to_string();
    assert_eq!(tip, local_sha);
}

#[test]
fn config_trunk_still_resolves_remote_qualified() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    // Multiple local branches so lone-branch rules don't apply
    fx.git(&["branch", "other"]);
    let sha = fx.git(&["rev-parse", "HEAD"]).trim().to_string();
    fx.git(&["update-ref", "refs/remotes/origin/main", &sha]);
    fx.set_config("fufu.trunk", "origin/main");
    let t = trunk(&fx);
    assert_eq!(t.name, "main");
    assert_eq!(
        t.kind,
        TrunkKind::Remote {
            remote: "origin".into()
        }
    );
    assert_eq!(t.source, ff_core::TrunkSource::Config);
    assert_eq!(t.full_ref, "refs/remotes/origin/main");
}

// --- trunk never writes ---

#[test]
fn trunk_never_writes() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    let refs_before = fx.git(&["for-each-ref", "--format=%(refname)"]);
    let config_before = std::fs::read(fx.path().join(".git/config")).expect("read config before");
    let _ = trunk(&fx);
    let refs_after = fx.git(&["for-each-ref", "--format=%(refname)"]);
    let config_after = std::fs::read(fx.path().join(".git/config")).expect("read config after");
    assert_eq!(refs_before, refs_after, "refs unchanged");
    assert_eq!(config_before, config_after, "config unchanged");
}
