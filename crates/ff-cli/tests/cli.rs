//! CLI conventions: the JSON envelope, exit codes, error prefixes, the
//! global flags, and the verbs too small for a file of their own. Runs the
//! real `ff` binary against hermetic fixtures. `ff log`, `ff status`,
//! `ff config`, and `ff explain` have their own files beside this one.

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

#[test]
fn bare_repo_status_errors_log_works() {
    let fx = Fixture::new_bare();
    let status = ff(&fx, &["status"]);
    assert_eq!(status.status.code(), Some(1));
    let stderr = String::from_utf8(status.stderr).unwrap();
    assert!(stderr.starts_with("ff: "), "error convention: {stderr:?}");
    let log = ff(&fx, &["log", "--json"]);
    assert!(log.status.success(), "bare log works");
}

#[test]
fn errors_carry_exactly_one_ff_prefix() {
    // The CLI owns the `ff: ` prefix (main.rs); engine messages must not
    // embed their own, or errors print as `ff: ff: ...`.
    let fx = Fixture::new();
    fx.set_config("user.name", "Test User");
    fx.set_config("user.email", "test@user.test");
    fx.write("a.txt", "a\n");
    fx.commit("init");

    let out = ff(&fx, &["switch", "nosuch"]);
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(stderr.starts_with("ff: "), "error convention: {stderr:?}");
    assert!(!stderr.contains("ff: ff: "), "doubled prefix: {stderr:?}");
}

#[test]
fn outside_repo_is_runtime_error() {
    let dir = tempfile::TempDir::new().unwrap();
    let out = ff_at(dir.path(), &["status"]);
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(stderr.starts_with("ff: "));
}

#[test]
fn usage_errors_exit_2() {
    let fx = Fixture::new();
    let unknown_flag = ff(&fx, &["status", "--nope"]);
    assert_eq!(unknown_flag.status.code(), Some(2));
    // `-m` is retired outright: the old snapshot message has no home on any
    // command line, so it exits 2 whether or not a verb follows it.
    let mixed = ff(&fx, &["-m", "msg", "status"]);
    assert_eq!(mixed.status.code(), Some(2));
    let retired = ff(&fx, &["-m", "msg"]);
    assert_eq!(retired.status.code(), Some(2));
    let bad_count = ff(&fx, &["log", "-n", "many"]);
    assert_eq!(bad_count.status.code(), Some(2));
}

#[test]
fn bare_ff_draws_the_map() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("f");
    fx.git(&["branch", "feature"]);
    fx.write("a.txt", "m1\n");
    fx.commit("main one");
    fx.write("a.txt", "m2\n");
    fx.commit("main two");
    fx.git(&["switch", "feature"]);
    fx.write("a.txt", "ft\n");
    fx.commit("feature one");
    fx.git(&["switch", "main"]);
    fx.write("a.txt", "dirty\n");

    let out = ff(&fx, &[]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let text = stdout(&out);
    let lines: Vec<&str> = text.lines().collect();
    assert!(
        lines.first().is_some_and(|line| line.starts_with('@')),
        "the open change leads the map: {text:?}"
    );
    assert!(
        lines.iter().any(|line| line.contains("feature")),
        "the other branch's name is on the map: {text:?}"
    );
    // The old confirmation line went away with the verb.
    assert!(
        !lines.iter().any(|line| line.contains("snapshot ")),
        "no snapshot confirmation: {text:?}"
    );
    // Capture did not go anywhere: bare ff still writes the chain first.
    let chain = fx.git(&["rev-parse", "--verify", "refs/fufu/snap/main"]);
    assert!(!chain.trim().is_empty());

    // The map is a view: a second run over the same tree draws the same
    // skeleton, and there is no "no changes since the last snapshot" line.
    let again = ff(&fx, &[]);
    assert!(
        again.status.success(),
        "{}",
        String::from_utf8_lossy(&again.stderr)
    );
    assert!(
        stdout(&again)
            .lines()
            .next()
            .is_some_and(|line| line.starts_with('@')),
        "still the map on the second run"
    );
}

