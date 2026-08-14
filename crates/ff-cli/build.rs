//! Emit `FF_BUILD_INFO` so `ff --version` can name the commit it came from.
//!
//! The value is either `" (<sha> <date>)"` or the empty string. An empty string
//! means the binary was built without git available (source tarball, crates.io
//! vendor, docker context without `.git`).
//!
//! **No `rerun-if-changed` directives.** Cargo re-runs the script whenever any
//! file in the `ff-cli` package changes, which is the behavior we want.
//! Watching `.git/HEAD` is the usual recipe and is wrong here: in a git
//! *worktree* `.git` is a file, not a directory, and the `--against` bench flow
//! builds precisely in worktrees.
//!
//! Known limit: a commit that touches only `ff-core` can leave the recorded sha
//! one commit stale in an incremental dev build. Release and bench builds are
//! clean, so they are exact.
//!
//! A git failure (missing `.git`, `git` not on `PATH`, non-zero exit,
//! non-UTF-8 output) is deliberately silent — the build succeeds with an empty
//! `FF_BUILD_INFO`.

use std::process::Command;

fn main() {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let dir = std::path::Path::new(&manifest_dir);

    let sha = run_git(dir, &["rev-parse", "--short=7", "HEAD"]);
    let date = run_git(dir, &["log", "-1", "--format=%cs"]);

    let info = if let (Some(sha), Some(date)) = (sha, date) {
        format!(" ({} {})", sha, date)
    } else {
        String::new()
    };

    println!("cargo::rustc-env=FF_BUILD_INFO={info}");
}

/// Run a `git` command in `dir`. Returns `None` on any failure instead of
/// aborting the build.
fn run_git(dir: &std::path::Path, args: &[&str]) -> Option<String> {
    let output = Command::new("git")
        .current_dir(dir)
        .args(args)
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let stdout = String::from_utf8(output.stdout).ok()?;
    stdout
        .trim_end()
        .lines()
        .next()
        .map(|l| l.trim().to_string())
}
