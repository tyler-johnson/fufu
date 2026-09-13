//! `ff status`: the human header and its rows, the JSON shape and its
//! stable keys, foreign motion, and the standing work it pins on the branch
//! underfoot — a held rewrite, and the resolution open on it. Status
//! reports — it never adopts the verb's exit code, and a lookup that cannot
//! run is a missing line, never a failed status.

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

fn repo() -> Fixture {
    let fx = Fixture::new();
    fx.set_config("user.name", "Status Tester");
    fx.set_config("user.email", "status@test.test");
    fx
}

/// `feature` and `main` edit the same line of the same file from the same
/// base, so a restack of `feature` conflicts and holds. Leaves the fixture
/// standing on `feature`.
fn held_stack(fx: &Fixture) {
    fx.write("f.txt", "one\n");
    fx.commit("base");
    fx.git(&["switch", "-q", "-c", "feature"]);
    fx.write("f.txt", "two\n");
    fx.commit("f1");
    fx.git(&["switch", "-q", "main"]);
    fx.write("f.txt", "three\n");
    fx.commit("m1");
    fx.git(&["switch", "-q", "feature"]);

    // The conflicting restack holds — the precondition for a standing hold.
    let held = ff(fx, &["restack"]);
    assert_eq!(held.status.code(), Some(3), "{}", out(&held));
}

/// A held rewrite is standing work: `ff status` names it and the way out,
/// and exits 0 — status reports, it does not adopt the verb's exit code.
#[test]
fn status_pins_a_standing_hold() {
    let fx = repo();
    held_stack(&fx);

    let output = ff(&fx, &["status"]);
    assert!(
        output.status.success(),
        "status reports, it does not fail: {}",
        out(&output)
    );
    let text = stdout(&output);
    assert!(text.contains("held:"), "the hold is named: {text}");
    assert!(
        text.contains("ff restack"),
        "and so is the verb that recorded it: {text}"
    );
    assert!(
        text.contains("in 1 file"),
        "the count of the conflict's files: {text}"
    );
    assert!(
        text.contains("ff resolve to fix them"),
        "and the way out: {text}"
    );
}

/// An open resolution is the more urgent fact: on the session it says the
/// conflicts are in the working copy and names `ff done`; the hold itself
/// stays on the branch you left, shown there beneath a line that names the
/// session.
#[test]
fn status_pins_an_open_resolution() {
    let fx = repo();
    held_stack(&fx);
    let opened = ff(&fx, &["--json", "resolve"]);
    assert!(opened.status.success(), "{}", out(&opened));
    let session = json(&opened)["data"]["resolve"]["session"]
        .as_str()
        .expect("the session branch")
        .to_string();

    let output = ff(&fx, &["status"]);
    assert!(
        output.status.success(),
        "status reports, it does not fail: {}",
        out(&output)
    );
    let text = stdout(&output);
    assert!(
        text.contains("resolving:"),
        "the resolution is named: {text}"
    );
    assert!(
        text.contains("in your working copy"),
        "the markers' location is said: {text}"
    );
    assert!(text.contains("ff done"), "and the way out: {text}");
    assert!(
        text.contains("ff resolve --abandon to drop it"),
        "and the way out of the session: {text}"
    );
    assert!(
        !text.contains("held:"),
        "the hold stands on the branch you left, not here: {text}"
    );
    assert!(
        text.contains("editing") && text.contains("lands back on feature"),
        "the session block reads as any editing session's: {text}"
    );

    // On the branch the hold stands on, the line points at the session.
    assert!(ff(&fx, &["switch", "feature"]).status.success());
    let text = stdout(&ff(&fx, &["status"]));
    assert!(
        text.contains(&format!(
            "resolving: 1 conflict from ff restack is on {session}"
        )),
        "the session is named: {text}"
    );
    assert!(
        text.contains(&format!("ff switch {session} to fix them")),
        "and the way there: {text}"
    );
    assert!(
        text.contains(&format!("being fixed on {session}")) && !text.contains("ff resolve to fix"),
        "the hold's own hint no longer points at ff resolve, which refuses here: {text}"
    );
    let resolving = text.find("resolving:").expect("the resolution block");
    let held = text.find("held:").expect("the hold block");
    assert!(
        resolving < held,
        "the resolution goes above the hold: {text}"
    );
}

