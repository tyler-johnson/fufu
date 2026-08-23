//! PATH extensions: `ff <name>` finds an executable `ff-<name>` on PATH the
//! way git finds its own. A builtin verb always wins — this module is
//! reached only after clap has already declined the word — and, exactly as
//! `ff git` does, fufu captures before handing over.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

/// Find `ff-<name>` on PATH. The name is validated before anything touches
/// the filesystem: it comes from a command line, and an unvalidated one
/// could build a path that escapes the PATH directory entirely (`ff
/// ../../evil`), so the rule is reject, never sanitize.
pub fn resolve(name: &str) -> Option<PathBuf> {
    let bytes = name.as_bytes();
    let valid = !bytes.is_empty()
        && bytes[0].is_ascii_alphanumeric()
        && bytes
            .iter()
            .all(|b| b.is_ascii_alphanumeric() || *b == b'-' || *b == b'_');
    if !valid {
        return None;
    }
    let path = std::env::var_os("PATH")?;
    let dir = std::env::split_paths(&path).filter(|entry| !entry.as_os_str().is_empty());
    for entry in dir {
        for file_name in file_names(name) {
            let candidate = entry.join(file_name);
            if is_executable(&candidate) {
                return Some(candidate);
            }
        }
    }
    None
}

#[cfg(unix)]
fn file_names(name: &str) -> [String; 1] {
    [format!("ff-{name}")]
}

#[cfg(not(unix))]
fn file_names(name: &str) -> [String; 2] {
    [format!("ff-{name}.exe"), format!("ff-{name}")]
}

#[cfg(unix)]
fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    match std::fs::metadata(path) {
        Ok(meta) => meta.is_file() && meta.permissions().mode() & 0o111 != 0,
        Err(_) => false,
    }
}

/// Best-effort off unix: a regular file is an executable, and `PATH`
/// already carries the `.exe` convention.
#[cfg(not(unix))]
fn is_executable(path: &Path) -> bool {
    path.is_file()
}

/// Everything after the extension's own word, with fufu's global flags
/// dropped. Nothing after that word is fufu's business any more — including
/// `--json`, which belongs to the extension once dispatch happens.
///
/// The scan steps over the value of a value-taking global rather than
/// comparing every token, because a value is not a verb: `ff --session tower
/// tower next` names a session and an extension that happen to spell the
/// same, and the word that dispatches is the second one.
pub fn rest_argv(name: &str) -> Vec<OsString> {
    let word = OsString::from(name);
    let mut argv = std::env::args_os().skip(1);
    let mut rest = Vec::new();
    while let Some(arg) = argv.next() {
        if arg == "--session" {
            let _ = argv.next();
            continue;
        }
        if arg == word {
            rest.extend(argv);
            break;
        }
    }
    rest
}

/// Hand the command to `ff-<name>` and never come back: capture first,
/// settle the session, then exec.
pub fn dispatch(name: &str, argv: Vec<OsString>) -> ! {
    let flag = session_flag();
    let env = std::env::var("FF_SESSION").ok();
    // The command line already failed to parse; refusing it twice helps
    // nobody, so an unresolvable session falls back to none.
    let session = crate::session::resolve(flag.as_deref(), env.as_deref()).unwrap_or(None);

    // Loud, exactly as `ff git` does: the user asked for something, so a
    // skipped net deserves a notice. `pre_loud` discovers the repository
    // itself and swallows failure, so an extension dispatches outside a
    // repository too.
    crate::capture::pre_loud(&crate::provenance::pre_ext(session.clone()));

    // Set in-process before the exec: on unix the exec replaces this
    // process and inherits its environment, which is the one way the
    // session reaches the child on both the exec and spawn paths, so the
    // extension's own `ff` calls inherit the tag without re-passing it.
    if let Some(session) = &session {
        unsafe { std::env::set_var("FF_SESSION", session) };
    }

    let path = resolve(name).expect("dispatch is called only after resolve returned Some");
    // exec replaces the process on every real path and exits on its own
    // failure paths (127/126); the codes below name the residue so this
    // function diverges honestly rather than dangling a Result.
    let program = path.to_string_lossy().into_owned();
    std::process::exit(match crate::cmd::git_exec::exec(&program, argv) {
        Ok(()) => 0,
        Err(_) => 1,
    });
}

/// The `--session` flag, scanned by hand because clap has already declined
/// this command line.
fn session_flag() -> Option<String> {
    let mut args = std::env::args();
    while let Some(arg) = args.next() {
        if let Some(value) = arg.strip_prefix("--session=") {
            return Some(value.to_string());
        }
        if arg == "--session" {
            return args.next();
        }
    }
    None
}
