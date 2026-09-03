//! The shells: bash, zsh, fish, powershell.
//!
//! One slug per shell, because the rc file and its syntax differ per shell,
//! but one trigger source, because every one of them installs a line that
//! calls the same `ff trigger shell`. That is the clearest case for the
//! two namespaces being different: `hook` names a thing you integrate
//! with, `trigger` names an event source, and there is no reason those
//! have to line up.
//!
//! Two independent pieces go into the rc file: the alias (`alias git='ff
//! git'`), so every git command you type snapshots first, and the prompt
//! hook, so a snapshot lands at every shell prompt too and the tree you
//! were about to change is already on the log. Marked-line editing:
//! install appends lines carrying the fufu marker, uninstall removes
//! exactly the marked lines. A hand-written alias
//! or a hand-written prompt hook is detected, respected, and never touched
//! — independently of the other piece. Every path is env-resolved (HOME,
//! ZDOTDIR, XDG_CONFIG_HOME, SHELL) so tests stay hermetic. The one
//! exception is PowerShell's profile on Windows, which lives under the
//! Documents known folder rather than under any variable; `FF_DOCUMENTS_DIR`
//! stands in for the known-folder lookup so the suite never writes a real
//! profile.

use std::path::{Path, PathBuf};

use ff_core::{Error, Result};

use super::runtime;
use super::{
    AgentEvent, Change, EventKind, InstallOptions, Integration, Label, Mechanism, Part, Presence,
    Status, Wiring,
};
use crate::ctx::Ctx;

/// The marker install writes today.
const MARKER: &str = "# fufu — added by `ff hook`";
/// Markers older installs carry. Recognized forever, so a line fufu wrote
/// under a retired spelling stays fufu-managed rather than becoming
/// something nobody will ever remove.
const LEGACY_MARKERS: [&str; 2] = [
    "# fufu — added by `ff hook shell install`",
    "# fufu — added by `ff shell install`",
];

/// The canonical trigger command, and every spelling ever shipped in an rc
/// file. A stored string is accepted forever, at the cost of a line here.
const TRIGGER: &str = "ff trigger shell";
const LEGACY_TRIGGERS: [&str; 1] = ["ff hook shell trigger"];

pub const SHELLS: [&str; 4] = ["bash", "zsh", "fish", "powershell"];

/// The console host's profile file name, the same under PowerShell 7 and
/// Windows PowerShell 5.1; only the directory differs.
const PROFILE: &str = "Microsoft.PowerShell_profile.ps1";

pub struct Shell {
    pub slug: &'static str,
}

fn is_marked(line: &str) -> bool {
    line.contains(MARKER) || LEGACY_MARKERS.iter().any(|m| line.contains(m))
}

/// Whether a line names the ambient trigger under any spelling it has ever
/// had.
fn names_trigger(line: &str) -> bool {
    line.contains(TRIGGER) || LEGACY_TRIGGERS.iter().any(|t| line.contains(t))
}

fn env_path(var: &str) -> Option<PathBuf> {
    std::env::var_os(var)
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
}

fn home() -> Result<PathBuf> {
    super::home()
}

/// The line ending a file already uses, so a rewrite keeps it. A profile
/// written by a Windows editor is CRLF, and a fufu that rejoined it with
/// `\n` would flip every line of it on the first `ff unhook`.
pub(super) fn line_ending(contents: &str) -> &'static str {
    if contents.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    }
}

/// `$PROFILE` for the console host, one file for both PowerShells.
///
/// On Windows that is `<Documents>\PowerShell\...` for PowerShell 7 and
/// `<Documents>\WindowsPowerShell\...` for 5.1; the 7 file is the one
/// wired unless only the 5.1 file exists, and it is created when neither
/// does. `documents` is the known folder (OneDrive can redirect it away
/// from `<home>\Documents`, and PowerShell follows the redirect), with
/// `<home>\Documents` as the fallback when the lookup failed. Elsewhere it
/// is `$XDG_CONFIG_HOME/powershell/...`, or `~/.config/powershell/...`.
fn powershell_profile(
    windows: bool,
    home: &Path,
    documents: Option<&Path>,
    xdg: Option<&Path>,
    exists: impl Fn(&Path) -> bool,
) -> PathBuf {
    if !windows {
        let config = xdg.map_or_else(|| home.join(".config"), Path::to_path_buf);
        return config.join("powershell").join(PROFILE);
    }
    let documents = documents.map_or_else(|| home.join("Documents"), Path::to_path_buf);
    let seven = documents.join("PowerShell").join(PROFILE);
    let five = documents.join("WindowsPowerShell").join(PROFILE);
    if exists(&seven) || !exists(&five) {
        seven
    } else {
        five
    }
}