#[test]
fn bare_ff_json_is_the_map() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("f");
    fx.git(&["branch", "feature"]);
    fx.write("a.txt", "m1\n");
    fx.commit("main one");
    fx.write("a.txt", "m2\n");
    fx.commit("main two");
    fx.git(&["switch", "feature"]);
    fx.write("a.txt", "ft\n");
    fx.commit("feature one");
    fx.git(&["switch", "main"]);
    fx.write("a.txt", "dirty\n");

    let out = ff(&fx, &["--json"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let v: serde_json::Value = serde_json::from_str(&stdout(&out)).expect("valid json");
    assert_eq!(v["ff"], 1);
    assert_eq!(v["cmd"], "map");
    let rows = v["data"]["rows"].as_array().expect("rows array");
    assert!(!rows.is_empty(), "the map has rows");
    assert!(v["data"]["truncated"].is_boolean());
    assert_eq!(rows[0]["node"]["kind"], "open");
    assert!(
        rows.iter().any(|row| row["node"]["kind"] == "commit"),
        "a commit row is on the map"
    );
}

#[test]
fn the_snapshot_verb_is_retired() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");

    let out = ff(&fx, &["-m", "before the refactor"]);
    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("-m is gone"),
        "stderr names the removal: {stderr:?}"
    );

    // The machine surface carries the same coded refusal.
    let out = ff(&fx, &["-m", "x", "--json"]);
    let v: serde_json::Value = serde_json::from_str(&stdout(&out)).expect("valid json");
    assert_eq!(v["error"]["id"], "usage/bad-flags");
}

#[test]
fn evolog_lists_snapshots_and_json() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");

    // Empty chain: a friendly line, exit 0.
    let out = ff(&fx, &["evolog"]);
    assert!(out.status.success());
    assert_eq!(stdout(&out), "no snapshots on main yet\n");

    fx.write("a.txt", "one\n");
    assert!(ff(&fx, &[]).status.success());
    fx.write("a.txt", "two\n");
    assert!(ff(&fx, &[]).status.success());

    let out = ff(&fx, &["evolog"]);
    assert!(out.status.success());
    let text = stdout(&out);
    let lines: Vec<&str> = text.lines().collect();
    // The tree did not change since the second capture, so this run is a
    // NoOp and the row count stays at the two captures above.
    assert_eq!(lines.len(), 2, "snapshot rows only: {text:?}");

    let out = ff(&fx, &["evolog", "--json"]);
    let v: serde_json::Value = serde_json::from_str(&stdout(&out)).unwrap();
    let snaps = v["data"]["snapshots"].as_array().unwrap();
    assert_eq!(snaps.len(), 2);
    // Newest first, and the human rows lead with the first twelve hex of
    // those same ids in the same order — the message is gone, so the ids
    // are what ties the two surfaces together.
    assert!(
        snaps[0]["time"].as_i64().unwrap() >= snaps[1]["time"].as_i64().unwrap(),
        "snapshots are newest first: {v:?}"
    );
    assert_eq!(
        lines[0].split_whitespace().next().unwrap(),
        &snaps[0]["id"].as_str().unwrap()[..12],
        "row 0 carries the newest id: {text:?}"
    );
    assert_eq!(
        lines[1].split_whitespace().next().unwrap(),
        &snaps[1]["id"].as_str().unwrap()[..12],
        "row 1 carries the older id: {text:?}"
    );

    for line in &lines {
        let token = line.split_whitespace().next().unwrap();
        assert_eq!(token.len(), 12, "hex12 id column: {line:?}");
        assert!(
            token
                .chars()
                .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()),
            "lowercase hex: {token:?}"
        );
    }

    assert!(
        snaps[0]["id"]
            .as_str()
            .unwrap()
            .chars()
            .all(|c| c.is_ascii_hexdigit()),
        "JSON ids stay hex"
    );
    // The chain walk leaves short_id empty and the display paths fill it;
    // this is the guard against an unfilled row reaching a reader.
    for snap in snaps {
        let (id, short) = (
            snap["id"].as_str().unwrap(),
            snap["short_id"].as_str().unwrap(),
        );
        assert!(!short.is_empty(), "short_id is filled: {snap}");
        assert!(id.starts_with(short), "short_id abbreviates id: {snap}");
    }
}

/// A hex id copied from evolog output round-trips into `ff restore --at-op`.
#[test]
fn restore_accepts_hex_id_from_evolog() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    fx.write("a.txt", "captured\n");
    assert!(ff(&fx, &[]).status.success());
    fx.write("a.txt", "diverged\n");

    let out = ff(&fx, &["evolog"]);
    let text = stdout(&out);
    // Capture-first: evolog captured the tree as it stood — "diverged" —
    // before it printed, so the row we want (the one holding "captured\n")
    // is the *older* capture, row 1, not the newest row 0.
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 2, "exactly two captures: {text:?}");
    let id = lines[1]
        .split_whitespace()
        .next()
        .expect("the hex id leads the row")
        .to_string();

    let out = ff(&fx, &["restore", "--all", "--at-op", &id]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        std::fs::read_to_string(fx.path().join("a.txt")).unwrap(),
        "captured\n"
    );
}

