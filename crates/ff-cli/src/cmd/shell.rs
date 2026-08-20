//! `ff hook shell` — two independent pieces of rc-file wiring: the alias
//! (`alias git='ff git'`) and the ambient prompt hook (`ff hook shell
//! trigger`, run at every prompt). Marked-line rc editing: install appends
//! marked lines tagged with the fufu marker; uninstall removes exactly the
//! marked lines. A hand-written alias or hand-written prompt hook is
//! detected, respected, and never touched — independently of the other
//! piece. All paths are env-resolved (HOME, ZDOTDIR, XDG_CONFIG_HOME,
//! SHELL) so tests stay hermetic.

use std::io::IsTerminal;
use std::path::PathBuf;

use ff_core::{Error, Result};

use crate::cli::HookVerb;

const MARKER: &str = "# fufu — added by `ff hook shell install`";
/// The Phase 1 spelling, still recognized so older installs stay managed.
const LEGACY_MARKERS: [&str; 1] = ["# fufu — added by `ff shell install`"];
const SHELLS: [&str; 3] = ["bash", "zsh", "fish"];

fn is_marked(line: &str) -> bool {
    line.contains(MARKER) || LEGACY_MARKERS.iter().any(|m| line.contains(m))
}

fn env_path(var: &str) -> Option<PathBuf> {
    std::env::var_os(var)
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
}

fn home() -> Result<PathBuf> {
    env_path("HOME").ok_or_else(|| Error::msg("HOME is not set"))
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
        other => {
            return Err(Error::msg(format!(
                "unsupported shell {other:?} (supported: bash, zsh, fish)"
            )));
        }
    })
}

fn alias_line(shell: &str) -> &'static str {
    // fish aliases take a space-separated body; bash/zsh take `=`.
    if shell == "fish" {
        "alias git 'ff git'"
    } else {
        "alias git='ff git'"
    }
}

/// The un-marked bodies of the ambient prompt-hook wiring; the caller
/// appends `  {MARKER}` to each, exactly as install does for the alias.
/// Bash is guarded against double-prepending because a `.bashrc` can be
/// sourced more than once in one shell — the marker only protects the
/// *file*, not the runtime.
fn ambient_lines(shell: &str) -> &'static [&'static str] {
    match shell {
        "bash" => &[
            r#"[[ $PROMPT_COMMAND == *"ff hook shell trigger"* ]] || PROMPT_COMMAND="ff hook shell trigger;$PROMPT_COMMAND""#,
        ],
        "zsh" => &[
            "_fufu_ambient() { ff hook shell trigger }",
            "precmd_functions+=(_fufu_ambient)",
        ],
        "fish" => &["function _fufu_ambient --on-event fish_prompt; ff hook shell trigger; end"],
        _ => &[],
    }
}

fn default_shell() -> Result<String> {
    let shell = std::env::var("SHELL")
        .map_err(|_| Error::msg("SHELL is not set; pass a shell name (bash, zsh, fish)"))?;
    let name = std::path::Path::new(&shell)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("")
        .to_string();
    if name.is_empty() {
        return Err(Error::msg("could not determine the shell from $SHELL"));
    }
    Ok(name)
}

/// The state of one piece of shell wiring — the alias or the ambient
/// prompt hook, detected independently of the other.
#[derive(Debug, PartialEq)]
pub(crate) enum AliasState {
    Installed,
    HandWritten,
    Absent,
}

/// `Installed` requires a *marked* line that also names the alias — a file
/// whose only marked line is the ambient wiring must not report the alias
/// installed. (This is the bug the alias/ambient split exists to fix: the
/// old single `state_of` returned `Installed` for *any* marked line, so a
/// file with only the ambient hook installed lied about the alias too.)
fn alias_state(contents: &str) -> AliasState {
    for line in contents.lines() {
        if is_marked(line) && line.contains("alias git") {
            return AliasState::Installed;
        }
    }
    for line in contents.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("alias git") && trimmed.contains("ff git") {
            return AliasState::HandWritten;
        }
    }
    AliasState::Absent
}

