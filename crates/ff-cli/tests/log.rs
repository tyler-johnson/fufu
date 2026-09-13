//! `ff log`: the change-centric default and its `@` row, the `-r` revset
//! axis, the JSON envelope and its row limit, and the path axis — the rows
//! are the commits that touch the named paths, the `@` row is the open
//! change when it touches them too, and a selector that names nothing is
//! refused rather than answered with an empty log.

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

/// `ff` with color forced on despite the captured (non-TTY) stdout —
/// anstream honors `CLICOLOR_FORCE`, which is the only way a test can reach
/// the styling paths at all.
fn ff_colored(fx: &Fixture, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_ff"))
        .current_dir(fx.path())
        .args(args)
        .env("CLICOLOR_FORCE", "1")
        .env("GIT_CONFIG_GLOBAL", null_device())
        .env("GIT_CONFIG_SYSTEM", null_device())
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .expect("spawn ff")
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
    fx.set_config("user.name", "Log Tester");
    fx.set_config("user.email", "log@test.test");
    fx
}

/// first writes a.txt and b.txt; second modifies a.txt; third modifies b.txt.
fn fixture() -> Fixture {
    let fx = repo();
    fx.write("a.txt", "a\n");
    fx.write("b.txt", "b\n");
    fx.commit("first");
    fx.write("a.txt", "a2\n");
    fx.commit("second");
    fx.write("b.txt", "b2\n");
    fx.commit("third");
    fx
}

fn rows(out: &Output) -> Vec<String> {
    stdout(out)
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(str::to_owned)
        .collect()
}