#[test]
fn restore_requires_paths_or_all() {
    let fx = Fixture::new();
    let out = ff(&fx, &["restore"]);
    assert_eq!(
        out.status.code(),
        Some(2),
        "paths XOR --all is a usage error"
    );
    let out = ff(&fx, &["restore", "--all", "some/path"]);
    assert_eq!(out.status.code(), Some(2));
}

#[test]
fn restore_round_trip_with_undo_hint() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    fx.write("a.txt", "captured\n");
    assert!(ff(&fx, &[]).status.success());
    fx.write("a.txt", "diverged\n");

    // The capture holding "captured" is an operation, so it is named in the
    // operation address space, and `@` names it: restore resolves its source
    // BEFORE taking its own pre-restore capture, so `@` still means the
    // timeline as the user just saw it. Bare `restore --all` now means the
    // commit under the open change — a different answer, checked below.
    let out = ff(&fx, &["restore", "--all", "--at-op", "@"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let text = stdout(&out);
    assert!(text.starts_with("restored from "), "header: {text:?}");
    assert!(text.contains("restored  a.txt"), "file list: {text:?}");
    assert!(
        text.trim_end().ends_with("undo: ff undo"),
        "undo hint: {text:?}"
    );
    assert_eq!(
        std::fs::read_to_string(fx.path().join("a.txt")).unwrap(),
        "captured\n"
    );

    // Bare restore --all takes the whole tree back to the commit under the
    // open change: "captured" was never committed, so it goes.
    let out = ff(&fx, &["restore", "--all"]);
    assert!(out.status.success());
    assert_eq!(
        std::fs::read_to_string(fx.path().join("a.txt")).unwrap(),
        "a\n"
    );
}

#[test]
fn restore_json_shape() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    fx.write("a.txt", "captured\n");
    assert!(ff(&fx, &[]).status.success());
    fx.write("a.txt", "diverged\n");

    let out = ff(&fx, &["restore", "--all", "--json"]);
    let v: serde_json::Value = serde_json::from_str(&stdout(&out)).unwrap();
    let d = &v["data"];
    // The origin says which address space named the source, so a reader
    // never has to infer it from the shape of an id.
    assert_eq!(d["origin"]["space"], "commit");
    assert!(d["origin"]["id"].is_string());
    assert_eq!(d["restored"][0], "a.txt");
    assert_eq!(d["undo"], "ff undo");
    assert!(d["pre_op"].is_string(), "pre-restore capture recorded");
}

#[test]
fn trim_reports_and_dry_runs() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    fx.write("a.txt", "fresh\n");
    assert!(ff(&fx, &[]).status.success());

    let out = ff(&fx, &["op", "trim"]);
    assert!(out.status.success());
    let text = stdout(&out);
    // One log, so one line: retention acts on the log, and a branch pointer
    // is a place in it rather than a chain of its own.
    assert!(
        text.contains("nothing to drop") && text.contains("operations kept"),
        "the log reports its own kept count: {text:?}"
    );
    assert!(
        !text.contains("main:"),
        "no per-branch retention row: {text:?}"
    );

    let out = ff(&fx, &["op", "trim", "--dry-run", "--json"]);
    let v: serde_json::Value = serde_json::from_str(&stdout(&out)).unwrap();
    let d = &v["data"];
    assert_eq!(d["dry_run"], true);
    assert_eq!(d["pointers"][0]["branch"], "main");
    assert_eq!(d["pointers"][0]["dropped"], 0);
}

#[test]
fn merge_conflict_renders_sections() {
    let fx = Fixture::new();
    fx.write("conflict.txt", "base\n");
    fx.commit("base");
    fx.git(&["checkout", "-q", "-b", "other"]);
    fx.write("conflict.txt", "theirs\n");
    fx.commit("theirs");
    fx.git(&["checkout", "-q", "main"]);
    fx.write("conflict.txt", "ours\n");
    fx.commit("ours");
    let merge = fx.try_git(&["merge", "other"]);
    assert!(!merge.status.success());

    let out = ff(&fx, &["status"]);
    let text = stdout(&out);
    assert!(text.starts_with("on main · merging\n"), "header: {text:?}");
    assert!(
        text.contains("conflicts:\n  conflict.txt\n"),
        "body: {text:?}"
    );
}

