//! Integrations: how fufu gets wired into the agent clients and shells on
//! this machine, and how those clients then feed the capture floor.
//!
//! Two namespaces, deliberately not the same one.
//!
//! **Slugs** are what `ff hook` and `ff unhook` take — `claude`, `codex`,
//! `cursor`, `gemini`, `bash`, `zsh`, `fish`. They are flat and permanent,
//! because they end up written inside config files fufu does not own. These
//! two verbs are for humans: an unknown slug is a real error, a failure is
//! loud, and `--json` emits a report envelope.
//!
//! **Sources** are what `ff trigger` takes — an event source, which is
//! finer-grained than a thing you integrate with. All three shell slugs
//! install rc lines calling one `ff trigger shell`; `manual` is a source
//! and not a slug, because there is nothing to install. `ff trigger` is
//! machine surface with one absolute contract: it always exits 0, it never
//! vetoes, and it says nothing about a failure unless `FF_DEBUG=1`.
//!
//! Nothing can collide across the two namespaces, which is what makes it
//! safe for them to be different.

use std::path::PathBuf;

use ff_core::{Error, Result};
use serde::Serialize;

use crate::ctx::Ctx;

pub mod briefing;
pub mod claude;
pub mod codex;
pub mod cursor;
pub mod event;
pub mod gemini;
pub mod manual;
pub mod payload;
pub mod runtime;
pub mod settings;
pub mod shell;
pub mod verbs;

pub use event::{AgentEvent, EventKind, Label};
pub use verbs::{hook, trigger, unhook};

// ---- what a status says ----------------------------------------------------

/// Whether the client is on this machine at all.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "kebab-case")]
pub enum Presence {
    Present { evidence: PathBuf },
    Absent,
}

impl Presence {
    pub fn is_present(&self) -> bool {
        matches!(self, Presence::Present { .. })
    }
}

/// How an integration is wired, when it is. Per-adapter and not a user
/// choice: each client has exactly one mechanism that works for it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Mechanism {
    /// A directory fufu owns entirely: install writes it whole, uninstall
    /// removes it. No foreign content to preserve, because there is none.
    Plugin,
    /// Entries merged into a settings file that belongs to the user.
    Settings,
    /// Marked lines appended to a shell rc file.
    Rc,
}

impl Mechanism {
    pub fn word(&self) -> &'static str {
        match self {
            Mechanism::Plugin => "plugin",
            Mechanism::Settings => "settings",
            Mechanism::Rc => "rc file",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "kebab-case")]
pub enum Wiring {
    NotWired,
    Wired {
        mechanism: Mechanism,
        at: PathBuf,
    },
    /// Some of the events are wired and some are not, which means capture
    /// is partial — the shape a half-finished install leaves behind.
    Partial {
        missing: String,
        at: PathBuf,
    },
    /// Somebody wrote this themselves. fufu reports it and never touches it.
    HandWritten,
    /// The wiring cannot be read at all: no HOME, or a file that is not
    /// valid JSON. Carries the complaint.
    Unavailable(String),
}

impl Wiring {
    /// Whether this piece actually feeds the capture floor. `Partial`
    /// counts: half-wired still captures on the events it has.
    pub fn feeds_capture(&self) -> bool {
        matches!(
            self,
            Wiring::Wired { .. } | Wiring::Partial { .. } | Wiring::HandWritten
        )
    }

    pub fn word(&self) -> String {
        match self {
            Wiring::NotWired => "not wired".into(),
            Wiring::Wired { mechanism, .. } => format!("wired ({})", mechanism.word()),
            Wiring::Partial { missing, .. } => format!("partial — {missing} missing"),
            Wiring::HandWritten => "hand-written (not fufu-managed)".into(),
            Wiring::Unavailable(complaint) => complaint.clone(),
        }
    }

    /// Where the wiring lives, when that is known.
    pub fn at(&self) -> Option<&std::path::Path> {
        match self {
            Wiring::Wired { at, .. } | Wiring::Partial { at, .. } => Some(at),
            _ => None,
        }
    }
}

/// One independently-wired piece of an integration that has more than one.
/// The shells are the only case: the alias and the ambient prompt hook are
/// detected, installed, and removed separately, and doctor reports them as
/// two findings because they answer two different questions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Part {
    pub name: &'static str,
    pub wiring: Wiring,
}

/// The single derivation `ff hook -l` and `ff doctor` both read. Two
/// renderings of one vector, so the two commands cannot disagree.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Status {
    pub slug: &'static str,
    pub presence: Presence,
    pub wiring: Wiring,
    /// Something true about this integration that a person needs told —
    /// Codex's trust step, Cursor's missing session start for cloud agents.
    /// Without it, capture can silently never happen with nothing saying why.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub parts: Vec<Part>,
    /// The wiring works, but it is written in a spelling install would
    /// rewrite — a retired command name, or the mechanism fufu has moved
    /// off. It keeps capturing, so this is never an outage; it is what
    /// `ff doctor --fix` repairs, because a user who never runs the
    /// installer again never gets rewritten by anything else.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub stale: bool,
}

