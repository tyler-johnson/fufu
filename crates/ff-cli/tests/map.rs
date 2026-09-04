//! The map (bare `ff`): rows, lanes, elisions, branch scope, and the fact
//! that it still captures first. Runs the real `ff` binary against
//! hermetic fixtures.

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

/// Whether any row carries `name` as a branch label. The brackets are what
/// make this exact: a bare name could collide with a word in a subject, and
/// `[name]` cannot.
fn a_row_names(lines: &[&str], name: &str) -> bool {
    let label = format!("[{name}]");
    lines
        .iter()
        .any(|line| line.split_whitespace().any(|token| token == label))
}

/// The node lines of a map: the glyph column is what a row starts with, and
/// the edge lines under each node are indented, so they never match.
fn node_lines(text: &str) -> Vec<&str> {
    text.lines()
        .filter(|line| line.starts_with('@') || line.starts_with('●') || line.starts_with('~'))
        .collect()
}

/// A two-branch fixture with real topology: a fork, one commit per side
/// beyond it, the open change back on main.
fn fork_fixture() -> Fixture {
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
    fx
}

#[test]
fn one_branch_is_three_rows() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("one");
    for i in 2..=5 {
        fx.write("a.txt", &format!("v{i}\n"));
        fx.commit(&format!("c{i}"));
    }

    let out = ff(&fx, &[]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let text = stdout(&out);
    let rows = node_lines(&text);
    // One branch has no topology: the skeleton is the open change, the tip,
    // and a frontier marker saying history continues.
    assert_eq!(rows.len(), 3, "open change, tip, frontier: {text:?}");
    assert!(rows[0].starts_with('@'), "the open change leads: {text:?}");
    assert!(
        rows[1].starts_with('●'),
        "the tip is a commit row: {text:?}"
    );
    assert_eq!(rows[2], "~", "the frontier marker stands alone: {text:?}");
}

#[test]
fn a_fork_draws_a_second_lane() {
    let fx = fork_fixture();
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
        lines.iter().any(|line| line.contains("│ ●")),
        "a row sits in lane 1: {text:?}"
    );
    // The lane folds back onto the spine at the fork. Always leftward: the
    // spine keeps lane 0 whichever branch happened to reach the shared parent
    // first, so this is `├─╯` and never its mirror.
    assert!(
        lines.iter().any(|line| line.contains("├─╯")),
        "a lane folds back at the fork: {text:?}"
    );
    assert!(
        !lines.iter().any(|line| line.contains("╰─┤")),
        "no rightward fold — the spine does not drift: {text:?}"
    );
    assert!(
        a_row_names(&lines, "feature"),
        "the second branch's name is on the map: {text:?}"
    );

    // The same shape must survive the styled path.
    let out = ff_colored(&fx, &[]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        stdout(&out).contains("feature"),
        "the branch name survives coloring"
    );
}

/// A branch name is the map's whole point, so it is called out three ways at
/// once: a leading sigil, brackets, and an underline, over the bold that
/// already means "what you can type". Two of the three are pure shape, which
/// is the property that matters — they survive a pipe, `NO_COLOR`, and a
/// screen reader, so the emphasis is never carried by color alone.
#[test]
fn branch_names_are_called_out_three_ways_and_two_survive_no_color() {
    let fx = fork_fixture();

    // Uncolored: the sigil and the brackets are still there.
    let plain = stdout(&ff(&fx, &[]));
    for name in ["main", "feature"] {
        assert!(
            plain.contains(&format!("\u{25b8} [{name}]")),
            "{name} keeps its sigil and brackets with color off: {plain:?}"
        );
    }

    // Colored: the name itself also carries bold + underline, and the current
    // branch adds the `at` green on top of that.
    let text = stdout(&ff_colored(&fx, &[]));
    assert!(
        text.contains("\u{1b}[1m\u{1b}[4mfeature\u{1b}[0m"),
        "a non-current branch is bold + underlined, uncolored: {text:?}"
    );
    let at_row = text.lines().next().expect("an @ row");
    assert!(
        at_row.contains("\u{1b}[1m\u{1b}[4m\u{1b}[38;5;71mmain"),
        "the current branch adds at-green over the same emphasis: {at_row:?}"
    );
}