/// The Documents known folder, the way PowerShell itself resolves
/// `$PROFILE`: through the shell API, so a OneDrive redirect lands on the
/// file PowerShell reads rather than on a `<home>\Documents` it never
/// opens. `FF_DOCUMENTS_DIR` wins when set, which is how the test suite
/// keeps a real profile out of reach. `None` when the lookup fails.
#[cfg(windows)]
fn documents_dir() -> Option<PathBuf> {
    use std::os::windows::ffi::OsStringExt;
    use windows_sys::Win32::System::Com::CoTaskMemFree;
    use windows_sys::Win32::UI::Shell::{FOLDERID_Documents, SHGetKnownFolderPath};

    if let Some(dir) = env_path("FF_DOCUMENTS_DIR") {
        return Some(dir);
    }
    let mut wide: windows_sys::core::PWSTR = std::ptr::null_mut();
    // SAFETY: the folder id is a static GUID, a null token means the calling
    // user, and on success the API hands back a NUL-terminated buffer the
    // caller owns and releases with CoTaskMemFree — which happens below,
    // after the copy, on every path that got one.
    let hr =
        unsafe { SHGetKnownFolderPath(&FOLDERID_Documents, 0, std::ptr::null_mut(), &mut wide) };
    if hr < 0 || wide.is_null() {
        return None;
    }
    let mut len = 0;
    // SAFETY: the buffer is NUL-terminated per the API contract, and the
    // reads stop at the terminator.
    while unsafe { *wide.add(len) } != 0 {
        len += 1;
    }
    // SAFETY: `len` counts initialized u16s before the terminator.
    let dir = PathBuf::from(std::ffi::OsString::from_wide(unsafe {
        std::slice::from_raw_parts(wide, len)
    }));
    // SAFETY: the buffer was allocated by the shell for us to free, once.
    unsafe { CoTaskMemFree(wide.cast()) };
    Some(dir)
}

#[cfg(not(windows))]
fn documents_dir() -> Option<PathBuf> {
    None
}

fn rc_file(shell: &str) -> Result<PathBuf> {
    Ok(match shell {
        "bash" => home()?.join(".bashrc"),
        "zsh" => match env_path("ZDOTDIR") {
            Some(zdot) => zdot.join(".zshrc"),
            None => home()?.join(".zshrc"),
        },
        "fish" => {
            let config = match env_path("XDG_CONFIG_HOME") {
                Some(xdg) => xdg,
                None => home()?.join(".config"),
            };
            config.join("fish/config.fish")
        }
        "powershell" => powershell_profile(
            cfg!(windows),
            &home()?,
            documents_dir().as_deref(),
            env_path("XDG_CONFIG_HOME").as_deref(),
            |p| p.is_file(),
        ),
        other => {
            return Err(Error::msg(format!(
                "unsupported shell {other:?} (supported: bash, zsh, fish, powershell)"
            )));
        }
    })
}

fn alias_line(shell: &str) -> &'static str {
    // fish aliases take a space-separated body; bash and zsh take `=`.
    // PowerShell's `Set-Alias` takes no arguments, so its alias is a
    // function that forwards them.
    match shell {
        "fish" => "alias git 'ff git'",
        "powershell" => "function git { ff git @args }",
        _ => "alias git='ff git'",
    }
}