/// The JSON envelope carries the hold under its key while the resolution is
/// null, and the resolution under its key once `ff resolve` opens it — the
/// hold stays, because it is what the session is resolving.
#[test]
fn status_json_carries_both() {
    let fx = repo();
    held_stack(&fx);

    let v = json(&ff(&fx, &["status", "--json"]));
    let held = &v["data"]["held"];
    assert!(!held.is_null(), "the hold is under its key: {held}");
    assert_eq!(held["verb"], "restack");
    assert!(
        held["paths"]
            .as_array()
            .unwrap()
            .contains(&serde_json::Value::String("f.txt".into())),
        "the conflict's file: {held}"
    );
    assert_eq!(held["at"]["what"], "commit");
    assert!(
        v["data"]["resolving"].is_null(),
        "no session is open yet: {:?}",
        v["data"]["resolving"]
    );

    assert!(ff(&fx, &["resolve"]).status.success());

    let v = json(&ff(&fx, &["status", "--json"]));
    let resolving = &v["data"]["resolving"];
    assert!(
        !resolving.is_null(),
        "the resolution is under its key: {resolving}"
    );
    assert_eq!(resolving["verb"], "restack");
    assert_eq!(resolving["conflicts"], 1);
    assert!(
        !resolving["steps"].as_array().unwrap().is_empty(),
        "what the session will land: {resolving}"
    );
    assert_eq!(resolving["here"], true, "HEAD is on the session");
    let session = resolving["session"].as_str().unwrap().to_string();
    assert_eq!(v["data"]["session"]["branch"], session);
    assert_eq!(v["data"]["session"]["onto"], "feature");
    assert!(
        v["data"]["held"].is_null(),
        "the hold stays on the branch the session left"
    );

    assert!(ff(&fx, &["switch", "feature"]).status.success());
    let v = json(&ff(&fx, &["status", "--json"]));
    assert_eq!(v["data"]["resolving"]["here"], false);
    assert_eq!(v["data"]["resolving"]["session"], session);
    assert!(
        !v["data"]["held"].is_null(),
        "the hold stays: it is what the session is resolving"
    );
}

/// Corrupt the branch's metadata by hand and the hold is a missing line,
/// never a failed `ff status`: exit 0, and the rest of the status still
/// renders.
#[test]
fn status_survives_an_unreadable_hold() {
    let fx = repo();
    held_stack(&fx);

    let meta = fx.path().join(".git/fufu/branch/feature");
    std::fs::write(&meta, "not json at all").unwrap();

    let output = ff(&fx, &["status"]);
    assert!(
        output.status.success(),
        "a missing line, never a failed status: {}",
        out(&output)
    );
    let text = stdout(&output);
    assert!(
        text.starts_with("on feature"),
        "the rest of the status still renders: {text:?}"
    );
    assert!(
        text.contains("no changes"),
        "the open change row is still there: {text:?}"
    );
    assert!(
        !text.contains("held:"),
        "the unreadable hold is a missing line: {text:?}"
    );
}

/// The third of the three held-rewrite disciplines is exits blocked: push
/// refuses to send while a hold stands, and a guard nobody is told about
/// is a guard that surprises people — so the status says so.
#[test]
fn a_standing_hold_says_the_exit_is_blocked() {
    let fx = repo();
    held_stack(&fx);

    let output = ff(&fx, &["status"]);
    assert!(
        output.status.success(),
        "status reports, it does not fail: {}",
        out(&output)
    );
    let text = stdout(&output);
    assert!(
        text.contains("exits are blocked"),
        "the exit is named as blocked: {text}"
    );
    assert!(
        text.contains("ff push will not send"),
        "and the verb it will hold back: {text}"
    );
    let held = text.find("held:").expect("the hold block");
    let blocked = text.find("exits are blocked").expect("the blocked note");
    assert!(held < blocked, "the blocked note follows the hold: {text}");
}

