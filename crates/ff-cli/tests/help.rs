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
    assert!(body.contains("Commands:"), "missing Commands list");
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
