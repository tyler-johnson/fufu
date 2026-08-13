//! Hook execution — gix has none, so fufu runs them itself, exactly the two
//! `ff commit` owes behavioral compatibility for: `pre-commit` and
//! `commit-msg`. Hooks resolve through `core.hooksPath` (default
//! `<common-dir>/hooks`), non-executable or missing hooks are skipped
//! silently, and a non-zero exit aborts the close. These are sanctioned
//! spawns, like trim's `gc --auto`.

use std::path::PathBuf;

use crate::error::{Error, Result};

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
    use std::os::unix::fs::PermissionsExt;
    if !md.is_file() || md.permissions().mode() & 0o111 == 0 {
        return Ok(None);
    }
    Ok(Some(hook))
}

/// Run a hook from the worktree root with the given arguments. `Ok(false)`
/// = no hook to run; `Ok(true)` = ran and succeeded; a non-zero exit is an
/// error carrying the hook's name.
fn run(repo: &gix::Repository, name: &str, args: &[&std::ffi::OsStr]) -> Result<bool> {
    let Some(hook) = find_hook(repo, name)? else {
        return Ok(false);
    };
    let workdir = repo
        .workdir()
        .ok_or_else(|| Error::msg("bare repository: no worktree for hooks"))?;
    let mut prepare = gix::command::prepare(hook)
        .with_shell()
        .with_context(repo.command_context().map_err(Error::repo)?);
    prepare.args = args.iter().map(|a| a.to_os_string()).collect();
    let mut cmd: std::process::Command = prepare.into();
    cmd.current_dir(workdir)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::inherit())
        .stderr(std::process::Stdio::inherit());
    let status = cmd
        .status()
        .map_err(|err| Error::msg(format!("could not run {name} hook: {err}")))?;
    if !status.success() {
        return Err(Error::msg(format!("{name} hook declined the commit")));
    }
    Ok(true)
}

/// Run `pre-commit`. Returns whether a hook actually ran (the caller
/// re-scans the tree if so — hooks format files).
pub fn pre_commit(repo: &gix::Repository) -> Result<bool> {
    run(repo, "pre-commit", &[])
}

/// Run `commit-msg` against `message`, returning the (possibly rewritten)
/// message. The hook gets the message in a temp file it may edit in place.
pub fn commit_msg(repo: &gix::Repository, message: &str) -> Result<String> {
    if find_hook(repo, "commit-msg")?.is_none() {
        return Ok(message.to_string());
    }
    let dir = repo.git_dir();
    let path = dir.join(format!("COMMIT_EDITMSG.fufu-{}", std::process::id()));
    std::fs::write(&path, message).map_err(Error::repo)?;
    let result = run(repo, "commit-msg", &[path.as_os_str()]);
    let rewritten = std::fs::read_to_string(&path);
    let _ = std::fs::remove_file(&path);
    result?;
    rewritten.map_err(Error::repo)
}
