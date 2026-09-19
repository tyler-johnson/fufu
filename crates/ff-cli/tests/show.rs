//! `ff show` — one revision, header and patch.
//!
//! The revision half of the patch layer, and the place the two address
//! spaces meet: an operation id typed here is the right id and the wrong
//! verb, and the refusal says which verb.

use std::path::Path;
use std::process::{Command, Output};

use ff_testsupport::Fixture;
use ff_testsupport::fixtures::{ageless, null_device};

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
    // Ageless: the header carries the open change's age, and two spawns
    // straddling a second boundary render `0s ago` against `1s ago`.
    assert_eq!(ageless(&bare), ageless(&at), "bare is `@`");

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

/// One revision, then paths. A second sha in the path slot used to read as
/// a filter that matched nothing and answer "it changed no files"; now it
/// is refused, and the exit names the one-revision shape.
#[test]
fn a_second_revision_in_the_path_slot_is_refused() {
    let fx = repo();
    fx.write("a.txt", "a\n");
    fx.commit("one");
    fx.write("a.txt", "b\n");
    fx.commit("two");
    let older = fx
        .git(&["rev-parse", "--short", "HEAD~1"])
        .trim()
        .to_string();

    let out = ff(&fx, &["show", "HEAD", &older]);
    assert_eq!(out.status.code(), Some(2), "{}", stderr(&out));
    let err = stderr(&out);
    assert!(err.contains(&older), "the token is named: {err}");
    assert!(
        err.contains("ff show <rev>"),
        "the one-revision shape: {err}"
    );
    assert!(
        !stdout(&out).contains("changed no files"),
        "not answered as an empty commit: {}",
        stdout(&out)
    );

    let out = ff(&fx, &["--json", "show", "HEAD", &older]);
    assert_eq!(out.status.code(), Some(2));
    let v = json(&out);
    assert_eq!(v["error"]["id"].as_str(), Some("usage/no-such-path"), "{v}");
}

/// A path on neither disk nor in HEAD is refused under both the commit
/// branch and the open-change branch, since neither diffs before the check.
#[test]
fn a_path_that_names_nothing_is_refused() {
    let fx = repo();
    fx.write("a.txt", "a\n");
    fx.commit("one");

    for args in [
        &["show", "HEAD", "bogus.txt"][..],
        &["show", "@", "bogus.txt"][..],
    ] {
        let out = ff(&fx, args);
        assert_eq!(out.status.code(), Some(2), "{args:?}: {}", stderr(&out));
        assert!(stderr(&out).contains("bogus.txt"), "{}", stderr(&out));

        let out = ff(&fx, &["--json", args[0], args[1], args[2]]);
        assert_eq!(out.status.code(), Some(2), "{args:?}");
        let v = json(&out);
        assert_eq!(v["error"]["id"].as_str(), Some("usage/no-such-path"), "{v}");
    }
}

/// The revision resolves first, so a bad revision keeps its own refusal
/// even when a bad path follows it.
#[test]
fn a_bad_revision_still_wins_over_a_bad_path() {
    let fx = repo();
    fx.write("a.txt", "a\n");
    fx.commit("one");

    let out = ff(&fx, &["--json", "show", "nope", "bogus.txt"]);
    assert_eq!(out.status.code(), Some(2));
    let id = json(&out)["error"]["id"]
        .as_str()
        .expect("a coded refusal")
        .to_string();
    assert!(
        id.starts_with("usage/revset-"),
        "the revision's own error: {id}"
    );
}

/// The message prints whole: the subject, a blank line, then the body with
/// its paragraphs intact, each line indented like the subject. JSON carries
/// the two halves as fields.
#[test]
fn a_multi_paragraph_message_prints_whole() {
    let fx = repo();
    fx.write("a.txt", "a\n");
    fx.commit("one");
    fx.write("a.txt", "b\n");
    fx.commit("subject\n\nfirst para\nsecond line\n\nsecond para\n");

    let out = ff(&fx, &["show", "HEAD"]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    let text = stdout(&out);
    assert!(
        text.contains("  subject\n\n  first para\n  second line\n\n  second para\n\ndiff --git"),
        "subject, blank line, body, blank line, patch: {text}"
    );

    let v = json(&ff(&fx, &["show", "HEAD", "--json"]));
    assert_eq!(v["data"]["subject"].as_str(), Some("subject"));
    assert_eq!(
        v["data"]["body"].as_str(),
        Some("first para\nsecond line\n\nsecond para")
    );
}

/// A one-line message has no body: nothing between the subject and the
/// patch but the one blank line, and an empty string on the machine surface.
#[test]
fn a_one_line_message_has_an_empty_body() {
    let fx = repo();
    fx.write("a.txt", "a\n");
    fx.commit("one");

    let text = stdout(&ff(&fx, &["show", "HEAD"]));
    assert!(text.contains("  one\n\ndiff --git"), "{text}");
    let v = json(&ff(&fx, &["show", "HEAD", "--json"]));
    assert_eq!(v["data"]["body"].as_str(), Some(""));
}

/// Trailing newlines on the stored message are the message's, not the
/// layout's: the body is trimmed, so the note after it sits one blank line
/// down rather than two.
#[test]
fn a_trailing_newline_adds_no_blank_line() {
    let fx = repo();
    fx.write("a.txt", "a\n");
    fx.commit("one");
    fx.git(&[
        "commit",
        "--cleanup=verbatim",
        "-q",
        "--allow-empty",
        "-m",
        "s\n\nb\n\n",
    ]);

    let text = stdout(&ff(&fx, &["show", "HEAD"]));
    assert!(
        text.contains("  s\n\n  b\n\n  (it changed no files)\n"),
        "{text}"
    );
    assert!(!text.contains("\n\n\n"), "no doubled blank line: {text}");
    let v = json(&ff(&fx, &["show", "HEAD", "--json"]));
    assert_eq!(v["data"]["body"].as_str(), Some("b"));
}

/// The pending description is stored whole and prints the way the commit
/// it becomes will: subject, blank line, body.
#[test]
fn the_open_change_prints_its_description_whole() {
    let fx = repo();
    fx.write("a.txt", "a\n");
    fx.commit("one");
    fx.write("a.txt", "b\n");
    let out = ff(&fx, &["describe", "-m", "subject\n\nbody"]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));

    let text = stdout(&ff(&fx, &["show"]));
    assert!(text.contains("  subject\n\n  body\n\ndiff --git"), "{text}");

    let v = json(&ff(&fx, &["show", "--json"]));
    assert_eq!(v["data"]["kind"].as_str(), Some("open"));
    assert_eq!(v["data"]["subject"].as_str(), Some("subject"));
    assert_eq!(v["data"]["body"].as_str(), Some("body"));
}