/// `ff status` is `ff log` cropped to two rows, and a crop must not lose a
/// column. The parent row's first field is the commit's change id, and both
/// views have to spell the same one; the op anchor survives on the machine
/// surface as `segment`.
#[test]
fn the_parent_row_names_the_same_change_ff_log_does() {
    let fx = repo();
    fx.write("a.txt", "one\n");
    let commit = ff(&fx, &["commit", "-m", "one"]);
    assert!(commit.status.success(), "{}", out(&commit));

    let parent = json(&ff(&fx, &["status", "--json"]))["data"]["parent"].clone();
    let segment = parent["segment"].as_str().unwrap_or_default().to_string();
    assert_eq!(
        segment.len(),
        40,
        "the parent row still carries its anchor, as hex: {segment:?}"
    );
    let change_id = parent["change_id"].as_str().unwrap_or_default().to_string();
    assert_eq!(change_id.len(), 32, "and its change id: {change_id:?}");

    let commit_column = |text: &str| -> String {
        text.lines()
            .find(|line| line.starts_with('●'))
            .and_then(|line| line.split_whitespace().nth(1))
            .unwrap_or("<no commit row>")
            .to_string()
    };
    let from_status = commit_column(&stdout(&ff(&fx, &["status"])));
    let from_log = commit_column(&stdout(&ff(&fx, &["log", "-n", "2"])));
    assert_eq!(
        from_status,
        &change_id[..8],
        "the column is the change id's first letters"
    );
    assert_eq!(
        from_status, from_log,
        "status and log disagree about the parent commit's id"
    );
}

/// Two remotes, neither named `origin`, and a branch whose own section names
/// none of them: `ff status` says the remote cannot be named, and does not
/// let the empty remote axis read as settled. It still exits 0 — status
/// reports, it never adopts a verb's exit code.
#[test]
fn status_says_the_remote_cannot_be_named() {
    let fx = repo();
    fx.write("root.txt", "root\n");
    fx.commit("root");
    fx.git(&["switch", "-q", "-c", "feature"]);

    // Two remotes, neither `origin` — the shape that leaves `for_branch`
    // with nothing to name.
    fx.set_config("remote.one.url", "/nonexistent/one.git");
    fx.set_config("remote.one.fetch", "+refs/heads/*:refs/remotes/one/*");
    fx.set_config("remote.two.url", "/nonexistent/two.git");
    fx.set_config("remote.two.fetch", "+refs/heads/*:refs/remotes/two/*");

    let output = ff(&fx, &["status"]);
    assert!(
        output.status.success(),
        "status reports, it does not fail: {}",
        out(&output)
    );
    let text = stdout(&output);
    assert!(
        text.contains("remote unnamed"),
        "the unnameable remote is said: {text}"
    );
    assert!(
        !text.contains("nothing to pull"),
        "an empty axis never reads as settled: {text}"
    );
}

/// One remote named `origin` that the branch's own section points at is a
/// named, settled remote: the `remote unnamed` part never appears.
#[test]
fn a_settled_remote_still_says_nothing_to_pull() {
    let fx = repo();
    fx.write("root.txt", "root\n");
    fx.commit("root");
    fx.git(&["switch", "-q", "-c", "feature"]);

    fx.set_config("remote.origin.url", "/nonexistent/origin.git");
    fx.set_config("remote.origin.fetch", "+refs/heads/*:refs/remotes/origin/*");
    fx.set_config("branch.feature.remote", "origin");
    fx.set_config("branch.feature.merge", "refs/heads/feature");

    let output = ff(&fx, &["status"]);
    assert!(
        output.status.success(),
        "status reports, it does not fail: {}",
        out(&output)
    );
    let text = stdout(&output);
    assert!(
        !text.contains("remote unnamed"),
        "a named remote is not unnameable: {text}"
    );
}

/// A path the way `ff status --json` spells one. Not `fs::canonicalize`:
/// the fixture's temp dir may sit behind a symlink, which has to be
/// resolved, but on Windows canonicalizing also adds the `\\?\` prefix
/// that fufu drops from every path it prints.
fn canonical(path: &Path) -> String {
    ff_core::linked::path::real(path).display().to_string()
}