/// The un-marked bodies of the ambient prompt-hook wiring; the caller
/// appends the marker to each, exactly as install does for the alias.
///
/// Bash is guarded against double-prepending because a `.bashrc` can be
/// sourced more than once in one shell — the marker only protects the
/// *file*, not the runtime. PowerShell has the same guard for the same
/// reason: the line saves the current `prompt` under `_fufu_prompt` and
/// redefines `prompt` to trigger and then call it, and a second dot-source
/// must not wrap the wrapper. `Out-Null` keeps anything the trigger ever
/// wrote to stdout out of the prompt string, since what the prompt
/// function outputs *is* the prompt.
fn ambient_lines(shell: &str) -> Vec<String> {
    match shell {
        "bash" => vec![format!(
            r#"[[ $PROMPT_COMMAND == *"{TRIGGER}"* ]] || PROMPT_COMMAND="{TRIGGER};$PROMPT_COMMAND""#
        )],
        "zsh" => vec![
            format!("_fufu_ambient() {{ {TRIGGER} }}"),
            "precmd_functions+=(_fufu_ambient)".to_string(),
        ],
        "fish" => vec![format!(
            "function _fufu_ambient --on-event fish_prompt; {TRIGGER}; end"
        )],
        "powershell" => vec![format!(
            "if (-not (Test-Path Function:_fufu_prompt)) {{ $function:global:_fufu_prompt = $function:prompt; function global:prompt {{ {TRIGGER} | Out-Null; _fufu_prompt }} }}"
        )],
        _ => Vec::new(),
    }
}

/// `Wired` requires a *marked* line that also names the alias — a file
/// whose only marked line is the ambient wiring must not report the alias
/// wired. (That is the bug the alias/ambient split exists to fix: a single
/// shared check returned installed for *any* marked line, so a file with
/// only the prompt hook lied about the alias too.)
fn alias_wiring(contents: &str, rc: &Path) -> Wiring {
    // `alias git` in three shells, `function git` in PowerShell.
    let names_alias = |line: &str| line.contains("alias git") || line.contains("function git");
    for line in contents.lines() {
        if is_marked(line) && names_alias(line) {
            return Wiring::Wired {
                mechanism: Mechanism::Rc,
                at: rc.to_path_buf(),
            };
        }
    }
    for line in contents.lines() {
        let trimmed = line.trim();
        if (trimmed.starts_with("alias git") || trimmed.starts_with("function git"))
            && trimmed.contains("ff git")
        {
            return Wiring::HandWritten;
        }
    }
    Wiring::NotWired
}

/// `Wired` requires a marked line naming either the trigger command or
/// `_fufu_ambient` — zsh's wiring is two marked lines and only the first
/// mentions the command literally, so either alternative marks the whole
/// piece wired.
fn ambient_wiring(contents: &str, rc: &Path) -> Wiring {
    for line in contents.lines() {
        if is_marked(line) && (names_trigger(line) || line.contains("_fufu_ambient")) {
            return Wiring::Wired {
                mechanism: Mechanism::Rc,
                at: rc.to_path_buf(),
            };
        }
    }
    for line in contents.lines() {
        if !is_marked(line) && names_trigger(line) {
            return Wiring::HandWritten;
        }
    }
    Wiring::NotWired
}

/// The shell fufu wires when no slug was named — but only when it is one of
/// `SHELLS`, so `ff hook` on an exotic login shell says so rather than
/// guessing. PowerShell's binary is `pwsh`, and its slug is not.
pub fn default_shell() -> Option<&'static str> {
    let shell = std::env::var("SHELL").ok()?;
    let name = Path::new(&shell).file_name()?.to_str()?;
    if name == "pwsh" {
        return Some("powershell");
    }
    SHELLS.into_iter().find(|s| *s == name)
}

impl Shell {
    fn rc(&self) -> Result<PathBuf> {
        rc_file(self.slug)
    }

    fn pieces(&self) -> (Wiring, Wiring, Option<PathBuf>) {
        let Ok(rc) = self.rc() else {
            let complaint = Wiring::Unavailable("HOME is not set".into());
            return (complaint.clone(), complaint, None);
        };
        let contents = std::fs::read_to_string(&rc).unwrap_or_default();
        (
            alias_wiring(&contents, &rc),
            ambient_wiring(&contents, &rc),
            Some(rc),
        )
    }
}

