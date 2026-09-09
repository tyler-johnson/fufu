//! The change id: the identity a commit keeps through rewrites, minted for
//! the open change and written into the commit as a `change-id` header.
//! Every rewrite fufu makes carries it; the open change's id moves with the
//! close and comes back with `ff undo`.

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

fn out(output: &Output) -> String {
    format!("{}{}", stdout(output), stderr(output))
}

fn json(output: &Output) -> serde_json::Value {
    serde_json::from_str(&stdout(output)).expect("valid json")
}

fn ok(output: Output) -> Output {
    assert!(output.status.success(), "{}", out(&output));
    output
}

fn repo() -> Fixture {
    let fx = Fixture::new();
    fx.set_config("user.name", "Change Tester");
    fx.set_config("user.email", "change@test.test");
    fx
}

fn head(fx: &Fixture) -> String {
    fx.git(&["rev-parse", "HEAD"]).trim().to_string()
}

/// A commit's change id as `ff show --json` reports it.
fn id_of(fx: &Fixture, rev: &str) -> String {
    json(&ok(ff(fx, &["--json", "show", rev])))["data"]["change_id"]
        .as_str()
        .expect("change_id on ff show --json")
        .to_string()
}

/// The `change-id` header on a commit, raw off the object.
fn header_of(fx: &Fixture, rev: &str) -> Option<String> {
    fx.git(&["cat-file", "-p", rev])
        .lines()
        .take_while(|line| !line.is_empty())
        .find_map(|line| line.strip_prefix("change-id ").map(str::to_string))
}

/// The open change's id as the branch metadata records it, read without
/// running a verb, so the read itself cannot mint one.
fn open_id_on_disk(fx: &Fixture, branch: &str) -> Option<String> {
    let path = fx.path().join(".git/fufu/branch").join(branch);
    let text = std::fs::read_to_string(path).ok()?;
    let meta: serde_json::Value = serde_json::from_str(&text).expect("branch metadata is json");
    meta["change_id"].as_str().map(str::to_string)
}

fn is_letters(s: &str) -> bool {
    s.len() == 32 && s.chars().all(|c| ('k'..='z').contains(&c))
}

/// The close writes the id as a header, and `ff show` reads the header.
#[test]
fn a_close_writes_the_header_and_show_reads_it() {
    let fx = repo();
    fx.write("a.txt", "a\n");
    fx.commit("root");
    fx.write("a.txt", "b\n");
    ok(ff(&fx, &["commit", "-m", "closed"]));

    let header = header_of(&fx, "HEAD").expect("a change-id header");
    assert!(is_letters(&header), "{header}");
    assert_eq!(id_of(&fx, "HEAD"), header);

    // The human header line carries the whole id after the sha.
    let text = stdout(&ok(ff(&fx, &["show", "HEAD"])));
    let first = text.lines().next().unwrap();
    let tokens: Vec<&str> = first.split_whitespace().collect();
    assert_eq!(tokens[1], header, "{first:?}");

    // A commit git made has no header and a derived id, the same one on
    // every read.
    assert_eq!(header_of(&fx, "HEAD^"), None);
    let derived = id_of(&fx, "HEAD^");
    assert!(is_letters(&derived), "{derived}");
    assert_eq!(id_of(&fx, "HEAD^"), derived);
    assert_ne!(derived, header);
}

/// `ff describe <rev>` rewrites the commit and the id rides along.
#[test]
fn the_id_survives_a_reword() {
    let fx = repo();
    fx.write("a.txt", "a\n");
    fx.commit("root");
    fx.write("a.txt", "b\n");
    ok(ff(&fx, &["commit", "-m", "first words"]));
    let before = head(&fx);
    let id = id_of(&fx, &before);

    ok(ff(&fx, &["describe", &before, "-m", "second words"]));
    let after = head(&fx);
    assert_ne!(before, after, "the reword rewrote the commit");
    assert_eq!(id_of(&fx, &after), id, "and the id stayed");
    assert_eq!(header_of(&fx, &after).as_deref(), Some(id.as_str()));
}

