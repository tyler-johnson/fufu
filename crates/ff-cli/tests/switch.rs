//! `ff switch` end to end against the real binary, under its three
//! spellings. One verb, one rule — find the branch, else mint it — and the
//! ladder a target climbs: a branch here, a branch a remote holds, a
//! revision, nothing for trunk. `-b` forks a branch target or names the
//! mint; `-m` describes the change a mint opens and is refused where
//! nothing opens.

use std::path::Path;
use std::process::{Command, Output};

use ff_testsupport::Fixture;
use ff_testsupport::fixtures::null_device;

fn ff_at(dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_ff"))
        .current_dir(dir)
        .args(args)
        .env("GIT_CONFIG_GLOBAL", null_device())
        .env("GIT_CONFIG_SYSTEM", null_device())
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env_remove("GIT_AUTHOR_NAME")
        .env_remove("GIT_AUTHOR_EMAIL")
        .env_remove("GIT_COMMITTER_NAME")
        .env_remove("GIT_COMMITTER_EMAIL")
        .env_remove("FF_SESSION")
        .env_remove("CLAUDE_CODE_SESSION_ID")
        .output()
        .expect("spawn ff")
}

fn ff(fx: &Fixture, args: &[&str]) -> Output {
    ff_at(&fx.path(), args)
}

fn stdout(out: &Output) -> String {
    String::from_utf8(out.stdout.clone()).expect("utf-8 stdout")
}

fn stderr(out: &Output) -> String {
    String::from_utf8(out.stderr.clone()).expect("utf-8 stderr")
}

fn json(out: &Output) -> serde_json::Value {
    serde_json::from_str(&stdout(out)).expect("valid json")
}

fn ok(out: Output) -> Output {
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    out
}

/// A repository with `main` and a `feature` branch off it.
fn repo() -> Fixture {
    let fx = Fixture::new();
    fx.set_config("user.name", "Switch Tester");
    fx.set_config("user.email", "switch@test.test");
    fx.write("a.txt", "a\n");
    fx.commit("init");
    fx.git(&["branch", "feature"]);
    fx
}

/// A clone whose remote holds `spike` and nothing here tracks it.
fn clone_with_remote_spike() -> (Fixture, String) {
    let fx = Fixture::new_cloned();
    fx.write("a.txt", "a\n");
    let sha = fx.commit("init");
    fx.git(&["push", "-q", "origin", "main"]);
    fx.remote_git(&["update-ref", "refs/heads/spike", &sha]);
    fx.git(&["fetch", "-q", "origin"]);
    (fx, sha)
}

fn current(fx: &Fixture) -> String {
    fx.git(&["symbolic-ref", "--short", "HEAD"]).trim().into()
}

fn branch_exists(fx: &Fixture, name: &str) -> bool {
    !fx.git(&["for-each-ref", &format!("refs/heads/{name}")])
        .trim()
        .is_empty()
}

fn config(fx: &Fixture, key: &str) -> Option<String> {
    let out = fx.try_git(&["config", "--get", key]);
    out.status
        .success()
        .then(|| String::from_utf8(out.stdout).unwrap().trim().to_string())
}

// --- the spellings ---

/// `ff start` and `ff new` parse to switch: one envelope, one payload.
#[test]
fn start_and_new_are_spellings_of_switch() {
    let fx = repo();
    for spelling in ["start", "new", "sw", "switch"] {
        let out = ok(ff(&fx, &[spelling, "--json"]));
        let v = json(&out);
        assert_eq!(v["cmd"], "switch", "{spelling}: {v}");
        let switch = &v["data"]["switch"];
        assert_eq!(switch["from"], "main", "{spelling}: {v}");
        assert!(
            switch["to"].as_str().unwrap().starts_with("ff/"),
            "{spelling}: {v}"
        );
        assert_eq!(switch["minted"]["forked_from"], "main", "{spelling}: {v}");
        assert!(switch["minted"]["tracking"].is_null(), "{spelling}: {v}");
        assert_eq!(v["data"]["undo"], "ff undo");
        ok(ff(&fx, &["switch", "main"]));
    }
}

/// `ff help start` is switch's page.
#[test]
fn help_start_is_switch_page() {
    let fx = repo();
    let via_alias = stdout(&ok(ff(&fx, &["help", "start"])));
    let direct = stdout(&ok(ff(&fx, &["help", "switch"])));
    assert_eq!(via_alias, direct);
    assert!(direct.contains("`ff start`"), "{direct}");
    assert!(direct.contains("`ff new`"), "{direct}");
}

