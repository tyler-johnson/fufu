//! Verbatim git execution. On unix the process is replaced (`exec`), so
//! git's exit code, signals, and terminal behavior are exactly git's own.
//! Returns only on failure: 126 = could not exec, 127 = git not found.

use std::ffi::OsString;

use ff_core::Result;

#[cfg(unix)]
pub fn exec(args: Vec<OsString>) -> Result<()> {
    use std::os::unix::process::CommandExt;
    let err = std::process::Command::new("git").args(&args).exec();
    let code = if err.kind() == std::io::ErrorKind::NotFound {
        eprintln!("ff: git not found on PATH");
        127
    } else {
        eprintln!("ff: failed to exec git: {err}");
        126
    };
    std::process::exit(code);
}

/// Child-proxy seam for platforms without exec: spawn, wait, mirror the code.
#[cfg(not(unix))]
pub fn exec(args: Vec<OsString>) -> Result<()> {
    match std::process::Command::new("git").args(&args).status() {
        Ok(status) => std::process::exit(status.code().unwrap_or(1)),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            eprintln!("ff: git not found on PATH");
            std::process::exit(127);
        }
        Err(err) => {
            eprintln!("ff: failed to run git: {err}");
            std::process::exit(126);
        }
    }
}

/// Spawn-and-wait sibling of `exec`, for when ff must speak after git:
/// runs git as a child, mirrors its exit code (127 not-found / 126 failed,
/// matching exec's codes), and RETURNS instead of exiting so the caller can
/// print before terminating the process.
pub fn run_wait(args: Vec<OsString>) -> i32 {
    match std::process::Command::new("git").args(&args).status() {
        Ok(status) => status.code().unwrap_or(1),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            eprintln!("ff: git not found on PATH");
            127
        }
        Err(err) => {
            eprintln!("ff: failed to run git: {err}");
            126
        }
    }
}
