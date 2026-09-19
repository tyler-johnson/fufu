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

/// `-p` on the three views that already compute a `ChangeStat`. The flag
/// only changes the depth of the same read, so the block each view already
/// printed stays put and the patch goes underneath it — furniture above,
/// format below.
#[test]
fn the_patch_flag_deepens_the_three_views_that_have_a_file_list() {
    let fx = repo();
    fx.write("a.txt", "1\n2\n3\n");
    fx.commit("one");
    fx.write("a.txt", "1\ntwo\n3\n");
    // A capture, so there are two operations to compare.
    ff(&fx, &["status"]);

    for args in [
        &["op", "show", "-p", "@"][..],
        &["op", "diff", "-p", "@^", "@"][..],
        &["evolog", "-p"][..],
    ] {
        let out = ff(&fx, args);
        assert!(out.status.success(), "ff {args:?}: {}", stderr(&out));
        let body = stdout(&out);
        assert!(
            body.contains("@@") && body.contains("+two"),
            "ff {args:?} printed no patch: {body}"
        );
    }

    // Without the flag, the same views stay stat-level.
    for args in [
        &["op", "show", "@"][..],
        &["op", "diff", "@^", "@"][..],
        &["evolog"][..],
    ] {
        let body = stdout(&ff(&fx, args));
        assert!(!body.contains("@@"), "ff {args:?} leaked content: {body}");
    }
}

/// And on the machine surface: the flag is a depth, not a rendering, so
/// `--json` carries the hunks too.
#[test]
fn the_patch_flag_reaches_the_machine_surface() {
    let fx = repo();
    fx.write("a.txt", "1\n2\n3\n");
    fx.commit("one");
    fx.write("a.txt", "1\ntwo\n3\n");
    ff(&fx, &["status"]);

    let v = json(&ff(&fx, &["op", "show", "-p", "@", "--json"]));
    assert_eq!(
        v["data"]["changes"][0]["hunks"][0]["header"].as_str(),
        Some("@@ -1,3 +1,3 @@")
    );

    let v = json(&ff(&fx, &["op", "diff", "-p", "@^", "@", "--json"]));
    assert!(v["data"]["changes"][0]["hunks"].is_array(), "{v}");

    // evolog assembles its patches per row, so the hunks ride on the row.
    let v = json(&ff(&fx, &["evolog", "-p", "--json"]));
    let row = &v["data"]["snapshots"][0];
    assert!(row["changes"][0]["hunks"].is_array(), "{row}");
    assert!(row["insertions"].is_number(), "{row}");

    // Bare, the row carries no patch at all — the payload is what it was.
    let v = json(&ff(&fx, &["evolog", "--json"]));
    assert!(
        v["data"]["snapshots"][0].get("changes").is_none(),
        "evolog invented a diffstat nobody asked for: {v}"
    );
}

/// The positional is paths only. A revision typed there used to select
/// nothing and print an empty patch, which reads as "no changes"; now it is
/// refused, and the exits name the verbs that read revisions.
#[test]
fn a_revision_in_the_path_slot_is_refused() {
    let fx = repo();
    fx.write("a.txt", "a\n");
    fx.commit("one");
    fx.write("a.txt", "b\n");
    fx.commit("two");

    for args in [&["diff", "main..HEAD"][..], &["diff", "HEAD~2", "HEAD"][..]] {
        let out = ff(&fx, args);
        assert_eq!(out.status.code(), Some(2), "{args:?}: {}", stderr(&out));
        let err = stderr(&out);
        assert!(err.contains(args[1]), "the token is named: {err}");
        assert!(
            err.contains("ff diff -r"),
            "the revision flag is named: {err}"
        );
        assert!(
            stdout(&out).is_empty(),
            "nothing on stdout that `git apply` could mistake for a patch"
        );
    }

    let out = ff(&fx, &["--json", "diff", "main..HEAD"]);
    assert_eq!(out.status.code(), Some(2));
    let v = json(&out);
    assert_eq!(v["error"]["id"].as_str(), Some("usage/no-such-path"), "{v}");
}

