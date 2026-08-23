//! Verbatim execution of an external program. For git the process is
//! replaced on unix (`exec`), so its exit code, signals, and terminal
//! behavior are exactly its own; the same seam now serves PATH extensions.
//! Returns only on failure: 126 = could not exec, 127 = program not found.

use std::ffi::OsString;

use ff_core::Result;

#[cfg(unix)]
pub fn exec(program: &str, args: Vec<OsString>) -> Result<()> {
    use std::os::unix::process::CommandExt;
    let err = std::process::Command::new(program).args(&args).exec();
    let code = if err.kind() == std::io::ErrorKind::NotFound {
        eprintln!("ff: {program} not found on PATH");
        127
    } else {
        eprintln!("ff: failed to exec {program}: {err}");
        126
    };
    std::process::exit(code);
}

/// Child-proxy seam for platforms without exec: spawn, wait, mirror the code.
#[cfg(not(unix))]
pub fn exec(program: &str, args: Vec<OsString>) -> Result<()> {
    match std::process::Command::new(program).args(&args).status() {
        Ok(status) => std::process::exit(status.code().unwrap_or(1)),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            eprintln!("ff: {program} not found on PATH");
            std::process::exit(127);
        }
        Err(err) => {
            eprintln!("ff: failed to run {program}: {err}");
            std::process::exit(126);
        }
    }
}

/// Spawn-and-wait sibling of `exec`, for when ff must speak after the
/// program: runs it as a child, mirrors its exit code (127 not-found /
/// 126 failed, matching exec's codes), and RETURNS instead of exiting so
/// the caller can print before terminating the process.
pub fn run_wait(program: &str, args: Vec<OsString>) -> i32 {
    match std::process::Command::new(program).args(&args).status() {
        Ok(status) => status.code().unwrap_or(1),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            eprintln!("ff: {program} not found on PATH");
            127
        }
        Err(err) => {
            eprintln!("ff: failed to run {program}: {err}");
            126
        }
    }
}
