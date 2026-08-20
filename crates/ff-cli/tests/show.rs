//! `ff show` — one revision, header and patch.
//!
//! The revision half of the patch layer, and the place the two address
//! spaces meet: an operation id typed here is the right id and the wrong
//! verb, and the refusal says which verb.

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

fn json(output: &Output) -> serde_json::Value {
    serde_json::from_str(&stdout(output)).expect("valid json")
}

/// The fixture stamps every commit `Fixture Author` through the git author
/// environment, which outranks `user.name` — so that is the name the header
/// has to print.
const AUTHOR: &str = "Fixture Author";

fn repo() -> Fixture {
    Fixture::new()
}

/// A commit's furniture, then what it did — measured against its first
/// parent, not against nothing.
#[test]
fn a_commit_shows_its_header_and_its_patch() {
    let fx = repo();
    fx.write("a.txt", "1\n2\n3\n");
    fx.commit("one");
    fx.write("a.txt", "1\ntwo\n3\n");
    fx.commit("two");

    let out = ff(&fx, &["show", "HEAD"]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    let body = stdout(&out);
    assert!(body.contains("two"), "the subject: {body}");
    assert!(body.contains(AUTHOR), "the author: {body}");
    assert!(body.contains("ago"), "the age: {body}");
    assert!(body.contains("@@ -1,3 +1,3 @@"), "the patch: {body}");
    assert!(body.contains("-2"), "the removed line: {body}");
    assert!(body.contains("+two"), "the added line: {body}");
    // Against the first parent, so the file's untouched lines are context
    // rather than a whole-file addition.
    assert!(
        !body.contains("new file mode"),
        "measured against the parent, not against nothing: {body}"
    );
}

/// The default argument is `@`, and `@` is the open change — which means
/// `ff show` and `ff diff` print the same body under different furniture.
#[test]
fn bare_show_is_the_open_change_and_shares_ff_diffs_body() {
    let fx = repo();
    fx.write("a.txt", "1\n");
    fx.commit("one");
    fx.write("a.txt", "2\n");
    fx.write("fresh.txt", "new\n");

    let bare = stdout(&ff(&fx, &["show"]));
    let at = stdout(&ff(&fx, &["show", "@"]));
    assert_eq!(bare, at, "bare is `@`");

    let patch = stdout(&ff(&fx, &["diff"]));
    assert!(!patch.is_empty(), "there is a patch to compare");
    assert!(
        bare.ends_with(&patch),
        "one renderer, called twice:\n--- show ---\n{bare}\n--- diff ---\n{patch}"
    );
    assert!(
        bare.contains("the open change on main"),
        "with a header of its own: {bare}"
    );
}

/// Paths narrow it the same way they narrow `ff diff` — one pathspec rule
/// for the tool, not one per verb.
#[test]
fn paths_narrow_the_patch() {
    let fx = repo();
    fx.write("root.txt", "a\n");
    fx.write("src/one.txt", "a\n");
    fx.commit("one");
    fx.write("root.txt", "b\n");
    fx.write("src/one.txt", "b\n");
    fx.commit("two");

    let dir = stdout(&ff(&fx, &["show", "HEAD", "src"]));
    assert!(dir.contains("src/one.txt"), "{dir}");
    assert!(!dir.contains("root.txt"), "only that directory: {dir}");
}

/// A merge names the ambiguity rather than picking a parent silently. git
/// prints no diff here either; saying why beats printing nothing.
#[test]
fn a_merge_names_the_ambiguity() {
    let fx = repo();
    fx.write("base.txt", "base\n");
    fx.commit("base");
    fx.git(&["switch", "-c", "side", "-q"]);
    fx.write("side.txt", "side\n");
    fx.commit("side work");
    fx.git(&["switch", "main", "-q"]);
    fx.write("main.txt", "main\n");
    fx.commit("main work");
    fx.git(&["merge", "--no-ff", "-m", "merge side", "side"]);

    let out = ff(&fx, &["show", "HEAD"]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    let body = stdout(&out);
    assert!(body.contains("merge side"), "the subject: {body}");
    assert!(
        body.contains("a merge — which parent to diff against is a choice"),
        "the ambiguity, named: {body}"
    );
    assert!(
        body.contains("ff git show -m"),
        "and where the per-parent view is: {body}"
    );
    assert!(!body.contains("@@"), "no diff was picked for you: {body}");

    let v = json(&ff(&fx, &["show", "HEAD", "--json"]));
    assert_eq!(v["data"]["merge"].as_bool(), Some(true));
    assert_eq!(v["data"]["parents"].as_array().map(Vec::len), Some(2));
}

/// The two address spaces do not mix, and the refusal says which verb the
/// id belongs to. This is the resolver `ff restore --from` already uses —
/// no new code, no new registry id.
#[test]
fn an_operation_id_here_is_refused_by_address_space() {
    let fx = repo();
    fx.write("a.txt", "a\n");
    fx.commit("one");
    fx.write("a.txt", "b\n");

    let op = {
        let out = ff(&fx, &["op", "log", "--json"]);
        let v: serde_json::Value = serde_json::from_str(&stdout(&out)).expect("valid json");
        v["data"]["ops"][0]["id"]
            .as_str()
            .expect("an operation id")
            .to_string()
    };

    let out = ff(&fx, &["--json", "show", &op]);
    assert!(!out.status.success(), "an op id is not a revision");
    let v = json(&out);
    assert_eq!(
        v["error"]["id"].as_str(),
        Some("usage/op-in-rev-position"),
        "{v}"
    );
    let message = v["error"]["message"].as_str().expect("a message");
    assert!(
        message.contains("this position takes revisions"),
        "the position is named generically — `-r` is only one of three: {message}"
    );
    let exits = v["error"]["exits"].to_string();
    assert!(
        exits.contains("ff op show"),
        "the verb that reads it: {exits}"
    );
}

/// The machine surface carries the header as fields and the patch as
/// hunks, so nothing has to parse the rendered output back.
#[test]
fn the_json_envelope_carries_header_and_hunks() {
    let fx = repo();
    fx.write("a.txt", "1\n2\n3\n");
    fx.commit("one");
    fx.write("a.txt", "1\ntwo\n3\n");
    fx.commit("two");

    let out = ff(&fx, &["show", "HEAD", "--json"]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    let v = json(&out);
    assert_eq!(v["cmd"].as_str(), Some("show"));
    let data = &v["data"];
    assert_eq!(data["kind"].as_str(), Some("commit"));
    assert_eq!(data["subject"].as_str(), Some("two"));
    assert_eq!(data["author_name"].as_str(), Some(AUTHOR));
    assert_eq!(data["merge"].as_bool(), Some(false));
    assert_eq!(
        data["changes"][0]["hunks"][0]["header"].as_str(),
        Some("@@ -1,3 +1,3 @@")
    );

    // And `@` reports what it is rather than pretending to be a commit.
    let open = json(&ff(&fx, &["show", "--json"]));
    assert_eq!(open["data"]["kind"].as_str(), Some("open"));
    assert_eq!(open["data"]["branch"].as_str(), Some("main"));
}

/// Blob and tree reads stay git's — the same call `ff blame` got. A revset
/// denotes a set of commits, so a spelling that peels to a tree or a blob
/// is refused here rather than answered.
#[test]
fn a_blob_or_tree_spelling_is_not_a_revision() {
    let fx = repo();
    fx.write("a.txt", "a\n");
    fx.commit("one");

    for spelling in ["HEAD:a.txt", "HEAD^{tree}"] {
        let out = ff(&fx, &["--json", "show", spelling]);
        assert!(!out.status.success(), "{spelling} is not a commit");
        let id = json(&out)["error"]["id"]
            .as_str()
            .expect("a coded refusal")
            .to_string();
        assert!(
            id.starts_with("usage/revset-"),
            "{spelling} earns a revset refusal, got {id}"
        );
    }
}
