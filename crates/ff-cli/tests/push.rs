//! `ff push` over one or more branches, end to end against the real `ff`
//! binary and a bare remote on the filesystem beside the clone. Nothing
//! here reaches the network. Covers the three ways to say which branches —
//! bare, one name, several — the block render and the envelope for a run
//! of several, a name refused before the wire, a lease refused beside one
//! that landed, and a hold blocking one branch beside one that lands.

use std::path::Path;
use std::process::{Command, Output};

use ff_testsupport::Fixture;
use ff_testsupport::fixtures::null_device;

fn ff_at(dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_ff"))
        .current_dir(dir)
        .args(args)
        // Hermetic like the fixtures: production discover() reads these.
        .env("GIT_CONFIG_GLOBAL", null_device())
        .env("GIT_CONFIG_SYSTEM", null_device())
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env_remove("GIT_AUTHOR_NAME")
        .env_remove("GIT_AUTHOR_EMAIL")
        .env_remove("GIT_AUTHOR_DATE")
        .env_remove("GIT_COMMITTER_NAME")
        .env_remove("GIT_COMMITTER_EMAIL")
        .env_remove("GIT_COMMITTER_DATE")
        .env_remove("EMAIL")
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

/// Both streams concatenated, so an assertion never misses the one an
/// output actually landed on.
fn out(output: &Output) -> String {
    format!("{}{}", stdout(output), stderr(output))
}

fn json(output: &Output) -> serde_json::Value {
    serde_json::from_str(&stdout(output)).expect("valid json")
}

fn ok(output: &Output) -> String {
    assert!(output.status.success(), "{}", out(output));
    stdout(output)
}

const TAIL: &str = "the push left the machine — ff undo cannot reach it\n\
                    ff undo then ff push rolls the shared copy back, under a lease\n";

/// A clone of an empty remote standing on `main`, one commit deep, with
/// `alpha` and `beta` forked from it and carrying one commit each. Neither
/// has a shared copy yet, and neither does `main`.
fn two_topics() -> Fixture {
    let fx = Fixture::new_cloned();
    fx.write("root.txt", "root\n");
    fx.commit("root");
    fx.git(&["switch", "-q", "-c", "alpha"]);
    fx.write("a.txt", "a\n");
    fx.commit("a1");
    fx.git(&["switch", "-q", "-c", "beta", "main"]);
    fx.write("b.txt", "b\n");
    fx.commit("b1");
    fx.git(&["switch", "-q", "main"]);
    fx
}

/// The tip of `branch` on the far side, or `None` when it has no copy.
fn remote_tip(fx: &Fixture, branch: &str) -> Option<String> {
    let full = format!("refs/heads/{branch}");
    let out = fx.try_git_in(&fx.remote_path(), &["rev-parse", "--verify", "-q", &full]);
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// How many pushes of `branch` the operation log holds.
fn pushes_recorded(fx: &Fixture, branch: &str) -> usize {
    let text = ok(&ff(fx, &["op", "log", "-n", "50"]));
    let row = format!("pushed {branch} to origin/{branch}");
    text.lines().filter(|l| l.contains(&row)).count()
}

/// Bare push is the branch underfoot, rendered as it always was: no block,
/// the line, the tail. `alpha` and `beta` keep no copy, and the envelope
/// files the one row the run had.
#[test]
fn bare_push_is_the_branch_underfoot() {
    let fx = two_topics();
    let main = fx.git(&["rev-parse", "main"]).trim().to_string();

    let text = ok(&ff(&fx, &["push"]));
    assert_eq!(text, format!("created origin/main\n{TAIL}"));
    assert_eq!(remote_tip(&fx, "main").as_deref(), Some(main.as_str()));
    assert_eq!(
        remote_tip(&fx, "alpha"),
        None,
        "a sibling is not in the run"
    );
    assert_eq!(remote_tip(&fx, "beta"), None);

    let v = json(&ff(&fx, &["--json", "push"]));
    assert_eq!(v["cmd"], "push");
    assert_eq!(v["data"]["push"]["branch"], "main");
    assert_eq!(v["data"]["push"]["push"], "UpToDate");
    assert_eq!(v["data"]["pushed"], false);
    let rows = v["data"]["branches"].as_array().expect("rows");
    assert_eq!(rows.len(), 1, "the run is one row: {v}");
    assert_eq!(rows[0]["branch"], "main");
    assert_eq!(rows[0]["push"], "UpToDate");
    assert_eq!(rows[0]["pushed"], false);
    assert!(rows[0]["error"].is_null(), "{v}");
}

/// One name is that branch, from wherever you stand: a block headed by its
/// name, the tail once, tracking set up, and the push on the log. The
/// branch underfoot keeps no copy. Run again, the block says there is
/// nothing to push and the tail stays away.
#[test]
fn one_name_pushes_that_branch_from_wherever_you_stand() {
    let fx = two_topics();
    let alpha = fx.git(&["rev-parse", "alpha"]).trim().to_string();

    let text = ok(&ff(&fx, &["push", "alpha"]));
    assert_eq!(
        text,
        format!("alpha\n    created origin/alpha and set alpha to track it\n{TAIL}")
    );
    assert_eq!(remote_tip(&fx, "alpha").as_deref(), Some(alpha.as_str()));
    assert_eq!(remote_tip(&fx, "main"), None, "underfoot, and not named");
    assert_eq!(fx.git(&["config", "branch.alpha.remote"]).trim(), "origin");
    assert_eq!(pushes_recorded(&fx, "alpha"), 1);
    assert_eq!(
        fx.git(&["rev-parse", "HEAD"]),
        fx.git(&["rev-parse", "main"])
    );

    let again = ok(&ff(&fx, &["push", "alpha"]));
    assert_eq!(again, "alpha\n    nothing to push\n");
}

/// Several names are each sent under their own lease, reported in the
/// order the ref namespace lists them whatever order they were typed in,
/// with one tail for the run, and a name resolves the way `ff restack`
/// resolves one: an unambiguous prefix names the branch.
#[test]
fn several_names_push_each_in_namespace_order() {
    let fx = two_topics();
    let alpha = fx.git(&["rev-parse", "alpha"]).trim().to_string();
    let beta = fx.git(&["rev-parse", "beta"]).trim().to_string();

    let text = ok(&ff(&fx, &["push", "be", "al"]));
    assert_eq!(
        text,
        format!(
            "alpha\n    created origin/alpha and set alpha to track it\n\
             beta\n    created origin/beta and set beta to track it\n{TAIL}"
        )
    );
    assert_eq!(remote_tip(&fx, "alpha").as_deref(), Some(alpha.as_str()));
    assert_eq!(remote_tip(&fx, "beta").as_deref(), Some(beta.as_str()));
    assert_eq!(remote_tip(&fx, "main"), None);
    assert_eq!(pushes_recorded(&fx, "alpha"), 1);
    assert_eq!(pushes_recorded(&fx, "beta"), 1);
}

/// A branch named twice, by its name and by a prefix of it, is in the run
/// once: one send, one note.
#[test]
fn a_branch_named_twice_is_pushed_once() {
    let fx = two_topics();
    let text = ok(&ff(&fx, &["push", "alpha", "al"]));
    assert_eq!(text.matches("alpha\n").count(), 1, "{text}");
    assert_eq!(pushes_recorded(&fx, "alpha"), 1);
}

/// Naming the branch underfoot beside another: the branch underfoot reads
/// first and without a block, the way it does bare, and the other is a
/// block after it. The envelope leads with the branch underfoot's push and
/// says it went.
#[test]
fn the_branch_underfoot_leads_when_it_is_among_the_names() {
    let fx = two_topics();
    let text = ok(&ff(&fx, &["push", "main", "alpha"]));
    assert_eq!(
        text,
        format!(
            "created origin/main\nalpha\n    created origin/alpha and set alpha to track it\n{TAIL}"
        )
    );
    assert!(remote_tip(&fx, "main").is_some());
    assert!(remote_tip(&fx, "alpha").is_some());
}

/// The envelope for a run of several: `push` and `pushed` are the branch
/// underfoot's and read `NotNamed` and false when names left it out, and
/// `branches` is every row in the run, each with its plan, whether it went,
/// and no error.
#[test]
fn the_envelope_files_every_branch_in_the_run() {
    let fx = two_topics();
    let alpha = fx.git(&["rev-parse", "alpha"]).trim().to_string();

    let output = ff(&fx, &["--json", "push", "beta", "alpha"]);
    assert!(output.status.success(), "{}", out(&output));
    let v = json(&output);
    assert_eq!(v["cmd"], "push");
    assert_eq!(v["data"]["push"]["branch"], "main");
    assert_eq!(v["data"]["push"]["push"], "NotNamed", "{v}");
    assert_eq!(v["data"]["push"]["dry_run"], false);
    assert_eq!(v["data"]["pushed"], false);
    let rows = v["data"]["branches"].as_array().expect("rows");
    let names: Vec<&str> = rows.iter().map(|r| r["branch"].as_str().unwrap()).collect();
    assert_eq!(names, ["alpha", "beta"], "namespace order: {v}");
    assert_eq!(rows[0]["push"]["Create"]["remote"], "origin");
    assert_eq!(rows[0]["push"]["Create"]["remote_branch"], "alpha");
    assert_eq!(rows[0]["push"]["Create"]["tip"], alpha);
    assert!(rows.iter().all(|r| r["pushed"] == true), "{v}");
    assert!(rows.iter().all(|r| r["error"].is_null()), "{v}");
}

/// A name that resolves to nothing is refused before the wire is touched,
/// with the error a misspelled branch gets everywhere else: the remote
/// here is unreachable, and the refusal is still the name's, not the
/// wire's, in both surfaces.
#[test]
fn an_unknown_name_is_refused_before_any_network() {
    let fx = two_topics();
    fx.set_config("remote.origin.url", "/nonexistent/remote.git");

    let output = ff(&fx, &["push", "nope"]);
    assert_eq!(output.status.code(), Some(1), "{}", out(&output));
    assert!(
        stderr(&output).contains("no branch named nope"),
        "{}",
        out(&output)
    );
    assert!(
        stdout(&output).is_empty(),
        "nothing reported: {}",
        out(&output)
    );

    let output = ff(&fx, &["--json", "push", "nope"]);
    assert_eq!(output.status.code(), Some(1), "{}", out(&output));
    let v = json(&output);
    assert_eq!(v["error"]["id"], "branch/not-found", "{v}");
    assert!(v.get("data").is_none(), "{v}");
    assert_eq!(pushes_recorded(&fx, "alpha"), 0);
}

/// `beta` has a shared copy, a commit of its own since, and a teammate who
/// pushed to it in between; `alpha` has no copy yet. Standing on `main`,
/// which is not in the run.
fn beta_moved_under_alpha_and_beta(fx: &Fixture) -> String {
    ok(&ff(fx, &["push", "beta"]));
    fx.git(&["switch", "-q", "beta"]);
    fx.write("b.txt", "bb\n");
    fx.commit("b2");
    fx.git(&["switch", "-q", "main"]);

    let mover = fx.root().join("mover");
    fx.git_in(
        fx.root(),
        &["clone", "-q", &fx.remote_path().to_string_lossy(), "mover"],
    );
    fx.git_in(&mover, &["checkout", "-q", "beta"]);
    std::fs::write(mover.join("theirs.txt"), "theirs\n").unwrap();
    fx.git_in(&mover, &["add", "-A"]);
    fx.git_in(&mover, &["commit", "-q", "-m", "theirs"]);
    fx.git_in(&mover, &["push", "-q", "origin", "beta"]);
    fx.git_in(&mover, &["rev-parse", "HEAD"]).trim().to_string()
}

/// Each branch goes out under its own lease, and a refusal is that
/// branch's alone: `alpha` lands, `beta`'s block carries what the wire
/// said and the way out, the tail is for what went, and the exit is 1. The
/// far side stands where the teammate left it, and nothing is recorded of
/// the push that did not happen.
#[test]
fn a_refused_lease_is_that_branchs_alone_and_the_exit_is_1() {
    let fx = two_topics();
    let theirs = beta_moved_under_alpha_and_beta(&fx);
    let alpha = fx.git(&["rev-parse", "alpha"]).trim().to_string();

    let output = ff(&fx, &["push", "alpha", "beta"]);
    assert_eq!(output.status.code(), Some(1), "{}", out(&output));
    assert_eq!(
        stdout(&output),
        format!(
            "alpha\n    created origin/alpha and set alpha to track it\n\
             beta\n    not pushed: origin/beta moved since you last looked, so nothing was pushed \
             — your commits are still here, and ff pull takes in what arrived\n    try:\n      \
             ff pull beta\n      ff push beta\n{TAIL}"
        )
    );
    assert_eq!(remote_tip(&fx, "alpha").as_deref(), Some(alpha.as_str()));
    assert_eq!(
        remote_tip(&fx, "beta").as_deref(),
        Some(theirs.as_str()),
        "the lease held"
    );
    assert_eq!(pushes_recorded(&fx, "alpha"), 1);
    assert_eq!(
        pushes_recorded(&fx, "beta"),
        1,
        "the first push, and not this one"
    );

    // The same run again: `alpha` has nothing to send and nothing went, so
    // there is no tail, and the refusal still exits 1.
    let output = ff(&fx, &["--json", "push", "alpha", "beta"]);
    assert_eq!(output.status.code(), Some(1), "{}", out(&output));
    let v = json(&output);
    assert_eq!(v["data"]["push"]["push"], "NotNamed", "{v}");
    assert_eq!(v["data"]["pushed"], false);
    let rows = v["data"]["branches"].as_array().expect("rows");
    assert_eq!(rows[0]["branch"], "alpha");
    assert_eq!(rows[0]["push"], "UpToDate");
    assert_eq!(rows[0]["pushed"], false);
    assert!(rows[0]["error"].is_null(), "{v}");
    assert_eq!(rows[1]["branch"], "beta");
    assert_eq!(rows[1]["push"]["Push"]["shape"], "replace", "{v}");
    assert_eq!(rows[1]["pushed"], false);
    assert_eq!(rows[1]["error"]["id"], "push/lease-refused", "{v}");
    assert_eq!(
        rows[1]["error"]["exits"],
        serde_json::json!(["ff pull beta", "ff push beta"]),
        "{v}"
    );
}

/// A run of one branch is the verb as it always was: the wire's refusal is
/// the run's error, an error envelope and exit 1, whether the branch is
/// named or underfoot.
#[test]
fn a_run_of_one_branch_makes_the_refusal_the_runs_error() {
    let fx = two_topics();
    beta_moved_under_alpha_and_beta(&fx);

    let output = ff(&fx, &["--json", "push", "beta"]);
    assert_eq!(output.status.code(), Some(1), "{}", out(&output));
    let v = json(&output);
    assert_eq!(v["error"]["id"], "push/lease-refused", "{v}");
    assert!(v.get("data").is_none(), "{v}");

    fx.git(&["switch", "-q", "beta"]);
    let output = ff(&fx, &["push"]);
    assert_eq!(output.status.code(), Some(1), "{}", out(&output));
    assert!(stdout(&output).is_empty(), "{}", out(&output));
    assert!(
        stderr(&output).contains("ff: origin/beta moved since you last looked"),
        "{}",
        out(&output)
    );
    assert!(
        stderr(&output).contains("    ff pull beta\n"),
        "{}",
        out(&output)
    );
}

/// A hold blocks that branch's exit and no other: `clean` lands, `side`'s
/// block says a rewrite is held on it, nothing of `side` reaches the far
/// side, and the exit is 3.
#[test]
fn a_held_branch_is_blocked_beside_one_that_lands_at_exit_3() {
    let fx = Fixture::new_cloned();
    fx.write("shared.txt", "base\n");
    fx.commit("root");
    fx.git(&["switch", "-q", "-c", "side"]);
    fx.write("shared.txt", "mine\n");
    fx.commit("mine");
    fx.git(&["switch", "-q", "-c", "clean", "main"]);
    fx.write("c.txt", "c\n");
    let clean = fx.commit("c1");
    fx.git(&["switch", "-q", "main"]);
    fx.write("shared.txt", "theirs\n");
    fx.commit("theirs");
    let held = ff(&fx, &["pull", "--no-fetch", "side"]);
    assert_eq!(held.status.code(), Some(3), "fixture: {}", out(&held));

    let output = ff(&fx, &["push", "side", "clean"]);
    assert_eq!(output.status.code(), Some(3), "{}", out(&output));
    assert_eq!(
        stdout(&output),
        format!(
            "clean\n    created origin/clean and set clean to track it\n\
             side\n    nothing sent: a rewrite is held on side — the exit stays blocked until it lands\n\
             {TAIL}"
        )
    );
    assert_eq!(remote_tip(&fx, "clean").as_deref(), Some(clean.as_str()));
    assert_eq!(remote_tip(&fx, "side"), None);

    let output = ff(&fx, &["--json", "push", "side", "clean"]);
    assert_eq!(output.status.code(), Some(3), "{}", out(&output));
    let v = json(&output);
    let rows = v["data"]["branches"].as_array().expect("rows");
    assert_eq!(rows[1]["branch"], "side");
    assert_eq!(rows[1]["push"], "Blocked", "{v}");
}

/// GitHub #7's shape: a clone with `main` pushed, and `feature` cut from it
/// the git way, `git checkout -b feature --track origin/main`, one commit
/// deep. Its upstream wears another branch's name, which fufu reads as the
/// base it was cut from — `ff start origin/main -b feature` — and not as a
/// shared copy. Standing on `feature`.
fn feature_tracking_main() -> Fixture {
    let fx = Fixture::new_cloned();
    fx.write("root.txt", "root\n");
    fx.commit("root");
    ok(&ff(&fx, &["push"]));
    fx.git(&["checkout", "-q", "-b", "feature", "--track", "origin/main"]);
    fx.write("f.txt", "f\n");
    fx.commit("f1");
    fx
}

/// `ff status` on the shape: `origin/main` is the base axis, nothing is
/// counted toward it as a remote, and when it moves the base is what says so.
#[test]
fn status_reads_an_upstream_under_another_name_as_the_base() {
    let fx = feature_tracking_main();

    let text = ok(&ff(&fx, &["status"]));
    assert!(!text.contains("to push"), "{text}");
    let v = json(&ff(&fx, &["--json", "status"]));
    assert_eq!(v["data"]["base"]["name"], "origin/main", "{v}");
    assert_eq!(v["data"]["base"]["role"], "parent", "{v}");
    assert_eq!(v["data"]["base"]["above"], 1, "{v}");
    assert!(v["data"]["futures"]["remote"].is_null(), "{v}");
    assert!(v["data"]["upstream"].is_null(), "{v}");

    // A teammate lands on main: the base moved, and no remote axis of
    // feature's own has anything to say about it.
    fx.git(&["switch", "-q", "main"]);
    fx.write("m2.txt", "m2\n");
    fx.commit("m2");
    fx.git(&["push", "-q", "origin", "main"]);
    fx.git(&["switch", "-q", "feature"]);
    let text = ok(&ff(&fx, &["status"]));
    assert!(
        text.contains("base origin/main moved — rebases cleanly (1 commit replayed)"),
        "{text}"
    );
    assert!(!text.contains("to pull"), "{text}");
    assert!(!text.contains("to push"), "{text}");
}

/// `ff push` on the shape is the create of `origin/feature`: `main` on the
/// remote is untouched, tracking is set to the copy it made, and the base
/// the old upstream named survives as the recorded parent.
#[test]
fn push_creates_a_copy_of_its_own_beside_the_base_it_tracked() {
    let fx = feature_tracking_main();
    let root = fx.git(&["rev-parse", "main"]).trim().to_string();
    let f1 = fx.git(&["rev-parse", "feature"]).trim().to_string();

    let text = ok(&ff(&fx, &["push", "-n"]));
    assert_eq!(
        text,
        "would create origin/feature and set feature to track it\n\
         nothing was sent — drop --dry-run to send it\n"
    );
    assert_eq!(remote_tip(&fx, "feature"), None);

    let text = ok(&ff(&fx, &["push"]));
    assert_eq!(
        text,
        format!("created origin/feature and set feature to track it\n{TAIL}")
    );
    assert_eq!(
        remote_tip(&fx, "main").as_deref(),
        Some(root.as_str()),
        "main on the remote is untouched"
    );
    assert_eq!(remote_tip(&fx, "feature").as_deref(), Some(f1.as_str()));
    assert_eq!(
        fx.git(&["config", "branch.feature.merge"]).trim(),
        "refs/heads/feature"
    );
    let v = json(&ff(&fx, &["--json", "status"]));
    assert_eq!(v["data"]["base"]["name"], "origin/main", "{v}");
    assert_eq!(
        v["data"]["futures"]["remote"]["against"]["name"], "origin/feature",
        "{v}"
    );
}

/// `--dry-run` with names says which push each would be and sends none:
/// one closing line for the run, no copy on the far side, nothing on the
/// log, and the envelope says so.
#[test]
fn a_dry_run_with_names_says_would_and_sends_nothing() {
    let fx = two_topics();

    let text = ok(&ff(&fx, &["push", "-n", "alpha", "beta"]));
    assert_eq!(
        text,
        "alpha\n    would create origin/alpha and set alpha to track it\n\
         beta\n    would create origin/beta and set beta to track it\n\
         nothing was sent — drop --dry-run to send it\n"
    );
    assert_eq!(remote_tip(&fx, "alpha"), None);
    assert_eq!(remote_tip(&fx, "beta"), None);
    assert_eq!(pushes_recorded(&fx, "alpha"), 0);

    let v = json(&ff(&fx, &["--json", "push", "-n", "alpha", "beta"]));
    assert_eq!(v["data"]["push"]["dry_run"], true);
    assert_eq!(v["data"]["pushed"], false);
    let rows = v["data"]["branches"].as_array().expect("rows");
    assert!(rows.iter().all(|r| r["pushed"] == false), "{v}");
    assert_eq!(rows[0]["push"]["Create"]["remote_branch"], "alpha", "{v}");
}

/// `--to` under a name records where that branch answers, and the branch
/// underfoot, which is not in the run, is not asked.
#[test]
fn to_under_a_name_records_where_that_branch_answers() {
    let fx = two_topics();
    let second = fx.root().join("second.git");
    fx.git_in(
        fx.root(),
        &["init", "-q", "--bare", "-b", "main", "second.git"],
    );
    fx.git(&["remote", "add", "two", &second.to_string_lossy()]);
    let alpha = fx.git(&["rev-parse", "alpha"]).trim().to_string();

    let text = ok(&ff(&fx, &["push", "--to", "two", "alpha"]));
    assert_eq!(
        text,
        format!("alpha\n    created two/alpha and set alpha to track it\n{TAIL}")
    );
    assert_eq!(fx.git(&["config", "branch.alpha.remote"]).trim(), "two");
    assert_eq!(
        fx.git_in(&second, &["rev-parse", "refs/heads/alpha"])
            .trim(),
        alpha
    );
    assert_eq!(fx.git(&["config", "branch.main.remote"]).trim(), "origin");
}

/// The tip `refs/fufu/seen/<branch>` holds, or `None` when there is no
/// record.
fn seen(fx: &Fixture, branch: &str) -> Option<String> {
    let full = format!("refs/fufu/seen/{branch}");
    let out = fx.try_git(&["rev-parse", "--verify", "-q", &full]);
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// The lease is fufu's record and not the tracking ref: a teammate moves
/// `beta`, a `git fetch` behind fufu's back moves the tracking ref onto
/// their tip, and the push is refused before the wire — the far side
/// untouched, nothing recorded, the record still where the last push left
/// it. `ff pull` takes their commit in and records the tip it read, and the
/// push afterwards goes, leaving the record at the tip sent.
#[test]
fn a_fetch_behind_fufus_back_does_not_refresh_the_lease() {
    let fx = two_topics();
    let theirs = beta_moved_under_alpha_and_beta(&fx);
    let sent = seen(&fx, "beta").expect("the push recorded the tip it sent");
    assert_ne!(sent, theirs);

    fx.git(&["fetch", "-q"]);
    assert_eq!(
        fx.git(&["rev-parse", "refs/remotes/origin/beta"]).trim(),
        theirs,
        "the fetch moved the tracking ref"
    );

    let output = ff(&fx, &["--json", "push", "beta"]);
    assert_eq!(output.status.code(), Some(1), "{}", out(&output));
    let v = json(&output);
    assert_eq!(v["error"]["id"], "push/lease-refused", "{v}");
    assert_eq!(
        v["error"]["message"],
        "origin/beta moved since you last looked (1 commit(s) you have not taken in), so \
         nothing was pushed — ff pull takes them in",
        "{v}"
    );
    assert_eq!(
        v["error"]["exits"],
        serde_json::json!(["ff pull beta", "ff push beta"]),
        "{v}"
    );
    assert_eq!(remote_tip(&fx, "beta").as_deref(), Some(theirs.as_str()));
    assert_eq!(pushes_recorded(&fx, "beta"), 1);
    assert_eq!(seen(&fx, "beta").as_deref(), Some(sent.as_str()));

    let output = ff(&fx, &["pull", "beta"]);
    assert!(output.status.success(), "{}", out(&output));
    assert_eq!(
        seen(&fx, "beta").as_deref(),
        Some(theirs.as_str()),
        "the pull recorded the tip it read"
    );

    let text = ok(&ff(&fx, &["push", "beta"]));
    assert_eq!(
        text,
        format!("beta\n    pushed beta to origin/beta\n{TAIL}")
    );
    let tip = fx.git(&["rev-parse", "beta"]).trim().to_string();
    assert_eq!(remote_tip(&fx, "beta").as_deref(), Some(tip.as_str()));
    assert_eq!(seen(&fx, "beta").as_deref(), Some(tip.as_str()));
    assert_eq!(pushes_recorded(&fx, "beta"), 2);
}

/// The same refusal under `--dry-run`, in the conditional: it is knowable
/// without the wire, nothing is sent, and the exit is still 1. Among
/// several the block is that branch's alone.
#[test]
fn a_dry_run_reports_the_local_refusal_in_the_conditional() {
    let fx = two_topics();
    let theirs = beta_moved_under_alpha_and_beta(&fx);
    fx.git(&["fetch", "-q"]);

    let output = ff(&fx, &["push", "-n", "beta"]);
    assert_eq!(output.status.code(), Some(1), "{}", out(&output));
    assert_eq!(
        stdout(&output),
        "beta\n    would not push: origin/beta moved since you last looked (1 commit(s) you \
         have not taken in), so nothing was pushed — ff pull takes them in\n    try:\n      \
         ff pull beta\n      ff push beta\n"
    );
    assert_eq!(remote_tip(&fx, "beta").as_deref(), Some(theirs.as_str()));
    assert_eq!(pushes_recorded(&fx, "beta"), 1);

    let output = ff(&fx, &["--json", "push", "-n", "alpha", "beta"]);
    assert_eq!(output.status.code(), Some(1), "{}", out(&output));
    let v = json(&output);
    let rows = v["data"]["branches"].as_array().expect("rows");
    assert_eq!(rows[0]["push"]["Create"]["remote_branch"], "alpha", "{v}");
    assert!(rows[0]["error"].is_null(), "{v}");
    assert_eq!(
        rows[1]["push"]["Refused"]["why"]["moved"]["behind"], 1,
        "{v}"
    );
    assert_eq!(rows[1]["error"]["id"], "push/lease-refused", "{v}");
    assert_eq!(remote_tip(&fx, "alpha"), None);
}

/// No record — a branch from before fufu kept one — and the push only
/// adds commits to the copy: it goes, and the record is written with it.
#[test]
fn a_fast_forward_with_no_record_goes_and_writes_one() {
    let fx = two_topics();
    ok(&ff(&fx, &["push", "alpha"]));
    fx.git(&["update-ref", "-d", "refs/fufu/seen/alpha"]);
    fx.git(&["switch", "-q", "alpha"]);
    fx.write("a.txt", "aa\n");
    fx.commit("a2");
    let tip = fx.git(&["rev-parse", "alpha"]).trim().to_string();

    let text = ok(&ff(&fx, &["push"]));
    assert_eq!(text, format!("pushed alpha to origin/alpha\n{TAIL}"));
    assert_eq!(remote_tip(&fx, "alpha").as_deref(), Some(tip.as_str()));
    assert_eq!(seen(&fx, "alpha").as_deref(), Some(tip.as_str()));
}

/// No record, and the copy holds a commit the branch does not: nothing
/// vouches for it, so the push is `push/unseen` and sends nothing.
#[test]
fn a_moved_copy_with_no_record_is_unseen() {
    let fx = two_topics();
    let theirs = beta_moved_under_alpha_and_beta(&fx);
    fx.git(&["fetch", "-q"]);
    fx.git(&["update-ref", "-d", "refs/fufu/seen/beta"]);

    let output = ff(&fx, &["--json", "push", "beta"]);
    assert_eq!(output.status.code(), Some(1), "{}", out(&output));
    let v = json(&output);
    assert_eq!(v["error"]["id"], "push/unseen", "{v}");
    assert_eq!(
        v["error"]["message"],
        "fufu has no record of where you last looked at origin/beta, and it holds 1 \
         commit(s) beta does not — nothing was pushed",
        "{v}"
    );
    assert_eq!(
        v["error"]["exits"],
        serde_json::json!(["ff pull beta", "ff push beta"]),
        "{v}"
    );
    assert_eq!(remote_tip(&fx, "beta").as_deref(), Some(theirs.as_str()));
    assert_eq!(seen(&fx, "beta"), None);
}

/// `ff pull` writes the record for every branch whose shared copy it read,
/// and only on a real run: a dry run fetches and leaves the record where
/// it was; a real run moves it to the tip read, whether the axis replayed,
/// found the divergence yours, or found nothing to do — and for the branch
/// underfoot as much as for a named one.
#[test]
fn pull_records_the_tip_it_read_and_a_dry_run_does_not() {
    let fx = two_topics();
    let theirs = beta_moved_under_alpha_and_beta(&fx);
    ok(&ff(&fx, &["push", "alpha"]));
    let b1 = seen(&fx, "beta").expect("recorded by the push");
    let a1 = seen(&fx, "alpha").expect("recorded by the push");

    // Dry: the fetch moves the tracking ref, the record stays.
    let output = ff(&fx, &["pull", "-n", "beta"]);
    assert!(output.status.success(), "{}", out(&output));
    assert_eq!(
        fx.git(&["rev-parse", "refs/remotes/origin/beta"]).trim(),
        theirs
    );
    assert_eq!(seen(&fx, "beta").as_deref(), Some(b1.as_str()));

    // Real, named, replayed: the record moves to the tip read.
    let output = ff(&fx, &["pull", "beta"]);
    assert!(output.status.success(), "{}", out(&output));
    assert_eq!(seen(&fx, "beta").as_deref(), Some(theirs.as_str()));

    // Named and up to date: read, so recorded, even from no record at all.
    fx.git(&["update-ref", "-d", "refs/fufu/seen/alpha"]);
    let output = ff(&fx, &["pull", "alpha"]);
    assert!(output.status.success(), "{}", out(&output));
    assert_eq!(seen(&fx, "alpha").as_deref(), Some(a1.as_str()));

    // Named and undone, then yours: `alpha` rewords its commit, so the
    // copy holds a commit the log accounts for — undone while the published
    // pointer stands at the copy's tip, yours once it is gone, which is the
    // shape a repository from before the pointer wears. Both read the tip,
    // and both record it: it is exactly the lease the push that replaces
    // the copy needs.
    fx.git(&["switch", "-q", "alpha"]);
    ok(&ff(&fx, &["describe", "HEAD", "-m", "a1, reworded"]));
    fx.git(&["switch", "-q", "main"]);
    fx.git(&["update-ref", "-d", "refs/fufu/seen/alpha"]);
    let output = ff(&fx, &["pull", "alpha"]);
    assert!(output.status.success(), "{}", out(&output));
    assert!(stdout(&output).contains("you undid"), "{}", out(&output));
    assert_eq!(seen(&fx, "alpha").as_deref(), Some(a1.as_str()));
    fx.git(&["update-ref", "-d", "refs/fufu/seen/alpha"]);
    fx.git(&["update-ref", "-d", "refs/fufu/published/alpha"]);
    let output = ff(&fx, &["pull", "alpha"]);
    assert!(output.status.success(), "{}", out(&output));
    assert!(
        stdout(&output).contains("stale copies of your own"),
        "{}",
        out(&output)
    );
    assert_eq!(seen(&fx, "alpha").as_deref(), Some(a1.as_str()));
    let text = ok(&ff(&fx, &["push", "alpha"]));
    assert_eq!(
        text,
        format!("alpha\n    pushed alpha to origin/alpha\n{TAIL}")
    );

    // Underfoot: the branch's own axis records the tip it read.
    fx.git(&["switch", "-q", "beta"]);
    fx.git(&["update-ref", "-d", "refs/fufu/seen/beta"]);
    let output = ff(&fx, &["pull"]);
    assert!(output.status.success(), "{}", out(&output));
    assert_eq!(seen(&fx, "beta").as_deref(), Some(theirs.as_str()));
}
