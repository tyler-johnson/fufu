//! Hook execution — gix has none, so fufu runs git's four commit-time hooks
//! itself: `pre-commit`, `prepare-commit-msg`, `commit-msg` and
//! `post-commit`. Hooks resolve through `core.hooksPath` (default
//! `<common-dir>/hooks`), non-executable or missing hooks are skipped
//! silently, and a non-zero exit aborts the verb. These are sanctioned
//! spawns, like trim's `gc --auto`.
//!
//! One rule decides which verb runs which: the tree hook runs where worktree
//! content becomes commit content, and the message hooks run where a message
//! is authored for a commit. So `pre-commit` guards `ff commit`, `ff absorb`
//! and both of `ff done`'s landings; the message hooks run for `ff commit`,
//! `ff describe <rev>` and a `ff done` whose session carries a new
//! description. `post-commit` stays on `ff commit` alone — git fires it from
//! `git commit` and not from `rebase`, and every other verb here is a
//! rebase.

use std::path::PathBuf;

use crate::error::{Error, Result};

/// Whether a verb runs the hooks `--no-verify` suppresses.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Verify {
    /// Run every hook the verb owes.
    #[default]
    Run,
    /// `--no-verify`.
    Skip,
}

impl Verify {
    /// Whether this run reaches `name`. `--no-verify` suppresses `pre-commit`
    /// and `commit-msg` and nothing else: githooks(5) is explicit that
    /// `prepare-commit-msg` is not skipped by it, and `post-commit` is a
    /// notification rather than a gate.
    fn runs(self, name: &str) -> bool {
        self == Verify::Run || !matches!(name, "pre-commit" | "commit-msg")
    }
}

/// `prepare-commit-msg`'s second argument: where the message came from.
pub enum MsgSource<'a> {
    /// `-m`, or a pending description — text the user already supplied.
    Message,
    /// An amend-shaped rewrite: the commit whose message is being reworked,
    /// passed on as the hook's third argument the way git does for
    /// `commit --amend`.
    Commit(&'a str),
    /// No source applies, so the hook gets the message file alone — which is
    /// what git does when it has nothing to say about where the text came
    /// from.
    Unspecified,
}

/// Resolve a hook by name to a runnable path, or `None` to skip.
fn find_hook(repo: &gix::Repository, name: &str) -> Result<Option<PathBuf>> {
    let dir = match repo.config_snapshot().trusted_path("core.hooksPath") {
        Some(Ok(path)) => {
            let path = path.into_owned();
            if path.is_absolute() {
                path
            } else {
                // Relative hooksPath is relative to the working directory.
                match repo.workdir() {
                    Some(workdir) => workdir.join(path),
                    None => repo.common_dir().join(path),
                }
            }
        }
        Some(Err(err)) => return Err(Error::msg(format!("core.hooksPath: {err}"))),
        None => repo.common_dir().join("hooks"),
    };
    let hook = dir.join(name);
    let Ok(md) = std::fs::metadata(&hook) else {
        return Ok(None);
    };
    // Unix skips non-executable hooks; Windows has no exec bit, so any
    // regular file runs — the same split git itself makes.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if !md.is_file() || md.permissions().mode() & 0o111 == 0 {
            return Ok(None);
        }
    }
    #[cfg(not(unix))]
    if !md.is_file() {
        return Ok(None);
    }
    Ok(Some(hook))
}

