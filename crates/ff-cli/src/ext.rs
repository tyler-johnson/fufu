//! PATH extensions: `ff <name>` finds an executable `ff-<name>` on PATH the
//! way git finds its own. A builtin verb always wins — this module is
//! reached only after clap has already declined the word — and, exactly as
//! `ff git` does, fufu captures before handing over.

use std::collections::BTreeSet;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// The rule an extension's name is spelled by: ASCII alphanumeric, `-` and
/// `_`, first character alphanumeric.
///
/// It is validated before anything touches the filesystem, because the name
/// comes from a command line and an unvalidated one could build a path that
/// escapes the PATH directory entirely (`ff ../../evil`), so the rule is
/// reject, never sanitize. A manifest's own `name` is held to the same
/// rule, which is what keeps the two spellings one spelling.
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
/// sorted — declared or not. `ff doctor` is the one caller: `resolve` finds
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
                vec!["ff status".into(), "ff worktree list".into()],
            ),
        );
    }

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
pub fn repo_var(workdir: &Path) -> OsString {
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

// ---- asking an extension a question, under fufu's time box -----------------

/// How long the briefing gives an extension to answer, and the largest box
/// anything here hands out.
///
/// The budget is fufu's rather than the extension's, because the agent's
/// turn is not the place to wait on somebody else's network call. A second
/// is orders of magnitude more than printing a line costs, and short enough
/// that a binary which hangs is a pause nobody attributes to fufu. It is a
/// briefing's number, and the briefing is paid once per audience per
/// session; a caller paying per event names a smaller one on its own
/// [`Ask`].
pub const BUDGET: Duration = Duration::from_secs(1);

/// How often the wait looks at the child. Small enough that the common
/// answer — a process that exited in a few milliseconds — is not rounded up
/// into something a person can feel.
const POLL: Duration = Duration::from_millis(2);

/// A question fufu puts to a declared extension out of band: `ff-<name>
/// <verb>` run as a child, rather than the exec [`dispatch`] does, with an
/// answer fufu may or may not get.
///
/// Both out-of-band callers take this shape — the briefing line asks for a
/// line, and the agent event's fan-out asks a subscriber for its reply — so
/// the doctrine sits here once. The child is handed the same three
/// variables an extension is handed anywhere else and the event's own
/// directory, it is given the budget the caller named to answer, and every
/// way it can fail is one answer.
pub struct Ask<'a> {
    /// The `<name>` in `ff-<name>`. [`ask`] walks PATH for it when the
    /// question is put, rather than taking the registry's recorded path, so
    /// a binary that moved between PATH directories is still found.
    pub name: &'a str,
    /// The word after it: `briefing`, `trigger`, `help`, or `explain`.
    pub verb: &'a str,
    /// Further words after `verb` — the error id for `explain`, and
    /// nothing for every other caller today.
    pub rest: &'a [&'a str],
    /// Where the child runs — the event's own directory, which is more
    /// specific than the worktree.
    pub cwd: &'a Path,
    /// The worktree `FF_REPO` names, and `None` outside one, which is the
    /// absence the variable spells by not being set.
    pub repo: Option<&'a Path>,
    /// The session tag for `FF_SESSION`. `None` leaves whatever the
    /// environment already holds, exactly as [`dispatch`] does.
    pub session: Option<&'a str>,
    /// What goes on stdin, followed by EOF. Empty is a question that is
    /// entirely in the verb.
    pub stdin: &'a [u8],
    /// How long this child has to answer. [`BUDGET`] is the briefing's; the
    /// fan-out hands out shares of a smaller box, because it is paid on
    /// every event rather than once per audience.
    pub budget: Duration,
}

/// Put the question, and answer with the child's stdout when it exited 0
/// inside the budget.
///
/// `None` otherwise, and there is no third answer: a name nothing on PATH
/// spells, a binary that would not start, one that exited nonzero, and one
/// still running when the budget ran out are the same outcome to a caller,
/// which is the trigger doctrine — an extension having a bad day costs the
/// agent nothing. `FF_DEBUG=1` is where the reason goes.
pub fn ask(question: &Ask<'_>) -> Option<Vec<u8>> {
    let path = resolve(question.name)?;
    ask_at(&path, question)
}

