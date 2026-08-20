//! `ff clone` end to end, over gix's `file` transport.
//!
//! A path is a URL to the git protocol, so every case here is a real clone —
//! ref negotiation, a pack, a checkout — with nothing reaching the network.
//! What no local fixture can prove is the credential path, and that is stated
//! rather than faked: an HTTPS or SSH clone against a real remote is a manual
//! check, because a fake helper would only prove the fake works.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use ff_testsupport::fixtures::null_device;

/// A bare remote holding `n` commits on `main`, plus the tempdir it lives in.
struct Remote {
    _tmp: tempfile::TempDir,
    root: PathBuf,
    path: PathBuf,
}

impl Remote {
    fn with_commits(count: usize) -> Self {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path().to_path_buf();
        let path = root.join("remote.git");
        git(&root, &["init", "-q", "--bare", "-b", "main", "remote.git"]);
        if count > 0 {
            let seed = root.join("seed");
            std::fs::create_dir(&seed).unwrap();
            git(&seed, &["init", "-q", "-b", "main"]);
            for n in 0..count {
                std::fs::write(seed.join("a.txt"), format!("{n}\n")).unwrap();
                git(&seed, &["add", "-A"]);
                git(&seed, &["commit", "-q", "-m", &format!("commit {n}")]);
            }
            git(&seed, &["branch", "release"]);
            git(
                &seed,
                &["push", "-q", path.to_str().unwrap(), "main", "release"],
            );
        }
        Remote {
            _tmp: tmp,
            root,
            path,
        }
    }

    fn url(&self) -> String {
        self.path.to_string_lossy().into_owned()
    }
}

fn env(cmd: &mut Command) -> &mut Command {
    cmd.env("GIT_CONFIG_GLOBAL", null_device())
        .env("GIT_CONFIG_SYSTEM", null_device())
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_AUTHOR_NAME", "Clone Author")
        .env("GIT_AUTHOR_EMAIL", "author@clone.test")
        .env("GIT_COMMITTER_NAME", "Clone Committer")
        .env("GIT_COMMITTER_EMAIL", "committer@clone.test")
        .env_remove("FF_SESSION")
}

fn git(dir: &Path, args: &[&str]) -> String {
    let mut cmd = Command::new("git");
    let out = env(cmd.current_dir(dir).args(args))
        .output()
        .expect("spawn git");
    assert!(
        out.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).expect("utf-8").trim().into()
}

fn ff(dir: &Path, args: &[&str]) -> Output {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_ff"));
    env(cmd.current_dir(dir).args(args))
        .output()
        .expect("spawn ff")
}

fn stdout(out: &Output) -> String {
    String::from_utf8(out.stdout.clone()).expect("utf-8 stdout")
}

fn stderr(out: &Output) -> String {
    String::from_utf8(out.stderr.clone()).expect("utf-8 stderr")
}

fn ok(dir: &Path, args: &[&str]) -> String {
    let out = ff(dir, args);
    assert!(out.status.success(), "ff {args:?} failed: {}", stderr(&out));
    stdout(&out)
}

fn err_id(out: &Output) -> String {
    let v: serde_json::Value = serde_json::from_str(&stdout(out)).expect("valid json envelope");
    v["error"]["id"].as_str().unwrap_or_default().to_string()
}

#[test]
fn a_clone_lands_armed_and_reports_in_fufus_vocabulary() {
    let remote = Remote::with_commits(3);
    let body = ok(&remote.root, &["clone", &remote.url(), "w"]);
    assert!(
        body.contains("cloned into ./w — 3 commits on main"),
        "commits and branch, not git's transcript: {body}"
    );
    assert!(body.contains("the net is on"), "{body}");

    let clone = remote.root.join("w");
    // The worktree really landed.
    assert_eq!(std::fs::read_to_string(clone.join("a.txt")).unwrap(), "2\n");
    // The gc guard, written before anything could expire it.
    assert_eq!(
        git(&clone, &["config", "--get", "gc.refs/fufu/*.reflogExpire"]),
        "never"
    );
    // The floor: one row, and the log's root note.
    let rows = ok(&clone, &["op", "log", "--captures"]);
    let rows: Vec<&str> = rows.lines().filter(|l| !l.trim().is_empty()).collect();
    assert_eq!(rows.len(), 1, "exactly the floor: {rows:#?}");
    assert!(rows[0].contains("note"), "{rows:#?}");
    // And the remote is configured the way anything downstream expects.
    assert_eq!(
        git(&clone, &["config", "--get", "remote.origin.url"]),
        remote.url()
    );
}

/// The directory is the URL's last path segment when none is named.
#[test]
fn the_target_directory_comes_from_the_url() {
    let remote = Remote::with_commits(1);
    let body = ok(&remote.root, &["clone", &remote.url()]);
    assert!(body.contains("cloned into ./remote"), "{body}");
    assert!(remote.root.join("remote/.git").is_dir());
}

/// A remote with nothing in it clones to an unborn branch and zero commits,
/// which is a true report rather than a failure.
#[test]
fn an_empty_remote_clones_to_zero_commits() {
    let remote = Remote::with_commits(0);
    let body = ok(&remote.root, &["clone", &remote.url(), "w"]);
    assert!(body.contains("0 commits on main"), "{body}");
    assert!(body.contains("the net is on"), "{body}");
    // Armed anyway: the floor is what makes the first commit here undoable.
    let clone = remote.root.join("w");
    assert_eq!(
        git(&clone, &["config", "--get", "gc.refs/fufu/*.reflogExpire"]),
        "never"
    );
}