/// `ff commit` on a clean tree with a pending description refuses: the
/// description does not make a change, nothing is written, and the
/// description is still there afterwards.
#[test]
fn commit_refuses_clean_tree_keeps_the_pending_description() {
    let fx = Fixture::new();
    fx.set_config("user.name", "Test User");
    fx.set_config("user.email", "test@user.test");
    fx.write("a.txt", "a\n");
    fx.commit("init");

    // Describe while clean.
    let out = ff(&fx, &["describe", "-m", "planned work"]);
    assert!(out.status.success());

    let before: u32 = fx
        .git(&["rev-list", "--count", "HEAD"])
        .trim()
        .parse()
        .unwrap();

    let out = ff(&fx, &["commit"]);
    assert_eq!(out.status.code(), Some(1), "a clean tree refuses");
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        err.contains("nothing to close on main"),
        "stderr names the refusal: {err}"
    );
    assert!(
        err.contains("the pending description stays put"),
        "stderr reassures the description survives: {err}"
    );

    let after: u32 = fx
        .git(&["rev-list", "--count", "HEAD"])
        .trim()
        .parse()
        .unwrap();
    assert_eq!(after, before, "rev-list count unchanged");

    // The description is still pending.
    let out = ff(&fx, &["log", "--json"]);
    let v: serde_json::Value = serde_json::from_str(&stdout(&out)).unwrap();
    assert_eq!(
        v["data"]["open"]["subject"].as_str(),
        Some("planned work"),
        "subject still pending"
    );
}

/// `ff commit -m` on a clean tree with no pending description refuses: the
/// flag message is discarded with the refusal, and nothing is written.
#[test]
fn commit_refuses_clean_tree_with_message_flag() {
    let fx = Fixture::new();
    fx.set_config("user.name", "Test User");
    fx.set_config("user.email", "test@user.test");
    fx.write("a.txt", "a\n");
    fx.commit("init");

    let before: u32 = fx
        .git(&["rev-list", "--count", "HEAD"])
        .trim()
        .parse()
        .unwrap();

    let out = ff(&fx, &["commit", "-m", "checkpoint"]);
    assert_eq!(out.status.code(), Some(1), "a clean tree refuses");
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        err.contains("nothing to close on main"),
        "stderr names the refusal: {err}"
    );
    assert!(
        !err.contains("the pending description stays put"),
        "no pending description, so no reassurance clause: {err}"
    );

    let after: u32 = fx
        .git(&["rev-list", "--count", "HEAD"])
        .trim()
        .parse()
        .unwrap();
    assert_eq!(after, before, "rev-list count unchanged");
}

/// `ff commit` on a clean tree with no description refuses: exit 1,
/// stderr naming the refusal, rev-list count unchanged.
#[test]
fn commit_totally_empty_refuses() {
    let fx = Fixture::new();
    fx.set_config("user.name", "Test User");
    fx.set_config("user.email", "test@user.test");
    fx.write("a.txt", "a\n");
    fx.commit("init");

    let before: u32 = fx
        .git(&["rev-list", "--count", "HEAD"])
        .trim()
        .parse()
        .unwrap();

    let out = ff(&fx, &["commit"]);
    assert_eq!(out.status.code(), Some(1), "exit 1 — the refusal");
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        err.contains("nothing to close on main"),
        "refusal message: {err}"
    );

    let after: u32 = fx
        .git(&["rev-list", "--count", "HEAD"])
        .trim()
        .parse()
        .unwrap();
    assert_eq!(after, before, "rev-list count unchanged");
}

/// `ff start` never creates a commit: a described change stays pending,
/// the description does not become a commit, and the new branch opens clean.
#[test]
fn start_never_commits() {
    let fx = Fixture::new();
    fx.set_config("user.name", "Test User");
    fx.set_config("user.email", "test@user.test");
    fx.write("a.txt", "a\n");
    fx.commit("init");

    // Describe something.
    let out = ff(&fx, &["describe", "-m", "planned work"]);
    assert!(out.status.success());

    let before: u32 = fx
        .git(&["rev-list", "--count", "HEAD"])
        .trim()
        .parse()
        .unwrap();

    // start does not commit.
    let out = ff(&fx, &["start"]);
    assert!(out.status.success());

    let after: u32 = fx
        .git(&["rev-list", "--count", "HEAD"])
        .trim()
        .parse()
        .unwrap();
    assert_eq!(after, before, "rev-list count unchanged");

    // The description did not become a commit — HEAD is still "init".
    assert_eq!(
        fx.git(&["log", "-1", "--format=%s"]).trim(),
        "init",
        "no commit was created for the description"
    );
}

