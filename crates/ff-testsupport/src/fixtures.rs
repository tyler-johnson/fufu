//! Hermetic git repository fixtures, driven by the real `git` binary.
//!
//! Every spawned git gets an explicit environment (no global/system config, fixed
//! identities) and a monotone clock: each invocation advances a per-fixture date
//! counter by 60 seconds, so commit timestamps are strictly increasing and
//! time-sorted log order is deterministic.

use std::cell::Cell;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{Duration, SystemTime};

/// Fixed base for the monotone commit clock.
const EPOCH: i64 = 1_600_000_000;

pub struct Fixture {
    root: tempfile::TempDir,
    clock: Cell<i64>,
}

impl Fixture {
    /// A fresh repository on branch `main` with an unborn HEAD.
    pub fn new() -> Self {
        let fx = Self::empty();
        fx.git(&["init", "-q", "-b", "main"]);
        fx
    }

    /// A fresh bare repository on branch `main`.
    pub fn new_bare() -> Self {
        let fx = Self::empty();
        fx.git(&["init", "-q", "--bare", "-b", "main"]);
        fx
    }

    fn empty() -> Self {
        let root = tempfile::TempDir::new().expect("create fixture tempdir");
        std::fs::create_dir_all(root.path().join("home/.config")).expect("create fixture home");
        std::fs::create_dir(root.path().join("repo")).expect("create fixture repo dir");
        Fixture {
            root,
            clock: Cell::new(EPOCH),
        }
    }

    /// The repository directory (worktree root, or the bare repo itself).
    pub fn path(&self) -> PathBuf {
        self.root.path().join("repo")
    }

    /// The directory containing the repository — a place for linked worktrees.
    pub fn root(&self) -> &Path {
        self.root.path()
    }

    /// Run git in the repository, panicking on failure. Advances the fixture clock.
    pub fn git(&self, args: &[&str]) -> String {
        self.git_in(&self.path(), args)
    }

    /// Run git in an arbitrary directory (e.g. a linked worktree), panicking on failure.
    pub fn git_in(&self, cwd: &Path, args: &[&str]) -> String {
        let out = self.try_git_in(cwd, args);
        if !out.status.success() {
            panic!(
                "git {:?} failed with {}\nstdout: {}\nstderr: {}",
                args,
                out.status,
                String::from_utf8_lossy(&out.stdout),
                String::from_utf8_lossy(&out.stderr),
            );
        }
        String::from_utf8(out.stdout).expect("git output is utf-8")
    }

    /// Run git in the repository, returning the raw output without checking status.
    pub fn try_git(&self, args: &[&str]) -> Output {
        self.try_git_in(&self.path(), args)
    }

    /// Run git with extra environment variables (e.g. `GIT_INDEX_FILE`),
    /// panicking on failure. Advances the fixture clock.
    pub fn git_env_in(&self, cwd: &Path, args: &[&str], envs: &[(&str, &str)]) -> String {
        let out = self.try_git_env_in(cwd, args, envs);
        if !out.status.success() {
            panic!(
                "git {:?} (env {:?}) failed with {}\nstdout: {}\nstderr: {}",
                args,
                envs,
                out.status,
                String::from_utf8_lossy(&out.stdout),
                String::from_utf8_lossy(&out.stderr),
            );
        }
        String::from_utf8(out.stdout).expect("git output is utf-8")
    }

    /// Set a repository-local config value.
    pub fn set_config(&self, key: &str, value: &str) {
        self.git(&["config", key, value]);
    }

    /// Run git with the hermetic environment. Advances the fixture clock.
    pub fn try_git_in(&self, cwd: &Path, args: &[&str]) -> Output {
        self.try_git_env_in(cwd, args, &[])
    }

    fn try_git_env_in(&self, cwd: &Path, args: &[&str], envs: &[(&str, &str)]) -> Output {
        self.clock.set(self.clock.get() + 60);
        let date = format!("@{} +0000", self.clock.get());
        let home = self.root.path().join("home");
        let mut cmd = Command::new("git");
        cmd.current_dir(cwd)
            .args(args)
            .env_clear()
            .env("PATH", std::env::var_os("PATH").unwrap_or_default())
            .env("HOME", &home)
            .env("XDG_CONFIG_HOME", home.join(".config"))
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_AUTHOR_NAME", "Fixture Author")
            .env("GIT_AUTHOR_EMAIL", "author@fixture.test")
            .env("GIT_COMMITTER_NAME", "Fixture Committer")
            .env("GIT_COMMITTER_EMAIL", "committer@fixture.test")
            .env("GIT_AUTHOR_DATE", &date)
            .env("GIT_COMMITTER_DATE", &date)
            .env("LC_ALL", "C")
            .env("TZ", "UTC");
        for (key, value) in envs {
            cmd.env(key, value);
        }
        cmd.output().expect("spawn git")
    }