/// `ff restack` replays the branch's commits and each keeps its id.
#[test]
fn the_id_survives_a_restack() {
    let fx = repo();
    fx.write("root.txt", "root\n");
    fx.commit("root");
    fx.git(&["switch", "-q", "-c", "feature"]);
    fx.write("f.txt", "f\n");
    ok(ff(&fx, &["commit", "-m", "f1"]));
    let f1 = head(&fx);
    let id = id_of(&fx, &f1);

    fx.git(&["switch", "-q", "main"]);
    fx.write("m.txt", "m\n");
    fx.commit("m1");
    fx.git(&["switch", "-q", "feature"]);

    ok(ff(&fx, &["restack"]));
    let replayed = head(&fx);
    assert_ne!(f1, replayed, "the restack replayed f1");
    assert_eq!(id_of(&fx, &replayed), id);
}

/// `ff absorb` rewrites the commit beneath the open change; the rewritten
/// commit is the same change, so it keeps the id.
#[test]
fn the_id_survives_an_absorb() {
    let fx = repo();
    fx.write("a.txt", "a\n");
    fx.commit("root");
    fx.write("a.txt", "b\n");
    ok(ff(&fx, &["commit", "-m", "work"]));
    let before = head(&fx);
    let id = id_of(&fx, &before);

    fx.write("a.txt", "c\n");
    ok(ff(&fx, &["absorb"]));
    let after = head(&fx);
    assert_ne!(before, after, "the absorb rewrote the commit");
    assert_eq!(id_of(&fx, &after), id);
}

/// `ff pull`'s replay onto the moved base carries the id too.
#[test]
fn the_id_survives_a_pull_replay() {
    let fx = repo();
    fx.write("root.txt", "root\n");
    fx.commit("root");
    fx.git(&["switch", "-q", "-c", "feature"]);
    fx.write("f.txt", "f\n");
    ok(ff(&fx, &["commit", "-m", "f1"]));
    let f1 = head(&fx);
    let id = id_of(&fx, &f1);

    fx.git(&["switch", "-q", "main"]);
    fx.write("m.txt", "m\n");
    fx.commit("m1");
    fx.git(&["switch", "-q", "feature"]);

    ok(ff(&fx, &["pull", "--no-fetch"]));
    let replayed = head(&fx);
    assert_ne!(f1, replayed, "the pull replayed f1");
    assert_eq!(id_of(&fx, &replayed), id);
}

/// `ff undo` of a close puts the id back on the open change, and `ff redo`
/// takes it off again: the identity moves with the close both ways.
#[test]
fn undo_of_a_close_puts_the_id_back_on_the_open_change() {
    let fx = repo();
    fx.write("a.txt", "a\n");
    fx.commit("root");
    fx.write("a.txt", "b\n");
    let open = json(&ok(ff(&fx, &["--json", "log"])))["data"]["open"]["change_id"]
        .as_str()
        .expect("the open change has an id once captured")
        .to_string();
    assert!(is_letters(&open), "{open}");

    ok(ff(&fx, &["commit", "-m", "closed"]));
    let log = json(&ok(ff(&fx, &["--json", "log"])));
    assert_eq!(log["data"]["commits"][0]["change_id"], open);
    assert!(
        log["data"]["open"]["change_id"].is_null(),
        "the id left with the close: {log}"
    );

    ok(ff(&fx, &["undo"]));
    let log = json(&ok(ff(&fx, &["--json", "log"])));
    assert_eq!(
        log["data"]["open"]["change_id"], open,
        "undo put the id back on @: {log}"
    );
    assert_ne!(log["data"]["commits"][0]["change_id"], open);

    ok(ff(&fx, &["redo"]));
    let log = json(&ok(ff(&fx, &["--json", "log"])));
    assert_eq!(log["data"]["commits"][0]["change_id"], open);
    assert!(
        log["data"]["open"]["change_id"].is_null(),
        "redo took it off again: {log}"
    );
}

/// A partial close leaves a remainder on disk, and the remainder is a change
/// with an id of its own the moment the close lands — before any capture.
#[test]
fn a_partial_close_gives_the_remainder_a_fresh_id_at_once() {
    let fx = repo();
    fx.write("a.txt", "a\n");
    fx.write("b.txt", "b\n");
    fx.commit("root");
    fx.write("a.txt", "a2\n");
    fx.write("b.txt", "b2\n");
    ok(ff(&fx, &[]));
    let open = open_id_on_disk(&fx, "main").expect("the capture minted an id");

    ok(ff(&fx, &["commit", "a.txt", "-m", "just a"]));
    assert_eq!(id_of(&fx, "HEAD"), open, "the slice wears the open id");
    let remainder = open_id_on_disk(&fx, "main").expect("the remainder has an id already");
    assert!(is_letters(&remainder), "{remainder}");
    assert_ne!(remainder, open, "and it is a fresh one");

    // The @ row wears it, and the close of the remainder writes it.
    let status = json(&ok(ff(&fx, &["--json", "status"])));
    assert_eq!(status["data"]["open"]["change_id"], remainder);
    ok(ff(&fx, &["commit", "-m", "the rest"]));
    assert_eq!(id_of(&fx, "HEAD"), remainder);
    assert_eq!(open_id_on_disk(&fx, "main"), None);
}

