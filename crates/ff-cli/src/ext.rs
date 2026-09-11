//! PATH extensions: `ff <name>` finds an executable `ff-<name>` on PATH the
//! way git finds its own. A builtin verb always wins — this module is
//! reached only after clap has already declined the word — and, exactly as
//! `ff git` does, fufu captures before handing over.

use std::collections::BTreeSet;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

/// The rule an extension's name is spelled by: ASCII alphanumeric, `-` and
/// `_`, first character alphanumeric.
///
/// It is validated before anything touches the filesystem, because the name
/// comes from a command line and an unvalidated one could build a path that
/// escapes the PATH directory entirely (`ff ../../evil`), so the rule is
/// reject, never sanitize.
pub fn valid_name(name: &str) -> bool {
    let bytes = name.as_bytes();
    !bytes.is_empty()
        && bytes[0].is_ascii_alphanumeric()
        && bytes
            .iter()
            .all(|b| b.is_ascii_alphanumeric() || *b == b'-' || *b == b'_')
}

/// Find `ff-<name>` on PATH.
pub fn resolve(name: &str) -> Option<PathBuf> {
    if !valid_name(name) {
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

/// Every `<name>` for which `ff-<name>` sits on PATH, deduplicated and
/// sorted. `ff doctor` is the one caller: `resolve` finds
/// one binary to run, and this finds the whole population it reports on.
///
/// A directory PATH names but cannot be read is skipped rather than failing
/// the walk, the same best-effort rule the object-store scan in doctor
/// already runs takes.
pub fn on_path() -> Vec<String> {
    let mut found = BTreeSet::new();
    let Some(path) = std::env::var_os("PATH") else {
        return Vec::new();
    };
    for dir in std::env::split_paths(&path).filter(|entry| !entry.as_os_str().is_empty()) {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let file_name = entry.file_name();
            let Some(name) = extension_name(&file_name.to_string_lossy()) else {
                continue;
            };
            if valid_name(&name) && is_executable(&entry.path()) {
                found.insert(name);
            }
        }
    }
    found.into_iter().collect()
}

/// The `<name>` in a PATH entry's filename, when it is spelled `ff-<name>`
/// (`ff-<name>.exe` off unix, where PATH carries that convention).
fn extension_name(file_name: &str) -> Option<String> {
    let stripped = file_name.strip_prefix("ff-")?;
    #[cfg(not(unix))]
    let stripped = stripped.strip_suffix(".exe").unwrap_or(stripped);
    (!stripped.is_empty()).then(|| stripped.to_string())
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

/// Where a value-taking global keeps its value.
enum Value {
    /// One token holds both: `--cwd=../bay`, `-C../bay`, `-C=../bay`.
    Here(String),
    /// The value is the token after this one: `--cwd ../bay`, `-C ../bay`.
    Next,
}

/// Match one raw argument against a global that takes a value.
///
/// Every spelling clap accepts is matched here, because clap never saw this
/// command line: a form the hand scan missed would reach the child both as
/// an unconsumed flag in its argv and as a directory fufu never moved to.
fn value_flag(arg: &str, long: &str, short: Option<&str>) -> Option<Value> {
    if arg == long {
        return Some(Value::Next);
    }
    if let Some(value) = arg.strip_prefix(&format!("{long}=")) {
        return Some(Value::Here(value.to_string()));
    }
    let short = short?;
    if arg == short {
        return Some(Value::Next);
    }
    let attached = arg.strip_prefix(short)?;
    // clap reads `-C=dir` as `-C dir`, so this does too.
    Some(Value::Here(
        attached.strip_prefix('=').unwrap_or(attached).to_string(),
    ))
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
        let text = arg.to_string_lossy();
        let global =
            value_flag(&text, "--session", None).or_else(|| value_flag(&text, "--cwd", Some("-C")));
        if let Some(global) = global {
            if matches!(global, Value::Next) {
                let _ = argv.next();
            }
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
    // First, for the reason `relocate` runs first in `settle`: `pre_loud`
    // below discovers on its own, and a capture taken from the old directory
    // would snapshot the wrong repository. `FF_REPO` would name it too.
    if let Some(dir) = cwd_flag(name)
        && let Err(err) = std::env::set_current_dir(&dir)
    {
        crate::report(
            false,
            "map",
            &ff_core::Error::coded(
                "usage/no-such-directory",
                format!("-C {}: {err}", dir.display()),
                vec!["ff status".into(), "ff worktree".into()],
            ),
        );
    }

    let flag = session_flag();
    let env = std::env::var("FF_SESSION").ok();
    let client = std::env::var(crate::integ::claude::SESSION_VAR).ok();
    // The command line already failed to parse; refusing it twice helps
    // nobody, so an unresolvable session falls back to none.
    let session =
        crate::session::resolve(flag.as_deref(), env.as_deref(), client.as_deref()).unwrap_or(None);

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

    // The other two thirds of the handshake, on the same mechanism and for
    // the same reason: an extension that cannot say which repository it was
    // invoked against, or which JSON contract it is about to parse, has to
    // guess at both.
    //
    // `FF_REPO` is emitted and never read back. fufu answers "which
    // repository" from the current directory alone, so a child that exported
    // a stale one could not silently redirect a later `ff` call — git's
    // `GIT_DIR` footgun, declined.
    if let Some(workdir) = repo_root() {
        unsafe { std::env::set_var("FF_REPO", workdir) };
    }
    unsafe { std::env::set_var("FF_CONTRACT", crate::machine::CONTRACT.to_string()) };

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

/// The worktree fufu is standing in, spelled the way every other fufu
/// surface spells one: absolute, symlinks resolved, forward slashes.
///
/// Absent rather than empty when there is none — outside a repository, or in
/// a bare one — so a child tests presence instead of parsing emptiness.
fn repo_root() -> Option<OsString> {
    let repo = ff_core::discover(".").ok()?;
    Some(repo_var(repo.workdir()?))
}

/// One worktree spelled the way `FF_REPO` spells one, for a caller that has
/// already discovered the repository and is handing it to a child.
fn repo_var(workdir: &Path) -> OsString {
    let real = ff_core::linked::path::real(workdir);
    ff_core::linked::path::as_git(&real).into()
}

/// The `--cwd` flag, on the path clap never reached.
///
/// The scan stops at the extension's own word: everything after it belongs
/// to the extension, which is the same line `rest_argv` draws, so a `-C` the
/// child owns is neither consumed here nor stripped from its argv.
fn cwd_flag(name: &str) -> Option<PathBuf> {
    let word = OsString::from(name);
    let mut args = std::env::args_os().skip(1);
    // Clap refuses a repeated `-C` on the parsed path. Here there is no
    // parser to refuse with — this command line already failed to parse, and
    // refusing it twice helps nobody — so the last one wins, the same
    // fallback the session takes two lines below.
    let mut found = None;
    while let Some(arg) = args.next() {
        let text = arg.to_string_lossy();
        if let Some(value) = value_flag(&text, "--cwd", Some("-C")) {
            match value {
                Value::Here(value) => found = Some(PathBuf::from(value)),
                Value::Next => found = args.next().map(PathBuf::from).or(found),
            }
            continue;
        }
        // The session's value is stepped over for the reason `rest_argv`
        // steps over it: `ff --session tower -C ../bay tower go` names a
        // session that spells the same as the extension, and stopping at the
        // first `tower` would stop at the value.
        if matches!(value_flag(&text, "--session", None), Some(Value::Next)) {
            let _ = args.next();
            continue;
        }
        if arg == word {
            break;
        }
    }
    found
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