impl Integration for Shell {
    fn slug(&self) -> &'static str {
        self.slug
    }

    /// Every shell feeds one source: the rc lines they install differ in
    /// syntax and call the same command.
    fn source(&self) -> &'static str {
        "shell"
    }

    fn detect(&self) -> Presence {
        // The rc file is the evidence when there is one. A shell that is
        // the login shell but has never been configured still counts —
        // that is precisely the shell worth offering to wire. On Windows
        // PowerShell is that shell unconditionally: 5.1 ships with the
        // OS, and there is no `$SHELL` to consult.
        match self.rc() {
            Ok(rc) if rc.is_file() => Presence::Present { evidence: rc },
            Ok(rc) if default_shell() == Some(self.slug) => Presence::Present { evidence: rc },
            Ok(rc) if cfg!(windows) && self.slug == "powershell" => {
                Presence::Present { evidence: rc }
            }
            _ => Presence::Absent,
        }
    }

    fn status(&self) -> Status {
        let (alias, ambient, rc) = self.pieces();
        let wiring = combine(&alias, &ambient);
        let stale = rc
            .as_ref()
            .and_then(|rc| std::fs::read_to_string(rc).ok())
            .is_some_and(|contents| contents.lines().any(is_outdated));
        Status {
            slug: self.slug,
            presence: self.detect(),
            wiring,
            note: None,
            parts: vec![
                Part {
                    name: "alias",
                    wiring: alias,
                },
                Part {
                    name: "ambient",
                    wiring: ambient,
                },
            ],
            skill: None,
            mcp: None,
            mcp_extensions: Vec::new(),
            mcp_orphaned: Vec::new(),
            stale,
        }
    }

    fn install(&self, _opts: &InstallOptions) -> Result<Change> {
        let rc = self.rc()?;
        let original = std::fs::read_to_string(&rc).unwrap_or_default();
        // Rewrite retired spellings in place first, the way the settings
        // engine upgrades a legacy command: one write, so there is never a
        // moment where the rc file has neither the old line nor the new.
        let eol = line_ending(&original);
        let contents = upgrade(&original);
        let upgraded = contents != original;
        let alias = alias_wiring(&contents, &rc);
        let ambient = ambient_wiring(&contents, &rc);

        // The two pieces are handled independently: whichever is absent
        // gets its marked lines queued for one append at the end.
        let mut change = Change {
            changed: upgraded,
            lines: Vec::new(),
        };
        if upgraded {
            change
                .lines
                .push(format!("rewrote retired spellings in {}", rc.display()));
        }
        let mut queued = String::new();

        match &alias {
            Wiring::Wired { .. } => change
                .lines
                .push(format!("alias already wired in {}", rc.display())),
            Wiring::HandWritten => change.lines.push(format!(
                "{} already aliases git to ff by hand — leaving it alone",
                rc.display()
            )),
            _ => queued.push_str(&format!("{}  {MARKER}{eol}", alias_line(self.slug))),
        }

        match &ambient {
            Wiring::Wired { .. } => change
                .lines
                .push(format!("prompt hook already wired in {}", rc.display())),
            Wiring::HandWritten => change.lines.push(format!(
                "{} already calls {TRIGGER} by hand — leaving it alone",
                rc.display()
            )),
            _ => {
                for line in ambient_lines(self.slug) {
                    queued.push_str(&format!("{line}  {MARKER}{eol}"));
                }
            }
        }

        if !queued.is_empty() || upgraded {
            if let Some(parent) = rc.parent() {
                std::fs::create_dir_all(parent).map_err(Error::repo)?;
            }
            let mut updated = contents;
            if !updated.is_empty() && !updated.ends_with('\n') {
                updated.push_str(eol);
            }
            updated.push_str(&queued);
            std::fs::write(&rc, updated).map_err(Error::repo)?;
            if !queued.is_empty() {
                change.changed = true;
                change.lines.push(format!("wired into {}", rc.display()));
            }
            change
                .lines
                .push("restart the shell (or source the file) to activate it".into());
        }
        Ok(change)
    }

    fn uninstall(&self, _opts: &InstallOptions) -> Result<Change> {
        let rc = self.rc()?;
        let Ok(contents) = std::fs::read_to_string(&rc) else {
            return Ok(Change::unchanged(format!(
                "nothing wired ({} not found)",
                rc.display()
            )));
        };
        let alias = alias_wiring(&contents, &rc);
        let ambient = ambient_wiring(&contents, &rc);
        let alias_wired = matches!(alias, Wiring::Wired { .. });
        let ambient_wired = matches!(ambient, Wiring::Wired { .. });

        if !alias_wired && !ambient_wired {
            let mut change = Change::unchanged(format!("nothing wired in {}", rc.display()));
            if alias == Wiring::HandWritten {
                change.lines.push(format!(
                    "the alias in {} was written by hand — not touching it",
                    rc.display()
                ));
            }
            if ambient == Wiring::HandWritten {
                change.lines.push(format!(
                    "the prompt hook in {} was written by hand — not touching it",
                    rc.display()
                ));
            }
            return Ok(change);
        }

        // A hand-written line is never marked, so it survives this filter
        // with no special case — and the same filter already removes every
        // marked line of a multi-line install, so the ambient piece needs
        // no removal logic of its own.
        let eol = line_ending(&contents);
        let kept: Vec<&str> = contents.lines().filter(|line| !is_marked(line)).collect();
        let mut updated = kept.join(eol);
        if contents.ends_with('\n') && !updated.is_empty() {
            updated.push_str(eol);
        }
        std::fs::write(&rc, updated).map_err(Error::repo)?;

        let what = match (alias_wired, ambient_wired) {
            (true, true) => "the alias and the prompt hook",
            (true, false) => "the alias",
            (false, true) => "the prompt hook",
            (false, false) => unreachable!("returned above when neither is wired"),
        };
        Ok(Change::changed(format!(
            "removed {what} from {}",
            rc.display()
        )))
    }

    /// The prompt hook: capture, and say nothing at all. A line at every
    /// shell prompt is noise where the snapshot is the whole point.
    fn trigger(&self, ctx: &Ctx, _forced: Option<EventKind>) {
        if let Err(err) = (|| -> Result<()> {
            let cwd = std::env::current_dir().map_err(Error::repo)?;
            let event = AgentEvent {
                // A prompt fires before whatever you are about to run,
                // which is what BeforeTool means — manual.rs reads it the
                // same way, for the same reason.
                kind: EventKind::BeforeTool,
                session: String::new(),
                agent: String::new(),
                cwd,
                label: Label::Text(String::new()),
                command: None,
                // A prompt is not a tool call, whatever kind it claims, so
                // there is no tool name for a subscription to match on.
                tool: None,
                path: None,
            };
            runtime::pipeline(ctx, "shell", &event, None)?;
            Ok(())
        })() {
            runtime::complain("shell", &err);
        }
    }
}