    /// Write a file (creating parent directories) relative to the repo.
    pub fn write(&self, rel: &str, contents: &str) {
        let p = self.path().join(rel);
        if let Some(parent) = p.parent() {
            std::fs::create_dir_all(parent).expect("create parent dirs");
        }
        std::fs::write(p, contents).expect("write fixture file");
    }

    /// Remove a file or directory relative to the repo.
    pub fn remove(&self, rel: &str) {
        let p = self.path().join(rel);
        if p.is_dir() {
            std::fs::remove_dir_all(p).expect("remove fixture dir");
        } else {
            std::fs::remove_file(p).expect("remove fixture file");
        }
    }

    /// Stage everything and commit with the next monotone timestamp; returns the commit id.
    pub fn commit(&self, msg: &str) -> String {
        self.git(&["add", "-A"]);
        self.git(&["commit", "-q", "--allow-empty", "-m", msg]);
        self.git(&["rev-parse", "HEAD"]).trim().to_string()
    }

    /// The raw bytes of `.git/index` (empty if no index exists yet).
    pub fn index_bytes(&self) -> Vec<u8> {
        index_bytes_at(&self.path())
    }

    /// Set every worktree file's mtime far into the past, killing racy-clean skew.
    pub fn backdate(&self) {
        let old = SystemTime::UNIX_EPOCH + Duration::from_secs((EPOCH - 86_400) as u64);
        backdate_dir(&self.path(), old);
    }

    /// Open the fixture with the isolated gix path used by all differential tests.
    pub fn repo(&self) -> ff_core::gix::Repository {
        ff_core::discover_isolated(self.path()).expect("discover fixture repo")
    }
}

impl Default for Fixture {
    fn default() -> Self {
        Self::new()
    }
}

/// Read the index bytes for a worktree directory, following `.git` files of
/// linked worktrees to their private index.
pub fn index_bytes_at(worktree: &Path) -> Vec<u8> {
    let dot_git = worktree.join(".git");
    let git_dir = if dot_git.is_file() {
        let contents = std::fs::read_to_string(&dot_git).expect("read .git file");
        let rel = contents
            .strip_prefix("gitdir:")
            .expect(".git file has gitdir line")
            .trim();
        let p = PathBuf::from(rel);
        if p.is_absolute() { p } else { worktree.join(p) }
    } else {
        dot_git
    };
    std::fs::read(git_dir.join("index")).unwrap_or_default()
}

fn backdate_dir(dir: &Path, old: SystemTime) {
    for entry in std::fs::read_dir(dir).expect("read dir for backdate") {
        let entry = entry.expect("dir entry");
        if entry.file_name() == ".git" {
            continue;
        }
        let ty = entry.file_type().expect("file type");
        if ty.is_dir() {
            backdate_dir(&entry.path(), old);
        } else if ty.is_file() {
            let f = std::fs::File::options()
                .append(true)
                .open(entry.path())
                .expect("open for backdate");
            f.set_modified(old).expect("set mtime");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn init_is_unborn() {
        let fx = Fixture::new();
        let head = fx.git(&["symbolic-ref", "HEAD"]);
        assert_eq!(head.trim(), "refs/heads/main");
        let out = fx.try_git(&["rev-parse", "--verify", "HEAD"]);
        assert!(!out.status.success(), "HEAD must be unborn after init");
        assert!(
            fx.index_bytes().is_empty(),
            "no index file before first add"
        );
    }

    #[test]
    fn commits_have_monotone_dates() {
        let fx = Fixture::new();
        fx.write("a.txt", "a");
        let c1 = fx.commit("one");
        fx.write("b.txt", "b");
        let c2 = fx.commit("two");
        assert_ne!(c1, c2);
        let t1: i64 = fx
            .git(&["log", "-1", "--format=%at", &c1])
            .trim()
            .parse()
            .unwrap();
        let t2: i64 = fx
            .git(&["log", "-1", "--format=%at", &c2])
            .trim()
            .parse()
            .unwrap();
        assert!(t2 > t1, "commit dates must be strictly increasing");
        assert!(!fx.index_bytes().is_empty(), "index exists after commit");
    }

    #[test]
    fn backdate_moves_mtimes() {
        let fx = Fixture::new();
        fx.write("deep/nested/f.txt", "x");
        fx.backdate();
        let meta = std::fs::metadata(fx.path().join("deep/nested/f.txt")).unwrap();
        let age = SystemTime::now()
            .duration_since(meta.modified().unwrap())
            .unwrap();
        assert!(
            age > Duration::from_secs(3600),
            "mtime should be far in the past"
        );
    }

    #[test]
    fn repo_opens_isolated() {
        let fx = Fixture::new();
        fx.write("a.txt", "a");
        fx.commit("one");
        let repo = fx.repo();
        assert!(repo.workdir().is_some());
    }
}