/// Every spelling is one operation, and one undo takes it back.
#[test]
fn one_undo_per_spelling() {
    let fx = repo();
    for spelling in ["start", "new", "switch"] {
        fx.write("a.txt", &format!("dirty under {spelling}\n"));
        let out = ok(ff(&fx, &[spelling]));
        let text = stdout(&out);
        assert!(text.contains("parked the open change on main"), "{text}");
        assert!(text.contains("minted ff/"), "{text}");
        assert!(text.contains("(forked from main)"), "{text}");
        assert!(text.contains("switched to ff/"), "{text}");
        assert!(
            !text.lines().any(|line| line.starts_with("open change on")),
            "the switched-to line says it: {text}"
        );
        let minted = current(&fx);
        assert_eq!(fx.git(&["status", "--porcelain=v2"]), "", "opens clean");

        ok(ff(&fx, &["undo"]));
        assert_eq!(current(&fx), "main");
        assert!(!branch_exists(&fx, &minted), "the mint is gone");
        assert_eq!(
            std::fs::read_to_string(fx.path().join("a.txt")).unwrap(),
            format!("dirty under {spelling}\n"),
            "the tree is dirty again"
        );
    }
}

// --- the ladder ---

/// A branch here continues; a prefix is enough.
#[test]
fn a_local_branch_continues() {
    let fx = repo();
    let out = ok(ff(&fx, &["switch", "feat", "--json"]));
    let v = json(&out);
    assert_eq!(v["data"]["switch"]["to"], "feature", "{v}");
    assert!(v["data"]["switch"]["minted"].is_null(), "{v}");
    assert_eq!(current(&fx), "feature");

    let text = stdout(&ok(ff(&fx, &["start", "main"])));
    assert_eq!(current(&fx), "main");
    assert!(text.contains("switched to main"), "{text}");
    assert!(!text.contains("minted"), "{text}");
}

/// A branch a remote holds is minted here under its own name, tracking it,
/// bare or qualified.
#[test]
fn a_remote_branch_is_minted_tracking_it() {
    let (fx, sha) = clone_with_remote_spike();
    let out = ok(ff(&fx, &["switch", "spike"]));
    let text = stdout(&out);
    assert!(
        text.contains("minted spike tracking origin/spike"),
        "{text}"
    );
    assert!(text.contains("switched to spike"), "{text}");
    assert_eq!(current(&fx), "spike");
    assert_eq!(fx.git(&["rev-parse", "spike"]).trim(), sha);
    assert_eq!(
        config(&fx, "branch.spike.remote").as_deref(),
        Some("origin")
    );
    assert_eq!(
        config(&fx, "branch.spike.merge").as_deref(),
        Some("refs/heads/spike")
    );

    // Listed under your branches now, not remote only; and the push has
    // nothing to send.
    let listing = stdout(&ok(ff(&fx, &["branch"])));
    assert!(!listing.contains("remote only:"), "{listing}");
    assert!(listing.contains("[spike]"), "{listing}");
    let push = json(&ok(ff(&fx, &["push", "-n", "--json"])));
    assert_eq!(push["cmd"], "push", "{push}");

    // Undo takes the branch and its section; redo brings both back.
    ok(ff(&fx, &["undo"]));
    assert_eq!(current(&fx), "main");
    assert!(!branch_exists(&fx, "spike"));
    assert_eq!(config(&fx, "branch.spike.remote"), None);
    ok(ff(&fx, &["redo"]));
    assert_eq!(current(&fx), "spike");
    assert_eq!(
        config(&fx, "branch.spike.remote").as_deref(),
        Some("origin")
    );

    // The qualified spelling, with a local branch of that name, continues it.
    ok(ff(&fx, &["switch", "main"]));
    let v = json(&ok(ff(&fx, &["switch", "origin/spike", "--json"])));
    assert_eq!(v["data"]["switch"]["to"], "spike", "{v}");
    assert!(v["data"]["switch"]["minted"].is_null(), "{v}");

    // The qualified spelling with no local branch mints it the same way.
    ok(ff(&fx, &["switch", "main"]));
    fx.git(&["branch", "-D", "spike"]);
    let v = json(&ok(ff(&fx, &["switch", "origin/spike", "--json"])));
    assert_eq!(v["data"]["switch"]["to"], "spike", "{v}");
    assert_eq!(
        v["data"]["switch"]["minted"]["tracking"], "origin/spike",
        "{v}"
    );
    assert!(
        v["data"]["switch"]["minted"]["forked_from"].is_null(),
        "{v}"
    );
    let op = json(&ok(ff(
        &fx,
        &["op", "log", "kind(op)", "-n", "1", "--json"],
    )));
    let newest = &op["data"]["ops"][0];
    assert_eq!(newest["verb"], "switch", "{op}");
    assert!(
        newest["summary"]
            .as_str()
            .unwrap()
            .contains("tracking origin/spike"),
        "{op}"
    );
}