/// `ff commit -b <fresh>` from a named branch lands on the new branch and
/// leaves the old one behind: the id and the pending description both go
/// with the commit, and the branch left behind keeps neither.
#[test]
fn a_fresh_branch_close_clears_the_branch_left_behind() {
    let fx = repo();
    fx.write("a.txt", "a\n");
    fx.commit("root");
    fx.write("a.txt", "b\n");
    ok(ff(&fx, &["describe", "-m", "planned"]));
    let open = open_id_on_disk(&fx, "main").expect("describe minted an id");

    ok(ff(&fx, &["commit", "-b", "fresh"]));
    assert_eq!(
        fx.git(&["rev-parse", "--abbrev-ref", "HEAD"]).trim(),
        "fresh"
    );
    assert_eq!(id_of(&fx, "HEAD"), open);
    assert_eq!(fx.git(&["log", "-1", "--format=%s"]).trim(), "planned");
    assert!(
        !fx.path().join(".git/fufu/branch/main").exists(),
        "main keeps neither the id nor the description"
    );
    assert_eq!(open_id_on_disk(&fx, "fresh"), None);
}

/// A detached tree closes nothing and an editing session amends a commit
/// that already has an id, so a capture mints nothing for either.
#[test]
fn no_id_is_minted_for_a_detached_tree_or_a_session() {
    let fx = repo();
    fx.write("a.txt", "a\n");
    let c0 = fx.commit("c0");
    fx.write("a.txt", "b\n");
    fx.commit("c1");

    fx.git(&["checkout", "-q", "--detach"]);
    fx.write("a.txt", "detached\n");
    ok(ff(&fx, &[]));
    let entries: Vec<String> = std::fs::read_dir(fx.path().join(".git/fufu/branch"))
        .map(|dir| {
            dir.map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    assert!(
        entries.is_empty(),
        "a detached capture wrote branch metadata: {entries:?}"
    );
    fx.git(&["checkout", "-q", "--", "a.txt"]);
    fx.git(&["switch", "-q", "main"]);

    ok(ff(&fx, &["edit", &c0]));
    let session = fx
        .git(&["rev-parse", "--abbrev-ref", "HEAD"])
        .trim()
        .to_string();
    fx.write("a.txt", "amended\n");
    ok(ff(&fx, &[]));
    assert_eq!(
        open_id_on_disk(&fx, &session),
        None,
        "a session's capture minted an id"
    );
    // The @ row wears the amended commit's id instead.
    let status = json(&ok(ff(&fx, &["--json", "status"])));
    assert_eq!(status["data"]["open"]["change_id"], id_of(&fx, &c0));
}

/// The map's `@` row and `ff log`'s agree on the open change's id, and it is
/// its own, not the tip's.
#[test]
fn the_map_shows_the_open_id_beside_the_tip() {
    let fx = repo();
    fx.write("a.txt", "a\n");
    fx.commit("root");
    fx.write("a.txt", "b\n");
    ok(ff(&fx, &["commit", "-m", "tip"]));
    fx.write("a.txt", "c\n");

    let open = json(&ok(ff(&fx, &["--json", "status"])))["data"]["open"]["change_id"]
        .as_str()
        .unwrap()
        .to_string();
    let tip = id_of(&fx, "HEAD");
    let map = stdout(&ok(ff(&fx, &[])));
    let at = map.lines().next().unwrap();
    assert_eq!(at.split_whitespace().nth(1).unwrap(), &open[..8], "{map}");
    let bullet = map
        .lines()
        .find(|l| l.contains(&tip[..8]))
        .expect("the tip's row");
    assert!(bullet.starts_with('●'), "{bullet}");
    assert_ne!(&open[..8], &tip[..8]);

    let json_map = json(&ok(ff(&fx, &["--json"])));
    let rows = json_map["data"]["rows"].as_array().unwrap();
    assert_eq!(rows[0]["node"]["change_id"], open, "{json_map}");
    assert_eq!(rows[1]["node"]["change_id"], tip, "{json_map}");
}