/// Two consecutive bare `ff start` runs on a clean tree produce two distinct
/// new branches; neither is a no-op.
#[test]
fn start_always_mints() {
    let fx = Fixture::new();
    fx.set_config("user.name", "Test User");
    fx.set_config("user.email", "test@user.test");
    fx.write("a.txt", "a\n");
    fx.commit("init");

    let out1 = ff(&fx, &["start", "--json"]);
    assert!(out1.status.success());
    let v1: serde_json::Value = serde_json::from_str(&stdout(&out1)).unwrap();
    assert_eq!(v1["cmd"], "switch", "start is a spelling of switch: {v1}");
    let branch1 = v1["data"]["switch"]["to"].as_str().unwrap();
    assert!(v1["data"]["switch"]["minted"].is_object(), "{v1}");

    let out2 = ff(&fx, &["start", "--json"]);
    assert!(out2.status.success());
    let v2: serde_json::Value = serde_json::from_str(&stdout(&out2)).unwrap();
    let branch2 = v2["data"]["switch"]["to"].as_str().unwrap();

    assert_ne!(branch1, branch2, "two starts produce two distinct branches");
}

/// `ff update` with the user roots pinned under a scratch home, so the
/// update-check cache it reads and writes is the scratch home's rather
/// than this machine's.
fn ff_update(fx: &Fixture, args: &[&str]) -> Output {
    let home = tempfile::TempDir::new().expect("scratch home");
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_ff"));
    cmd.current_dir(fx.path())
        .args(args)
        .env("GIT_CONFIG_GLOBAL", null_device())
        .env("GIT_CONFIG_SYSTEM", null_device())
        .env("GIT_CONFIG_NOSYSTEM", "1");
    ff_testsupport::userdirs::pin(&mut cmd, home.path())
        .output()
        .expect("spawn ff")
}

/// `ff update` on an unofficial (test) build names cargo and stops before any
/// network: classification precedes the API call, so this is hermetic.
/// Nothing failed — the command that owns this binary was reported — so 0.
#[test]
fn update_on_unofficial_build_advises_cargo() {
    let fx = Fixture::new();
    let out = ff_update(&fx, &["update"]);
    assert_eq!(out.status.code(), Some(0));
    let text = stdout(&out);
    assert!(text.contains("ff was built from source"), "got: {text}");
    assert!(
        text.contains("cargo install --git https://github.com/tyler-johnson/fufu ff-cli"),
        "got: {text}"
    );
}

/// `-y` asked for an update this channel cannot perform, and saying so on
/// stdout with a 0 would let a script believe it updated.
#[test]
fn update_yes_on_unofficial_build_fails() {
    let fx = Fixture::new();
    let out = ff_update(&fx, &["update", "-y"]);
    assert_eq!(out.status.code(), Some(1));
    let err = String::from_utf8(out.stderr.clone()).expect("utf-8 stderr");
    assert!(err.contains("ff: ff was built from source"), "got: {err}");
    assert!(
        err.contains("cargo install --git https://github.com/tyler-johnson/fufu ff-cli"),
        "got: {err}"
    );
}