/// A path that is on neither disk nor in HEAD is a typo, not a filter that
/// happens to match nothing.
#[test]
fn a_path_that_names_nothing_is_refused() {
    let fx = repo();
    fx.write("a.txt", "a\n");
    fx.commit("one");

    let out = ff(&fx, &["diff", "bogus.txt"]);
    assert_eq!(out.status.code(), Some(2), "{}", stderr(&out));
    assert!(stderr(&out).contains("bogus.txt"), "{}", stderr(&out));

    let out = ff(&fx, &["--json", "diff", "bogus.txt"]);
    assert_eq!(out.status.code(), Some(2));
    let v = json(&out);
    assert_eq!(v["error"]["id"].as_str(), Some("usage/no-such-path"), "{v}");
}

/// The refusal is only for a path that names nothing. A path that exists
/// and has no changes keeps git's convention: an empty patch, exit 0, so
/// `ff diff a.txt | git apply` on a clean file is a no-op rather than an
/// error.
#[test]
fn an_existing_path_with_no_changes_is_an_empty_patch() {
    let fx = repo();
    fx.write("a.txt", "a\n");
    fx.commit("one");

    let out = ff(&fx, &["diff", "a.txt"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(stdout(&out).is_empty(), "{}", stdout(&out));

    let v = json(&ff(&fx, &["--json", "diff", "a.txt"]));
    assert_eq!(v["data"]["changes"], serde_json::json!([]), "{v}");
}

/// `-r HEAD` is what one commit did: its first parent's tree to its own.
#[test]
fn revisions_show_one_commits_patch() {
    let fx = repo();
    fx.write("a.txt", "one\n");
    let c1 = fx.commit("one");
    fx.write("a.txt", "two\n");
    let c2 = fx.commit("two");
    fx.write("b.txt", "three\n");
    let c3 = fx.commit("three");

    let out = ff(&fx, &["diff", "-r", "HEAD"]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    let body = stdout(&out);
    assert!(body.contains("+three"), "the last commit's hunk: {body}");
    assert!(
        !body.contains("+two") && !body.contains("-one"),
        "and none of the earlier ones: {body}"
    );

    let v = json(&ff(&fx, &["diff", "-r", "HEAD", "--json"]));
    assert_eq!(v["data"]["from"].as_str(), Some(c2.as_str()), "{v}");
    assert_eq!(v["data"]["to"].as_str(), Some(c3.as_str()), "{v}");
    assert_ne!(c1, c2);
}

/// A two-commit range sums its members: from the root's parent to the head.
#[test]
fn revisions_show_a_two_commit_range() {
    let fx = repo();
    fx.write("a.txt", "one\n");
    let c1 = fx.commit("one");
    fx.write("a.txt", "two\n");
    fx.commit("two");
    fx.write("b.txt", "three\n");
    fx.commit("three");

    let body = stdout(&ff(&fx, &["diff", "-r", "HEAD~2..HEAD"]));
    assert!(body.contains("-one") && body.contains("+two"), "{body}");
    assert!(body.contains("+three"), "{body}");

    let v = json(&ff(&fx, &["diff", "-r", "HEAD~2..HEAD", "--json"]));
    assert_eq!(v["data"]["from"].as_str(), Some(c1.as_str()), "{v}");

    // The quoted spelling with a sha is the same set.
    let spelled = format!("{c1}..HEAD");
    let again = stdout(&ff(&fx, &["diff", "-r", &spelled]));
    assert_eq!(again, body);
}

/// The open change is a member like any other, sitting on HEAD: a range
/// that ends at `@` includes the open edit, and `-r @` is the default.
#[test]
fn revisions_include_the_open_change() {
    let fx = repo();
    fx.write("a.txt", "one\n");
    fx.commit("one");
    fx.write("a.txt", "two\n");
    fx.commit("two");
    fx.write("b.txt", "open\n");

    let body = stdout(&ff(&fx, &["diff", "-r", "HEAD~1..@"]));
    assert!(body.contains("+two"), "the commit: {body}");
    assert!(body.contains("+open"), "and the open edit: {body}");
    let v = json(&ff(&fx, &["diff", "-r", "HEAD~1..@", "--json"]));
    assert_eq!(v["data"]["to"].as_str(), Some("@"), "{v}");

    let bare = ff(&fx, &["diff"]);
    let at = ff(&fx, &["diff", "-r", "@"]);
    assert_eq!(
        at.stdout, bare.stdout,
        "`-r @` is the default, byte for byte"
    );
    assert!(!bare.stdout.is_empty());
}

/// A set with a hole in it is two pieces, each with a head, and not one
/// range.
#[test]
fn a_gapped_set_is_refused() {
    let fx = repo();
    fx.write("a.txt", "one\n");
    fx.commit("one");
    fx.write("a.txt", "two\n");
    fx.commit("two");
    fx.write("a.txt", "three\n");
    fx.commit("three");

    let out = ff(&fx, &["diff", "-r", "HEAD | HEAD~2"]);
    assert_eq!(out.status.code(), Some(2), "{}", stderr(&out));
    let err = stderr(&out);
    assert!(err.contains("2 heads"), "{err}");
    assert!(err.contains("ff diff --from"), "the two-point exit: {err}");
    assert!(stdout(&out).is_empty());

    let v = json(&ff(&fx, &["--json", "diff", "-r", "HEAD | HEAD~2"]));
    assert_eq!(
        v["error"]["id"].as_str(),
        Some("usage/revset-not-a-range"),
        "{v}"
    );
}

/// Two branches from one base are two heads, and no single head to end at.
#[test]
fn a_two_headed_set_is_refused() {
    let fx = repo();
    fx.write("base.txt", "base\n");
    fx.commit("base");
    fx.git(&["switch", "-c", "side", "-q"]);
    fx.write("side.txt", "side\n");
    fx.commit("side work");
    fx.git(&["switch", "main", "-q"]);
    fx.write("main.txt", "main\n");
    fx.commit("main work");

    let out = ff(&fx, &["diff", "-r", "main | side"]);
    assert_eq!(out.status.code(), Some(2), "{}", stderr(&out));
    assert!(stderr(&out).contains("2 heads"), "{}", stderr(&out));

    let v = json(&ff(&fx, &["--json", "diff", "-r", "main | side"]));
    assert_eq!(
        v["error"]["id"].as_str(),
        Some("usage/revset-not-a-range"),
        "{v}"
    );
}

/// A merge at the root has two parents to measure from, and the exits
/// spell both so the reader picks.
#[test]
fn a_merge_rooted_set_is_refused() {
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
    let merge = fx.git(&["rev-parse", "HEAD"]).trim().to_string();

    let out = ff(&fx, &["diff", "-r", &merge]);
    assert_eq!(out.status.code(), Some(2), "{}", stderr(&out));
    let err = stderr(&out);
    assert!(err.contains("a merge"), "{err}");
    assert!(err.contains("^2"), "the second parent is offered: {err}");

    let v = json(&ff(&fx, &["--json", "diff", "-r", &merge]));
    assert_eq!(
        v["error"]["id"].as_str(),
        Some("usage/revset-not-a-range"),
        "{v}"
    );
}

/// Two points are `git diff a b`, and the differential oracle is git itself:
/// the same two commits, the same patch, `index` lines aside.
#[test]
fn two_points_diff_like_git() {
    let fx = repo();
    fx.write("a.txt", "1\n2\n3\n");
    fx.write("gone.txt", "bye\n");
    let c1 = fx.commit("one");
    fx.write("a.txt", "1\ntwo\n3\n");
    fx.remove("gone.txt");
    let c2 = fx.commit("two");
    fx.write("b.txt", "new\n");
    fx.write("a.txt", "1\ntwo\n3\n4\n");
    let c3 = fx.commit("three");

    let strip = |patch: &str| -> String {
        patch
            .lines()
            .filter(|l| !l.starts_with("index "))
            .map(|l| format!("{l}\n"))
            .collect()
    };
    let out = ff(&fx, &["diff", "--from", &c1, "--to", &c3]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    let ours = strip(&stdout(&out));
    let theirs = strip(&fx.git(&["diff", &c1, &c3]));
    assert_eq!(ours, theirs, "fufu and git disagree on the same two trees");
    assert!(ours.contains("+new"), "{ours}");

    // `--from` alone runs to the open change, edit included.
    fx.write("c.txt", "open\n");
    let body = stdout(&ff(&fx, &["diff", "--from", &c1]));
    assert!(body.contains("+open"), "the open edit: {body}");
    assert!(body.contains("+new"), "and the commits between: {body}");
    let v = json(&ff(&fx, &["diff", "--from", &c1, "--json"]));
    assert_eq!(v["data"]["from"].as_str(), Some(c1.as_str()), "{v}");
    assert_eq!(v["data"]["to"].as_str(), Some("@"), "{v}");

    // `--to` alone measures from `@^`, which is HEAD.
    let v = json(&ff(&fx, &["diff", "--to", &c2, "--json"]));
    assert_eq!(v["data"]["from"].as_str(), Some(c3.as_str()), "{v}");
    assert_eq!(v["data"]["to"].as_str(), Some(c2.as_str()), "{v}");
    let body = stdout(&ff(&fx, &["diff", "--to", &c2]));
    assert!(
        body.contains("-new"),
        "HEAD to c2 removes what three added: {body}"
    );
    assert!(
        !body.contains("open"),
        "and never sees the open edit: {body}"
    );
}

/// `-r` names a set and the endpoint flags name two points; the verb does
/// not rank one pair of ends over the other.
#[test]
fn endpoints_and_revisions_together_are_refused() {
    let fx = repo();
    fx.write("a.txt", "a\n");
    fx.commit("one");

    let out = ff(&fx, &["diff", "-r", "HEAD", "--from", "main"]);
    assert_eq!(out.status.code(), Some(2), "{}", stderr(&out));
    assert!(stderr(&out).contains("--from/--to"), "{}", stderr(&out));

    let v = json(&ff(&fx, &["--json", "diff", "-r", "HEAD", "--to", "main"]));
    assert_eq!(v["error"]["id"].as_str(), Some("usage/bad-flags"), "{v}");
}

/// Paths narrow a revision's patch the way they narrow the open change's.
#[test]
fn paths_narrow_a_revision_patch() {
    let fx = repo();
    fx.write("root.txt", "a\n");
    fx.write("src/one.txt", "a\n");
    fx.commit("one");
    fx.write("root.txt", "b\n");
    fx.write("src/one.txt", "b\n");
    fx.commit("two");

    let dir = stdout(&ff(&fx, &["diff", "-r", "HEAD", "src/"]));
    assert!(dir.contains("src/one.txt"), "{dir}");
    assert!(!dir.contains("root.txt"), "only that directory: {dir}");
}

/// `--stat` is the diffstat block in the patch's place: the same rows
/// `ff status` prints, and JSON keeps the counts and drops `hunks`.
#[test]
fn stat_prints_the_diffstat_in_place_of_the_patch() {
    let fx = repo();
    fx.write("a.txt", "1\n2\n3\n");
    fx.commit("one");
    fx.write("a.txt", "1\ntwo\n3\n");

    let out = ff(&fx, &["diff", "--stat"]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    let body = stdout(&out);
    assert!(body.contains("M a.txt"), "{body}");
    assert!(body.contains("1 file"), "the summary row: {body}");
    assert!(!body.contains("@@"), "no patch: {body}");

    let v = json(&ff(&fx, &["diff", "--stat", "--json"]));
    let data = &v["data"];
    assert!(data["changes"][0]["insertions"].is_number(), "{v}");
    assert!(data["changes"][0].get("hunks").is_none(), "{v}");
    assert!(data["insertions"].is_number(), "the totals stay: {v}");
}

/// `--name-only` is the kind letter and the path, one file per line, and
/// JSON keeps exactly the four keys that name a file.
#[test]
fn name_only_lists_paths_with_kind_letters() {
    let fx = repo();
    fx.write("a.txt", "1\n");
    fx.write("gone.txt", "bye\n");
    fx.commit("one");
    fx.write("a.txt", "2\n");
    fx.write("new.txt", "new\n");
    fx.remove("gone.txt");

    let out = ff(&fx, &["diff", "--name-only"]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    let body = stdout(&out);
    assert_eq!(body, "│  M a.txt\n│  D gone.txt\n│  A new.txt\n");
    assert!(!body.contains("+1"), "no counts: {body}");

    let v = json(&ff(&fx, &["diff", "--name-only", "--json"]));
    let data = &v["data"];
    for file in data["changes"].as_array().expect("changes") {
        let mut keys: Vec<&str> = file
            .as_object()
            .expect("file")
            .keys()
            .map(String::as_str)
            .collect();
        keys.sort_unstable();
        assert_eq!(keys, ["binary", "from", "kind", "path"], "{v}");
    }
    assert!(data.get("insertions").is_none(), "no totals: {v}");
    assert!(data.get("deletions").is_none(), "no totals: {v}");
}

/// A clean tree is an empty patch under every depth: the diffstat's summary
/// row would be prose in a stream meant for `git apply`.
#[test]
fn a_clean_tree_prints_nothing_under_every_view() {
    let fx = repo();
    fx.write("a.txt", "a\n");
    fx.commit("one");
    for flag in ["--stat", "--name-only"] {
        let out = ff(&fx, &["diff", flag]);
        assert!(out.status.success(), "stderr: {}", stderr(&out));
        assert_eq!(stdout(&out), "", "{flag} on a clean tree");
    }
}

/// The two views each pick one form of the files, so together they are
/// refused with the coded id rather than clap's bare conflict.
#[test]
fn stat_and_name_only_do_not_combine() {
    let fx = repo();
    fx.write("a.txt", "a\n");
    fx.commit("one");
    let out = ff(&fx, &["diff", "--stat", "--name-only"]);
    assert_eq!(out.status.code(), Some(2), "{}", stderr(&out));
    let v = json(&ff(&fx, &["--json", "diff", "--stat", "--name-only"]));
    assert_eq!(v["error"]["id"].as_str(), Some("usage/bad-flags"), "{v}");
}

/// `-U` is git's dial, and git is the oracle: the same two commits at the
/// same width, byte for byte past the `index` lines.
#[test]
fn context_lines_follow_git() {
    let fx = repo();
    fx.write("a.txt", "1\n2\n3\n");
    fx.write("gone.txt", "bye\n");
    let c1 = fx.commit("one");
    fx.write("a.txt", "1\ntwo\n3\n");
    fx.remove("gone.txt");
    fx.commit("two");
    fx.write("b.txt", "new\n");
    fx.write("a.txt", "1\ntwo\n3\n4\n");
    let c3 = fx.commit("three");

    let strip = |patch: &str| -> String {
        patch
            .lines()
            .filter(|l| !l.starts_with("index "))
            .map(|l| format!("{l}\n"))
            .collect()
    };
    for n in ["0", "1", "10"] {
        let out = ff(&fx, &["diff", "-U", n, "--from", &c1, "--to", &c3]);
        assert!(out.status.success(), "stderr: {}", stderr(&out));
        let ours = strip(&stdout(&out));
        let theirs = strip(&fx.git(&["diff", &format!("-U{n}"), &c1, &c3]));
        assert_eq!(ours, theirs, "fufu and git disagree at -U{n}");
    }

    // Without a patch the dial turns nothing, and is not refused.
    let out = ff(&fx, &["diff", "--stat", "-U", "5"]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));
}