/// The positional narrows the rows to the commits that touch it, and no
/// `--` separator is needed to mean it.
#[test]
fn a_path_narrows_the_rows_and_needs_no_separator() {
    let fx = fixture();
    let out = ff(&fx, &["log", "--commits", "a.txt"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert_eq!(rows(&out).len(), 2);
    let out = ff(&fx, &["log", "--commits", "--", "a.txt"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert_eq!(rows(&out).len(), 2);
}

/// The `@` row appears only when the open change touches the paths.
#[test]
fn the_open_change_row_appears_only_when_the_paths_are_touched() {
    let fx = fixture();
    fx.write("a.txt", "dirty\n");
    let a = ff(&fx, &["log", "a.txt"]);
    assert!(a.status.success(), "{}", stderr(&a));
    assert!(stdout(&a).lines().any(|line| line.contains('@')));
    let b = ff(&fx, &["log", "b.txt"]);
    assert!(b.status.success(), "{}", stderr(&b));
    assert!(!stdout(&b).lines().any(|line| line.contains('@')));
}

/// `--commits` drops the `@` row and keeps the path filter.
#[test]
fn commits_drops_the_open_row_and_keeps_the_filter() {
    let fx = fixture();
    fx.write("a.txt", "dirty\n");
    let out = ff(&fx, &["log", "--commits", "a.txt"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(!stdout(&out).lines().any(|line| line.contains('@')));
    assert_eq!(rows(&out).len(), 2);
}

/// A path that names nothing is refused, and the refusal is not everything.
#[test]
fn a_path_that_names_nothing_is_refused() {
    let fx = fixture();
    let out = ff(&fx, &["log", "bogus.txt"]);
    assert!(!out.status.success());
    assert!(stderr(&out).contains("bogus.txt"));
    let out = ff(&fx, &["log", "main"]);
    assert!(!out.status.success());
    assert!(ff(&fx, &["log", "a.txt"]).status.success());
}

/// A sentence in the path slot names the two flag-shaped exits, -r and -m.
#[test]
fn a_sentence_in_the_path_slot_names_the_missing_flag() {
    let fx = fixture();
    let out = ff(&fx, &["log", "fix the parser"]);
    assert!(!out.status.success());
    let err = stderr(&out);
    assert!(err.contains("-r"), "{}", err);
    assert!(err.contains("-m"), "{}", err);
}

/// The JSON envelope keeps its keys; the path axis only removes rows.
#[test]
fn json_keeps_its_shape_with_fewer_rows() {
    let fx = fixture();
    fx.write("a.txt", "dirty\n");
    let v = json(&ff(&fx, &["log", "--json", "a.txt"]));
    let data = v["data"].as_object().expect("envelope with a data object");
    assert!(data.contains_key("commits"));
    assert!(data.contains_key("open"));
    assert_eq!(data["commits"].as_array().expect("an array").len(), 2);
    assert!(!data["open"].is_null());
    let v = json(&ff(&fx, &["log", "--json", "b.txt"]));
    let data = v["data"].as_object().expect("envelope with a data object");
    assert!(data.contains_key("commits"));
    assert!(data.contains_key("open"));
    assert!(data["commits"].is_array());
    assert!(data["open"].is_null());
}

/// A reader that walks away ends the log, it does not crash it. `ff log
/// --commits` printed through `println!`, which panics when the pipe closes
/// — so `ff log --commits | head` died with "failed printing to stdout"
/// instead of exiting the way git does. Closing the read end before the
/// child writes a byte reproduces it every time; the other log views never
/// had it, because they already write through the pager's writer.
#[test]
fn a_closed_pipe_ends_the_log_rather_than_crashing_it() {
    let fx = fixture();
    let mut child = Command::new(env!("CARGO_BIN_EXE_ff"))
        .current_dir(fx.path())
        .args(["log", "--commits", "-n", "0"])
        .env("GIT_CONFIG_GLOBAL", null_device())
        .env("GIT_CONFIG_SYSTEM", null_device())
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("spawn ff");

    // The reader leaves: every write from here on hits a closed pipe.
    drop(child.stdout.take());
    let out = child.wait_with_output().expect("wait for ff");
    let err = String::from_utf8_lossy(&out.stderr).to_string();

    assert!(
        !err.contains("panicked"),
        "a closed pipe panicked the log:\n{err}"
    );
    assert!(
        out.status.success(),
        "a closed pipe should exit clean, got {:?}:\n{err}",
        out.status.code()
    );
}

#[test]
fn log_json_envelope_and_limit() {
    let fx = Fixture::new();
    for i in 0..4 {
        fx.write("f.txt", &format!("{i}\n"));
        fx.commit(&format!("c{i}"));
    }
    let out = ff(&fx, &["log", "--json", "-n", "2"]);
    assert!(out.status.success());
    let v: serde_json::Value = serde_json::from_str(&stdout(&out)).unwrap();
    let commits = v["data"]["commits"].as_array().unwrap();
    assert_eq!(commits.len(), 2);
    assert_eq!(commits[0]["subject"], "c3");
    for key in [
        "id",
        "short_id",
        "subject",
        "author_name",
        "author_email",
        "time",
    ] {
        assert!(!commits[0][key].is_null(), "missing key {key}");
    }
}

#[test]
fn log_defaults_to_25() {
    let fx = Fixture::new();
    for i in 0..30 {
        fx.write("f.txt", &format!("{i}\n"));
        fx.commit(&format!("c{i}"));
    }
    let out = ff(&fx, &["log", "--json"]);
    let v: serde_json::Value = serde_json::from_str(&stdout(&out)).unwrap();
    assert_eq!(v["data"]["commits"].as_array().unwrap().len(), 25);
    let out = ff(&fx, &["log", "--json", "-n", "0"]);
    let v: serde_json::Value = serde_json::from_str(&stdout(&out)).unwrap();
    assert_eq!(
        v["data"]["commits"].as_array().unwrap().len(),
        30,
        "-n 0 is unlimited"
    );
}

#[test]
fn log_unborn_is_empty_success() {
    let fx = Fixture::new();
    let out = ff(&fx, &["log", "--commits", "--json"]);
    assert!(out.status.success(), "unborn log exits 0");
    let v: serde_json::Value = serde_json::from_str(&stdout(&out)).unwrap();
    assert!(v["data"]["commits"].as_array().unwrap().is_empty());
    // The default view still carries the open change, with null fields.
    let out = ff(&fx, &["log", "--json"]);
    let v: serde_json::Value = serde_json::from_str(&stdout(&out)).unwrap();
    let d = &v["data"];
    assert!(d["commits"].as_array().unwrap().is_empty());
    assert_eq!(d["open"]["branch"], "main");
    assert!(d["open"]["id"].is_null());
    assert!(d["open"]["clean"].is_boolean());
    let human = ff(&fx, &["log", "--commits"]);
    assert!(human.status.success());
    assert_eq!(stdout(&human), "", "unborn --commits human prints nothing");
    // The default human view shows the @ row alone: no commits, no ● rows.
    let human = ff(&fx, &["log"]);
    let text = stdout(&human);
    assert!(text.starts_with("@"), "unborn @ row: {text:?}");
    assert!(text.contains("(no commits yet)"), "{text:?}");
    assert!(!text.contains('●'), "no commit rows: {text:?}");
}

#[test]
fn log_default_is_change_centric() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    fx.write("a.txt", "one\n");
    assert!(ff(&fx, &[]).status.success());
    fx.write("a.txt", "two\n");
    fx.commit("landed");
    fx.write("a.txt", "three\n");
    assert!(ff(&fx, &[]).status.success());

    // Human view: the @ row leads, ● commit rows follow, subjects on │ rails.
    let out = ff(&fx, &["log"]);
    let text = stdout(&out);
    assert!(text.starts_with("@  "), "@ row leads: {text:?}");
    assert!(text.contains("\n●  "), "commit rows: {text:?}");
    assert!(text.contains("\n│  "), "subject rails: {text:?}");
    assert!(
        text.contains("(no description)"),
        "open change without a pending description: {text:?}"
    );
    assert!(
        text.contains("│  landed") && text.contains("│  init"),
        "commit subjects present: {text:?}"
    );

    // JSON: commits key preserved, open object present, timeline gone.
    let out = ff(&fx, &["log", "--json"]);
    let v: serde_json::Value = serde_json::from_str(&stdout(&out)).unwrap();
    let d = &v["data"];
    assert!(d["commits"].is_array());
    assert!(d.get("timeline").is_none(), "timeline key retired");
    assert_eq!(d["open"]["branch"], "main");
    let id = d["open"]["id"].as_str().expect("chain tip present");
    assert!(
        id.len() == 40 && id.chars().all(|c| c.is_ascii_hexdigit()),
        "the anchor is the operation's full hex: {id:?}"
    );
    assert!(
        d["open"].get("id_letters").is_none(),
        "letters are a change id and nothing else: {d}"
    );
    assert_eq!(d["open"]["clean"], false, "uncaptured-free but dirty tree");
    let change_id = d["open"]["change_id"].as_str().unwrap();
    assert_eq!(change_id.len(), 32, "the open change's id: {change_id:?}");
    for row in d["commits"].as_array().unwrap() {
        assert_eq!(row["change_id"].as_str().unwrap().len(), 32, "{row}");
    }
    assert!(
        d["open"]["pending"].is_null(),
        "no identity configured in the fixture"
    );

    // --commits --json keeps the exact Phase 0 envelope.
    let out = ff(&fx, &["log", "--commits", "--json"]);
    let v: serde_json::Value = serde_json::from_str(&stdout(&out)).unwrap();
    let d = &v["data"];
    assert!(d["commits"].is_array());
    assert!(d.get("open").is_none(), "no open key in commits view");
    assert!(
        d.get("timeline").is_none(),
        "no timeline key in commits view"
    );
}

/// The @ row states: clean undescribed collapses to "no changes", dirty shows
/// the change id + the open commit's sha, describe changes the sha and not
/// the id, close lands that exact sha and returns to "no changes" with the
/// new ● row wearing the id the @ row wore, clean+described shows a fresh
/// id and no sha — fufu writes no empty commit.
#[test]
fn log_at_row_states() {
    let fx = Fixture::new();
    fx.set_config("user.name", "At Row");
    fx.set_config("user.email", "at@row.test");
    fx.write("a.txt", "a\n");
    fx.commit("init");

    // Clean, undescribed: collapsed "no changes" line.
    let text = stdout(&ff(&fx, &["log"]));
    let lines = text.lines().collect::<Vec<&str>>();
    assert_eq!(lines[0], "@  no changes", "{text:?}");
    assert_eq!(lines[1], "│  (no description)", "{text:?}");

    // Dirty tree: ff log's own pre-capture becomes the tip.
    fx.write("a.txt", "dirty\n");
    let text = stdout(&ff(&fx, &["log"]));
    let at_line = text.lines().next().unwrap();
    let tokens: Vec<&str> = at_line.split_whitespace().collect();
    assert_eq!(tokens[0], "@");
    assert!(
        tokens[1].len() == 8 && tokens[1].chars().all(|c| ('k'..='z').contains(&c)),
        "tip letters: {at_line:?}"
    );
    assert!(
        tokens[2].len() == 8 && tokens[2].chars().all(|c| c.is_ascii_hexdigit()),
        "pending sha: {at_line:?}"
    );
    let dirty_letters = tokens[1].to_string();
    let dirty_sha = tokens[2].to_string();

    // Describe while dirty: letters unchanged, pending sha changes.
    assert!(
        ff(&fx, &["describe", "-m", "work in progress"])
            .status
            .success()
    );
    let text = stdout(&ff(&fx, &["log"]));
    assert_eq!(text.lines().nth(1).unwrap(), "│  work in progress");
    let tokens: Vec<&str> = text.lines().next().unwrap().split_whitespace().collect();
    assert_eq!(tokens[1], dirty_letters, "letters unchanged after describe");
    assert_ne!(tokens[2], dirty_sha, "pending sha changed after describe");

    let described_sha = tokens[2].to_string();

    // Close: back to "no changes", and the commit is the open commit — the
    // sha the @ row showed is the sha the ● row wears, and it wears the id
    // the open change wore too: the letters column is an identity, so it
    // follows the change from the @ row into the ● row.
    assert!(ff(&fx, &["commit"]).status.success());
    let text = stdout(&ff(&fx, &["log"]));
    assert_eq!(text.lines().next().unwrap(), "@  no changes");
    let bullet_line = text
        .lines()
        .find(|l| l.starts_with('●'))
        .expect("a ● row exists");
    let bullet_tokens: Vec<&str> = bullet_line.split_whitespace().collect();
    assert_eq!(
        bullet_tokens[1], dirty_letters,
        "the closed commit wears the open change's id: {text:?}"
    );
    assert_eq!(
        bullet_tokens[2], described_sha,
        "the close moved the branch onto the open commit: {text:?}"
    );

    // Describe while clean: a fresh id, since the last one left with the
    // commit, and no sha — there is no commit until there is a change.
    assert!(ff(&fx, &["describe", "-m", "next up"]).status.success());
    let text = stdout(&ff(&fx, &["log"]));
    let tokens: Vec<&str> = text.lines().next().unwrap().split_whitespace().collect();
    assert!(
        tokens[1].len() == 8 && tokens[1].chars().all(|c| ('k'..='z').contains(&c)),
        "clean+described letters: {text:?}"
    );
    assert!(
        !(tokens[2].len() == 8 && tokens[2].chars().all(|c| c.is_ascii_hexdigit())),
        "clean+described has no sha: {text:?}"
    );
    assert_ne!(
        tokens[1], dirty_letters,
        "a new change is a new identity: {text:?}"
    );
    let bullet_line = text
        .lines()
        .find(|l| l.starts_with('●'))
        .expect("a ● row exists");
    assert_eq!(
        bullet_line.split_whitespace().nth(1).unwrap(),
        dirty_letters,
        "and the commit keeps its own: {text:?}"
    );
}

/// Every ● row wears a change id, whoever made the commit. A commit fufu
/// closed carries the id its open change wore, as a header; a commit git
/// made carries none, and its id is derived from the sha — the same answer
/// on every read and in every clone, with no walk.
#[test]
fn log_every_row_wears_a_change_id() {
    let fx = Fixture::new();
    fx.set_config("user.name", "Segment User");
    fx.set_config("user.email", "segment@test");
    fx.write("a.txt", "a\n");
    let bare = fx.commit("no header here");
    fx.write("a.txt", "b\n");
    let before = stdout(&ff(&fx, &["log"]));
    let open_letters = before
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .expect("the @ row wears letters")
        .to_string();
    assert!(ff(&fx, &["commit", "-m", "landed by ff"]).status.success());
    let landed = fx.git(&["rev-parse", "HEAD"]).trim().to_string();
    fx.write("a.txt", "c\n");
    fx.write("other.txt", "x\n");
    assert!(ff(&fx, &[]).status.success());
    fx.git(&["add", "a.txt"]);
    fx.git(&["commit", "-q", "-m", "partial"]);
    let partial = fx.git(&["rev-parse", "HEAD"]).trim().to_string();

    let text = stdout(&ff(&fx, &["log"]));
    let row_of = |text: &str, sha: &str| {
        text.lines()
            .find(|line| line.starts_with('●') && line.contains(&sha[..7]))
            .unwrap_or_else(|| panic!("no ● row for {sha}: {text:?}"))
            .to_string()
    };
    let letters_of = |row: &str| row.split_whitespace().nth(1).unwrap().to_string();
    let is_letters = |s: &str| s.len() == 8 && s.chars().all(|c| ('k'..='z').contains(&c));

    // landed's row: the id the @ row wore before the close, now a header.
    let landed_letters = letters_of(&row_of(&text, &landed));
    assert_eq!(
        landed_letters, open_letters,
        "the commit wears the id its open change wore: {text:?}"
    );
    let raw = fx.git(&["cat-file", "-p", &landed]);
    let header = raw
        .lines()
        .find_map(|line| line.strip_prefix("change-id "))
        .expect("a change-id header on the commit");
    assert_eq!(header.len(), 32, "the header is the whole id: {header:?}");
    assert!(
        header.starts_with(&landed_letters),
        "and the column is its prefix: {header} vs {landed_letters}"
    );

    // bare's and partial's rows: no header, so a derived id — filled, never
    // the dash, and stable across reads.
    for sha in [&bare, &partial] {
        let row = row_of(&text, sha);
        let letters = letters_of(&row);
        assert!(
            is_letters(&letters),
            "a derived id fills the column: {row:?}"
        );
        assert_ne!(letters, landed_letters, "and is its own: {text:?}");
    }
    let again = stdout(&ff(&fx, &["log"]));
    assert_eq!(
        letters_of(&row_of(&again, &bare)),
        letters_of(&row_of(&text, &bare)),
        "a derived id is the same on every read"
    );

    // The machine surface carries the whole id on every row.
    let out = ff(&fx, &["log", "--json"]);
    let v: serde_json::Value = serde_json::from_str(&stdout(&out)).unwrap();
    for row in v["data"]["commits"].as_array().unwrap() {
        let id = row["change_id"].as_str().expect("change_id on every row");
        assert_eq!(id.len(), 32, "{row}");
        assert!(id.chars().all(|c| ('k'..='z').contains(&c)), "{row}");
    }
    assert_eq!(v["data"]["commits"][1]["change_id"], header, "{v}");
}

/// The anchor walk stops early — once every displayed commit is answered, or
/// once the chain predates the oldest of them — so what it reports must not
/// depend on how far it happened to walk. Same chain, two window sizes, same
/// letters.
#[test]
fn log_segment_tips_ignore_how_far_the_walk_went() {
    let fx = Fixture::new();
    fx.set_config("user.name", "Segment User");
    fx.set_config("user.email", "segment@test");
    fx.write("a.txt", "a\n");
    fx.commit("root");

    // Three ff-made commits, each with snapshots piled on top, so every
    // anchor sits well below newer chain links.
    let mut landed = Vec::new();
    for round in 0..3 {
        for noise in 0..3 {
            fx.write("noise.txt", &format!("{round}-{noise}\n"));
            assert!(ff(&fx, &[]).status.success());
        }
        fx.write("a.txt", &format!("round {round}\n"));
        assert!(
            ff(&fx, &["commit", "-m", &format!("landed {round}")])
                .status
                .success()
        );
        landed.push(fx.git(&["rev-parse", "HEAD"]).trim().to_string());
    }

    let letters_for = |text: &str, sha: &str| -> String {
        let row = text
            .lines()
            .find(|line| line.starts_with('●') && line.contains(&sha[..7]))
            .unwrap_or_else(|| panic!("no ● row for {sha}: {text:?}"));
        row.split_whitespace().nth(1).unwrap().to_string()
    };

    // The tree is clean and stays clean, so these reads add no snapshots and
    // the two windows see the identical chain.
    let full = stdout(&ff(&fx, &["log"]));
    let narrow = stdout(&ff(&fx, &["log", "-n", "1"]));

    for sha in &landed {
        let id = letters_for(&full, sha);
        assert!(
            id.chars().all(|c| ('k'..='z').contains(&c)),
            "ff-made commit keeps its anchor behind newer snapshots: {id:?}"
        );
    }
    assert_eq!(
        letters_for(&narrow, landed.last().unwrap()),
        letters_for(&full, landed.last().unwrap()),
        "-n 1 and the full log agree on the newest commit's anchor"
    );
}

/// Three commits on main, clean tree, identity configured — the shape every
/// `-r` test below reads.
fn three_commits() -> Fixture {
    let fx = Fixture::new();
    fx.set_config("user.name", "Revset User");
    fx.set_config("user.email", "revset@test");
    for n in ["one", "two", "three"] {
        fx.write("a.txt", &format!("{n}\n"));
        fx.commit(n);
    }
    fx
}

fn bullet_rows(text: &str) -> Vec<&str> {
    text.lines().filter(|l| l.starts_with('●')).collect()
}

/// The subject of each ● row, which the renderer puts on the continuation
/// line beneath it.
fn bullet_subjects(text: &str) -> Vec<String> {
    let lines: Vec<&str> = text.lines().collect();
    lines
        .iter()
        .enumerate()
        .filter(|(_, l)| l.starts_with('●'))
        .map(|(i, _)| lines[i + 1].trim_start_matches('│').trim().to_string())
        .collect()
}

/// `-r` replaces where the rows come from and nothing else: the same renderer,
/// the same columns, a different set.
#[test]
fn log_revisions_narrow_the_rows() {
    let fx = three_commits();

    let all = stdout(&ff(&fx, &["log"]));
    assert_eq!(bullet_rows(&all).len(), 3, "{all:?}");

    let narrowed = stdout(&ff(&fx, &["log", "-r", "main"]));
    assert_eq!(
        bullet_subjects(&narrowed),
        vec!["three".to_string()],
        "one revision, one row — main's tip: {narrowed:?}"
    );

    // The long spelling is the same flag.
    assert_eq!(
        stdout(&ff(&fx, &["log", "--revisions", "main"])),
        narrowed,
        "-r and --revisions are one flag"
    );
}

/// The whole point of the membership rule: `ff log -r main` is a question
/// about main, and main does not contain the open change.
#[test]
fn log_revisions_without_the_open_change_print_no_at_row() {
    let fx = three_commits();

    let narrowed = stdout(&ff(&fx, &["log", "-r", "main"]));
    assert!(
        !narrowed.lines().any(|l| l.starts_with('@')),
        "no @ row when the set excludes it: {narrowed:?}"
    );

    // And it comes back when the set does contain it.
    let with_open = stdout(&ff(&fx, &["log", "-r", "::@"]));
    assert!(
        with_open.lines().next().unwrap().starts_with('@'),
        "@ heads the rows when it is a member: {with_open:?}"
    );
}

/// `--commits` is the plain history view of whatever set it is given, so the
/// two flags compose rather than conflict.
#[test]
fn log_revisions_compose_with_the_commits_view() {
    let fx = three_commits();

    let text = stdout(&ff(&fx, &["log", "--commits", "-r", "main"]));
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 1, "one revision, one row: {text:?}");
    assert!(lines[0].contains("three"), "{text:?}");

    let out = ff(&fx, &["log", "--commits", "-r", "main", "--json"]);
    let v: serde_json::Value = serde_json::from_str(&stdout(&out)).unwrap();
    let d = &v["data"];
    assert_eq!(d["commits"].as_array().unwrap().len(), 1);
    assert!(d.get("open").is_none(), "no open key in commits view");
}

/// `-n` bounds the commit rows, exactly as it does without `-r`. The `@` row
/// is not one of them, so a set containing the open change still shows it.
#[test]
fn log_revisions_respect_the_row_limit() {
    let fx = three_commits();

    let text = stdout(&ff(&fx, &["log", "-r", "::@", "-n", "2"]));
    assert!(text.lines().next().unwrap().starts_with('@'), "{text:?}");
    assert_eq!(
        bullet_subjects(&text),
        vec!["three".to_string(), "two".to_string()],
        "-n 2 bounds the commit rows and the @ row is not one of them: {text:?}"
    );
}

/// `--ops` is a removal, not a rename: `ff op log` is a different command
/// with a different output shape, so typing the old flag is answered with a
/// redirect rather than a bare "unexpected argument".
#[test]
fn log_ops_redirects_to_the_op_family() {
    let fx = three_commits();
    let out = ff(&fx, &["log", "--ops", "--json"]);
    assert_eq!(out.status.code(), Some(2));
    let v: serde_json::Value = serde_json::from_str(&stdout(&out)).unwrap();
    assert_eq!(v["error"]["id"], "usage/bad-flags");
    let hints = v["error"]["exits"].as_array().expect("exits");
    assert!(
        hints.iter().any(|h| h == "ff op log"),
        "the redirect names the verb that runs: {v}"
    );

    // And the global --session is a tag, never a filter: it rides ff log
    // without being mistaken for one.
    let out = ff(&fx, &["log", "-r", "main", "--session", "work", "--json"]);
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// A bad revset surfaces as its own coded refusal, not as an empty log.
#[test]
fn log_revisions_surface_revset_errors() {
    let fx = three_commits();
    for (src, id) in [
        ("nosuchbranch", "usage/revset-unknown-revision"),
        ("@@{1}", "usage/revset-open-suffix"),
        ("main...trunk", "usage/revset-no-symmetric-difference"),
    ] {
        let out = ff(&fx, &["log", "-r", src, "--json"]);
        assert!(!out.status.success(), "{src} must fail");
        let v: serde_json::Value = serde_json::from_str(&stdout(&out)).unwrap();
        assert_eq!(v["error"]["id"], id, "{src}");
    }

    // And without --json it is prose on stderr, stdout untouched.
    let out = ff(&fx, &["log", "-r", "nosuchbranch"]);
    assert_eq!(out.status.code(), Some(2));
    assert_eq!(stdout(&out), "");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.starts_with("ff: "), "{stderr:?}");
}

/// The JSON contract under `-r`: `commits` unchanged, `open` still always
/// present, and null exactly when the set excludes the open change.
#[test]
fn log_revisions_json_shape() {
    let fx = three_commits();

    let out = ff(&fx, &["log", "-r", "main", "--json"]);
    assert!(out.status.success());
    let v: serde_json::Value = serde_json::from_str(&stdout(&out)).unwrap();
    let d = &v["data"];
    let commits = d["commits"].as_array().unwrap();
    assert_eq!(commits.len(), 1);
    assert_eq!(commits[0]["subject"], "three");
    assert!(commits[0]["short_id"].is_string(), "row shape unchanged");
    assert!(commits[0].get("session").is_some(), "per-row session kept");
    assert!(
        d.get("open").is_some() && d["open"].is_null(),
        "open present and null when @ is not a member: {d}"
    );

    // A set that does contain it gets the same object as ever.
    let out = ff(&fx, &["log", "-r", "::@", "--json"]);
    let v: serde_json::Value = serde_json::from_str(&stdout(&out)).unwrap();
    let d = &v["data"];
    assert_eq!(d["commits"].as_array().unwrap().len(), 3);
    assert_eq!(d["open"]["branch"], "main");
    assert!(d["open"]["clean"].is_boolean());
}

/// Running `ff log --json` twice on the same dirty tree must produce the same
/// pending hash (no-op pre-captures must not move it). A further tree change
/// must produce a different hash. Dropping identity makes pending null.
#[test]
fn log_pending_hash_stability() {
    let fx = Fixture::new();
    fx.set_config("user.name", "Test User");
    fx.set_config("user.email", "test@user.test");
    fx.write("a.txt", "a\n");
    fx.commit("init");
    fx.write("a.txt", "dirty\n");

    // Two runs on the same dirty tree → same pending hash.
    let out1 = ff(&fx, &["log", "--json"]);
    let v1: serde_json::Value = serde_json::from_str(&stdout(&out1)).unwrap();
    let open1 = &v1["data"]["open"];

    let out2 = ff(&fx, &["log", "--json"]);
    let v2: serde_json::Value = serde_json::from_str(&stdout(&out2)).unwrap();
    let open2 = &v2["data"]["open"];

    let pending1 = open1["pending"].as_str().expect("pending is a string");
    assert_eq!(pending1.len(), 40, "40-char hex");
    assert!(pending1.chars().all(|c| c.is_ascii_hexdigit()), "ascii hex");

    let pending_short1 = open1["pending_short"]
        .as_str()
        .expect("pending_short is a string");
    assert_eq!(pending_short1, &pending1[..8]);

    assert_eq!(
        open2["pending"].as_str().unwrap(),
        pending1,
        "pending hash stable across runs"
    );

    // Edit again → hash changes.
    fx.write("a.txt", "more\n");
    let out3 = ff(&fx, &["log", "--json"]);
    let v3: serde_json::Value = serde_json::from_str(&stdout(&out3)).unwrap();
    assert_ne!(
        v3["data"]["open"]["pending"].as_str().unwrap(),
        pending1,
        "pending hash differs after tree change"
    );

    // Identity absent when the tree is captured → no open commit, and
    // pending and pending_short are null. The commit is the capture's, so
    // an identity dropped between captures changes nothing.
    fx.git(&["config", "--unset", "user.name"]);
    fx.git(&["config", "--unset", "user.email"]);
    fx.write("a.txt", "unauthored\n");
    let out4 = ff(&fx, &["log", "--json"]);
    assert!(out4.status.success());
    let v4: serde_json::Value = serde_json::from_str(&stdout(&out4)).unwrap();
    assert!(
        v4["data"]["open"]["pending"].is_null(),
        "pending null without identity"
    );
    assert!(
        v4["data"]["open"]["pending_short"].is_null(),
        "pending_short null without identity"
    );
}

/// `ff log` never builds the op-id index: its letters column is change ids,
/// priced among the ids on the page, so neither an uncolored run nor a
/// colored one leaves an index behind. `ff evolog` still abbreviates op ids
/// through the index, and is the view that builds it. If someone ever routes
/// the log's column back through the index, this fails rather than letting a
/// read-only checkout pay for it.
#[test]
fn the_log_never_builds_the_id_index() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    // A log deep enough that the append path needs an existing file, and then
    // a clean tree so neither run below captures anything.
    for i in 0..4 {
        fx.write("a.txt", &format!("v{i}\n"));
        assert!(ff(&fx, &[]).status.success());
    }
    fx.commit("settle");
    assert!(ff(&fx, &[]).status.success());

    // One log means one index file, not one per branch.
    let index = fx.path().join(".git/fufu/ops/main/live");
    std::fs::remove_file(&index).expect("remove index");

    // stdout here is a pipe, so anstream resolves color to never.
    let out = ff(&fx, &["log", "-n", "5"]);
    assert!(out.status.success(), "uncolored log should succeed");
    assert!(!index.exists(), "an uncolored log builds no index");

    let out = ff_colored(&fx, &["log", "-n", "5"]);
    assert!(out.status.success(), "colored log should succeed");
    assert!(
        stdout(&out).contains('\u{1b}'),
        "forced color really did emit ANSI, so the assertion below means something"
    );
    assert!(
        !index.exists(),
        "the change-id column is priced on the page, never through the index"
    );

    let out = ff(&fx, &["evolog", "-n", "5"]);
    assert!(out.status.success(), "evolog should succeed");
    assert!(
        index.exists(),
        "evolog abbreviates op ids through the index, and builds it"
    );
}