#[test]
fn version_names_the_build() {
    let out = ff_at(&std::env::temp_dir(), &["--version"]);

    assert!(
        out.status.success(),
        "exit status: {}; stderr: {}",
        out.status,
        String::from_utf8_lossy(&out.stderr)
    );

    let out_str = stdout(&out).trim_end().to_string();
    let mut lines = out_str.lines();

    // Line one names the tool by its full name, not by the two letters it is
    // typed as: `ff` is not a searchable string, and this is the output a bug
    // report gets pasted from.
    let first = lines.next().unwrap_or_default();
    let prefix = format!("fufu {}", env!("CARGO_PKG_VERSION"));
    assert!(
        first.starts_with(&prefix),
        "stdout did not start with \"{prefix}\": {out_str}"
    );

    // Line two is where to go next, and it comes from the manifest rather
    // than from a literal in the source.
    assert_eq!(
        lines.next(),
        Some(env!("CARGO_PKG_REPOSITORY")),
        "second line is the project's home: {out_str}"
    );
    assert_eq!(lines.next(), None, "two lines and no more: {out_str}");

    let rest = &first[prefix.len()..];

    if !rest.is_empty() {
        assert!(
            rest.starts_with(" (") && rest.ends_with(')'),
            "build info should be parenthesised: {rest:?}"
        );

        let inner = &rest[2..rest.len() - 1];
        let parts: Vec<&str> = inner.splitn(2, ' ').collect();
        assert_eq!(
            parts.len(),
            2,
            "build info inner should have exactly two space-separated parts: {inner:?}"
        );

        let sha = parts[0];
        assert!(
            sha.len() >= 7
                && sha
                    .chars()
                    .all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c)),
            "sha part is not 7+ lowercase hex: {sha:?}"
        );

        let date = parts[1];
        assert!(
            date.len() == 10
                && date.as_bytes()[4] == b'-'
                && date.as_bytes()[7] == b'-'
                && date
                    .chars()
                    .enumerate()
                    .all(|(i, c)| (i == 4 || i == 7) || c.is_ascii_digit()),
            "date part is not YYYY-MM-DD: {date:?}"
        );
    }

    assert!(
        out.stderr.is_empty(),
        "stderr was not empty: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// The three spellings of one question. `-v` is the verb itself, so it
/// cannot drift from `ff version`, and `-V` — what almost every other tool
/// spells this — must be answered rather than met with clap's
/// unknown-argument error.
#[test]
fn the_version_is_asked_three_ways_and_answered_once() {
    let tmp = std::env::temp_dir();

    let long = ff_at(&tmp, &["--version"]);
    let short = ff_at(&tmp, &["-v"]);
    let verb = ff_at(&tmp, &["version"]);
    for out in [&long, &short, &verb] {
        assert!(out.status.success(), "exit 0: {:?}", out.status);
    }

    assert_eq!(stdout(&short), stdout(&long), "-v is --version");
    // The flag is the verb, so the spellings match line for line.
    let line = stdout(&long);
    assert!(
        stdout(&verb).starts_with(line.trim_end()),
        "ff version does not lead with the flag's line: {:?} vs {line:?}",
        stdout(&verb)
    );

    // `-V` is gone as a spelling and present as an answer.
    let shouted = ff_at(&tmp, &["-V"]);
    assert!(!shouted.status.success(), "-V no longer prints a version");
    let err = String::from_utf8_lossy(&shouted.stderr).to_string();
    assert!(err.contains("ff -v"), "names the spelling: {err}");
    assert!(err.contains("ff version"), "names the verb: {err}");
}

/// The envelope names the verb that ran. `ff -v --json` settles as the
/// version verb and not as the map, so the flag cannot answer a different
/// question from the verb it spells.
#[test]
fn the_version_flag_takes_the_envelope() {
    let out = ff_at(&std::env::temp_dir(), &["-v", "--json"]);
    assert!(out.status.success(), "exit 0: {:?}", out.status);
    let v: serde_json::Value = serde_json::from_str(&stdout(&out)).expect("valid json");
    assert_eq!(v["ff"], 1);
    assert_eq!(
        v["cmd"], "version",
        "the flag settled as the verb, not the map"
    );
    assert_eq!(v["data"]["version"], env!("CARGO_PKG_VERSION"));
}

/// The flag does not ride another verb: `-v status` is two commands on one
/// line, refused with the two spellings that would each be right alone.
#[test]
fn the_version_flag_does_not_ride_another_verb() {
    let out = ff_at(&std::env::temp_dir(), &["-v", "status"]);
    assert_eq!(out.status.code(), Some(2), "usage error: {:?}", out.status);
    let err = String::from_utf8_lossy(&out.stderr).to_string();
    assert!(err.contains("ff -v"), "names the flag: {err}");
    assert!(err.contains("ff version"), "names the verb: {err}");
}

/// The envelope carries the line as fields, so a caller never takes the
/// display string apart.
#[test]
fn version_json_splits_the_line_into_fields() {
    let out = ff_at(&std::env::temp_dir(), &["version", "--json"]);
    assert!(out.status.success(), "exit 0: {:?}", out.status);
    let v: serde_json::Value = serde_json::from_str(&stdout(&out)).expect("valid json");
    assert_eq!(v["ff"], 1);
    assert_eq!(v["cmd"], "version");
    assert_eq!(v["data"]["version"], env!("CARGO_PKG_VERSION"));

    // Commit and date are both recorded or both null — never one alone, which
    // is what the build script's "both or neither" rule buys.
    let commit = &v["data"]["commit"];
    let date = &v["data"]["date"];
    assert_eq!(
        commit.is_null(),
        date.is_null(),
        "half a provenance: {commit} / {date}"
    );
    if let Some(commit) = commit.as_str() {
        assert!(
            commit.len() >= 7 && commit.chars().all(|c| c.is_ascii_hexdigit()),
            "not a short sha: {commit}"
        );
        assert_eq!(date.as_str().map(str::len), Some(10), "not YYYY-MM-DD");
        // And the display line is built from exactly these two.
        let line = stdout(&ff_at(&std::env::temp_dir(), &["-v"]));
        assert!(line.contains(commit), "the line drops the commit: {line}");
    }

    // The update lane always reports one of its four states, and names a tag
    // only when there is one to name.
    let status = v["data"]["update"]["status"].as_str().expect("a status");
    assert!(
        ["unofficial", "unchecked", "available", "current"].contains(&status),
        "unknown update status: {status}"
    );
    assert_eq!(
        v["data"]["update"]["latest"].is_null(),
        status != "available",
        "a tag is named exactly when one is available"
    );
}

/// Every ``--json`` output carries the versioned envelope.
#[test]
fn json_output_carries_the_envelope() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("one");

    let out = ff(&fx, &["status", "--json"]);
    assert!(out.status.success());
    let v: serde_json::Value = serde_json::from_str(&stdout(&out)).expect("valid json");
    assert_eq!(v["ff"], 1, "contract version");
    assert_eq!(v["cmd"], "status", "command name");
    assert!(v["data"].is_object(), "data is an object");
    assert!(
        v["data"].get("changes").is_some(),
        "data contains the changes key"
    );
}