/// `Installed` requires a marked line naming either the trigger command or
/// `_fufu_ambient` — zsh's wiring is two marked lines and only the first
/// mentions the command literally, so either alternative marks the whole
/// piece installed.
fn ambient_state(contents: &str) -> AliasState {
    for line in contents.lines() {
        if is_marked(line)
            && (line.contains("ff hook shell trigger") || line.contains("_fufu_ambient"))
        {
            return AliasState::Installed;
        }
    }
    for line in contents.lines() {
        if !is_marked(line) && line.contains("ff hook shell trigger") {
            return AliasState::HandWritten;
        }
    }
    AliasState::Absent
}

pub(crate) struct ShellAlias {
    pub(crate) shell: &'static str,
    pub(crate) state: AliasState,
    /// The prompt-hook line, detected independently of the alias.
    pub(crate) ambient: AliasState,
    /// None when the rc path cannot be resolved (HOME unset).
    pub(crate) rc: Option<std::path::PathBuf>,
}

pub(crate) fn alias_states() -> Vec<ShellAlias> {
    SHELLS
        .into_iter()
        .map(|shell| {
            let rc = rc_file(shell).ok();
            let (state, ambient) = match &rc {
                // Read the rc file once; both states are derived from the
                // same read.
                Some(path) => {
                    let contents = std::fs::read_to_string(path).unwrap_or_default();
                    (alias_state(&contents), ambient_state(&contents))
                }
                None => (AliasState::Absent, AliasState::Absent),
            };
            ShellAlias {
                shell,
                state,
                ambient,
                rc,
            }
        })
        .collect()
}

pub fn run(verb: HookVerb) -> Result<()> {
    match verb {
        HookVerb::Install { name } => install(&resolve_shell(name)?),
        HookVerb::Uninstall { name } => uninstall(&resolve_shell(name)?),
        HookVerb::List { name } => list(name.as_deref()),
        HookVerb::Trigger { .. } => {
            trigger();
            Ok(())
        }
    }
}

fn resolve_shell(arg: Option<String>) -> Result<String> {
    let shell = match arg {
        Some(shell) => shell,
        None => default_shell()?,
    };
    if !SHELLS.contains(&shell.as_str()) {
        return Err(Error::msg(format!(
            "unsupported shell {shell:?} (supported: bash, zsh, fish)"
        )));
    }
    Ok(shell)
}

fn install(shell: &str) -> Result<()> {
    let rc = rc_file(shell)?;
    let contents = std::fs::read_to_string(&rc).unwrap_or_default();
    let alias = alias_state(&contents);
    let ambient = ambient_state(&contents);

    // Handle the two pieces independently: whichever is absent gets its
    // marked lines queued for a single append at the end.
    let mut queued = String::new();

    match alias {
        AliasState::Installed => {
            println!("{shell}: alias already installed in {}", rc.display());
        }
        AliasState::HandWritten => {
            println!(
                "{shell}: {} already aliases git to ff by hand — leaving it alone",
                rc.display()
            );
        }
        AliasState::Absent => {
            queued.push_str(&format!("{}  {MARKER}\n", alias_line(shell)));
        }
    }

    match ambient {
        AliasState::Installed => {
            println!(
                "{shell}: ambient prompt hook already installed in {}",
                rc.display()
            );
        }
        AliasState::HandWritten => {
            println!(
                "{shell}: {} already calls ff hook shell trigger by hand — leaving it alone",
                rc.display()
            );
        }
        AliasState::Absent => {
            for line in ambient_lines(shell) {
                queued.push_str(&format!("{line}  {MARKER}\n"));
            }
        }
    }

    if !queued.is_empty() {
        if let Some(parent) = rc.parent() {
            std::fs::create_dir_all(parent).map_err(Error::repo)?;
        }
        let mut updated = contents;
        if !updated.is_empty() && !updated.ends_with('\n') {
            updated.push('\n');
        }
        updated.push_str(&queued);
        std::fs::write(&rc, updated).map_err(Error::repo)?;
        println!("restart the shell (or source the file) to activate it");
    }
    Ok(())
}