#[test]
fn an_elided_run_reports_its_count() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("f");
    fx.git(&["branch", "feature"]);
    for i in 1..=7 {
        fx.write("a.txt", &format!("m{i}\n"));
        fx.commit(&format!("main {i}"));
    }
    fx.git(&["switch", "feature"]);
    fx.write("a.txt", "ft\n");
    fx.commit("feature one");
    fx.git(&["switch", "main"]);

    let out = ff(&fx, &[]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let text = stdout(&out);
    // Elision rows read `~  N commits`: the token before "commits" is the
    // count. A single commit never elides, so a count of 1 is a bug.
    let counts: Vec<u64> = text
        .lines()
        .filter_map(|line| {
            let tokens: Vec<&str> = line.split_whitespace().collect();
            let at = tokens.iter().position(|t| *t == "commits")?;
            if at == 0 {
                return None;
            }
            tokens[at - 1].parse().ok()
        })
        .collect();
    assert!(
        counts.iter().any(|n| *n >= 2),
        "an elided run reports its count: {text:?}"
    );
    assert!(
        !counts.contains(&1),
        "a run of one is never elided: {text:?}"
    );
}

#[test]
fn merges_of_deleted_branches_leave_no_rows() {
    // A merge-heavy trunk: two side branches merged and deleted, plus one
    // live topic branch to walk relative to. The merges and the sides'
    // fork points are history, not branch relations — the map is the two
    // tips, the fork they share, and one elision along trunk's line.
    let fx = Fixture::new();
    fx.write("a.txt", "base\n");
    fx.commit("base");
    fx.write("a.txt", "f\n");
    fx.commit("f");
    fx.git(&["branch", "feature"]);
    fx.write("a.txt", "m1\n");
    fx.commit("m1");
    for side in ["side1", "side2"] {
        fx.git(&["switch", "-c", side]);
        fx.write(&format!("{side}.txt"), "s\n");
        fx.commit(&format!("work on {side}"));
        fx.git(&["switch", "main"]);
        fx.git(&["merge", "--no-ff", "-m", &format!("landed {side}"), side]);
        fx.git(&["branch", "-D", side]);
    }
    fx.write("a.txt", "tip\n");
    fx.commit("main tip");
    fx.git(&["switch", "feature"]);
    fx.write("f.txt", "ft\n");
    fx.commit("feature one");
    fx.git(&["switch", "main"]);

    let out = ff(&fx, &[]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let text = stdout(&out);
    assert!(
        !text.contains("landed"),
        "a merge of vanished history has no row: {text:?}"
    );
    assert_eq!(
        text.matches('●').count(),
        3,
        "two tips and the fork are the only commit rows: {text:?}"
    );
    // The elision counts trunk's own line — the merges and m1, not the
    // side commits they landed.
    assert!(
        text.lines().any(|line| line.contains("3 commits")),
        "the run through the invisible merges is counted: {text:?}"
    );
}

#[test]
fn n_bounds_the_branches_and_all_lifts_it() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("f");
    // Oldest first, so the newest tip is unambiguous.
    for name in ["b1", "b2", "b3", "b4"] {
        fx.git(&["branch", name]);
        fx.git(&["switch", name]);
        fx.write("a.txt", &format!("{name}\n"));
        fx.commit(&format!("commit on {name}"));
        fx.git(&["switch", "main"]);
    }

    let names = ["b1", "b2", "b3", "b4"];
    let named = |text: &str| {
        let lines: Vec<&str> = text.lines().collect();
        names
            .iter()
            .filter(|name| a_row_names(&lines, name))
            .count()
    };

    let all = stdout(&ff(&fx, &["--all"]));
    assert_eq!(named(&all), 4, "--all names all four: {all:?}");

    let bounded = stdout(&ff(&fx, &["-n", "1"]));
    assert!(
        named(&bounded) < named(&all),
        "-n 1 shows fewer branches: {bounded:?}"
    );

    let zero = stdout(&ff(&fx, &["-n", "0"]));
    assert_eq!(
        named(&zero),
        named(&all),
        "-n 0 lifts the bound like --all: {zero:?}"
    );
}

