//! The shells: bash, zsh, fish.
//!
//! Three slugs, because the rc file and its syntax differ per shell, but
//! one trigger source, because all three install a line that calls the same
//! `ff trigger shell`. That is the clearest case for the two namespaces
//! being different: `hook` names a thing you integrate with, `trigger`
//! names an event source, and there is no reason those have to line up.
//!
//! Two independent pieces go into the rc file: the alias (`alias git='ff
//! git'`), so every git command you type snapshots first, and the ambient
//! prompt hook, so the shell can tell you what syncing would cost before
//! you ask. Marked-line editing: install appends lines carrying the fufu
//! marker, uninstall removes exactly the marked lines. A hand-written alias
//! or a hand-written prompt hook is detected, respected, and never touched
//! — independently of the other piece. Every path is env-resolved (HOME,
//! ZDOTDIR, XDG_CONFIG_HOME, SHELL) so tests stay hermetic.

use std::io::IsTerminal;
use std::path::PathBuf;

use ff_core::{Error, Result};

use super::{
    Change, EventKind, InstallOptions, Integration, Mechanism, Part, Presence, Status, Wiring,
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

pub const SHELLS: [&str; 3] = ["bash", "zsh", "fish"];

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
    // fish aliases take a space-separated body; bash and zsh take `=`.
    if shell == "fish" {
        "alias git 'ff git'"
    } else {
        "alias git='ff git'"
    }
}

/// The un-marked bodies of the ambient prompt-hook wiring; the caller
/// appends the marker to each, exactly as install does for the alias.
///
/// Bash is guarded against double-prepending because a `.bashrc` can be
/// sourced more than once in one shell — the marker only protects the
/// *file*, not the runtime.
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
        _ => Vec::new(),
    }
}

/// `Wired` requires a *marked* line that also names the alias — a file
/// whose only marked line is the ambient wiring must not report the alias
/// wired. (That is the bug the alias/ambient split exists to fix: a single
/// shared check returned installed for *any* marked line, so a file with
/// only the prompt hook lied about the alias too.)
fn alias_wiring(contents: &str, rc: &std::path::Path) -> Wiring {
    for line in contents.lines() {
        if is_marked(line) && line.contains("alias git") {
            return Wiring::Wired {
                mechanism: Mechanism::Rc,
                at: rc.to_path_buf(),
            };
        }
    }
    for line in contents.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("alias git") && trimmed.contains("ff git") {
            return Wiring::HandWritten;
        }
    }
    Wiring::NotWired
}

/// `Wired` requires a marked line naming either the trigger command or
/// `_fufu_ambient` — zsh's wiring is two marked lines and only the first
/// mentions the command literally, so either alternative marks the whole
/// piece wired.
fn ambient_wiring(contents: &str, rc: &std::path::Path) -> Wiring {
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
/// the three, so `ff hook` on an exotic login shell says so rather than
/// guessing.
pub fn default_shell() -> Option<&'static str> {
    let shell = std::env::var("SHELL").ok()?;
    let name = std::path::Path::new(&shell).file_name()?.to_str()?;
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

    /// All three shells feed one source: the rc lines they install differ in
    /// syntax and call the same command.
    fn source(&self) -> &'static str {
        "shell"
    }

    fn detect(&self) -> Presence {
        // The rc file is the evidence when there is one. A shell that is
        // the login shell but has never been configured still counts —
        // that is precisely the shell worth offering to wire.
        match self.rc() {
            Ok(rc) if rc.is_file() => Presence::Present { evidence: rc },
            Ok(rc) if default_shell() == Some(self.slug) => Presence::Present { evidence: rc },
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
            stale,
        }
    }

    fn install(&self, _opts: &InstallOptions) -> Result<Change> {
        let rc = self.rc()?;
        let original = std::fs::read_to_string(&rc).unwrap_or_default();
        // Rewrite retired spellings in place first, the way the settings
        // engine upgrades a legacy command: one write, so there is never a
        // moment where the rc file has neither the old line nor the new.
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
            _ => queued.push_str(&format!("{}  {MARKER}\n", alias_line(self.slug))),
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
                    queued.push_str(&format!("{line}  {MARKER}\n"));
                }
            }
        }

        if !queued.is_empty() || upgraded {
            if let Some(parent) = rc.parent() {
                std::fs::create_dir_all(parent).map_err(Error::repo)?;
            }
            let mut updated = contents;
            if !updated.is_empty() && !updated.ends_with('\n') {
                updated.push('\n');
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
        let kept: Vec<&str> = contents.lines().filter(|line| !is_marked(line)).collect();
        let mut updated = kept.join("\n");
        if contents.ends_with('\n') && !updated.is_empty() {
            updated.push('\n');
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

    /// The ambient channel, not an agent pipeline: it reads no payload, and
    /// its whole job is one status line at a shell prompt.
    fn trigger(&self, _ctx: &Ctx, _forced: Option<EventKind>) {
        ambient();
    }
}

/// Rewrite every retired spelling on fufu's own lines. A line nobody
/// marked is never touched, so a hand-written prompt hook naming the old
/// command keeps naming it — that line belongs to whoever wrote it.
fn upgrade(contents: &str) -> String {
    let mut out = String::with_capacity(contents.len());
    for (n, line) in contents.lines().enumerate() {
        if n > 0 {
            out.push('\n');
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
        out.push('\n');
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

// ---------------------------------------------------------------------
// The ambient runtime: `ff trigger shell`, run at every shell prompt.
// ---------------------------------------------------------------------

/// The ambient channel's runtime. Speaks (to stderr) only when the verdict
/// it would report has changed since the last time it spoke; otherwise
/// silent. Never fails: every fallible step below degrades to silence.
fn ambient() {
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
    let _ = run_ambient(&repo);
}

fn run_ambient(repo: &ff_core::gix::Repository) -> Result<()> {
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
            remote_unnamed: false,
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

#[cfg(test)]
mod tests {
    use super::*;

    const RC: &str = "/home/u/.bashrc";

    fn rc() -> &'static std::path::Path {
        std::path::Path::new(RC)
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