/// The ``cmd`` field matches the verb for every JSON-emitting command.
#[test]
fn json_envelope_names_each_command() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");

    for (args, expected_cmd) in [
        (["status", "--json"], "status"),
        (["log", "--json"], "log"),
        (["evolog", "--json"], "evolog"),
        (["doctor", "--json"], "doctor"),
    ] {
        let out = ff(&fx, &args);
        let text = stdout(&out);
        // doctor may exit 1 with findings; it still emits an envelope.
        let v: serde_json::Value = serde_json::from_str(&text)
            .unwrap_or_else(|err| panic!("valid json from {args:?}: {err}: {text}"));
        assert_eq!(v["cmd"], expected_cmd, "cmd mismatch for {:?}", args);
    }
}

/// ``--json`` output is exactly one line terminated by a single newline.
#[test]
fn json_output_is_one_line() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("one");

    let out = ff(&fx, &["status", "--json"]);
    assert!(out.status.success());
    let text = stdout(&out);
    assert!(
        text.ends_with('\n') && !text[..text.len() - 1].contains('\n'),
        "one line + one newline: {:?}",
        text
    );
}

/// A sub-mode of one verb shares its name; a different verb does not. So
/// ``ff log --commits`` is still ``log``, while the operation log — a
/// different command with a different output shape — stamps ``op log``.
/// ``ff session`` is the anti-precedent: two shapes under one name.
#[test]
fn each_shape_carries_its_own_envelope_name() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    fx.write("a.txt", "so there is an operation to show\n");
    assert!(ff(&fx, &[]).status.success());

    for (args, name) in [
        (vec!["log", "--commits", "--json"], "log"),
        (vec!["op", "log", "--json"], "op log"),
        (vec!["op", "show", "--json"], "op show"),
        (vec!["op", "trim", "--dry-run", "--json"], "op trim"),
        // The older spelling is the same verb, so the envelope says so.
        (vec!["trim", "--dry-run", "--json"], "op trim"),
    ] {
        let out = ff(&fx, &args);
        assert!(
            out.status.success(),
            "stderr: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        let v: serde_json::Value = serde_json::from_str(&stdout(&out)).expect("valid json");
        assert_eq!(v["cmd"], name, "cmd for {args:?}");
    }
}

#[test]
fn error_json_uses_the_envelope() {
    // Running ff status --json outside any repository provokes a discovery error.
    let tmp = tempfile::tempdir().expect("create temp dir");
    let out = ff_at(tmp.path(), &["status", "--json"]);
    assert_eq!(out.status.code(), Some(1));
    let text = stdout(&out);
    let v: serde_json::Value = serde_json::from_str(&text).expect("valid json");
    assert_eq!(v["ff"], 1, "envelope version");
    assert!(v.get("error").is_some(), "has error object");
    assert!(!v["error"]["id"].is_null(), "error.id is non-empty");
    assert!(
        !v["error"]["message"].is_null(),
        "error.message is non-empty"
    );
    assert!(v.get("data").is_none(), "has no data key");
}