/// Rewrite every retired spelling on fufu's own lines. A line nobody
/// marked is never touched, so a hand-written prompt hook naming the old
/// command keeps naming it — that line belongs to whoever wrote it.
fn upgrade(contents: &str) -> String {
    let eol = line_ending(contents);
    let mut out = String::with_capacity(contents.len());
    for (n, line) in contents.lines().enumerate() {
        if n > 0 {
            out.push_str(eol);
        }
        if !is_marked(line) {
            out.push_str(line);
            continue;
        }
        let mut line = line.to_string();
        for trigger in LEGACY_TRIGGERS {
            line = line.replace(trigger, TRIGGER);
        }
        for marker in LEGACY_MARKERS {
            line = line.replace(marker, MARKER);
        }
        out.push_str(&line);
    }
    if contents.ends_with('\n') && !out.is_empty() {
        out.push_str(eol);
    }
    out
}

/// A line fufu wrote under a spelling it no longer writes. It still works
/// — that is why every shipped spelling stays recognized — so this is what
/// `ff doctor --fix` offers to rewrite, not a failure to report.
fn is_outdated(line: &str) -> bool {
    if !is_marked(line) {
        return false;
    }
    LEGACY_MARKERS.iter().any(|m| line.contains(m))
        || LEGACY_TRIGGERS.iter().any(|t| line.contains(t))
}

