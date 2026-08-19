//! `ff status`'s futures line and `ff branch list`'s verdict notes: the two
//! surfaces the futures simulation reports through. Runs the real `ff`
//! binary against hermetic fixtures — the runner idiom is copied from
//! `tests/cli.rs`.

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

/// `ff` with `NO_COLOR` set, on top of the hermetic env.
fn ff_no_color(fx: &Fixture, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_ff"))
        .current_dir(fx.path())
        .args(args)
        .env("NO_COLOR", "1")
        .env("GIT_CONFIG_GLOBAL", null_device())
        .env("GIT_CONFIG_SYSTEM", null_device())
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .expect("spawn ff")
}

fn stdout(out: &Output) -> String {
    String::from_utf8(out.stdout.clone()).expect("utf-8 stdout")
}

/// `line` with its escape sequences stripped — its visible content.
fn first_visible(line: &str) -> String {
    let mut out = String::new();
    let mut chars = line.chars();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            for c in chars.by_ref() {
                if c == 'm' {
                    break;
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

// --- Fixture builders ------------------------------------------------------

/// `feature` three commits ahead of `base`, `main` one commit ahead of
/// `base` (touching an unrelated file), landed on `feature`. A clean rebase.
fn clean_fixture() -> Fixture {
    let fx = Fixture::new();
    fx.write("shared.txt", "line1\nline2\nline3\n");
    fx.commit("base");
    fx.git(&["branch", "feature"]);
    fx.write("m.txt", "m\n");
    fx.commit("main moves");
    fx.git(&["switch", "feature"]);
    fx.write("f1.txt", "f1\n");
    fx.commit("add f1");
    fx.write("f2.txt", "f2\n");
    fx.commit("add f2");
    fx.write("f3.txt", "f3\n");
    fx.commit("add f3");
    fx
}

/// `feature` one commit ahead of `base`, `main` moved once. A clean rebase
/// of exactly one commit, for the singular-noun test.
fn clean_one_commit_fixture() -> Fixture {
    let fx = Fixture::new();
    fx.write("shared.txt", "line1\nline2\nline3\n");
    fx.commit("base");
    fx.git(&["branch", "feature"]);
    fx.write("m.txt", "m\n");
    fx.commit("main moves");
    fx.git(&["switch", "feature"]);
    fx.write("f1.txt", "f1\n");
    fx.commit("add f1");
    fx
}

/// `feature` three commits ahead of `base` (one of which conflicts with
/// `main`'s edit to `shared.txt`), landed on `feature`.
fn conflicting_fixture() -> Fixture {
    let fx = Fixture::new();
    fx.write("shared.txt", "line1\nline2\nline3\n");
    fx.commit("base");
    fx.git(&["branch", "feature"]);
    fx.write("shared.txt", "MAIN\nline2\nline3\n");
    fx.commit("main edits line1");
    fx.git(&["switch", "feature"]);
    fx.write("other.txt", "other\n");
    fx.commit("add other.txt");
    fx.write("shared.txt", "FEATURE\nline2\nline3\n");
    fx.commit("feat two conflicts");
    fx.write("third.txt", "third\n");
    fx.commit("add third.txt");
    fx
}

/// Same shape as `conflicting_fixture`, but the conflicting edit is left
/// uncommitted: one committed step ("add other.txt"), then `shared.txt`
/// rewritten in the working tree and never committed.
fn open_change_conflict_fixture() -> Fixture {
    let fx = Fixture::new();
    fx.write("shared.txt", "line1\nline2\nline3\n");
    fx.commit("base");
    fx.git(&["branch", "feature"]);
    fx.write("shared.txt", "MAIN\nline2\nline3\n");
    fx.commit("main edits line1");
    fx.git(&["switch", "feature"]);
    fx.write("other.txt", "other\n");
    fx.commit("add other.txt");
    fx.write("shared.txt", "FEATURE\nline2\nline3\n");
    fx
}

/// `feature` never moved past `base`; `main` moved twice. A fast-forward.
fn fast_forward_fixture() -> Fixture {
    let fx = Fixture::new();
    fx.write("shared.txt", "line1\nline2\nline3\n");
    fx.commit("base");
    fx.git(&["branch", "feature"]);
    fx.write("m1.txt", "m1\n");
    fx.commit("main one");
    fx.write("m2.txt", "m2\n");
    fx.commit("main two");
    fx.git(&["switch", "feature"]);
    fx
}

/// `feature` and `main` at the same commit: nothing to replay, nothing to
/// be ahead of.
fn up_to_date_fixture() -> Fixture {
    let fx = Fixture::new();
    fx.write("shared.txt", "line1\nline2\nline3\n");
    fx.commit("base");
    fx.git(&["branch", "feature"]);
    fx.git(&["switch", "feature"]);
    fx
}

/// `feature` two commits ahead of `base`; `main` never moved.
fn ahead_fixture() -> Fixture {
    let fx = Fixture::new();
    fx.write("shared.txt", "line1\nline2\nline3\n");
    fx.commit("base");
    fx.git(&["branch", "feature"]);
    fx.git(&["switch", "feature"]);
    fx.write("f1.txt", "f1\n");
    fx.commit("f1");
    fx.write("f2.txt", "f2\n");
    fx.commit("f2");
    fx
}

/// A lone `main`, no second branch, no upstream: neither axis exists, so
/// there is nothing to measure against and nothing honest to claim.
fn no_base_fixture() -> Fixture {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("base");
    fx
}

/// The sha `rev` resolves to.
fn sha(fx: &Fixture, rev: &str) -> String {
    fx.git(&["rev-parse", rev]).trim().to_string()
}

/// Wire `branch`'s upstream to `origin/<branch>` and point that tracking ref
/// at `target` — or leave it absent when `target` is empty, which is exactly
/// what a branch deleted on the forge looks like from here. No network is
/// involved: a tracking ref is just a ref.
fn set_upstream(fx: &Fixture, branch: &str, target: &str) {
    let remote_key = format!("branch.{branch}.remote");
    let merge_key = format!("branch.{branch}.merge");
    let merge_val = format!("refs/heads/{branch}");
    let tracking = format!("refs/remotes/origin/{branch}");
    fx.git(&["config", "remote.origin.url", "file:///nonexistent"]);
    fx.git(&[
        "config",
        "remote.origin.fetch",
        "+refs/heads/*:refs/remotes/origin/*",
    ]);
    fx.git(&["config", &remote_key, "origin"]);
    fx.git(&["config", &merge_key, &merge_val]);
    if !target.is_empty() {
        fx.git(&["update-ref", &tracking, target]);
    }
}

/// Two commits on `main` the remote has not seen. Standing on trunk, so the
/// base axis has nothing to say and the remote axis has all of it.
fn to_push_fixture() -> Fixture {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("base");
    let at = sha(&fx, "main");
    set_upstream(&fx, "main", &at);
    fx.write("b.txt", "b\n");
    fx.commit("second");
    fx.write("c.txt", "c\n");
    fx.commit("third");
    fx
}

/// The mirror of `to_push_fixture`: the remote moved twice, this copy did not.
fn to_pull_fixture() -> Fixture {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("base");
    let at = sha(&fx, "main");
    fx.write("b.txt", "b\n");
    fx.commit("second");
    fx.write("c.txt", "c\n");
    fx.commit("third");
    let ahead = sha(&fx, "main");
    fx.git(&["reset", "--hard", &at]);
    set_upstream(&fx, "main", &ahead);
    fx
}

/// An upstream configured against a tracking ref that is not there — what a
/// branch looks like once someone deletes it on the forge.
fn gone_remote_fixture() -> Fixture {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.commit("base");
    set_upstream(&fx, "main", "");
    fx
}

/// `clean_fixture` with a remote copy of `feature` still sitting at the fork
/// point: the base moved *and* there is work to publish, so both axes speak.
fn both_axes_fixture() -> Fixture {
    let fx = clean_fixture();
    let fork = sha(&fx, "feature~3");
    set_upstream(&fx, "feature", &fork);
    fx
}

/// The 200-character subject `long_subject_conflict_fixture` commits with.
/// Kept away from the word "conflicts": `the_verdict_survives_a_long_subject`
/// asserts that word stays on the note line, where the verdict lives.
fn long_subject() -> String {
    const PREFIX: &str = "feat two — a subject long enough to push the verdict off the right edge ";
    let subject = format!("{PREFIX}{}", "x".repeat(200 - PREFIX.chars().count()));
    assert_eq!(subject.chars().count(), 200, "the fixture's guard");
    subject
}

/// A conflicting `feature` whose tip subject is 200 characters — the row
/// shape that used to push the verdict off the right edge.
fn long_subject_conflict_fixture() -> Fixture {
    let fx = Fixture::new();
    fx.write("shared.txt", "line1\nline2\nline3\n");
    fx.commit("base");
    fx.git(&["branch", "feature"]);
    fx.write("shared.txt", "MAIN\nline2\nline3\n");
    fx.commit("main edits line1");
    fx.git(&["switch", "feature"]);
    fx.write("shared.txt", "FEATURE\nline2\nline3\n");
    fx.commit(&long_subject());
    fx
}

// --- Human output ------------------------------------------------------

#[test]
fn clean_verdict_prints_the_clean_line() {
    let fx = clean_fixture();
    let out = ff(&fx, &["status"]);
    assert!(out.status.success());
    let text = stdout(&out);
    assert!(
        text.contains("base moved — rebases cleanly (3 commits replayed)"),
        "got: {text}"
    );
}

#[test]
fn singular_commit_reads_naturally() {
    let fx = clean_one_commit_fixture();
    let out = ff(&fx, &["status"]);
    assert!(out.status.success());
    let text = stdout(&out);
    assert!(text.contains("(1 commit replayed)"), "got: {text}");
    assert!(!text.contains("(1 commits replayed)"), "got: {text}");
}

#[test]
fn conflict_line_names_the_commit_and_counts_files() {
    let fx = conflicting_fixture();
    let out = ff(&fx, &["status"]);
    assert!(out.status.success());
    let text = stdout(&out);
    assert!(
        text.contains("base moved — conflicts at \"feat two conflicts\" in 1 file"),
        "got: {text}"
    );
}

#[test]
fn open_change_conflict_line() {
    let fx = open_change_conflict_fixture();
    let out = ff(&fx, &["status"]);
    assert!(out.status.success());
    let text = stdout(&out);
    assert!(
        text.contains("conflicts with your open change in 1 file"),
        "got: {text}"
    );
}

#[test]
fn fast_forward_line() {
    let fx = fast_forward_fixture();
    let out = ff(&fx, &["status"]);
    assert!(out.status.success());
    let text = stdout(&out);
    assert!(text.contains("base moved — fast-forwards"), "got: {text}");
}

#[test]
fn a_settled_axis_collapses_to_nothing_to_sync() {
    let fx = up_to_date_fixture();
    let out = ff(&fx, &["status"]);
    assert!(out.status.success());
    let text = stdout(&out);
    assert!(text.contains("nothing to sync"), "got: {text}");
    // Never "in sync", which a reader can hear as "merged".
    assert!(!text.contains("in sync"), "got: {text}");
}

#[test]
fn ahead_of_the_base_is_silent() {
    // Sync never merges you into your base, so unmerged work is a branch's
    // permanent condition rather than pending work. Saying so every time
    // would teach people to stop reading the line.
    let fx = ahead_fixture();
    let out = ff(&fx, &["status"]);
    assert!(out.status.success());
    let text = stdout(&out);
    assert!(!text.contains("ahead"), "got: {text}");
    assert!(text.contains("nothing to sync"), "got: {text}");
}

// --- The remote axis -------------------------------------------------------

#[test]
fn unpushed_commits_are_what_there_is_to_push() {
    // The same verdict the base axis stays silent about: against the remote
    // it names precisely the commits sync will send.
    let fx = to_push_fixture();
    let out = ff(&fx, &["status"]);
    assert!(out.status.success());
    let text = stdout(&out);
    assert!(text.contains("2 to publish"), "got: {text}");
    assert!(
        !text.contains("origin/"),
        "ref syntax never appears: {text}"
    );
}

#[test]
fn a_remote_that_moved_ahead_is_what_there_is_to_pull() {
    let fx = to_pull_fixture();
    let out = ff(&fx, &["status"]);
    assert!(out.status.success());
    let text = stdout(&out);
    assert!(text.contains("2 to sync"), "got: {text}");
}

#[test]
fn a_deleted_remote_branch_says_so() {
    let fx = gone_remote_fixture();
    let out = ff(&fx, &["status"]);
    assert!(out.status.success());
    let text = stdout(&out);
    assert!(text.contains("remote is gone"), "got: {text}");
}

#[test]
fn both_axes_speak_in_one_vocabulary() {
    let fx = both_axes_fixture();
    let out = ff(&fx, &["status"]);
    assert!(out.status.success());
    let text = stdout(&out);
    assert!(
        text.contains("base moved — rebases cleanly (3 commits replayed)"),
        "got: {text}"
    );
    assert!(text.contains("3 to publish"), "got: {text}");
    // Base first, remote second: the thing underneath you before the thing
    // beside you.
    let base_at = text.find("base moved").expect("a base phrase");
    let push_at = text.find("3 to publish").expect("a remote phrase");
    assert!(base_at < push_at, "base comes first: {text}");
    assert!(!text.contains("nothing to sync"), "got: {text}");
}

#[test]
fn unknown_line_says_it_cannot_simulate() {
    // The brief's literal recipe (base; branch side; main advances; switch
    // side; side advances; merge main into side) does not in fact reach the
    // Unknown/MergeCommits verdict: once main is merged into side, main's
    // tip is a direct ancestor of side's tip, so `probe` finds it via
    // `bases.contains(&onto)` and returns UpToDate (verified empirically:
    // "ahead 3 of main"), never walking far enough to see the merge commit.
    // Reaching MergeCommits requires main to move *again* after the merge,
    // so neither tip is an ancestor of the other and the replay walk from
    // side's tip has to cross the merge commit. See "Contradictions found"
    // in the brief-D final report.
    let fx = Fixture::new();
    fx.write("base.txt", "base\n");
    fx.commit("base");
    fx.git(&["branch", "side"]);
    fx.write("m1.txt", "m1\n");
    fx.commit("main advances");
    fx.git(&["switch", "side"]);
    fx.write("s1.txt", "s1\n");
    fx.commit("side advances");
    fx.git(&["merge", "--no-edit", "main"]);
    fx.git(&["switch", "main"]);
    fx.write("m2.txt", "m2\n");
    fx.commit("main advances again");
    fx.git(&["switch", "side"]);

    let out = ff(&fx, &["status"]);
    assert!(out.status.success());
    let text = stdout(&out);
    assert!(
        text.contains("base moved — can't simulate (merge commits in the range)"),
        "got: {text}"
    );
}

#[test]
fn a_long_subject_is_truncated() {
    let fx = Fixture::new();
    fx.write("shared.txt", "line1\nline2\nline3\n");
    fx.commit("base");
    fx.git(&["branch", "feature"]);
    fx.write("shared.txt", "MAIN\nline2\nline3\n");
    fx.commit("main edits line1");
    fx.git(&["switch", "feature"]);
    fx.write("shared.txt", "FEATURE\nline2\nline3\n");
    let long_subject = "this subject is deliberately far longer than forty characters for sure";
    assert!(
        long_subject.len() >= 60,
        "fixture guard: subject must be 60+ chars"
    );
    fx.commit(long_subject);

    let out = ff(&fx, &["status"]);
    assert!(out.status.success());
    let text = stdout(&out);
    assert!(text.contains('\u{2026}'), "got: {text}");
    for line in text.lines() {
        assert!(
            line.chars().count() <= 100,
            "line exceeds 100 chars: {line:?}"
        );
    }
}

#[test]
fn no_axis_at_all_means_no_sync_line() {
    // `nothing to sync` is a claim, and fufu can only make it about axes it
    // can name. With neither, it says nothing rather than guessing.
    let fx = no_base_fixture();
    let out = ff(&fx, &["status"]);
    assert!(out.status.success());
    let text = stdout(&out);
    assert!(!text.contains("moved —"), "got: {text}");
    assert!(!text.contains("nothing to sync"), "got: {text}");
    assert!(!text.contains("to publish"), "got: {text}");
    assert!(!text.contains("can't simulate"), "got: {text}");
}

// --- Color per theme -----------------------------------------------------

// Read directly from `Palette`'s three constants (render.rs:434-472):
// muted/vivid use 256-color codes (`38;5;<n>`), terminal uses the base
// 16-color codes (30-37 range) via plain SGR params.
struct ThemeCodes {
    theme: &'static str,
    ok: &'static str,
    warn: &'static str,
    ahead: &'static str,
    at: &'static str,
}
// `at` and `ok` share a hue in all three palettes — the field is kept
// because the branch row asserts the `at` code as its own, not because the
// values ever differ.
const THEMES: [ThemeCodes; 3] = [
    ThemeCodes {
        theme: "muted",
        ok: "38;5;71",
        warn: "38;5;173",
        ahead: "38;5;67",
        at: "38;5;71",
    },
    ThemeCodes {
        theme: "vivid",
        ok: "38;5;41",
        warn: "38;5;208",
        ahead: "38;5;39",
        at: "38;5;41",
    },
    ThemeCodes {
        theme: "terminal",
        ok: "32",
        warn: "33",
        ahead: "34",
        at: "32",
    },
];

#[test]
fn verdict_colors_per_theme() {
    for t in THEMES {
        let clean = clean_fixture();
        clean.set_config("fufu.theme", t.theme);
        let out = ff_colored(&clean, &["status"]);
        assert!(out.status.success());
        let text = stdout(&out);
        let line = text
            .lines()
            .find(|l| l.contains("rebases cleanly"))
            .unwrap_or_else(|| panic!("no clean futures line in {text:?}"));
        assert!(
            line.contains(&format!("\x1b[{}m", t.ok)),
            "theme {}: expected ok code {} in {:?}",
            t.theme,
            t.ok,
            line
        );

        let conflicting = conflicting_fixture();
        conflicting.set_config("fufu.theme", t.theme);
        let out = ff_colored(&conflicting, &["status"]);
        assert!(out.status.success());
        let text = stdout(&out);
        let line = text
            .lines()
            .find(|l| l.contains("conflicts at"))
            .unwrap_or_else(|| panic!("no conflict futures line in {text:?}"));
        assert!(
            line.contains(&format!("\x1b[{}m", t.warn)),
            "theme {}: expected warn code {} in {:?}",
            t.theme,
            t.warn,
            line
        );

        // Blue is for commits pending against the remote, either direction —
        // so the fixture that earns it is unpushed work, not ahead-of-base.
        let ahead = to_push_fixture();
        ahead.set_config("fufu.theme", t.theme);
        let out = ff_colored(&ahead, &["status"]);
        assert!(out.status.success());
        let text = stdout(&out);
        let line = text
            .lines()
            .find(|l| l.contains("to publish"))
            .unwrap_or_else(|| panic!("no pending-against-remote line in {text:?}"));
        assert!(
            line.contains(&format!("\x1b[{}m", t.ahead)),
            "theme {}: expected ahead code {} in {:?}",
            t.theme,
            t.ahead,
            line
        );
    }
}

#[test]
fn no_color_keeps_every_word() {
    let fx = clean_fixture();
    let out = ff_no_color(&fx, &["status"]);
    assert!(out.status.success());
    let text = stdout(&out);
    assert!(
        text.contains("base moved — rebases cleanly (3 commits replayed)"),
        "got: {text}"
    );
    assert!(
        !text.contains('\x1b'),
        "no escape bytes with NO_COLOR: {text:?}"
    );
}

// --- JSON ------------------------------------------------------------------

#[test]
fn json_carries_the_whole_object() {
    let fx = conflicting_fixture();
    let out = ff(&fx, &["status", "--json"]);
    assert!(out.status.success());
    let text = stdout(&out);
    let v: serde_json::Value = serde_json::from_str(&text).expect("valid json");
    assert!(v["ff"].is_number(), "ff envelope key present");
    assert_eq!(v["cmd"], "status");

    // The human line compresses; the JSON carries the whole object, one slot
    // per axis, so a script never has to parse prose back into facts.
    let base = &v["data"]["futures"]["base"];
    assert_eq!(base["against"]["name"], "main");
    assert_eq!(base["against"]["role"], "trunk");
    assert_eq!(base["against"]["ref"], "refs/heads/main");
    let tip = base["against"]["tip"].as_str().expect("tip is a string");
    assert_eq!(tip.len(), 40, "tip is 40-char hex: {tip:?}");
    assert!(tip.chars().all(|c| c.is_ascii_hexdigit()));

    assert_eq!(base["verdict"]["kind"], "conflict");
    assert_eq!(base["verdict"]["at"]["what"], "commit");
    assert_eq!(base["verdict"]["at"]["subject"], "feat two conflicts");
    assert_eq!(base["verdict"]["paths"], serde_json::json!(["shared.txt"]));

    assert_eq!(v["data"]["futures"]["remote"], serde_json::Value::Null);
}

#[test]
fn json_carries_the_remote_axis_too() {
    let fx = both_axes_fixture();
    let out = ff(&fx, &["status", "--json"]);
    assert!(out.status.success());
    let text = stdout(&out);
    let v: serde_json::Value = serde_json::from_str(&text).expect("valid json");
    let remote = &v["data"]["futures"]["remote"];
    assert_eq!(remote["against"]["name"], "origin/feature");
    assert_eq!(remote["against"]["role"], "remote");
    assert_eq!(remote["against"]["ref"], "refs/remotes/origin/feature");
    assert_eq!(remote["verdict"]["kind"], "up-to-date");
    assert_eq!(remote["verdict"]["ahead"], 3);
    // The line the human sees says nothing about the base being ahead; the
    // model still carries every fact behind it.
    assert_eq!(v["data"]["futures"]["base"]["verdict"]["kind"], "clean");
}

#[test]
fn json_both_axes_are_null_without_either() {
    let fx = no_base_fixture();
    let out = ff(&fx, &["status", "--json"]);
    assert!(out.status.success());
    let text = stdout(&out);
    let v: serde_json::Value = serde_json::from_str(&text).expect("valid json");
    assert_eq!(v["data"]["futures"]["base"], serde_json::Value::Null);
    assert_eq!(v["data"]["futures"]["remote"], serde_json::Value::Null);
}

#[test]
fn json_clean_shape() {
    let fx = clean_fixture();
    let out = ff(&fx, &["status", "--json"]);
    assert!(out.status.success());
    let text = stdout(&out);
    let v: serde_json::Value = serde_json::from_str(&text).expect("valid json");
    assert_eq!(v["data"]["futures"]["base"]["verdict"]["kind"], "clean");
    assert_eq!(v["data"]["futures"]["base"]["verdict"]["replayed"], 3);
}

#[test]
fn json_gone_remote_shape() {
    let fx = gone_remote_fixture();
    let out = ff(&fx, &["status", "--json"]);
    assert!(out.status.success());
    let text = stdout(&out);
    let v: serde_json::Value = serde_json::from_str(&text).expect("valid json");
    assert_eq!(v["data"]["futures"]["remote"]["verdict"]["kind"], "gone");
}

#[test]
fn json_is_byte_identical_run_to_run() {
    let fx = clean_fixture();
    let a = ff(&fx, &["status", "--json"]);
    let b = ff(&fx, &["status", "--json"]);
    assert!(a.status.success() && b.status.success());
    assert_eq!(a.stdout, b.stdout, "identical bytes run to run");
}

// --- `ff branch` -----------------------------------------------------------

#[test]
fn branch_row_notes_a_clean_rebase() {
    let fx = clean_fixture();
    let out = ff(&fx, &["branch", "list"]);
    assert!(out.status.success());
    let text = stdout(&out);
    let lines: Vec<&str> = text.lines().collect();
    let head = lines
        .iter()
        .position(|l| l.starts_with('@') && l.contains("feature"))
        .unwrap_or_else(|| panic!("no feature head line in {text:?}"));
    let note = *lines
        .get(head + 1)
        .unwrap_or_else(|| panic!("no note line under {:?}", lines[head]));
    assert!(note.starts_with("    "), "note line: {note:?}");
    assert!(note.contains("base moved — rebases cleanly"), "got: {note}");
}

#[test]
fn branch_row_notes_a_conflict() {
    let fx = conflicting_fixture();
    let out = ff(&fx, &["branch", "list"]);
    assert!(out.status.success());
    let text = stdout(&out);
    let lines: Vec<&str> = text.lines().collect();
    let head = lines
        .iter()
        .position(|l| l.starts_with('@') && l.contains("feature"))
        .unwrap_or_else(|| panic!("no feature head line in {text:?}"));
    let note = *lines
        .get(head + 1)
        .unwrap_or_else(|| panic!("no note line under {:?}", lines[head]));
    assert!(note.starts_with("    "), "note line: {note:?}");
    assert!(note.contains("base moved — conflicts at"), "got: {note}");
}

#[test]
fn branch_row_is_silent_when_up_to_date() {
    let fx = up_to_date_fixture();
    let out = ff(&fx, &["branch", "list"]);
    assert!(out.status.success());
    let text = stdout(&out);
    let lines: Vec<&str> = text.lines().collect();
    let head = lines
        .iter()
        .position(|l| l.starts_with('@') && l.contains("feature"))
        .unwrap_or_else(|| panic!("no feature head line in {text:?}"));
    // No second line at all: absent, or itself a head line (a note line
    // would begin with four spaces).
    let next = lines.get(head + 1).copied();
    assert!(
        next.is_none() || !next.unwrap().starts_with("    "),
        "the feature row has no note line:\n{text}"
    );
    assert!(
        !lines.iter().any(|l| l.starts_with("    ")),
        "no line begins with four spaces:\n{text}"
    );
}

#[test]
fn branch_and_status_agree_about_the_open_change() {
    // Anti-contradiction test: `ff branch list` and `ff status` must never
    // disagree about the same fact. The row you are standing on carries the
    // open change into its simulation for exactly this reason.
    //
    // `ff branch list` deliberately never captures ("reads don't reconcile
    // here; `ff status` owns loudness" — cmd/branch.rs), so it only ever
    // sees an open change once *some* fufu command has recorded one. Run
    // `ff status` first to record the open change, then check that `ff
    // branch list` reads the same fact `ff status` just reported.
    let fx = open_change_conflict_fixture();

    let status_out = ff(&fx, &["status"]);
    assert!(status_out.status.success());
    let status_text = stdout(&status_out);
    assert!(
        status_text.contains("conflicts with your open change"),
        "got: {status_text}"
    );
    assert!(
        !status_text.contains("rebases cleanly"),
        "got: {status_text}"
    );

    let branch_out = ff(&fx, &["branch", "list"]);
    assert!(branch_out.status.success());
    let branch_text = stdout(&branch_out);
    assert!(
        branch_text.contains("conflicts with your open change"),
        "got: {branch_text}"
    );
    assert!(
        !branch_text.contains("rebases cleanly"),
        "got: {branch_text}"
    );
}

/// The test that would have caught the drift: the row's note line must be
/// the *same words* `ff status` puts on its header for the same branch —
/// one dialect, enforced at the byte level.
#[test]
fn branch_and_status_use_the_same_words() {
    let fx = clean_fixture();

    let status_out = ff(&fx, &["status"]);
    assert!(status_out.status.success());
    let status_text = stdout(&status_out);
    let header = status_text.lines().next().expect("a header line");
    let part = header
        .split(" · ")
        .find(|p| p.contains("base moved"))
        .unwrap_or_else(|| panic!("no base phrase in {header:?}"));

    let branch_out = ff(&fx, &["branch", "list"]);
    assert!(branch_out.status.success());
    let branch_text = stdout(&branch_out);
    let note = branch_text
        .lines()
        .find(|l| l.starts_with("    "))
        .unwrap_or_else(|| panic!("no note line in {branch_text:?}"));
    assert_eq!(
        note.trim(),
        part,
        "the row and the status line word the base axis alike"
    );
}

/// Pins the layout regression: a long subject used to push the verdict off
/// the right edge of the head line. Now the subject may run as long as it
/// wants — the verdict rides the note line and survives.
#[test]
fn the_verdict_survives_a_long_subject() {
    let fx = long_subject_conflict_fixture();
    let subject = long_subject();
    let out = ff(&fx, &["branch", "list"]);
    assert!(out.status.success());
    let text = stdout(&out);
    let lines: Vec<&str> = text.lines().collect();
    let head = lines
        .iter()
        .position(|l| l.starts_with('@') && l.contains("feature"))
        .unwrap_or_else(|| panic!("no feature head line in {text:?}"));
    let head_line = lines[head];
    assert!(
        head_line.contains(&subject),
        "the full 200-char subject stays on the head line: {head_line:?}"
    );
    assert!(
        !head_line.contains("conflicts"),
        "the verdict is off the head line: {head_line:?}"
    );
    let note = lines
        .get(head + 1)
        .copied()
        .unwrap_or_else(|| panic!("no note line in {text:?}"));
    assert!(note.starts_with("    "), "note line: {note:?}");
    assert!(note.contains("base moved — conflicts at"), "got: {note}");
}

/// The branch listing spends the same hues `ff status` does, per theme —
/// including the `at` green on the row you are standing on, in the same
/// bold + underline + hue run the map pins.
#[test]
fn branch_list_colors_per_theme() {
    for t in THEMES {
        let clean = clean_fixture();
        clean.set_config("fufu.theme", t.theme);
        let out = ff_colored(&clean, &["branch", "list"]);
        assert!(out.status.success());
        let text = stdout(&out);
        let line = text
            .lines()
            .find(|l| l.contains("rebases cleanly"))
            .unwrap_or_else(|| panic!("no clean note line in {text:?}"));
        assert!(
            line.contains(&format!("\x1b[{}m", t.ok)),
            "theme {}: expected ok code {} in {line:?}",
            t.theme,
            t.ok
        );

        let conflicting = conflicting_fixture();
        conflicting.set_config("fufu.theme", t.theme);
        let out = ff_colored(&conflicting, &["branch", "list"]);
        assert!(out.status.success());
        let text = stdout(&out);
        let line = text
            .lines()
            .find(|l| l.contains("conflicts at"))
            .unwrap_or_else(|| panic!("no conflict note line in {text:?}"));
        assert!(
            line.contains(&format!("\x1b[{}m", t.warn)),
            "theme {}: expected warn code {} in {line:?}",
            t.theme,
            t.warn
        );

        // Blue is for commits pending against the remote, either direction.
        let ahead = to_push_fixture();
        ahead.set_config("fufu.theme", t.theme);
        let out = ff_colored(&ahead, &["branch", "list"]);
        assert!(out.status.success());
        let text = stdout(&out);
        let line = text
            .lines()
            .find(|l| l.contains("to publish"))
            .unwrap_or_else(|| panic!("no to-push note line in {text:?}"));
        assert!(
            line.contains(&format!("\x1b[{}m", t.ahead)),
            "theme {}: expected ahead code {} in {line:?}",
            t.theme,
            t.ahead
        );

        // The current row carries the `at` hue over bold + underline — the
        // same three-escape run `tests/map.rs` pins for the map.
        let clean = clean_fixture();
        clean.set_config("fufu.theme", t.theme);
        let out = ff_colored(&clean, &["branch", "list"]);
        assert!(out.status.success());
        let text = stdout(&out);
        let line = text
            .lines()
            .find(|l| first_visible(l).starts_with('@'))
            .unwrap_or_else(|| panic!("no current row in {text:?}"));
        assert!(
            line.contains(&format!("\x1b[1m\x1b[4m\x1b[{}mfeature", t.at)),
            "theme {}: expected the at-hue code {} on the current row: {line:?}",
            t.theme,
            t.at
        );
    }
}

/// The redundant-encoding contract, mirroring the map's: piped and plain,
/// the listing still says where you are by shape, not by color.
#[test]
fn branch_list_piped_is_plain_and_still_says_where_you_are() {
    let fx = clean_fixture();
    let out = ff(&fx, &["branch", "list"]);
    assert!(out.status.success());
    let text = stdout(&out);
    assert!(!text.contains('\x1b'), "piped output is plain: {text:?}");
    // Every branch row keeps its sigil and brackets.
    for name in ["main", "feature"] {
        assert!(
            text.contains(&format!("\u{25b8} [{name}]")),
            "{name} keeps its sigil and brackets with color off: {text:?}"
        );
    }
    let at_rows = text.lines().filter(|l| l.starts_with("@ ")).count();
    assert_eq!(at_rows, 1, "exactly one you-are-here row:\n{text}");
    let at_row = text.lines().find(|l| l.starts_with("@ ")).expect("the row");
    assert!(
        at_row.contains("feature"),
        "that row names the branch: {at_row:?}"
    );
}

/// An unborn branch has no tip — the sha column says so with the dim em
/// dash, and the row still lands.
#[test]
fn an_unborn_branch_gets_an_em_dash() {
    let fx = Fixture::new(); // no commits at all
    let out = ff(&fx, &["branch", "list"]);
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let text = stdout(&out);
    let rows: Vec<&str> = text.lines().filter(|l| !l.is_empty()).collect();
    assert_eq!(rows.len(), 1, "one row in {text:?}");
    let row = rows[0];
    assert!(
        row.starts_with("@ "),
        "the unborn branch is the one you are on: {row:?}"
    );
    assert!(row.contains("\u{25b8} [main]"), "got: {row:?}");
    assert!(
        row.contains('\u{2014}'),
        "em dash where the sha would be: {row:?}"
    );
}

/// The verdict rides the header line, not a trailing one. It replaced the
/// upstream phrase there because the verdict is the half that needs a
/// decision — so a refactor that moves it back below the rows should fail
/// here rather than silently reintroduce the duplicate line Tyler spotted.
#[test]
fn the_verdict_rides_the_header_line() {
    let fx = clean_fixture();
    let out = ff(&fx, &["status"]);
    let text = stdout(&out);
    let first = text.lines().next().expect("a header line");
    assert!(
        first.starts_with("on feature ·"),
        "header names the branch first: {first:?}"
    );
    assert!(
        first.contains("base moved — rebases cleanly"),
        "the verdict is on the header line: {first:?}"
    );
    // And nowhere else: exactly one line mentions it.
    let mentions = text
        .lines()
        .filter(|l| l.contains("rebases cleanly"))
        .count();
    assert_eq!(mentions, 1, "the verdict is stated once:\n{text}");
}

/// When the base and the upstream are the same ref, the header says it once.
/// This is the duplicate that prompted the move: `on main · in sync with
/// origin/main` followed by a trailing `up to date with main`.
#[test]
fn standing_on_trunk_states_the_upstream_once() {
    // The shape that used to say it twice: standing ON trunk, whose base rung
    // reached for its own upstream while the header carried an upstream
    // phrase besides. Trunk has no base now — it sits on nothing — so the
    // remote is the only axis, and a settled one collapses to one phrase.
    let fx = Fixture::new();
    fx.write("shared.txt", "line1\nline2\nline3\n");
    fx.commit("base");
    let at = sha(&fx, "main");
    set_upstream(&fx, "main", &at);

    let out = ff(&fx, &["status"]);
    assert!(out.status.success());
    let text = stdout(&out);
    let first = text.lines().next().expect("a header line");
    assert!(!first.contains("in sync with"), "got: {first:?}");
    assert!(!first.contains("origin/"), "got: {first:?}");
    assert!(!first.contains("base"), "trunk sits on nothing: {first:?}");
    assert_eq!(
        text.lines()
            .filter(|l| l.contains("nothing to sync"))
            .count(),
        1,
        "stated once, on the header:\n{text}"
    );
}