#[test]
fn json_carries_the_rows() {
    let fx = fork_fixture();
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

    // Parents point at rows drawn later: the walk is newest-first, so an
    // edge can only reach a greater index.
    for (i, row) in rows.iter().enumerate() {
        for parent in row["parents"].as_array().expect("parents array") {
            let p = parent.as_u64().expect("parent is an index");
            assert!(p > i as u64, "row {i}'s parent {p} is after it: {v:?}");
        }
        // An elision either counts the run it stands for or marks an open
        // frontier; a count of one would be a single commit in disguise.
        if row["node"]["kind"] == "elided" {
            let count = &row["node"]["count"];
            assert!(
                count.is_null() || (count.is_i64() && count.as_i64().unwrap() >= 2),
                "elision count is null or >= 2: {count:?}"
            );
        }
    }
}

#[test]
fn the_map_still_captures() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("init");
    fx.write("a.txt", "dirty\n");

    let out = ff(&fx, &[]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );

    // The op log is the receipt for the capture-first rule: the dirty tree
    // must land on it as a capture row.
    let out = ff(&fx, &["op", "log", "--json"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let v: serde_json::Value = serde_json::from_str(&stdout(&out)).expect("valid json");
    let ops = v["data"]["ops"].as_array().expect("ops array");
    assert!(
        ops.iter().any(|op| op["kind"] == "capture"),
        "a capture row is on the op log: {v:?}"
    );
}

#[test]
fn branch_scope_flags_do_not_ride_a_verb() {
    let fx = Fixture::new();
    for args in [&["-n", "2", "status"][..], &["--all", "status"][..]] {
        let out = ff(&fx, args);
        assert_eq!(out.status.code(), Some(2), "ff {args:?} is a usage error");
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(
            stderr.contains("branch scope"),
            "stderr names the map's scope: {stderr:?}"
        );
    }
}

/// A fixture whose commits are fufu's, so the operation log has captures for
/// the anchor walk to find — `Fixture::commit` is raw git and leaves none.
fn captured_repo() -> Fixture {
    let fx = Fixture::new();
    fx.set_config("user.name", "Map Tester");
    fx.set_config("user.email", "map@test.test");
    fx
}

fn ff_commit(fx: &Fixture, msg: &str) {
    let out = ff(fx, &["commit", "-m", msg]);
    assert!(
        out.status.success(),
        "ff commit -m {msg}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// The op-id column on the map's commit rows is the same chain-segment
/// anchor `ff log` prints for that commit — the sibling surface is the
/// oracle, because "one operation, one spelling, whichever surface asks" is
/// the whole claim. The map used to leave this column blank.
#[test]
fn commit_rows_carry_their_operation_id() {
    let fx = captured_repo();
    fx.write("a.txt", "a\n");
    ff_commit(&fx, "one");
    fx.write("a.txt", "b\n");
    ff_commit(&fx, "two");

    let out = ff(&fx, &["log", "--json"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let v: serde_json::Value = serde_json::from_str(&stdout(&out)).expect("valid json");
    let tip_sha = v["data"]["commits"][0]["short_id"]
        .as_str()
        .expect("a short id")
        .to_string();

    let map_text = stdout(&ff(&fx, &[]));
    let log_text = stdout(&ff(&fx, &["log"]));
    let on_map = letters_beside(&map_text, &tip_sha);
    let in_log = letters_beside(&log_text, &tip_sha);
    assert_ne!(
        on_map, "\u{2014}",
        "the map's letters column is filled: {map_text:?}"
    );
    assert_eq!(
        on_map, in_log,
        "map and log spell the same commit's operation the same way:\n{map_text}\n{log_text}"
    );
}

/// The token in the op-id column of the row carrying `sha` — the one
/// immediately before the sha, both surfaces sharing the column order.
fn letters_beside(text: &str, sha: &str) -> String {
    let row = text
        .lines()
        .find(|line| line.split_whitespace().any(|token| token == sha))
        .unwrap_or_else(|| panic!("a row names {sha}: {text:?}"));
    let tokens: Vec<&str> = row.split_whitespace().collect();
    let at = tokens
        .iter()
        .position(|token| *token == sha)
        .expect("the sha is a token");
    assert!(at > 0, "something precedes the sha: {row:?}");
    tokens[at - 1].to_string()
}

/// With nothing open, the map's `@` row says what `ff status` and `ff log`
/// say — and keeps the branch name, which is the map's whole point. It used
/// to show the chain tip's operation instead: the same id the `●` row below
/// it carries, printed twice, on a row about nothing.
#[test]
fn a_clean_open_change_says_no_changes_and_keeps_its_branch() {
    let fx = captured_repo();
    fx.write("a.txt", "a\n");
    ff_commit(&fx, "one");

    let text = stdout(&ff(&fx, &[]));
    let lines: Vec<&str> = text.lines().collect();
    let rows = node_lines(&text);
    let open = rows.first().expect("an open row").to_string();
    assert!(
        open.contains("no changes"),
        "the clean open row says so: {text:?}"
    );
    assert!(
        a_row_names(&lines, "main"),
        "and still names the branch: {text:?}"
    );
    // The tip's operation belongs to the tip's row, and nowhere else.
    let tip = rows
        .iter()
        .find(|row| row.starts_with('\u{25cf}'))
        .expect("a commit row");
    let letters = tip
        .split_whitespace()
        .nth(1)
        .expect("the tip's op-id column");
    assert_ne!(
        letters, "\u{2014}",
        "the tip carries an operation: {text:?}"
    );
    assert!(
        !open.contains(letters),
        "which the open row does not repeat: {text:?}"
    );

    // Conditional, not a deletion: dirty the tree and the full row is back.
    fx.write("a.txt", "dirty\n");
    let text = stdout(&ff(&fx, &[]));
    let open = node_lines(&text).first().expect("an open row").to_string();
    assert!(
        !open.contains("no changes"),
        "an open change is not nothing: {text:?}"
    );
    assert!(
        a_row_names(&text.lines().collect::<Vec<_>>(), "main"),
        "the branch survives either way: {text:?}"
    );
}

/// A parked change is one change, so it is one number. The map counted only
/// the tracked half of a park, so a branch carrying an edit and a new file
/// read `(+ parked change, 1 file)` while `ff switch` back to it resumed
/// `2 file(s)` — the same change, and the smaller number first.
#[test]
fn a_parked_change_counts_its_untracked_files_too() {
    let fx = captured_repo();
    fx.write("a.txt", "a\n");
    ff_commit(&fx, "one");
    fx.git(&["switch", "-c", "feature"]);
    fx.write("b.txt", "b\n");
    ff_commit(&fx, "two");

    // The park's two halves: an edit to a tracked file, and a new one.
    fx.write("b.txt", "edited\n");
    fx.write("c.txt", "new\n");
    let out = ff(&fx, &["switch", "main"]);
    assert!(
        out.status.success(),
        "ff switch main: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let text = stdout(&ff(&fx, &[]));
    let row = text
        .lines()
        .find(|line| line.split_whitespace().any(|token| token == "[feature]"))
        .unwrap_or_else(|| panic!("a row naming feature: {text:?}"));
    assert!(
        row.contains("(+ parked change, 2 files)"),
        "the parked row counts both halves: {text:?}"
    );

    // The claim is the agreement: arrival puts back what the map counted.
    let text = stdout(&ff(&fx, &["switch", "feature"]));
    assert!(
        text.contains("resumed the parked change (2 file(s))"),
        "and arrival says the same number: {text:?}"
    );
}