/// The orientation a fresh agent asks for first: the root, which worktree
/// this is, and the remote. On trunk there is no base, so `base` is null the
/// way `futures.base` is.
#[test]
fn status_json_says_where_it_stands() {
    let fx = repo();
    fx.write("a.txt", "one\n");
    let commit = ff(&fx, &["commit", "-m", "one"]);
    assert!(commit.status.success(), "{}", out(&commit));

    let data = json(&ff(&fx, &["status", "--json"]))["data"].clone();
    let root = canonical(&fx.path());
    assert_eq!(data["root"], root, "the root is the canonical checkout");
    assert_eq!(
        data["worktree"],
        serde_json::json!({"id": "main", "linked": false, "main": root}),
        "the main worktree names itself"
    );
    assert!(
        data["remote"].is_null(),
        "no remote to name: {}",
        data["remote"]
    );
    assert!(
        data["base"].is_null(),
        "trunk sits on nothing: {}",
        data["base"]
    );
}

/// Above trunk, `base` names what the branch sits on and counts the commits
/// above it; back on trunk it is null again.
#[test]
fn status_json_names_the_base_and_the_count_above_it() {
    let fx = repo();
    fx.write("a.txt", "one\n");
    fx.commit("one");
    fx.write("a.txt", "two\n");
    fx.commit("two");
    let start = ff(&fx, &["start", "-b", "feat"]);
    assert!(start.status.success(), "{}", out(&start));
    fx.write("b.txt", "three\n");
    fx.commit("three");
    fx.write("b.txt", "four\n");
    fx.commit("four");

    let base = json(&ff(&fx, &["status", "--json"]))["data"]["base"].clone();
    assert_eq!(base["name"], "main", "{base}");
    assert_eq!(base["ref"], "refs/heads/main", "{base}");
    assert_eq!(base["role"], "trunk", "{base}");
    assert_eq!(base["above"], 2, "{base}");
    assert_eq!(
        base["tip"].as_str().map(str::len),
        Some(40),
        "the base tip is a full sha: {base}"
    );

    let back = ff(&fx, &["switch", "main"]);
    assert!(back.status.success(), "{}", out(&back));
    let data = json(&ff(&fx, &["status", "--json"]))["data"].clone();
    assert!(
        data["base"].is_null(),
        "trunk sits on nothing: {}",
        data["base"]
    );
}

/// A clone answers to `origin`, and `remote` says so.
#[test]
fn status_json_names_the_remote() {
    let fx = ff_testsupport::Fixture::new_cloned();
    fx.write("a.txt", "one\n");
    fx.commit("one");

    let data = json(&ff(&fx, &["status", "--json"]))["data"].clone();
    assert_eq!(data["remote"], "origin", "{}", data["remote"]);
}

/// From a linked worktree, `worktree` carries the id its chain is keyed by
/// and points back at the main checkout, and `root` is the bay itself.
#[test]
fn status_json_in_a_linked_worktree() {
    let fx = repo();
    fx.write("a.txt", "one\n");
    fx.commit("one");
    let bay = fx.root().join("bay");
    let add = ff(&fx, &["worktree", bay.to_str().unwrap(), "side"]);
    assert!(add.status.success(), "{}", out(&add));

    let data = json(&ff_at(&bay, &["status", "--json"]))["data"].clone();
    assert_eq!(data["root"], canonical(&bay), "the bay is the root");
    assert_eq!(data["worktree"]["id"], "bay", "{}", data["worktree"]);
    assert_eq!(data["worktree"]["linked"], true, "{}", data["worktree"]);
    assert_eq!(
        data["worktree"]["main"],
        canonical(&fx.path()),
        "the main checkout is named: {}",
        data["worktree"]
    );
    assert_eq!(data["head"]["name"], "side", "{}", data["head"]);
}