/// Whether any of `names` is going to run at all. A verb populates the index
/// before its first hook so hook-runners keyed on staging see the change
/// staged, and that work — a tree assembly and two index writes — is worth
/// nothing in a repo with no hooks; gating on this costs one `stat` per
/// name. Each verb passes the hooks it will actually run, `--no-verify`
/// already subtracted, so the window opens exactly when something will use
/// it.
pub fn will_run(repo: &gix::Repository, names: &[&str]) -> Result<bool> {
    for name in names {
        if find_hook(repo, name)?.is_some() {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Run a hook from the worktree root with the given arguments. `Ok(false)`
/// = no hook to run; `Ok(true)` = ran and succeeded; a non-zero exit is an
/// error carrying the hook's name, whose exit is `verb`'s own `--no-verify`
/// spelling — `verb` is the bare verb name, `commit` or `absorb`.
fn run(repo: &gix::Repository, name: &str, args: &[&std::ffi::OsStr], verb: &str) -> Result<bool> {
    let Some(hook) = find_hook(repo, name)? else {
        return Ok(false);
    };
    // The hook path is spliced into an `sh -c` command line, where
    // backslashes are escapes: hand sh POSIX separators, as git does.
    #[cfg(windows)]
    let hook = std::path::PathBuf::from(hook.to_string_lossy().replace('\\', "/"));
    let workdir = repo.workdir().ok_or_else(|| {
        Error::coded(
            "repo/bare",
            "bare repository: no worktree for hooks",
            vec![],
        )
    })?;
    let mut prepare = gix::command::prepare(hook)
        .with_shell()
        .with_context(repo.command_context().map_err(Error::repo)?);
    prepare.args = args.iter().map(|a| a.to_os_string()).collect();
    let mut cmd: std::process::Command = prepare.into();
    cmd.current_dir(workdir)
        // git sets this for every commit hook it runs from a command that
        // will not bring an editor up. fufu never opens one at close time,
        // so it is always set.
        .env("GIT_EDITOR", ":")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::inherit())
        .stderr(std::process::Stdio::inherit());
    let status = cmd
        .status()
        .map_err(|err| Error::msg(format!("could not run {name} hook: {err}")))?;
    if !status.success() {
        return Err(Error::coded(
            "hook/declined",
            format!("{name} hook declined the commit"),
            vec![format!("ff {verb} --no-verify")],
        ));
    }
    Ok(true)
}

/// Run `pre-commit`. Returns whether a hook actually ran (the caller
/// re-scans the tree if so — hooks format files).
fn pre_commit(repo: &gix::Repository, verb: &str) -> Result<bool> {
    run(repo, "pre-commit", &[], verb)
}

/// Run `post-commit`. Notification only: a missing hook, a spawn failure and
/// a non-zero exit are all indistinguishable to the caller, exactly as under
/// git, so this returns nothing and cannot fail the operation.
pub fn post_commit(repo: &gix::Repository) {
    let _ = run(repo, "post-commit", &[], "commit");
}

/// The message file both message hooks edit in place, deleted on drop.
/// Pid-suffixed rather than git's own `COMMIT_EDITMSG`: two fufu processes
/// in one repository must not edit each other's message, which is worth more
/// than the handful of hooks that hardcode git's path.
struct MessageFile {
    path: PathBuf,
}

impl MessageFile {
    fn write(repo: &gix::Repository, message: &str) -> Result<Self> {
        let path = repo
            .git_dir()
            .join(format!("COMMIT_EDITMSG.fufu-{}", std::process::id()));
        std::fs::write(&path, message).map_err(Error::repo)?;
        Ok(Self { path })
    }

    fn read(&self) -> Result<String> {
        std::fs::read_to_string(&self.path).map_err(Error::repo)
    }
}

impl Drop for MessageFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

/// Run `prepare-commit-msg` then `commit-msg` over one file, returning the
/// message as they left it — the way git hands one `COMMIT_EDITMSG` to both.
/// `verify` gates `commit-msg` only: `prepare-commit-msg` is not suppressed
/// by `--no-verify`, as git documents.
pub fn message_hooks(
    repo: &gix::Repository,
    message: &str,
    source: MsgSource<'_>,
    verify: Verify,
    verb: &str,
) -> Result<String> {
    let prepare = find_hook(repo, "prepare-commit-msg")?.is_some();
    let finish = verify.runs("commit-msg") && find_hook(repo, "commit-msg")?.is_some();
    if !prepare && !finish {
        return Ok(message.to_string());
    }
    let file = MessageFile::write(repo, message)?;
    let path = file.path.clone().into_os_string();
    if prepare {
        let mut args: Vec<std::ffi::OsString> = vec![path.clone()];
        match source {
            MsgSource::Message => args.push("message".into()),
            MsgSource::Commit(sha) => {
                args.push("commit".into());
                args.push(sha.into());
            }
            MsgSource::Unspecified => {}
        }
        let args: Vec<&std::ffi::OsStr> = args.iter().map(std::ffi::OsString::as_os_str).collect();
        run(repo, "prepare-commit-msg", &args, verb)?;
    }
    if finish {
        run(repo, "commit-msg", &[path.as_os_str()], verb)?;
    }
    file.read()
}

/// The index window a `pre-commit` gate runs inside. `staged` is the tree the
/// verb is about to make commit content — hook-runners keyed on
/// `git diff --cached` see exactly that. `differs` names the paths the
/// worktree will still hold differently afterwards, so their stat data is not
/// carried over (see [`crate::index::write_index_for_tree_except`]).
///
/// The index is restored byte-for-byte on drop unless [`Window::landed`] is
/// called, so every `?` on the way out already rolls back — git's own
/// behavior when a commit does not land.
///
/// Callers gate on [`will_run`] before opening one: assembling the staged
/// tree is real work in a repository with no hooks.
pub struct Window {
    backup: crate::index::IndexBackup,
}

impl Window {
    /// Stage `staged`, then run `pre-commit`. The second result is true when
    /// a hook actually ran, and the caller must then re-read the worktree:
    /// pre-commit hooks format files.
    pub fn open(
        repo: &gix::Repository,
        staged: gix::ObjectId,
        differs: &[String],
        verify: Verify,
        verb: &str,
    ) -> Result<(Self, bool)> {
        let backup = crate::index::IndexBackup::take(repo)?;
        crate::index::write_index_for_tree_except(repo, staged, differs)?;
        let window = Self { backup };
        // Bound before the hook runs: a decline drops the window and the
        // index goes back.
        let ran = if verify.runs("pre-commit") {
            pre_commit(repo, verb)?
        } else {
            false
        };
        Ok((window, ran))
    }

    /// The verb landed: the staged index is no longer provisional, and
    /// putting the old one back would contradict the refs that just moved.
    pub fn landed(self) {
        self.backup.disarm();
    }
}