/// One answer for a slug that wires two independent pieces.
fn combine(alias: &Wiring, ambient: &Wiring) -> Wiring {
    match (alias, ambient) {
        (Wiring::Unavailable(complaint), _) => Wiring::Unavailable(complaint.clone()),
        (Wiring::Wired { mechanism, at }, Wiring::Wired { .. }) => Wiring::Wired {
            mechanism: *mechanism,
            at: at.clone(),
        },
        (Wiring::Wired { at, .. }, _) => Wiring::Partial {
            missing: "ambient".into(),
            at: at.clone(),
        },
        (_, Wiring::Wired { at, .. }) => Wiring::Partial {
            missing: "alias".into(),
            at: at.clone(),
        },
        (Wiring::HandWritten, _) | (_, Wiring::HandWritten) => Wiring::HandWritten,
        _ => Wiring::NotWired,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RC: &str = "/home/u/.bashrc";

    fn rc() -> &'static Path {
        Path::new(RC)
    }

    /// The bug the split exists to fix: a file whose only marked line is
    /// the ambient wiring must not report the alias wired.
    #[test]
    fn the_two_pieces_are_detected_independently() {
        let only_ambient = format!("_fufu_ambient() {{ {TRIGGER} }}  {MARKER}\n");
        assert_eq!(alias_wiring(&only_ambient, rc()), Wiring::NotWired);
        assert!(matches!(
            ambient_wiring(&only_ambient, rc()),
            Wiring::Wired { .. }
        ));

        let only_alias = format!("alias git='ff git'  {MARKER}\n");
        assert!(matches!(
            alias_wiring(&only_alias, rc()),
            Wiring::Wired { .. }
        ));
        assert_eq!(ambient_wiring(&only_alias, rc()), Wiring::NotWired);
    }

    /// What keeps fufu from touching a line a person wrote.
    #[test]
    fn a_hand_written_line_is_never_claimed() {
        assert_eq!(
            alias_wiring("alias git='ff git' # mine\n", rc()),
            Wiring::HandWritten
        );
        assert_eq!(
            ambient_wiring(&format!("{TRIGGER}\n"), rc()),
            Wiring::HandWritten
        );
    }

    /// PowerShell's alias is a function, and the rule is the same for it:
    /// marked and naming `git` is wired, unmarked and naming `ff git` is
    /// hand-written, and a function that forwards to something else is
    /// neither — that line belongs to whoever wrote it.
    #[test]
    fn a_git_function_is_the_powershell_alias() {
        let marked = format!("{}  {MARKER}\n", alias_line("powershell"));
        assert!(matches!(alias_wiring(&marked, rc()), Wiring::Wired { .. }));
        assert_eq!(
            alias_wiring("function git { ff git @args }  # mine\n", rc()),
            Wiring::HandWritten
        );
        assert_eq!(
            alias_wiring("function git { jog git @args }\n", rc()),
            Wiring::NotWired
        );
        let ambient = ambient_lines("powershell");
        assert_eq!(ambient.len(), 1);
        assert!(ambient[0].starts_with("if (-not (Test-Path Function:_fufu_prompt))"));
        assert!(matches!(
            ambient_wiring(&format!("{}  {MARKER}\n", ambient[0]), rc()),
            Wiring::Wired { .. }
        ));
    }

    /// The profile is one file: PowerShell 7's when it exists or when
    /// neither does, 5.1's only when it is the sole profile on disk, and
    /// under the Documents folder PowerShell resolves rather than the one
    /// under home.
    #[test]
    fn the_powershell_profile_is_one_file() {
        let home = Path::new("C:\\Users\\u");
        let docs = home.join("Documents");
        let seven = docs.join("PowerShell").join(PROFILE);
        let five = docs.join("WindowsPowerShell").join(PROFILE);
        let profile = |exists: &dyn Fn(&Path) -> bool| {
            powershell_profile(true, home, Some(&docs), None, exists)
        };
        assert_eq!(profile(&|p| p == seven), seven, "7 exists");
        assert_eq!(profile(&|p| p == five), five, "only 5.1 exists");
        assert_eq!(profile(&|p| p == seven || p == five), seven, "both");
        assert_eq!(profile(&|_| false), seven, "neither: 7 is created");

        let redirected = home.join("OneDrive").join("Documents");
        assert_eq!(
            powershell_profile(true, home, Some(&redirected), None, |_| false),
            redirected.join("PowerShell").join(PROFILE),
            "the known folder wins over <home>\\Documents"
        );
        assert_eq!(
            powershell_profile(true, home, None, None, |_| false),
            seven,
            "a failed lookup falls back to <home>\\Documents"
        );

        let home = Path::new("/home/u");
        assert_eq!(
            powershell_profile(false, home, None, None, |_| false),
            Path::new("/home/u/.config/powershell").join(PROFILE)
        );
        assert_eq!(
            powershell_profile(false, home, None, Some(Path::new("/xdg")), |_| false),
            Path::new("/xdg/powershell").join(PROFILE),
            "XDG_CONFIG_HOME is honored, and Documents is not consulted"
        );
    }

    /// A CRLF file stays CRLF through the two rewrites fufu does to a file
    /// it did not create whole.
    #[test]
    fn a_crlf_file_keeps_its_line_endings() {
        let legacy = format!("alias git='ff git'  {}", LEGACY_MARKERS[0]);
        let file = format!("# mine\r\n{legacy}\r\n");
        let upgraded = upgrade(&file);
        assert_eq!(
            upgraded,
            format!("# mine\r\nalias git='ff git'  {MARKER}\r\n")
        );

        let eol = line_ending(&upgraded);
        let kept: Vec<&str> = upgraded.lines().filter(|l| !is_marked(l)).collect();
        assert_eq!(format!("{}{eol}", kept.join(eol)), "# mine\r\n");
        assert_eq!(line_ending("a\nb\n"), "\n");
    }

    /// A stored spelling is accepted forever: an rc file written by an
    /// older fufu stays fufu-managed rather than becoming a line nobody
    /// will ever remove.
    #[test]
    fn every_shipped_marker_and_trigger_spelling_is_still_recognized() {
        for marker in LEGACY_MARKERS {
            let line = format!("alias git='ff git'  {marker}\n");
            assert!(
                matches!(alias_wiring(&line, rc()), Wiring::Wired { .. }),
                "{marker} must stay managed"
            );
        }
        for trigger in LEGACY_TRIGGERS {
            let line = format!("{trigger}  {}\n", LEGACY_MARKERS[0]);
            assert!(
                matches!(ambient_wiring(&line, rc()), Wiring::Wired { .. }),
                "{trigger} must stay managed"
            );
        }
    }

    /// The repair is one rewrite, so the rc file never sits without either
    /// spelling — and an unmarked line is left exactly as its author wrote
    /// it, retired command and all.
    #[test]
    fn upgrading_rewrites_only_fufus_own_lines() {
        let mine = format!("alias git='ff git'  {}\n", LEGACY_MARKERS[0]);
        let theirs = format!("{}  # mine\n", LEGACY_TRIGGERS[0]);
        let upgraded = upgrade(&format!("{mine}{theirs}"));
        assert!(upgraded.contains(&format!("alias git='ff git'  {MARKER}")));
        assert!(
            upgraded.contains(&theirs.trim_end().to_string()),
            "a hand-written line is untouched: {upgraded:?}"
        );
        assert!(!upgrade(&mine).lines().any(is_outdated));
    }

    #[test]
    fn one_slug_answers_for_two_pieces() {
        let wired = Wiring::Wired {
            mechanism: Mechanism::Rc,
            at: RC.into(),
        };
        assert!(matches!(combine(&wired, &wired), Wiring::Wired { .. }));
        assert!(matches!(
            combine(&wired, &Wiring::NotWired),
            Wiring::Partial { .. }
        ));
        assert_eq!(
            combine(&Wiring::NotWired, &Wiring::NotWired),
            Wiring::NotWired
        );
        assert_eq!(
            combine(&Wiring::HandWritten, &Wiring::NotWired),
            Wiring::HandWritten
        );
    }
}