/// `-b` checks out a branch instead of the remote's HEAD.
#[test]
fn a_named_branch_is_what_gets_checked_out() {
    let remote = Remote::with_commits(2);
    let body = ok(
        &remote.root,
        &["clone", &remote.url(), "w", "-b", "release"],
    );
    assert!(body.contains("on release"), "{body}");
    let clone = remote.root.join("w");
    assert_eq!(
        git(&clone, &["rev-parse", "--abbrev-ref", "HEAD"]),
        "release"
    );
}

/// `--depth` takes only the tip. A shallow clone is a shorter history, and
/// the count fufu reports is the history it actually has.
#[test]
fn depth_takes_only_the_last_commits() {
    let remote = Remote::with_commits(4);
    let body = ok(&remote.root, &["clone", &remote.url(), "w", "--depth", "1"]);
    assert!(
        body.contains("1 commit on main"),
        "singular, and one: {body}"
    );
    let clone = remote.root.join("w");
    assert!(clone.join(".git/shallow").exists(), "really shallow");
}

/// `-o` names the remote.
#[test]
fn the_remote_can_be_named() {
    let remote = Remote::with_commits(1);
    ok(
        &remote.root,
        &["clone", &remote.url(), "w", "-o", "upstream"],
    );
    let clone = remote.root.join("w");
    assert_eq!(
        git(&clone, &["config", "--get", "remote.upstream.url"]),
        remote.url()
    );
}

/// A target that already has something in it is refused, not merged into —
/// a failed clone removes the directory it built, and that must never be
/// somebody else's directory.
#[test]
fn a_non_empty_target_is_refused_and_left_alone() {
    let remote = Remote::with_commits(1);
    let target = remote.root.join("w");
    std::fs::create_dir(&target).unwrap();
    std::fs::write(target.join("mine.txt"), "mine\n").unwrap();

    let out = ff(&remote.root, &["clone", &remote.url(), "w", "--json"]);
    assert!(!out.status.success());
    assert_eq!(err_id(&out), "clone/target-exists");
    assert_eq!(
        std::fs::read_to_string(target.join("mine.txt")).unwrap(),
        "mine\n",
        "the refusal touched nothing"
    );
}

/// An empty target directory is fine: git's own rule, and the shape you get
/// from `mkdir thing && ff clone <url> thing`.
#[test]
fn an_empty_target_directory_is_fine() {
    let remote = Remote::with_commits(1);
    std::fs::create_dir(remote.root.join("w")).unwrap();
    let body = ok(&remote.root, &["clone", &remote.url(), "w"]);
    assert!(body.contains("1 commit on main"), "{body}");
}

/// A URL that names no directory is refused before anything is created.
#[test]
fn a_url_with_no_directory_in_it_is_refused() {
    let tmp = tempfile::TempDir::new().unwrap();
    let out = ff(tmp.path(), &["clone", "https://example.com/", "--json"]);
    assert!(!out.status.success());
    assert_eq!(err_id(&out), "clone/bad-url");
}

/// A remote that is not there is reported as such, and leaves nothing behind
/// — `PrepareFetch` takes the half-built directory with it.
#[test]
fn a_remote_that_is_not_there_leaves_nothing_behind() {
    let tmp = tempfile::TempDir::new().unwrap();
    let missing = tmp.path().join("nope.git");
    let out = ff(
        tmp.path(),
        &["clone", missing.to_str().unwrap(), "w", "--json"],
    );
    assert!(!out.status.success());
    assert!(
        matches!(err_id(&out).as_str(), "clone/refused" | "clone/unreachable"),
        "one of the two wire failures: {}",
        err_id(&out)
    );
    assert!(!tmp.path().join("w").exists(), "nothing half-built is left");
}

/// A branch the remote does not have is the remote answering and saying no,
/// not a URL fufu could not read.
#[test]
fn a_branch_the_remote_does_not_have_is_refused() {
    let remote = Remote::with_commits(1);
    let out = ff(
        &remote.root,
        &["clone", &remote.url(), "w", "-b", "nope", "--json"],
    );
    assert!(!out.status.success());
    assert_eq!(err_id(&out), "clone/refused");
    assert!(
        !remote.root.join("w").exists(),
        "nothing half-built is left"
    );
}

/// The machine envelope carries what the prose reports from, plus the two
/// facts only a clone has: where it came from, and what the remote is called.
#[test]
fn the_json_envelope_carries_the_clone() {
    let remote = Remote::with_commits(2);
    let body = ok(&remote.root, &["clone", &remote.url(), "w", "--json"]);
    let v: serde_json::Value = serde_json::from_str(&body).expect("valid json");
    assert_eq!(v["cmd"], "clone");
    assert_eq!(v["data"]["url"], remote.url());
    assert_eq!(v["data"]["remote"], "origin");
    assert_eq!(v["data"]["branch"], "main");
    assert_eq!(v["data"]["commits"], 2);
    assert_eq!(v["data"]["created"], true);
    assert!(v["data"]["floor"].is_string(), "a floor was taken");
}

/// `--json` means no progress line: the envelope is the whole of stdout, and
/// stderr stays empty so a caller reading both is not surprised.
#[test]
fn json_draws_no_progress() {
    let remote = Remote::with_commits(2);
    let out = ff(&remote.root, &["clone", &remote.url(), "w", "--json"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert_eq!(stderr(&out), "", "no progress under --json");
}