fn uninstall(shell: &str) -> Result<()> {
    let rc = rc_file(shell)?;
    let Ok(contents) = std::fs::read_to_string(&rc) else {
        println!("{shell}: nothing installed ({} not found)", rc.display());
        return Ok(());
    };
    let alias = alias_state(&contents);
    let ambient = ambient_state(&contents);
    let alias_installed = alias == AliasState::Installed;
    let ambient_installed = ambient == AliasState::Installed;

    if !alias_installed && !ambient_installed {
        println!("{shell}: nothing installed in {}", rc.display());
        if alias == AliasState::HandWritten {
            println!(
                "{shell}: the alias in {} was written by hand — not touching it",
                rc.display()
            );
        }
        if ambient == AliasState::HandWritten {
            println!(
                "{shell}: the prompt hook in {} was written by hand — not touching it",
                rc.display()
            );
        }
        return Ok(());
    }

    // A hand-written line is never marked, so it survives this filter
    // untouched with no special case — the same filter already removes
    // every marked line of a multi-line install, so no new removal logic
    // is needed for the ambient piece either.
    let kept: Vec<&str> = contents.lines().filter(|line| !is_marked(line)).collect();
    let mut updated = kept.join("\n");
    if contents.ends_with('\n') && !updated.is_empty() {
        updated.push('\n');
    }
    std::fs::write(&rc, updated).map_err(Error::repo)?;

    let what = match (alias_installed, ambient_installed) {
        (true, true) => "the fufu alias and prompt hook",
        (true, false) => "the fufu alias",
        (false, true) => "the fufu prompt hook",
        (false, false) => unreachable!("returned above when neither is installed"),
    };
    println!("{shell}: removed {what} from {}", rc.display());
    Ok(())
}

fn state_word(state: &AliasState) -> &'static str {
    match state {
        AliasState::Installed => "installed",
        AliasState::HandWritten => "hand-written (not fufu-managed)",
        AliasState::Absent => "not installed",
    }
}

fn list(only: Option<&str>) -> Result<()> {
    let filter: Option<&str> = match only {
        Some(name) => {
            if !SHELLS.contains(&name) {
                return Err(Error::msg(format!(
                    "unsupported shell {name:?} (supported: bash, zsh, fish)"
                )));
            }
            Some(name)
        }
        None => None,
    };
    for entry in alias_states()
        .into_iter()
        .filter(|e| filter.map(|f| e.shell == f).unwrap_or(true))
    {
        let rc = entry
            .rc
            .as_ref()
            .ok_or_else(|| Error::msg("HOME is not set"))?;
        println!(
            "{:<5} alias {}, ambient {}  ({})",
            entry.shell,
            state_word(&entry.state),
            state_word(&entry.ambient),
            rc.display()
        );
    }
    Ok(())
}

// ---------------------------------------------------------------------
// The trigger runtime: `ff hook shell trigger`, run at every shell prompt.
// ---------------------------------------------------------------------

/// The ambient channel's runtime. Speaks (to stderr) only when the verdict
/// it would report has changed since the last time it spoke; otherwise
/// silent. Never fails: every fallible step below degrades to silence.
fn trigger() {
    // Cheapest gate first, in exactly this order, because this runs on
    // every shell prompt: no-TTY is one fstat and no repository work at
    // all, so it goes first; repository discovery goes second because it's
    // still cheap and is the common case in a non-repo directory like
    // $HOME; repository config goes last because it cannot be read before
    // a repository has been discovered.
    if !std::io::stdout().is_terminal() {
        return;
    }
    let Ok(repo) = ff_core::discover(".") else {
        return;
    };
    let ambient = repo
        .config_snapshot()
        .boolean("fufu.ambient")
        .unwrap_or(true);
    if !ambient {
        return;
    }
    // A prompt hook that printed `ff: ...` on every prompt because a
    // repository is mid-rebase would be worse than useless — no error path
    // below may reach the CLI's error reporter.
    let _ = run_trigger(&repo);
}