/// A bare name two remotes hold is refused, naming both.
#[test]
fn a_name_two_remotes_hold_is_ambiguous() {
    let fx = repo();
    let sha = fx.git(&["rev-parse", "HEAD"]).trim().to_string();
    for remote in ["origin", "upstream"] {
        fx.set_config(&format!("remote.{remote}.url"), "file:///nonexistent");
        fx.set_config(
            &format!("remote.{remote}.fetch"),
            &format!("+refs/heads/*:refs/remotes/{remote}/*"),
        );
        fx.git(&["update-ref", &format!("refs/remotes/{remote}/spike"), &sha]);
    }
    let out = ff(&fx, &["switch", "spike", "--json"]);
    assert_eq!(out.status.code(), Some(1));
    let v = json(&out);
    assert_eq!(v["error"]["id"], "branch/ambiguous", "{v}");
    assert_eq!(
        v["error"]["message"],
        "ambiguous branch name spike: origin/spike, upstream/spike"
    );
    assert_eq!(current(&fx), "main");
}

/// A revision mints an anonymous branch at it; the old redirect line is
/// gone, and the mint line says where it forked.
#[test]
fn a_revision_mints_an_anonymous_branch() {
    let fx = repo();
    let first = fx.git(&["rev-parse", "HEAD"]).trim().to_string();
    fx.write("a.txt", "b\n");
    fx.commit("second");

    let out = ok(ff(&fx, &["switch", &first[..8]]));
    let text = stdout(&out);
    assert!(!text.contains("is a revision, not a branch"), "{text}");
    assert!(text.contains("minted ff/"), "{text}");
    assert!(
        text.contains(&format!("(forked from {})", &first[..8])),
        "{text}"
    );
    assert!(current(&fx).starts_with("ff/"));
    assert_eq!(fx.git(&["rev-parse", "HEAD"]).trim(), first);
}

/// A bare word that names nothing is `branch/not-found`, and the explain
/// entry says a remote's branch would have been a target.
#[test]
fn an_unknown_name_is_branch_not_found() {
    let fx = repo();
    let out = ff(&fx, &["switch", "nosuch", "--json"]);
    assert_eq!(out.status.code(), Some(1));
    let v = json(&out);
    assert_eq!(v["error"]["id"], "branch/not-found", "{v}");
    assert_eq!(v["error"]["message"], "no branch named nosuch", "{v}");
    let explain = stdout(&ok(ff(&fx, &["explain", "branch/not-found"])));
    assert!(explain.contains("ff switch <branch>"), "{explain}");
    let explain = stdout(&ok(ff(&fx, &["explain", "switch/nothing-opened"])));
    assert!(explain.contains("ff describe -m"), "{explain}");
}

/// No target is trunk's tip, whatever branch you stand on.
#[test]
fn no_target_is_trunk() {
    let fx = repo();
    let main_tip = fx.git(&["rev-parse", "main"]).trim().to_string();
    ok(ff(&fx, &["switch", "feature"]));
    fx.write("b.txt", "b\n");
    fx.commit("on feature");
    let v = json(&ok(ff(&fx, &["start", "--json"])));
    assert_eq!(v["data"]["switch"]["from"], "feature", "{v}");
    assert_eq!(v["data"]["switch"]["minted"]["forked_from"], "main", "{v}");
    assert_eq!(fx.git(&["rev-parse", "HEAD"]).trim(), main_tip);
}

// --- -b ---