/// `--stat` and `--name-only` shorten the patch to the diffstat and to the
/// paths, under the same header, and JSON drops the keys each view has no
/// use for. The rail rows hang directly under a one-line message the way
/// `ff status` hangs them under `@`.
#[test]
fn stat_and_name_only_replace_the_patch() {
    let fx = repo();
    fx.write("a.txt", "1\n2\n3\n");
    fx.commit("one");
    fx.write("a.txt", "1\ntwo\n3\n");
    fx.commit("two");

    let out = ff(&fx, &["show", "--stat", "HEAD"]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    let body = stdout(&out);
    assert!(
        body.contains("  two\n│  M a.txt"),
        "rows under the subject: {body}"
    );
    assert!(body.contains("1 file"), "{body}");
    assert!(!body.contains("@@"), "no patch: {body}");

    let names = stdout(&ff(&fx, &["show", "--name-only", "HEAD"]));
    assert!(names.contains("│  M a.txt\n"), "{names}");
    assert!(!names.contains("+1"), "no counts: {names}");

    let v = json(&ff(&fx, &["show", "--stat", "HEAD", "--json"]));
    assert!(v["data"]["changes"][0]["insertions"].is_number(), "{v}");
    assert!(v["data"]["changes"][0].get("hunks").is_none(), "{v}");
    let v = json(&ff(&fx, &["show", "--name-only", "HEAD", "--json"]));
    assert!(v["data"]["changes"][0].get("insertions").is_none(), "{v}");
    assert!(v["data"].get("insertions").is_none(), "{v}");
    assert_eq!(v["data"]["changes"][0]["kind"].as_str(), Some("modified"));

    // The open change hangs its rows under the description the same way.
    fx.write("a.txt", "1\ntwo\n3\n4\n");
    let out = ff(&fx, &["describe", "-m", "open work"]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    let open = stdout(&ff(&fx, &["show", "--stat"]));
    assert!(open.contains("  open work\n│  M a.txt"), "{open}");
}

/// `--no-patch` stops at the message: no patch, no note about the files,
/// and JSON without the three keys the files would fill.
#[test]
fn no_patch_stops_at_the_message() {
    let fx = repo();
    fx.write("a.txt", "1\n");
    fx.commit("one");
    fx.write("a.txt", "2\n");
    fx.commit("two\n\nwhy two\n");

    let out = ff(&fx, &["show", "--no-patch", "HEAD"]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    let body = stdout(&out);
    assert!(body.contains("  two\n\n  why two\n"), "{body}");
    assert!(!body.contains("@@"), "{body}");
    assert!(!body.contains("it changed no files"), "{body}");
    assert!(!body.contains("a.txt"), "{body}");

    let v = json(&ff(&fx, &["show", "--no-patch", "HEAD", "--json"]));
    let data = &v["data"];
    assert!(data.get("changes").is_none(), "{v}");
    assert!(data.get("insertions").is_none(), "{v}");
    assert!(data.get("deletions").is_none(), "{v}");
    assert_eq!(data["subject"].as_str(), Some("two"));
    assert_eq!(data["body"].as_str(), Some("why two"));
    assert_eq!(data["merge"].as_bool(), Some(false));
}

/// Three views, one at a time: any two together are refused with the coded
/// id under both surfaces.
#[test]
fn the_file_views_exclude_one_another() {
    let fx = repo();
    fx.write("a.txt", "a\n");
    fx.commit("one");
    for pair in [
        ["--stat", "--no-patch"],
        ["--name-only", "--no-patch"],
        ["--stat", "--name-only"],
    ] {
        let out = ff(&fx, &["show", pair[0], pair[1], "HEAD"]);
        assert_eq!(out.status.code(), Some(2), "{pair:?}: {}", stderr(&out));
        let v = json(&ff(&fx, &["--json", "show", pair[0], pair[1], "HEAD"]));
        assert_eq!(v["error"]["id"].as_str(), Some("usage/bad-flags"), "{v}");
    }
}

/// `-U` reaches the commit's patch through the same dial `ff diff` turns.
#[test]
fn context_lines_reach_show() {
    let fx = repo();
    fx.write("a.txt", "1\n2\n3\n");
    fx.commit("one");
    fx.write("a.txt", "1\ntwo\n3\n");
    fx.commit("two");
    let zero = stdout(&ff(&fx, &["show", "-U", "0", "HEAD"]));
    assert!(zero.contains("@@ -2 +2 @@"), "{zero}");
    let one = stdout(&ff(&fx, &["show", "-U", "1", "HEAD"]));
    assert!(one.contains("@@ -1,3 +1,3 @@"), "{one}");
}