/// What `ff hook` was asked for beyond the slugs themselves.
///
/// One field, and it exists because exactly one adapter has two mechanisms:
/// Claude's plugin is the default, and `--settings` is the way back to
/// settings entries if the plugin path ever misbehaves. Every other adapter
/// ignores it, because a mechanism is a property of the client rather than
/// a choice the user should be asked to make.
#[derive(Debug, Default, Clone, Copy)]
pub struct InstallOptions {
    pub settings: bool,
}

/// What an install or uninstall did, and what to say about it.
pub struct Change {
    pub changed: bool,
    pub lines: Vec<String>,
}

impl Change {
    pub fn changed(line: impl Into<String>) -> Change {
        Change {
            changed: true,
            lines: vec![line.into()],
        }
    }

    pub fn unchanged(line: impl Into<String>) -> Change {
        Change {
            changed: false,
            lines: vec![line.into()],
        }
    }

    pub fn absorb(&mut self, other: Change) {
        self.changed |= other.changed;
        self.lines.extend(other.lines);
    }
}

// ---- the two traits --------------------------------------------------------

/// Everything a slug can do. Install, detect, and status are shared by every
/// integration; parsing a payload is only for the ones that are agents, so
/// that half lives on a second trait a shell simply does not implement.
pub trait Integration: Sync {
    fn slug(&self) -> &'static str;

    /// The trigger source name this integration answers to. Defaults to the
    /// slug; the three shells override it to one shared `shell`.
    fn source(&self) -> &'static str {
        self.slug()
    }

    /// Is this client on the machine?
    fn detect(&self) -> Presence;

    fn status(&self) -> Status;

    fn install(&self, opts: &InstallOptions) -> Result<Change>;

    fn uninstall(&self, opts: &InstallOptions) -> Result<Change>;

    /// The consented repair behind `ff doctor --fix`.
    ///
    /// Separate from `install` because they answer different questions.
    /// `install` is a person asking for wiring and is free to choose the
    /// best mechanism; a repair is a person asking for what is already
    /// there to be made current, and must not move them onto a mechanism
    /// their running client will not pick up until it restarts. The
    /// default is install, because for every integration with one
    /// mechanism the two are the same thing.
    fn repair(&self) -> Result<Change> {
        self.install(&InstallOptions::default())
    }

    /// The payload dialect, for the integrations that are agent clients.
    fn protocol(&self) -> Option<&'static dyn AgentProtocol> {
        None
    }

    /// The runtime a trigger reaches. The default is the shared agent
    /// pipeline; `shell` overrides it with the ambient status line, which
    /// reads no payload at all.
    fn trigger(&self, ctx: &Ctx, forced: Option<EventKind>) {
        let Some(proto) = self.protocol() else { return };
        runtime::agent(ctx, self.slug(), proto, forced);
    }
}

/// How one vendor spells a payload, and how it wants to be spoken back to.
/// Everything between the two is vendor-blind and lives in `runtime`.
pub trait AgentProtocol: Sync {
    /// `forced` is the event hint from a `<vendor>-<event>` trigger name,
    /// when the name gave one. It supplies the event for a client whose
    /// payload does not carry its own, and overrides the payload's when it
    /// does.
    ///
    /// `Ok(None)` is a payload fufu has nothing to do with — a well-formed
    /// event of a kind no adapter maps. That is a success, not a failure.
    fn parse(&self, stdin: &[u8], forced: Option<EventKind>) -> Result<Option<AgentEvent>>;

    /// The briefing, wrapped however this client accepts injected context.
    /// Claude Code and Codex read plain stdout; Gemini and Cursor need JSON,
    /// which is why this is asked of the adapter rather than assumed.
    fn briefing_envelope(&self, text: &str) -> String;
}

// ---- the registry ----------------------------------------------------------

static CLAUDE: claude::Claude = claude::Claude;
static CODEX: codex::Codex = codex::Codex;
static CURSOR: cursor::Cursor = cursor::Cursor;
static GEMINI: gemini::Gemini = gemini::Gemini;
static BASH: shell::Shell = shell::Shell { slug: "bash" };
static ZSH: shell::Shell = shell::Shell { slug: "zsh" };
static FISH: shell::Shell = shell::Shell { slug: "fish" };

/// Every slug, in the order `ff hook -l` and `ff hook --all` walk them:
/// agent clients first, then shells.
pub fn all() -> [&'static dyn Integration; 7] {
    [&CLAUDE, &CODEX, &CURSOR, &GEMINI, &BASH, &ZSH, &FISH]
}

pub fn by_slug(slug: &str) -> Option<&'static dyn Integration> {
    all().into_iter().find(|i| i.slug() == slug)
}

/// Every slug's name, for the error a wrong one earns.
pub fn slugs() -> String {
    all()
        .iter()
        .map(|i| i.slug())
        .collect::<Vec<_>>()
        .join(", ")
}