/// `--json` after the verb parses and emits an envelope. The "before the verb"
/// position (`ff --json status`) does not work because clap's
/// `args_conflicts_with_subcommands` does not exempt `global = true` args.
#[test]
fn json_flag_parses_after_the_verb() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("one");

    let out = ff(&fx, &["status", "--json"]);
    assert!(out.status.success(), "exit 0");
    let v: serde_json::Value = serde_json::from_str(&stdout(&out)).expect("valid json");
    assert_eq!(v["cmd"], "status");

    let out = ff(&fx, &["log", "--json"]);
    assert!(out.status.success(), "exit 0");
    let v: serde_json::Value = serde_json::from_str(&stdout(&out)).expect("valid json");
    assert_eq!(v["cmd"], "log");
}

/// `--json` is accepted by every verb (clap does not reject it with exit 2).
#[test]
fn json_is_accepted_by_every_verb() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("one");

    for verb in [
        "status", "log", "evolog", "doctor", "config", "branch", "worktree",
    ] {
        let out = ff(&fx, &[verb, "--json"]);
        // A clap usage error exits 2 with "unexpected argument". We assert
        // that does NOT happen: the flag is accepted.
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(
            !stderr.contains("unexpected argument"),
            "--json rejected by {verb}: {stderr}"
        );
    }
}

/// `ff -m x status` remains a usage error. clap no longer refuses it — the
/// setting that did also refused the global flags — so main names the one
/// real conflict itself, and it must keep landing as `usage/bad-flags`.
#[test]
fn bare_flags_still_conflict_with_subcommands() {
    let fx = Fixture::new();
    let out = ff(&fx, &["-m", "x", "status"]);
    assert_eq!(
        out.status.code(),
        Some(2),
        "bare ff args must still conflict with subcommands"
    );

    let out = ff(&fx, &["-m", "x", "status", "--json"]);
    let v: serde_json::Value = serde_json::from_str(&stdout(&out)).expect("valid json");
    assert_eq!(v["error"]["id"], "usage/bad-flags");
}

/// This error is raised before there is a `Ctx` to consult, which is exactly
/// where a rendering can start depending on how early the failure happened
/// rather than on what the caller asked for. Without `--json` it must read as
/// prose on stderr and leave stdout empty, the same as every other failure.
#[test]
fn a_usage_error_without_json_stays_prose_on_stderr() {
    let fx = Fixture::new();
    let out = ff(&fx, &["-m", "x", "status"]);
    assert_eq!(out.status.code(), Some(2));
    assert_eq!(stdout(&out), "", "stdout must stay empty without --json");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.starts_with("ff: "),
        "expected prose on stderr, got {stderr:?}"
    );
    assert!(
        !stderr.contains("\"ff\":1"),
        "the machine envelope must not appear without --json: {stderr:?}"
    );
}

/// The globals are `global = true` precisely so they can ride any verb, and
/// `ff --json status` — the spelling DESIGN uses for `--at-op` too — was a
/// clap usage error until the conflict setting came off.
#[test]
fn globals_ride_ahead_of_the_subcommand() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");

    let out = ff(&fx, &["--json", "status"]);
    assert!(
        out.status.success(),
        "ff --json status must parse: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let v: serde_json::Value = serde_json::from_str(&stdout(&out)).expect("valid json");
    assert_eq!(v["cmd"], "status", "envelope names the verb");
    assert!(v["data"].is_object(), "envelope carries a payload");

    // The trailing spelling, which always worked, still does.
    let trailing = ff(&fx, &["status", "--json"]);
    assert!(trailing.status.success());
    let v: serde_json::Value = serde_json::from_str(&stdout(&trailing)).expect("valid json");
    assert_eq!(v["cmd"], "status");

    // And --session rides the same way — it is a tag on the capture this
    // invocation takes, not a verb of its own and not a filter.
    let out = ff(&fx, &["--session", "leading", "op", "log", "--json"]);
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let v: serde_json::Value = serde_json::from_str(&stdout(&out)).expect("valid json");
    assert_eq!(v["cmd"], "op log");
}
