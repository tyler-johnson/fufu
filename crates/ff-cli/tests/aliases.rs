//! Spellings: the short aliases, the map's own name, and the git words fufu
//! answers rather than runs.
//!
//! All three are the same contract seen from different sides — what a person
//! may type, and what fufu does with a word it recognizes but does not run.
//! The aliases are pinned by the verb they *dispatch* to rather than by the
//! help page they resolve to, because an alias hung on the wrong variant
//! would still print a plausible page.

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
        .env_remove("FF_SESSION")
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

fn json(out: &Output) -> serde_json::Value {
    serde_json::from_str(&stdout(out)).expect("valid json")
}

fn repo() -> Fixture {
    let fx = Fixture::new();
    fx.set_config("user.name", "Alias Tester");
    fx.set_config("user.email", "alias@test.test");
    fx.write("a.txt", "a\n");
    fx.commit("init");
    fx
}

/// The ids `ff op log --json` prints, captures included — the whole log, so
/// a verb that wrote anything at all shows up here.
fn op_count(fx: &Fixture) -> usize {
    let out = ff(fx, &["op", "log", "--json", "-n", "0"]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    json(&out)["data"]["ops"]
        .as_array()
        .expect("ops array")
        .len()
}

/// The aliases whose verb reads and can be run for free. The envelope name
/// is the proof: it comes from the variant that was dispatched, so a wrong
/// one cannot be papered over by the alias resolving to a help page. jj's
/// `bookmark` and `workspace` are on the same list, since they are the
/// same kind of thing: a second spelling of a verb fufu has.
#[test]
fn short_spellings_dispatch_to_their_verbs() {
    let fx = repo();
    for (alias, envelope) in [
        ("st", "status"),
        ("ev", "evolog"),
        ("br", "branch list"),
        ("cfg", "config"),
        ("bookmark", "branch list"),
        ("workspace", "worktree list"),
    ] {
        let out = ff(&fx, &[alias, "--json"]);
        assert!(out.status.success(), "ff {alias} failed: {}", stderr(&out));
        assert_eq!(
            json(&out)["cmd"].as_str(),
            Some(envelope),
            "ff {alias} should be {envelope}"
        );
    }
}

/// The others mutate, so they are pinned at the parser instead: clap
/// prints the canonical name in the usage line, never the alias.
#[test]
fn the_mutating_aliases_resolve_to_their_verbs() {
    let fx = repo();
    for (alias, verb) in [
        ("ci", "commit"),
        ("sw", "switch"),
        ("desc", "describe"),
        ("squash", "absorb"),
        ("rebase", "restack"),
    ] {
        let out = ff(&fx, &[alias, "--help"]);
        assert!(out.status.success(), "ff {alias} --help failed");
        assert!(
            stdout(&out).contains(&format!("Usage: ff {verb}")),
            "ff {alias} --help should be {verb}'s page"
        );
    }
}

/// An alias is a second spelling of a verb, so it rides that verb's row as
/// a suffix rather than taking a row of its own: the command list is what
/// fufu does, and a row per spelling would list some verbs twice.
#[test]
fn the_aliases_ride_their_rows() {
    let fx = repo();
    let page = stdout(&ff(&fx, &["--help"]));
    // The list is grouped now, so there is no one `Commands:` header to split
    // on: it runs from the usage line to the options, and a row is indented
    // where a heading is not.
    let list = page
        .split_once("Usage: ff")
        .and_then(|(_, rest)| rest.split_once("\nOptions:"))
        .map(|(list, _)| list.to_string())
        .expect("a command list");
    // The suffix is on the verb's row: one alias singular, two plural, and
    // jj's name beside the short form where a verb has both.
    for (verb, suffix) in [
        ("status", "[alias: st]"),
        ("branch", "[aliases: br, bookmark]"),
        ("restack", "[alias: rebase]"),
    ] {
        let row = list
            .lines()
            .find(|line| line.strip_prefix("  ").is_some_and(|r| r.starts_with(verb)))
            .unwrap_or_else(|| panic!("a row for {verb}: {list}"));
        assert!(row.contains(suffix), "{verb}'s row carries {suffix}: {row}");
    }
    // The row's own name, not a substring of it: `  status` starts with `st`.
    let rows: Vec<&str> = list
        .lines()
        .filter_map(|line| line.strip_prefix("  "))
        .filter_map(|row| row.split_whitespace().next())
        .collect();
    for alias in [
        "st",
        "ci",
        "sw",
        "br",
        "ev",
        "desc",
        "cfg",
        "new",
        "bookmark",
        "workspace",
        "squash",
        "rebase",
    ] {
        assert!(
            !rows.contains(&alias),
            "{alias} should not be a row in the command list: {rows:?}"
        );
    }
    // The list is of what fufu does, so a git or jj word it merely answers
    // is not a row either.
    for foreign in [
        "checkout", "stash", "pull", "merge", "blame", "tag", "abandon", "split",
    ] {
        assert!(
            !rows.contains(&foreign),
            "{foreign} is answered, not offered: {rows:?}"
        );
    }
}

/// Bare `ff` is the map, and "the map" is a word the docs use — so it is a
/// word you can type. Same command, same page, same envelope.
#[test]
fn the_map_has_a_name_of_its_own() {
    let fx = repo();
    let bare = ff(&fx, &["--json"]);
    let named = ff(&fx, &["map", "--json"]);
    assert!(named.status.success(), "stderr: {}", stderr(&named));
    assert_eq!(json(&named)["cmd"].as_str(), Some("map"));
    assert_eq!(
        json(&bare)["data"]["rows"].as_array().map(Vec::len),
        json(&named)["data"]["rows"].as_array().map(Vec::len),
        "the named form draws what the bare form draws"
    );

    // The page is the root page, because it is the same command.
    let page = stdout(&ff(&fx, &["help", "map"]));
    assert!(page.contains("Bare `ff` is the map"), "root page: {page}");
}

/// The scope flags belong to whichever spelling is being used, and they are
/// refused on the other side rather than silently ignored.
#[test]
fn the_map_takes_its_scope_after_its_name() {
    let fx = repo();
    for args in [
        &["map", "-n", "1", "--json"][..],
        &["map", "--all", "--json"],
    ] {
        let out = ff(&fx, args);
        assert!(out.status.success(), "ff {args:?}: {}", stderr(&out));
        assert_eq!(json(&out)["cmd"].as_str(), Some("map"));
    }

    let out = ff(&fx, &["-n", "1", "map", "--json"]);
    assert!(!out.status.success(), "root scope must not ride a verb");
    assert_eq!(json(&out)["error"]["id"].as_str(), Some("usage/bad-flags"));
}

/// A git or jj word fufu chose not to have is a question, not a typo, so it
/// gets an answer with an id rather than a parse error.
#[test]
fn foreign_verbs_are_answered_with_the_verb_that_replaced_them() {
    let fx = repo();
    for (verb, expected) in [
        ("checkout", "ff switch"),
        ("stash", "ff switch"),
        ("pull", "ff sync"),
        // A position rather than a gap: principle 12 names rebase over
        // merge, and the replay verbs are what fufu has instead.
        ("merge", "ff restack"),
        // Reads stay git's; what earns the entry is the half blame cannot
        // see, which is the work that is not history yet.
        ("blame", "ff evolog"),
        // Making a tag is git's. Losing one is not — refs/tags/ rides every
        // operation's ref table, so undo is the answer git has not got.
        ("tag", "ff undo"),
        // jj's two: a change is dropped by a different verb at each stage,
        // and a commit comes apart by closing slices rather than splitting.
        ("abandon", "ff restore --all"),
        ("split", "ff commit"),
    ] {
        let out = ff(&fx, &[verb]);
        assert_eq!(out.status.code(), Some(2), "ff {verb} exits 2");
        let said = stderr(&out);
        assert!(
            said.contains(expected),
            "ff {verb} should point at {expected}: {said}"
        );

        let out = ff(&fx, &[verb, "--json"]);
        assert_eq!(
            json(&out)["error"]["id"].as_str(),
            Some("usage/foreign-verb"),
            "ff {verb} carries the id"
        );
        assert_eq!(
            json(&out)["cmd"].as_str(),
            Some(verb),
            "the envelope names the word that was typed"
        );
    }
}

/// The house pattern from `ff branch <name>`: an exit that names what you
/// typed is one you can run, and a placeholder is one you have to finish.
#[test]
fn a_foreign_verb_carries_what_you_typed_into_its_exits() {
    // `--json` goes in front: everything after a foreign verb's first word
    // is its tail, flags included, which is what makes the passthrough exit
    // repeat the command faithfully.
    let fx = repo();
    let out = ff(&fx, &["--json", "checkout", "some-branch"]);
    let exits = json(&out)["error"]["exits"].to_string();
    assert!(
        exits.contains("ff switch some-branch"),
        "the target rides along: {exits}"
    );

    // A flag names nothing an exit could act on, so the placeholder stands.
    let out = ff(&fx, &["--json", "checkout", "-b"]);
    let exits = json(&out)["error"]["exits"].to_string();
    assert!(
        exits.contains("ff switch <branch>"),
        "a flag leaves the placeholder: {exits}"
    );

    // Where the answer is the passthrough, the whole tail rides along.
    let out = ff(&fx, &["--json", "tag", "v1"]);
    let exits = json(&out)["error"]["exits"].to_string();
    assert!(
        exits.contains("ff git tag v1"),
        "the passthrough exit is runnable: {exits}"
    );

    // jj's words fold what was typed in the same way: a path into the
    // close, a revision into the lift.
    let out = ff(&fx, &["--json", "split", "src/"]);
    let exits = json(&out)["error"]["exits"].to_string();
    assert!(
        exits.contains("ff commit src/"),
        "the path rides into the close: {exits}"
    );
    let out = ff(&fx, &["--json", "abandon", "abc123"]);
    let exits = json(&out)["error"]["exits"].to_string();
    assert!(
        exits.contains("ff lift --from abc123"),
        "the revision rides into the lift: {exits}"
    );
}

/// Every fufu verb captures first. A refusal is not a verb: capturing on
/// behalf of a command that does not exist would put a row on the log for
/// something that never ran.
#[test]
fn foreign_verbs_never_touch_the_repository() {
    let fx = repo();
    fx.write("a.txt", "dirty\n");
    let before = op_count(&fx);
    for verb in [
        "checkout", "stash", "pull", "merge", "blame", "tag", "abandon", "split",
    ] {
        assert!(!ff(&fx, &[verb]).status.success(), "ff {verb} refuses");
    }
    assert_eq!(before, op_count(&fx), "no capture, no operation");
}