/// The same question against a binary already resolved: [`ask`] finds one
/// on PATH, and this runs it. The split is `manifest::handshake`'s, and for
/// the same reason — a caller holding the path has already walked.
pub fn ask_at(path: &Path, question: &Ask<'_>) -> Option<Vec<u8>> {
    let mut cmd = Command::new(path);
    cmd.arg(question.verb)
        .args(question.rest)
        .current_dir(question.cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        // Not shown to anyone: whatever an extension has to say to a person
        // has no person in front of it here.
        .stderr(Stdio::null())
        .env("FF_CONTRACT", crate::machine::CONTRACT.to_string());
    match question.repo {
        Some(workdir) => cmd.env("FF_REPO", repo_var(workdir)),
        None => cmd.env_remove("FF_REPO"),
    };
    if let Some(session) = question.session {
        cmd.env("FF_SESSION", session);
    }

    let mut child = match cmd.spawn() {
        Ok(child) => child,
        Err(err) => return quiet(question, format!("it would not run: {err}")),
    };

    // Both pipes are drained on threads of their own. A child that never
    // reads its stdin, or prints more than a pipe holds before it exits,
    // would otherwise deadlock against a fufu waiting on the other end —
    // and a deadlock is the one failure the time box below cannot see,
    // since the child is still perfectly alive.
    if let Some(mut pipe) = child.stdin.take() {
        let payload = question.stdin.to_vec();
        std::thread::spawn(move || {
            use std::io::Write;
            let _ = pipe.write_all(&payload);
        });
    }
    let (tx, rx) = std::sync::mpsc::channel();
    if let Some(mut pipe) = child.stdout.take() {
        std::thread::spawn(move || {
            use std::io::Read;
            let mut said = Vec::new();
            let _ = pipe.read_to_end(&mut said);
            let _ = tx.send(said);
        });
    }

    let deadline = Instant::now() + question.budget;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Err(_) => break None,
            Ok(None) if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
            Ok(None) => std::thread::sleep(POLL),
        }
    };
    let Some(status) = status else {
        return quiet(question, "it did not answer inside the time box");
    };
    if !status.success() {
        return quiet(question, format!("it exited with {status}"));
    }

    // The pipe is on the same budget as the process, and for a reason the
    // process alone does not cover: a child that forked and exited leaves
    // its grandchild holding the write end, so a read waiting for EOF is
    // waiting on something fufu never spawned and cannot kill. The reader
    // is left where it is when that happens — it is a thread blocked on a
    // pipe in a process that is about to exit.
    let grace = deadline
        .saturating_duration_since(Instant::now())
        .max(POLL * 10);
    match rx.recv_timeout(grace) {
        Ok(said) => Some(said),
        Err(_) => quiet(question, "it exited without closing its stdout"),
    }
}

/// The one place a question that came back with nothing says why, and it
/// says it only under `FF_DEBUG`, beside fufu's own complaint. Answers
/// `None` so a caller returns it.
fn quiet(question: &Ask<'_>, why: impl std::fmt::Display) -> Option<Vec<u8>> {
    if std::env::var_os("FF_DEBUG").is_some() {
        eprintln!(
            "ff[debug]: ff-{} {} said nothing: {why}",
            question.name, question.verb
        );
    }
    None
}

// ---- help and explain, delegated to a declared extension -------------------

/// `ff help <name>` and `ff explain <name>/<id>` for a declared extension:
/// `ff-<name> help` or `ff-<name> explain <id>`, asked exactly as [`ask_at`]
/// asks anything else.
///
/// This is `ask`'s doctrine with one clause struck out. Everywhere else, a
/// question that comes back with nothing costs the caller nothing — an
/// extension having a bad day on a briefing line or an event nobody sees
/// fail. Here a person typed the command and is looking at a terminal for
/// the answer, so every way [`ask_at`] collapses to `None` — the binary has
/// left PATH since it was declared, would not start, exited nonzero, or ran
/// past the time box — is reported here as `extension/delegate-failed`
/// rather than handed back as a page with nothing on it.
///
/// Neither question discovers a repository — `ff help <name>` runs before
/// `Ctx` exists at all, and `ff explain` never discovers one even for
/// fufu's own ids — so the child is not handed `FF_REPO`, the one variable
/// of the usual three that names one. It still gets `FF_CONTRACT`, and
/// `FF_SESSION` when the environment already carries one.
pub fn delegate(
    declared: &crate::registry::Declared,
    verb: &str,
    rest: &[&str],
) -> ff_core::Result<Vec<u8>> {
    let path = declared
        .resolve()
        .ok_or_else(|| delegate_failed(declared.name()))?;
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let session = std::env::var("FF_SESSION").ok();
    ask_at(
        &path,
        &Ask {
            name: declared.name(),
            verb,
            rest,
            cwd: &cwd,
            repo: None,
            session: session.as_deref(),
            stdin: &[],
            budget: BUDGET,
        },
    )
    .ok_or_else(|| delegate_failed(declared.name()))
}

fn delegate_failed(name: &str) -> ff_core::Error {
    ff_core::Error::coded(
        "extension/delegate-failed",
        format!(
            "ff-{name} did not answer: it may have left PATH since it was declared, refused to \
             start, exited nonzero, or run past the time box fufu gives it"
        ),
        vec!["ff doctor".into(), "ff extension list".into()],
    )
}

