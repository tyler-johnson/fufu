//! The neutral agent event: what every client's payload becomes once its
//! adapter is done with it, and the only thing the shared pipeline reads.
//!
//! It carries four fields because the core consumes four things — which
//! event this is, whose session it belongs to, which directory to discover
//! a repository from, and what the snapshot's subject should say. A vendor
//! field that nothing downstream reads would be a field to keep in sync for
//! nobody.

use std::path::{Path, PathBuf};

/// The events fufu has a use for, named after what they mean rather than
/// after any one vendor's spelling.
///
/// `SubagentStart`, `TurnEnd`, and `SessionEnd` are mapped by the adapters
/// and consumed by nothing yet — declared so an adapter has somewhere
/// honest to put an event it receives, on the same discipline that defers
/// editor hooks. `TurnEnd` is the next one with a use: the daily auto-trim
/// rides `ContextStart` today, which puts an inline walk on the agent's
/// critical path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventKind {
    /// A turn or a session is starting and context can be injected.
    ContextStart,
    /// A tool is about to run. This is the one capture cannot miss.
    BeforeTool,
    SubagentStart,
    TurnEnd,
    SessionEnd,
    /// An event this adapter has no meaning for. It still captures, and the
    /// label names the event — a payload fufu does not recognize is exactly
    /// when a snapshot is worth the most, and a client that grows an event
    /// mid-release must not lose its floor waiting for fufu to catch up.
    Other,
}

impl EventKind {
    /// The event named by a `<vendor>-<event>` trigger name, for the clients
    /// whose payloads do not identify their own event. Punctuation and case
    /// are ignored, so `claude-posttooluse`, `claude-PostToolUse`, and
    /// `claude-post_tool_use` are one name.
    ///
    /// Every vendor's spelling of a given meaning maps to the same variant:
    /// the tail is a hint about what happened, not about who said it.
    pub fn from_hint(hint: &str) -> Option<EventKind> {
        let key: String = hint
            .chars()
            .filter(char::is_ascii_alphanumeric)
            .map(|c| c.to_ascii_lowercase())
            .collect();
        Some(match key.as_str() {
            "contextstart" | "sessionstart" | "userpromptsubmit" | "beforesubmitprompt" => {
                EventKind::ContextStart
            }
            "beforetool" | "pretooluse" | "beforeshellexecution" => EventKind::BeforeTool,
            "subagentstart" | "presubagent" => EventKind::SubagentStart,
            "turnend" | "stop" | "posttooluse" => EventKind::TurnEnd,
            "sessionend" => EventKind::SessionEnd,
            _ => return None,
        })
    }
}

/// A snapshot subject's detail, before the worktree is known.
///
/// Most details are finished text by the time the adapter has read the
/// payload. A tool naming a file is the exception: the path wants to be
/// relative to the worktree, and the worktree is discovered from the
/// event's own `cwd`, so it cannot be known until after the parse. Holding
/// the two apart keeps the adapters out of repository discovery.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Label {
    /// Already-rendered: `Bash(…)`, `prompt "…"`, `event PostToolUse`.
    Text(String),
    /// `Edit(src/x.rs)` — the path shown relative to the worktree.
    Path { tool: String, path: PathBuf },
}

impl Label {
    /// Convenience for the common case.
    pub fn text(text: impl Into<String>) -> Label {
        Label::Text(text.into())
    }

    /// The detail as it lands in the snapshot subject.
    pub fn render(&self, workdir: Option<&Path>) -> String {
        match self {
            Label::Text(text) => text.clone(),
            Label::Path { tool, path } => format!("{tool}({})", rela(path, workdir)),
        }
    }
}

/// A tool path relative to the worktree when it is inside one, absolute
/// otherwise. Provenance lands in commit subjects, so the separator is
/// git's regardless of the host's.
fn rela(path: &Path, workdir: Option<&Path>) -> String {
    if let Some(workdir) = workdir
        && let Ok(rel) = path.strip_prefix(workdir)
    {
        let text = rel.to_string_lossy();
        if !text.is_empty() {
            if cfg!(windows) {
                return text.replace('\\', "/");
            }
            return text.into_owned();
        }
    }
    path.to_string_lossy().into_owned()
}

/// One client event, translated.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentEvent {
    pub kind: EventKind,
    /// The vendor's session id, verbatim. Empty when the payload has none.
    pub session: String,
    /// The directory the repository is discovered from.
    pub cwd: PathBuf,
    pub label: Label,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_hints_ignore_case_and_punctuation() {
        for spelling in [
            "posttooluse",
            "PostToolUse",
            "post_tool_use",
            "post-tool-use",
        ] {
            assert_eq!(EventKind::from_hint(spelling), Some(EventKind::TurnEnd));
        }
        assert_eq!(EventKind::from_hint("nonsense"), None);
    }

    /// Every vendor's word for the same moment lands on the same variant.
    #[test]
    fn vendor_spellings_converge() {
        for spelling in ["PreToolUse", "BeforeTool", "preToolUse"] {
            assert_eq!(EventKind::from_hint(spelling), Some(EventKind::BeforeTool));
        }
        for spelling in ["UserPromptSubmit", "SessionStart", "sessionStart"] {
            assert_eq!(
                EventKind::from_hint(spelling),
                Some(EventKind::ContextStart)
            );
        }
    }

    #[test]
    fn a_tool_path_is_shown_relative_to_the_worktree() {
        let workdir = Path::new("/repo");
        let label = Label::Path {
            tool: "Edit".into(),
            path: PathBuf::from("/repo/src/lib.rs"),
        };
        assert_eq!(label.render(Some(workdir)), "Edit(src/lib.rs)");
        // Outside the worktree, and with no worktree at all, it stays whole.
        assert_eq!(
            label.render(Some(Path::new("/elsewhere"))),
            "Edit(/repo/src/lib.rs)"
        );
        assert_eq!(label.render(None), "Edit(/repo/src/lib.rs)");
    }
}