/// The one status derivation. `ff hook -l` renders it as a table and
/// `ff doctor` renders it as rows, so the two cannot drift apart.
pub fn statuses() -> Vec<Status> {
    all().into_iter().map(|i| i.status()).collect()
}

// ---- trigger-name resolution ----------------------------------------------

/// What `ff trigger <name>` resolved to.
pub enum Source {
    /// The hand-taken snapshot: loud, `--json` capable, errors like any verb.
    Manual,
    /// A registered source, with the event forced from the name when the
    /// name carried one.
    Registered(&'static dyn Integration, Option<EventKind>),
}

/// Resolve a trigger name.
///
/// `None` means the name is unknown, and an unknown name is the published
/// extension point: exit 0, silently. That is what makes it safe to install
/// a fufu trigger into a client fufu has never heard of.
pub fn resolve_trigger(name: Option<&str>) -> Option<Source> {
    let Some(name) = name else {
        return Some(Source::Manual);
    };
    if name == manual::SOURCE {
        return Some(Source::Manual);
    }
    if let Some(integration) = all().into_iter().find(|i| i.source() == name) {
        return Some(Source::Registered(integration, None));
    }
    // `<vendor>-<event>`: split on the first `-`, require a known vendor,
    // and treat the tail as the event hint. A known vendor with a tail
    // nothing recognizes is still that vendor — the payload decides.
    let (vendor, event) = name.split_once('-')?;
    let integration = all().into_iter().find(|i| i.source() == vendor)?;
    Some(Source::Registered(integration, EventKind::from_hint(event)))
}

// ---- HOME ------------------------------------------------------------------

/// The user's home. Windows has `USERPROFILE` where unix has `HOME`, and
/// the rest of the tool reads its environment through `var_os` for the same
/// reason: a non-UTF-8 value is still a value.
pub fn home() -> Result<PathBuf> {
    std::env::var_os("HOME")
        .filter(|v| !v.is_empty())
        .or_else(|| std::env::var_os("USERPROFILE").filter(|v| !v.is_empty()))
        .map(PathBuf::from)
        .ok_or_else(|| Error::msg("HOME is not set"))
}

/// This binary's own path, quoted if it needs to be, for baking into a
/// client's config. Absolute rather than a bare `ff` so the wiring does not
/// depend on fufu being on whatever `PATH` the client happens to have.
pub fn exe_command(args: &str) -> String {
    let exe = std::env::current_exe()
        .ok()
        .map(|p| p.display().to_string())
        .filter(|p| !p.is_empty())
        .unwrap_or_else(|| "ff".to_string());
    if exe.contains(char::is_whitespace) {
        format!("\"{exe}\" {args}")
    } else {
        format!("{exe} {args}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_slug_is_unique_and_resolves() {
        let mut seen = std::collections::HashSet::new();
        for integration in all() {
            assert!(
                seen.insert(integration.slug()),
                "duplicate slug {:?}",
                integration.slug()
            );
            assert!(by_slug(integration.slug()).is_some());
        }
    }

    /// The two namespaces are separate on purpose, and this is the guard
    /// that they stay separate rather than accidentally identical: three
    /// shell slugs share one source name.
    #[test]
    fn the_three_shells_share_one_trigger_source() {
        for slug in ["bash", "zsh", "fish"] {
            assert_eq!(by_slug(slug).unwrap().source(), "shell");
        }
        assert!(matches!(
            resolve_trigger(Some("shell")),
            Some(Source::Registered(..))
        ));
        // And the shell slugs are not trigger names.
        assert!(resolve_trigger(Some("bash")).is_none());
    }

    #[test]
    fn bare_and_manual_are_the_same_source() {
        assert!(matches!(resolve_trigger(None), Some(Source::Manual)));
        assert!(matches!(
            resolve_trigger(Some("manual")),
            Some(Source::Manual)
        ));
    }

    #[test]
    fn a_vendor_event_name_forces_the_event() {
        let Some(Source::Registered(integration, forced)) =
            resolve_trigger(Some("claude-posttooluse"))
        else {
            panic!("claude-posttooluse resolves to the claude adapter");
        };
        assert_eq!(integration.slug(), "claude");
        assert_eq!(forced, Some(EventKind::TurnEnd));

        // A known vendor with an unrecognized tail is still that vendor;
        // the payload decides the event.
        let Some(Source::Registered(integration, forced)) =
            resolve_trigger(Some("claude-whatever-they-add-next"))
        else {
            panic!("a known vendor resolves whatever the tail says");
        };
        assert_eq!(integration.slug(), "claude");
        assert_eq!(forced, None);
    }

    #[test]
    fn an_unknown_name_resolves_to_nothing() {
        for name in ["notaclient", "notaclient-pretooluse", "-", "manual-x"] {
            assert!(
                resolve_trigger(Some(name)).is_none(),
                "{name:?} must exit 0 silently"
            );
        }
    }
}