/// `last_op` is `ff op log --json`'s own row for the newest operation,
/// session and all. Status's own pre-capture on a dirty tree is stepped
/// over, so a read never reports itself as the last thing that happened.
#[test]
fn status_json_carries_the_last_operation_and_its_session() {
    let fx = repo();
    fx.write("a.txt", "one\n");
    let commit = ff(&fx, &["--session", "s1", "commit", "-m", "one"]);
    assert!(commit.status.success(), "{}", out(&commit));

    let last = json(&ff(&fx, &["status", "--json"]))["data"]["last_op"].clone();
    assert_eq!(last["verb"], "commit", "{last}");
    assert_eq!(last["kind"], "op", "{last}");
    assert_eq!(last["session"], "s1", "{last}");
    assert_eq!(last["branch"], "main", "{last}");
    let id = last["id"].as_str().unwrap_or_default();
    assert!(
        id.len() == 40 && id.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')),
        "an operation id is forty lowercase hex: {id:?}"
    );

    // A dirty tree makes status capture before it reads. The capture is
    // this invocation's own, so the row is still the commit.
    fx.write("a.txt", "two\n");
    let last =
        json(&ff(&fx, &["--session", "probe", "status", "--json"]))["data"]["last_op"].clone();
    assert_eq!(
        last["session"], "s1",
        "the own capture was stepped over: {last}"
    );
    assert_eq!(last["verb"], "commit", "{last}");
    assert!(
        !last["summary"]
            .as_str()
            .unwrap_or_default()
            .starts_with("pre: ff status"),
        "status never reports itself: {last}"
    );

    // Motion outside fufu is reconciled before the row is read, so the row
    // agrees with `foreign`.
    fx.git(&["commit", "-q", "-am", "behind fufu's back"]);
    let data = json(&ff(&fx, &["status", "--json"]))["data"].clone();
    assert_eq!(data["last_op"]["kind"], "foreign", "{}", data["last_op"]);
    assert!(
        data["foreign"].is_array(),
        "the foreign block agrees: {}",
        data["foreign"]
    );
}

#[test]
fn status_json_shape() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("one");
    fx.write("a.txt", "changed\n");
    fx.write("new.txt", "untracked\n");
    let out = ff(&fx, &["status", "--json"]);
    assert!(out.status.success());
    let text = stdout(&out);
    assert!(
        text.ends_with('\n') && !text[..text.len() - 1].contains('\n'),
        "one line + one newline"
    );
    let v: serde_json::Value = serde_json::from_str(&text).expect("valid json");
    let d = &v["data"];
    assert_eq!(d["head"]["state"], "branch");
    assert_eq!(d["head"]["name"], "main");
    assert_eq!(d["head"]["ref"], "refs/heads/main");
    assert_eq!(d["operation"], serde_json::Value::Null);
    assert_eq!(d["upstream"], serde_json::Value::Null);
    // Old keys are gone
    assert_eq!(d["staged"], serde_json::Value::Null);
    assert_eq!(d["unstaged"], serde_json::Value::Null);
    assert_eq!(d["untracked"], serde_json::Value::Null);
    // New shape: changes array with modified + added (untracked = ordinary addition)
    assert!(d["changes"].is_array(), "changes is array");
    let changes = d["changes"].as_array().unwrap();
    assert!(changes.len() >= 2, "at least modified and added entries");
    let kinds: std::collections::HashSet<_> = changes
        .iter()
        .map(|e| e["kind"].as_str().unwrap())
        .collect();
    assert!(kinds.contains("modified"), "modified entry present");
    assert!(kinds.contains("added"), "added entry present");
    assert_eq!(d["conflicts"].as_array().unwrap().len(), 0);
}

#[test]
fn status_json_is_stable_bytes() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("one");
    let a = ff(&fx, &["status", "--json"]);
    let b = ff(&fx, &["status", "--json"]);
    assert_eq!(a.stdout, b.stdout, "identical bytes run to run");
}

#[test]
fn status_human_header() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("one");
    let out = ff(&fx, &["status"]);
    assert!(out.status.success());
    let text = stdout(&out);
    assert!(text.starts_with("on main\n"), "header: {text:?}");
    assert!(
        text.contains("no changes"),
        "clean tree shows no changes: {text:?}"
    );
}

#[test]
fn status_human_unborn() {
    let fx = Fixture::new();
    let out = ff(&fx, &["status"]);
    assert!(out.status.success());
    let text = stdout(&out);
    assert!(text.starts_with("on main (no commits yet)"));
    // No commit row (●) when unborn
    assert!(!text.contains("●"), "no parent row when unborn: {text:?}");
}

#[test]
fn status_human_shows_stat_rows() {
    let fx = Fixture::new();
    fx.write("a.txt", "hello\nworld\n");
    fx.commit("initial");
    fx.write("a.txt", "changed\n");
    fx.write("new.txt", "untracked content\n");
    let out = ff(&fx, &["status"]);
    assert!(out.status.success());
    let text = stdout(&out);
    assert!(text.contains("a.txt"), "modified file visible: {text:?}");
    assert!(text.contains("new.txt"), "untracked file visible: {text:?}");
    assert!(text.contains("+"), "insertion count present: {text:?}");
    assert!(text.contains("2 files"), "summary row: {text:?}");
}

