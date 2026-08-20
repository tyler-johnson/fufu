//! `ff diff` — the open change as a patch.
//!
//! The property that matters most here is not how the output looks: it is
//! that the output *applies*. A patch format that only fufu can read would
//! be exactly the dialect this verb exists not to invent, so the round trip
//! through `git apply` is the test that keeps it honest.

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

fn repo() -> Fixture {
    let fx = Fixture::new();
    fx.set_config("user.name", "Diff Tester");
    fx.set_config("user.email", "diff@test.test");
    fx
}

/// The gap this verb closes, and the reason it captures first: a file
/// written since the last operation is not in any tree yet, so a diff that
/// did not capture would report a clean worktree on the file you just made.
/// No `ff status` beforehand — that is the whole point.
#[test]
fn an_untracked_file_shows_its_content_with_no_status_first() {
    let fx = repo();
    fx.write("tracked.txt", "a\n");
    fx.commit("one");
    fx.write("tracked.txt", "a\nb\n");
    fx.write("newfile.txt", "brand new\n");

    let out = ff(&fx, &["diff"]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    let body = stdout(&out);
    assert!(body.contains("+b"), "the modified file's content: {body}");
    assert!(
        body.contains("diff --git a/newfile.txt b/newfile.txt"),
        "the untracked file has a header: {body}"
    );
    assert!(
        body.contains("new file mode 100644"),
        "and git's created-file line: {body}"
    );
    assert!(
        body.contains("+brand new"),
        "and its content — the half git diff cannot see: {body}"
    );

    // The other side of the table the gaps report drew: git's own diff, on
    // the same tree, misses the file entirely.
    let git = fx.git(&["diff"]);
    assert!(
        !git.contains("newfile.txt"),
        "git diff still cannot see it — that is the gap: {git}"
    );
}

/// The headline property: what comes out of `ff diff` is a patch, and a
/// patch is something `git apply` takes. Applied onto a clean checkout of
/// HEAD it must reproduce the open change's tree exactly.
#[test]
fn the_output_applies() {
    let fx = repo();
    fx.write("keep.txt", "1\n2\n3\n4\n5\n");
    fx.write("gone.txt", "delete me\n");
    fx.write("nested/deep.txt", "x\n");
    fx.commit("one");

    fx.write("keep.txt", "1\ntwo\n3\n4\n5\n");
    fx.remove("gone.txt");
    fx.write("fresh.txt", "never tracked\n");
    fx.write("nested/deep.txt", "x\ny\n");

    let patch = stdout(&ff(&fx, &["diff"]));
    let patch_file = fx.path().join("..").join("open.patch");
    std::fs::write(&patch_file, &patch).expect("write patch");

    // A clean checkout of HEAD, with none of the open change in it.
    let clean = fx.path().join("..").join("clean");
    fx.git(&[
        "worktree",
        "add",
        "-q",
        clean.to_str().expect("utf-8 path"),
        "HEAD",
    ]);

    let applied = Command::new("git")
        .current_dir(&clean)
        .args(["apply", patch_file.to_str().expect("utf-8 path")])
        .output()
        .expect("spawn git apply");
    assert!(
        applied.status.success(),
        "git apply refused the patch:\n{}\n--- patch ---\n{patch}",
        String::from_utf8_lossy(&applied.stderr)
    );

    // The tree the patch produced, against the tree fufu says the open
    // change is. Hashing both through git's own object database is the only
    // comparison that cannot be fooled by a formatting accident.
    let applied_tree = {
        fx.git_in(&clean, &["add", "-A"]);
        fx.git_in(&clean, &["write-tree"])
    };
    let open_tree = {
        fx.git(&["add", "-A"]);
        fx.git(&["write-tree"])
    };
    assert_eq!(
        applied_tree.trim(),
        open_tree.trim(),
        "the applied tree is not the open change's tree"
    );
}

/// Paths narrow it by the rule `ff restore` already speaks: a file, or a
/// directory prefix.
#[test]
fn paths_narrow_the_patch() {
    let fx = repo();
    fx.write("root.txt", "a\n");
    fx.write("src/one.txt", "a\n");
    fx.commit("one");
    fx.write("root.txt", "b\n");
    fx.write("src/one.txt", "b\n");

    let one = stdout(&ff(&fx, &["diff", "src/one.txt"]));
    assert!(one.contains("src/one.txt"), "{one}");
    assert!(!one.contains("root.txt"), "only the named file: {one}");

    let dir = stdout(&ff(&fx, &["diff", "src"]));
    assert!(dir.contains("src/one.txt"), "{dir}");
    assert!(!dir.contains("root.txt"), "only that directory: {dir}");
}

/// A binary file has no lines to show, and says so in git's words rather
/// than printing nothing and leaving the reader to guess.
#[test]
fn a_binary_file_says_so() {
    let fx = repo();
    fx.write("kept.txt", "x\n");
    fx.commit("one");
    std::fs::write(fx.path().join("blob.bin"), [0u8, 1, 2, 0, 3, 4]).expect("write binary");

    let body = stdout(&ff(&fx, &["diff"]));
    assert!(
        body.contains("Binary files /dev/null and b/blob.bin differ"),
        "git's own wording, with its null side: {body}"
    );
    assert!(!body.contains("@@"), "and no hunks: {body}");
}

/// A rename is one file that moved, not a delete and an add, and the header
/// says so with the two lines git uses.
#[test]
fn a_rename_carries_both_paths() {
    let fx = repo();
    let body: String = (1..=20).map(|n| format!("line {n}\n")).collect();
    fx.write("old.txt", &body);
    fx.commit("one");
    fx.remove("old.txt");
    fx.write("new.txt", &body);

    let patch = stdout(&ff(&fx, &["diff"]));
    assert!(
        patch.contains("rename from old.txt"),
        "missing rename from: {patch}"
    );
    assert!(
        patch.contains("rename to new.txt"),
        "missing rename to: {patch}"
    );
}

/// A clean tree prints nothing at all. This output is meant to be piped
/// into `git apply`, and prose on that stream is a bug for whatever reads
/// it.
#[test]
fn a_clean_tree_prints_nothing() {
    let fx = repo();
    fx.write("a.txt", "a\n");
    fx.commit("one");

    let out = ff(&fx, &["diff"]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    assert_eq!(stdout(&out), "", "silence, git's convention");
}

/// The machine surface carries the same content as fields: kind and text
/// per line, so a consumer never parses the rendered patch back.
#[test]
fn the_json_envelope_carries_hunks() {
    let fx = repo();
    fx.write("a.txt", "1\n2\n3\n");
    fx.commit("one");
    fx.write("a.txt", "1\ntwo\n3\n");

    let out = ff(&fx, &["diff", "--json"]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    let v = json(&out);
    assert_eq!(v["cmd"].as_str(), Some("diff"));
    let file = &v["data"]["changes"][0];
    assert_eq!(file["path"].as_str(), Some("a.txt"));
    let hunk = &file["hunks"][0];
    assert_eq!(hunk["header"].as_str(), Some("@@ -1,3 +1,3 @@"));
    let kinds: Vec<&str> = hunk["lines"]
        .as_array()
        .expect("lines")
        .iter()
        .map(|l| l["kind"].as_str().expect("kind"))
        .collect();
    assert_eq!(kinds, vec!["context", "delete", "insert", "context"]);
    assert_eq!(hunk["lines"][2]["text"].as_str(), Some("two"));
}

/// The stat surfaces did not grow a patch just because one exists: nobody
/// asked, so their payloads carry no `hunks` key at all.
#[test]
fn the_stat_surfaces_stay_stat_shaped() {
    let fx = repo();
    fx.write("a.txt", "1\n");
    fx.commit("one");
    fx.write("a.txt", "2\n");

    let v = json(&ff(&fx, &["status", "--json"]));
    let file = &v["data"]["changes"][0];
    assert!(file["insertions"].is_number(), "still a diffstat: {file}");
    assert!(
        file.get("hunks").is_none(),
        "status invented content nobody asked for: {file}"
    );
}

/// `ff diff` is `ff status -p`, and the retired refusal is gone: typing the
/// word now runs the verb rather than explaining that there is not one.
#[test]
fn the_foreign_refusal_retired() {
    let fx = repo();
    fx.write("a.txt", "a\n");
    fx.commit("one");
    fx.write("a.txt", "b\n");

    let out = ff(&fx, &["--json", "diff"]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    let v = json(&out);
    assert!(
        v["error"].is_null(),
        "no refusal left where the verb now is: {v}"
    );

    // And the page names it where the counts live.
    let page = stdout(&ff_at(&fx.path(), &["help", "status"]));
    assert!(page.contains("ff diff"), "status page names it: {page}");
}