fn run_trigger(repo: &ff_core::gix::Repository) -> Result<()> {
    let status = ff_core::status(repo)?;
    let branch = ff_core::snapshot::chain::chain_name(&status.head);
    let tip_hex = head_tip_hex(&status.head);

    // The same three inputs `ff status` uses to compute futures: branch, its
    // tip, and its open tree.
    let futures = match &status.head {
        ff_core::HeadState::Branch { commit, .. } => {
            let tip = ff_core::gix::ObjectId::from_hex(commit.as_bytes()).ok();
            let open = ff_core::futures::open_tree(repo, &branch)?;
            ff_core::futures::futures_for(repo, &branch, tip, open)?
        }
        _ => ff_core::futures::Futures {
            base: None,
            remote: None,
        },
    };

    let foreign = foreign_tip(repo);

    let fingerprint = fingerprint_of(&branch, &tip_hex, &futures, foreign);
    let path = repo.common_dir().join("fufu/ambient");
    let previous = std::fs::read_to_string(&path).unwrap_or_default();
    if previous == fingerprint {
        // Silent at almost every prompt: this is what "speaks at pause
        // points" means in practice.
        return Ok(());
    }

    crate::render::init_palette(repo);
    let colored = crate::pager::color_enabled();
    let mut message = String::new();
    // Nothing to sync is not news. `ff status` fills that silence with a dim
    // phrase because someone asked it a question; a prompt hook nobody asked
    // stays quiet — but the fingerprint below is still stored, so the next
    // prompt after something *does* change speaks exactly once.
    let sync = crate::render::sync_parts(&futures, colored);
    if !sync.is_empty() {
        message.push_str(&sync.join(" · "));
        message.push('\n');
    }
    if foreign {
        message.push_str("changes made outside fufu — ff status has the detail\n");
    }
    if !message.is_empty() {
        // stdout at a prompt belongs to whatever the user is about to run.
        eprint!("{message}");
    }

    // Best-effort: a fingerprint that cannot be stored must not fail the
    // command — it just means the next prompt repeats itself.
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(&path, &fingerprint);

    Ok(())
}

/// The commit HEAD resolves to, as hex — empty only when HEAD is unborn
/// (no commits yet). Detached HEAD still has a tip.
fn head_tip_hex(head: &ff_core::HeadState) -> String {
    match head {
        ff_core::HeadState::Branch { commit, .. } | ff_core::HeadState::Detached { commit } => {
            commit.clone()
        }
        ff_core::HeadState::Unborn { .. } => String::new(),
    }
}

/// Whether the operation log's tip is a foreign entry. Read-only mirror of
/// `cmd::status::reconcile_foreign`, minus the `reconcile` call that
/// precedes it there — the ambient channel must never write a ref.
fn foreign_tip(repo: &ff_core::gix::Repository) -> bool {
    (|| -> Option<bool> {
        let log = ff_core::ops::OpLog::open(repo).ok()?;
        let op = log.get(log.tip().ok().flatten()?).ok()?;
        Some(op.kind() == ff_core::ops::OpKind::Foreign)
    })()
    .unwrap_or(false)
}

/// The fields, joined with the unit separator (`\u{1f}`, this codebase's
/// existing delimiter for bench-style formats) that together are the
/// message's *identity* — not its payload. Only each verdict's kind is
/// included, never its payload: a branch that replays cleanly and then
/// gains one more clean commit has not changed verdict kind, so it must
/// not change the fingerprint either. Both axes contribute, so a remote
/// that moved while the base stood still is still news.
fn fingerprint_of(
    branch: &str,
    tip_hex: &str,
    futures: &ff_core::futures::Futures,
    foreign: bool,
) -> String {
    let axis = |f: &Option<ff_core::futures::Future>| match f {
        Some(f) => format!("{}:{}", f.against.tip, verdict_kind(&f.verdict)),
        None => String::new(),
    };
    let base = axis(&futures.base);
    let remote = axis(&futures.remote);
    [
        branch,
        tip_hex,
        base.as_str(),
        remote.as_str(),
        if foreign { "foreign" } else { "" },
    ]
    .join("\u{1f}")
}

/// The verdict's kind only, spelled the way `ff status --json`'s tag does.
fn verdict_kind(verdict: &ff_core::futures::Verdict) -> &'static str {
    use ff_core::futures::Verdict;
    match verdict {
        Verdict::UpToDate { .. } => "up-to-date",
        Verdict::FastForward { .. } => "fast-forward",
        Verdict::Clean { .. } => "clean",
        Verdict::Conflict { .. } => "conflict",
        Verdict::Unknown { .. } => "unknown",
        Verdict::Gone => "gone",
        Verdict::Unpublished => "unpublished",
        Verdict::Undone { .. } => "undone",
    }
}