#[test]
fn status_human_has_no_staging_words() {
    let fx = Fixture::new();
    fx.write("a.txt", "hello\n");
    fx.commit("initial");
    fx.write("a.txt", "changed\n");
    let out = ff(&fx, &["status"]);
    assert!(out.status.success());
    let text = stdout(&out);
    assert!(!text.contains("staged"), "no 'staged': {text:?}");
    assert!(!text.contains("unstaged"), "no 'unstaged': {text:?}");
    assert!(!text.contains("untracked"), "no 'untracked': {text:?}");
}

#[test]
fn status_human_parent_row() {
    let fx = Fixture::new();
    fx.write("a.txt", "one\n");
    fx.commit("first");
    fx.write("a.txt", "two\n");
    fx.commit("second");
    let out = ff(&fx, &["status"]);
    assert!(out.status.success());
    let text = stdout(&out);
    assert!(text.contains("●"), "parent row bullet present: {text:?}");
    assert!(text.contains("second"), "parent subject visible: {text:?}");
}

#[test]
fn status_json_parent_null_when_unborn() {
    let fx = Fixture::new();
    let out = ff(&fx, &["status", "--json"]);
    assert!(out.status.success());
    let text = stdout(&out);
    let v: serde_json::Value = serde_json::from_str(&text).expect("valid json");
    assert_eq!(v["data"]["parent"], serde_json::Value::Null);
}

#[test]
fn status_json_reports_foreign_motion() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("one");
    // Bootstrap the journal so reconcile has a baseline
    let _ = ff(&fx, &["status"]);
    // Move HEAD with raw git so reconcile detects foreign motion
    fx.git(&["commit", "--amend", "--no-edit"]);
    // First ff status after the amend: reconcile absorbs AND reports the foreign change
    let out = ff(&fx, &["status", "--json"]);
    assert!(out.status.success());
    let text = stdout(&out);
    let v: serde_json::Value = serde_json::from_str(&text).expect("valid json");
    let foreign = &v["data"]["foreign"];
    assert!(foreign.is_array(), "foreign is array: {foreign}");
    assert!(
        !foreign.as_array().unwrap().is_empty(),
        "foreign is non-empty"
    );
    let first = &foreign[0];
    assert!(first.get("ref").is_some(), "has ref key");
    assert!(first.get("old").is_some(), "has old key");
    assert!(first.get("new").is_some(), "has new key");
}

#[test]
fn status_human_reports_one_foreign_change_on_one_line() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("one");
    let _ = ff(&fx, &["status"]);
    fx.git(&["commit", "--amend", "--no-edit"]);
    let out = ff(&fx, &["status"]);
    assert!(out.status.success());
    let text = stdout(&out);
    let lines: Vec<&str> = text
        .lines()
        .filter(|l| l.contains("made outside fufu"))
        .collect();
    assert_eq!(lines.len(), 1, "one summary line: {text:?}");
    assert!(
        lines[0].starts_with("1 change "),
        "singular count: {text:?}"
    );
    assert!(
        lines[0].contains("refs/heads/"),
        "the one ref is named: {text:?}"
    );
    assert!(
        !text.lines().any(|l| l.starts_with("  refs/")),
        "no per-ref rows: {text:?}"
    );
}

#[test]
fn reconcile_preamble_is_one_line_carrying_the_ref_and_hint() {
    let fx = Fixture::new();
    fx.set_config("user.name", "Foreign Motion");
    fx.set_config("user.email", "foreign@motion.test");
    fx.write("a.txt", "a\n");
    fx.commit("one");
    let _ = ff(&fx, &["status"]);
    fx.git(&["commit", "--amend", "--no-edit"]);
    // The next mutating verb absorbs the amend and says so once, on stderr.
    fx.write("b.txt", "b\n");
    let out = ff(&fx, &["commit", "-m", "two"]);
    assert!(out.status.success(), "{}", stderr(&out));
    let err = stderr(&out);
    let lines: Vec<&str> = err
        .lines()
        .filter(|l| l.starts_with("ff: absorbed"))
        .collect();
    assert_eq!(lines.len(), 1, "one absorbed line: {err:?}");
    assert!(
        lines[0].starts_with("ff: absorbed 1 change made outside fufu: refs/heads/main moved to "),
        "the ref and its motion: {err:?}"
    );
    assert!(
        lines[0].ends_with("(commit (amend): one)"),
        "git's reflog hint in parentheses: {err:?}"
    );
}

