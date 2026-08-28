//! Help pages: the help subcommand resolves at depth and every command
//! has a page with examples.

use std::process::{Command, Output};

fn ff(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_ff"))
        .args(args)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
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

#[test]
fn bare_help_prints_the_root_page() {
    let out = ff(&["help"]);
    assert!(out.status.success(), "exit 0: {:?}", out.status);
    let body = stdout(&out);
    assert!(body.contains("Usage: ff"), "missing Usage line");
    // The command list is grouped, git-style, so the headings are the list.
    for heading in [
        "start a working area:",
        "work on the current change:",
        "examine the history and state:",
        "grow, mark and tweak your common history:",
        "collaborate:",
        "go back:",
        "wire it in, and check on it:",
        "fufu itself:",
    ] {
        assert!(body.contains(heading), "missing heading {heading:?}");
    }
    assert!(
        body.contains("snapshots your working tree"),
        "missing root page fragment"
    );
}

#[test]
fn help_command_equals_the_long_flag() {
    for cmd in ["status", "restore", "commit", "config"] {
        let help_out = ff(&["help", cmd]);
        let flag_out = ff(&[cmd, "--help"]);
        assert_eq!(
            help_out.stdout, flag_out.stdout,
            "ff help {} != ff {} --help",
            cmd, cmd
        );
    }
}

#[test]
fn help_resolves_nested_subcommands() {
    let out = ff(&["help", "op"]);
    assert!(out.status.success(), "exit 0: {:?}", out.status);
    let body = stdout(&out);
    assert!(body.contains("Usage: ff op"), "missing usage line");

    let out = ff(&["help", "op", "log"]);
    assert!(out.status.success(), "exit 0: {:?}", out.status);
    let body = stdout(&out);
    assert!(
        body.contains("Usage: ff op log"),
        "missing nested usage line"
    );
}

#[test]
fn every_command_has_a_page() {
    let commands = [
        "map", "status", "collide", "diff", "show", "log", "history", "evolog", "git", "restore",
        "trim", "commit", "switch", "undo", "redo", "op", "new", "describe", "branch", "hook",
        "unhook", "trigger", "config", "doctor", "update", "resolve", "init", "clone", "remote",
        "version", "worktree",
    ];
    for cmd in &commands {
        let out = ff(&["help", cmd]);
        assert!(out.status.success(), "ff help {} exits 0", cmd);
        let body = stdout(&out);
        assert!(
            body.contains("Examples:"),
            "ff help {} missing Examples:",
            cmd
        );
    }
}

#[test]
fn help_for_git_does_not_reach_git() {
    // See tests/git_passthrough.rs::help_reaches_git_not_clap for the
    // sibling contract that ff git --help still reaches real git.
    let out = ff(&["help", "git"]);
    let body = stdout(&out);
    assert!(body.contains("Snapshots first"), "missing fufu about line");
    assert!(body.contains("alias git="), "missing alias mention");
}

/// One kind per flag, and the page has to say which is which — the whole
/// point of splitting `--at` was that a reader never guesses from the shape
/// of what they typed.
#[test]
fn restore_page_names_all_three_sources() {
    let out = ff(&["help", "restore"]);
    let body = stdout(&out);
    for flag in ["--from <rev>", "--at-op <op>", "--at <time>"] {
        assert!(body.contains(flag), "missing {flag}: {body}");
    }
    // `@{n}` counted positions on one branch's reflog; `--at-op @^` says the
    // same thing in the address space that owns the question.
    assert!(!body.contains("@{"), "the reflog spelling is gone");
}

/// The command list above is of commands, so a new flag adds no page. `-r`
/// still has to be taught somewhere, and the log page is that somewhere.
#[test]
fn log_page_teaches_the_revset_flag() {
    let out = ff(&["help", "log"]);
    let body = stdout(&out);
    assert!(body.contains("--revisions"), "missing the long spelling");
    assert!(body.contains("revset"), "missing the word revset");
    assert!(
        body.contains("ff log -r main"),
        "missing a -r example: {body}"
    );
}

#[test]
fn unknown_help_target_fails() {
    let out = ff(&["help", "bogus"]);
    assert!(!out.status.success(), "should exit non-zero");
    let err = stderr(&out);
    assert!(
        err.contains("unrecognized subcommand"),
        "stderr should mention unrecognized subcommand: {}",
        err
    );
}

#[test]
fn short_help_stays_short() {
    let out_short = ff(&["restore", "-h"]);
    let body_short = stdout(&out_short);
    assert!(
        !body_short.contains("Examples:"),
        "short help (-h) should not contain Examples:"
    );

    let out_long = ff(&["restore", "--help"]);
    let body_long = stdout(&out_long);
    assert!(
        body_long.contains("Examples:"),
        "long help (--help) should contain Examples:"
    );
}

/// The command list on the root page: everything between the usage line and
/// the options, which is what the grouping replaced clap's flat `Commands:`
/// section with.
fn command_list(page: &str) -> &str {
    page.split_once("Usage: ff")
        .and_then(|(_, rest)| rest.split_once("\nOptions:"))
        .map(|(list, _)| list)
        .expect("a command list")
}

/// One row per command: the name, and the column its description starts at.
fn rows(list: &str) -> Vec<(&str, usize)> {
    list.lines()
        .filter(|line| line.starts_with("  "))
        .filter_map(|line| {
            let name = line[2..].split_whitespace().next()?;
            let gap = line.find(name)? + name.len();
            let about = line[gap..].trim_start();
            // The footer is indented like a row and has no description
            // column; a row always has one.
            let start = line.len() - about.len();
            (!about.is_empty() && line[gap..].starts_with("  ")).then_some((name, start))
        })
        .collect()
}

/// `-h` is the common verbs and nothing else, and it has to say where the
/// rest went — otherwise a reader has no way to learn a command is missing.
#[test]
fn short_root_help_is_a_subset_that_names_the_long_one() {
    let short = stdout(&ff(&["-h"]));
    let long = stdout(&ff(&["--help"]));

    let short_rows: Vec<&str> = rows(command_list(&short))
        .into_iter()
        .map(|(name, _)| name)
        .collect();
    let long_rows: Vec<&str> = rows(command_list(&long))
        .into_iter()
        .map(|(name, _)| name)
        .collect();

    assert!(!short_rows.is_empty(), "the short page lists nothing");
    assert!(
        short_rows.len() < long_rows.len(),
        "the short page should be shorter: {short_rows:?}"
    );
    for name in &short_rows {
        assert!(
            long_rows.contains(name),
            "{name} is on the short page but not the long one: {long_rows:?}"
        );
    }
    assert!(
        short.contains("ff --help"),
        "the short page should name `ff --help`: {short}"
    );
}

/// One name column for the whole list, not one per heading — a column that
/// moved under each heading would undo the grouping it is there to serve.
#[test]
fn the_command_list_aligns_across_headings() {
    for flag in ["-h", "--help"] {
        let page = stdout(&ff(&[flag]));
        let rows = rows(command_list(&page));
        let (first, start) = rows.first().copied().expect("at least one row");
        for (name, col) in &rows {
            assert_eq!(
                *col, start,
                "ff {flag}: {name}'s description starts at {col}, {first}'s at {start}"
            );
        }
    }
}

/// The list is painted by fufu rather than by clap, so it has to be painted
/// in clap's own styles: headings bold+underline like `Usage:`, names bold
/// like every other thing you can type.
#[test]
fn the_command_list_wears_claps_styles() {
    let out = Command::new(env!("CARGO_BIN_EXE_ff"))
        .args(["--help"])
        .env("CLICOLOR_FORCE", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .expect("spawn ff");
    let page = stdout(&out);
    assert!(
        page.contains("\u{1b}[1m\u{1b}[4mstart a working area:\u{1b}[0m"),
        "the heading should be bold+underline: {page:?}"
    );
    assert!(
        page.contains("\u{1b}[1minit\u{1b}[0m"),
        "the command name should be bold: {page:?}"
    );
    assert!(
        page.contains("\u{1b}[1m\u{1b}[4mOptions:\u{1b}[0m"),
        "the hand-written Options: heading should match: {page:?}"
    );

    // And nothing at all when the stream is captured, which is the state
    // every other test above reads the page in.
    let plain = stdout(&ff(&["--help"]));
    assert!(
        !plain.contains('\u{1b}'),
        "piped help carries no escape byte"
    );
}