/// The time box and the doctrine around it, against a real binary. Unix
/// only, for the reason `tests/ext.rs` is: a shell script is the smallest
/// binary to write.
#[cfg(all(test, unix))]
mod asking {
    use std::fs::Permissions;
    use std::os::unix::fs::PermissionsExt;

    use super::*;

    /// An `ff-<name>` in a fresh directory, asked directly. Nothing here
    /// touches PATH: the walk is `resolve`, which `tests/ext.rs` already
    /// covers, and moving a process-wide variable under a threaded test
    /// binary is not a thing to do for a convenience.
    fn asking(body: &str, stdin: &[u8]) -> Option<String> {
        let dir = tempfile::TempDir::new().expect("create tempdir");
        let path = dir.path().join("ff-asked");
        std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).expect("write script");
        std::fs::set_permissions(&path, Permissions::from_mode(0o755)).expect("chmod script");

        ask_at(
            &path,
            &Ask {
                name: "asked",
                verb: "briefing",
                rest: &[],
                cwd: dir.path(),
                repo: None,
                session: None,
                stdin,
                budget: BUDGET,
            },
        )
        .map(|said| String::from_utf8_lossy(&said).into_owned())
    }

    #[test]
    fn stdout_comes_back_from_a_binary_that_exited_zero() {
        assert_eq!(
            asking("echo a line", &[]).as_deref(),
            Some("a line\n"),
            "verbatim, trailing newline and all"
        );
    }

    /// The payload reaches the child and EOF follows it, so a handler that
    /// reads stdin to the end is not left waiting. `read` and `printf` are
    /// the shell's own, because PATH is the script's directory and nothing
    /// else; `read` answering nonzero at an EOF with no newline before it is
    /// why `printf` is the last command and the exit status.
    #[test]
    fn stdin_arrives_and_ends() {
        assert_eq!(
            asking(
                "read line\nprintf '%s' \"$line\"",
                b"{\"kind\":\"SessionStart\"}"
            )
            .as_deref(),
            Some("{\"kind\":\"SessionStart\"}")
        );
    }

    /// The variables are the handshake's three and nothing else is added:
    /// `FF_REPO` is absent outside a worktree, which is the absence the
    /// variable spells by not being set.
    #[test]
    fn the_child_is_handed_the_contract_and_no_repo() {
        assert_eq!(
            asking(r#"echo "$FF_CONTRACT|${FF_REPO-unset}""#, &[]).as_deref(),
            Some(format!("{}|unset\n", crate::machine::CONTRACT).as_str())
        );
    }

    /// Every way it can fail is one answer, and none of them is an error a
    /// caller has to handle.
    #[test]
    fn a_binary_that_fails_answers_with_nothing() {
        // Exited nonzero, having printed anyway.
        assert_eq!(asking("echo a line; exit 1", &[]), None);
        // Killed by a signal.
        assert_eq!(asking("kill -TERM $$", &[]), None);
        // Not on PATH at all.
        assert_eq!(
            ask(&Ask {
                name: "nothing-on-path-answers-to-this",
                verb: "briefing",
                rest: &[],
                cwd: std::path::Path::new("."),
                repo: None,
                session: None,
                stdin: &[],
                budget: BUDGET,
            }),
            None
        );
    }

    /// The box is fufu's. A binary still thinking when it runs out answers
    /// with nothing, and the wait is the budget rather than the binary's
    /// own idea of how long it may take.
    #[test]
    fn a_binary_that_hangs_is_cut_off_at_the_budget() {
        let started = Instant::now();
        // PATH is the script's own directory for the length of the ask, so
        // it names a system one to reach `sleep`.
        let said = asking("PATH=/bin:/usr/bin; export PATH; sleep 120", &[]);
        let waited = started.elapsed();
        assert_eq!(said, None);
        assert!(
            waited >= BUDGET && waited < BUDGET * 10,
            "waited {waited:?} against a budget of {BUDGET:?}"
        );
    }

    /// A child that forked and exited leaves its grandchild holding the
    /// write end of the pipe. The read is on the same budget as the process
    /// for exactly this: nothing here waits on something fufu never
    /// spawned.
    #[test]
    fn a_grandchild_holding_the_pipe_open_does_not_hold_fufu() {
        let started = Instant::now();
        let said = asking("PATH=/bin:/usr/bin; export PATH; sleep 120 & exit 0", &[]);
        let waited = started.elapsed();
        assert_eq!(said, None);
        assert!(
            waited < BUDGET * 10,
            "waited {waited:?} against a budget of {BUDGET:?}"
        );
    }
}