/// `-b` forks each kind of branch target and names the mint; bare `-b`
/// forks anonymously and goes after the target.
#[test]
fn dash_b_forks_a_branch_target_or_names_the_mint() {
    let fx = repo();
    let main_tip = fx.git(&["rev-parse", "main"]).trim().to_string();

    // A local branch, named.
    let v = json(&ok(ff(&fx, &["start", "feature", "-b", "top", "--json"])));
    assert_eq!(v["data"]["switch"]["to"], "top", "{v}");
    assert_eq!(v["data"]["switch"]["minted"]["parent"], "feature", "{v}");
    assert_eq!(current(&fx), "top");
    assert!(branch_exists(&fx, "feature"), "feature itself is untouched");

    // A local branch, bare `-b` after the target.
    let text = stdout(&ok(ff(&fx, &["switch", "main", "-b"])));
    assert!(text.contains("minted ff/"), "{text}");
    assert!(text.contains("(forked from main)"), "{text}");
    assert!(current(&fx).starts_with("ff/"));
    assert_eq!(fx.git(&["rev-parse", "HEAD"]).trim(), main_tip);

    // Named, no target: the mint at trunk wears the name.
    let v = json(&ok(ff(&fx, &["start", "-b", "hotfix", "--json"])));
    assert_eq!(v["data"]["switch"]["to"], "hotfix", "{v}");
    assert_eq!(v["data"]["switch"]["minted"]["forked_from"], "main", "{v}");
    assert!(v["data"]["switch"]["minted"]["parent"].is_null(), "{v}");

    // `-b main` before the target is a branch *named* main.
    let out = ff(&fx, &["switch", "-b", "main", "--json"]);
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(json(&out)["error"]["id"], "branch/exists");

    // A name already taken.
    let out = ff(&fx, &["start", "-b", "feature", "--json"]);
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(json(&out)["error"]["id"], "branch/exists");
}

/// `-b` on a remote's branch forks it with no upstream.
#[test]
fn dash_b_on_a_remote_branch_forks_it() {
    let (fx, sha) = clone_with_remote_spike();
    let v = json(&ok(ff(&fx, &["switch", "spike", "-b", "mine", "--json"])));
    assert_eq!(v["data"]["switch"]["to"], "mine", "{v}");
    assert_eq!(
        v["data"]["switch"]["minted"]["parent"], "origin/spike",
        "{v}"
    );
    assert!(v["data"]["switch"]["minted"]["tracking"].is_null(), "{v}");
    assert_eq!(fx.git(&["rev-parse", "mine"]).trim(), sha);
    assert!(!branch_exists(&fx, "spike"));
    assert_eq!(config(&fx, "branch.mine.remote"), None);
}

/// `@ -b <name>` on a dirty tree carries a copy of the open change, and the
/// carry is said.
#[test]
fn at_carries_the_open_change() {
    let fx = repo();
    fx.write("a.txt", "dirty\n");
    let out = ok(ff(&fx, &["start", "@", "-b", "spike"]));
    let text = stdout(&out);
    assert!(text.contains("parked the open change on main"), "{text}");
    assert!(text.contains("minted spike (forked from"), "{text}");
    assert!(text.contains("switched to spike"), "{text}");
    assert!(
        text.contains("carried the open change onto spike"),
        "{text}"
    );
    assert_eq!(current(&fx), "spike");
    assert_eq!(
        std::fs::read_to_string(fx.path().join("a.txt")).unwrap(),
        "dirty\n"
    );
    let v = json(&ok(ff(
        &fx,
        &["op", "log", "kind(op)", "-n", "1", "--json"],
    )));
    let newest = &v["data"]["ops"][0];
    assert_eq!(newest["verb"], "switch", "{v}");
    assert!(
        newest["summary"]
            .as_str()
            .unwrap()
            .contains("carrying the open change"),
        "{v}"
    );
}

// --- -m ---

/// `-m` describes the change a mint opens, and is refused on a continue.
#[test]
fn dash_m_describes_a_mint_and_is_refused_on_a_continue() {
    let fx = repo();
    ok(ff(&fx, &["start", "-m", "the next thing"]));
    let status = json(&ok(ff(&fx, &["status", "--json"])));
    assert_eq!(
        status["data"]["open"]["subject"], "the next thing",
        "{status}"
    );

    let out = ff(&fx, &["switch", "main", "-m", "nope", "--json"]);
    assert_eq!(out.status.code(), Some(1));
    let v = json(&out);
    assert_eq!(v["error"]["id"], "switch/nothing-opened", "{v}");
    assert!(
        v["error"]["exits"]
            .to_string()
            .contains("ff describe -m <msg>"),
        "{v}"
    );
    assert_ne!(current(&fx), "main", "nothing moved");
}