#[test]
fn several_foreign_changes_fold_to_counts_by_kind() {
    let fx = Fixture::new();
    fx.set_config("user.name", "Foreign Motion");
    fx.set_config("user.email", "foreign@motion.test");
    fx.write("a.txt", "a\n");
    fx.commit("one");
    let _ = ff(&fx, &["status"]);
    fx.git(&["branch", "a"]);
    fx.git(&["branch", "b"]);
    // Status pins the folded line while the log's tip is foreign.
    let out = ff(&fx, &["status"]);
    assert!(out.status.success());
    let text = stdout(&out);
    assert!(
        text.contains("2 changes made outside fufu, 2 created"),
        "status folds to counts: {text:?}"
    );
    assert!(
        !text.contains("refs/heads/a"),
        "no ref names when folded: {text:?}"
    );
    // Status absorbed those two; two more, and the next mutating verb's
    // preamble folds the same way.
    fx.git(&["branch", "c"]);
    fx.git(&["branch", "d"]);
    fx.write("b.txt", "b\n");
    let out = ff(&fx, &["commit", "-m", "two"]);
    assert!(out.status.success(), "{}", stderr(&out));
    let err = stderr(&out);
    assert!(
        err.lines()
            .any(|l| l == "ff: absorbed 2 changes made outside fufu: 2 created"),
        "preamble folds to counts: {err:?}"
    );
}

#[test]
fn status_json_foreign_is_null_when_clean() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("one");
    let out = ff(&fx, &["status", "--json"]);
    assert!(out.status.success());
    let text = stdout(&out);
    let v: serde_json::Value = serde_json::from_str(&text).expect("valid json");
    assert_eq!(v["data"]["foreign"], serde_json::Value::Null);
}

#[test]
fn status_json_keys_are_unchanged() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("one");
    fx.write("b.txt", "modified\n");
    let out = ff(&fx, &["status", "--json"]);
    assert!(out.status.success());
    let text = stdout(&out);
    let v: serde_json::Value = serde_json::from_str(&text).expect("valid json");
    let d = &v["data"];
    // Every pre-existing key must be present (non-null for non-optional fields)
    for key in [
        "head",
        "changes",
        "insertions",
        "deletions",
        "open",
        "conflicts",
    ] {
        assert!(!d[key].is_null(), "key {} is non-null", key);
    }
    // Optional keys exist (may be null)
    for key in ["operation", "upstream", "parent", "foreign"] {
        assert!(d.get(key).is_some(), "key {} exists", key);
    }
    // open sub-keys
    let open = &d["open"];
    for key in ["id", "change_id", "pending", "subject", "clean"] {
        assert!(open.get(key).is_some(), "open.{} exists", key);
    }
}

#[test]
fn status_human_output_is_unchanged() {
    let fx = Fixture::new();
    fx.write("a.txt", "hello\n");
    fx.commit("initial");
    fx.write("a.txt", "changed\n");
    let out = ff(&fx, &["status"]);
    assert!(out.status.success());
    let text = stdout(&out);
    // Two-row shape: header + open change row (plus diffstat)
    assert!(text.starts_with("on main\n"), "header line: {text:?}");
    // Diffstat line for the modified file
    assert!(
        text.contains("a.txt"),
        "modified file in diffstat: {text:?}"
    );
    assert!(text.contains("1 file"), "summary row: {text:?}");
}

#[test]
fn status_and_log_capture_first() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    fx.write("a.txt", "dirty\n");
    let out = ff(&fx, &["status"]);
    assert!(out.status.success());
    let subject = fx.git(&["log", "-1", "--format=%s", "refs/fufu/snap/main"]);
    assert_eq!(subject.trim(), "pre: ff status");

    fx.write("a.txt", "more dirt\n");
    let out = ff(&fx, &["log"]);
    assert!(out.status.success());
    let subject = fx.git(&["log", "-1", "--format=%s", "refs/fufu/snap/main"]);
    assert_eq!(subject.trim(), "pre: ff log");
}
