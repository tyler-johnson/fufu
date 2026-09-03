//! The neutral agent event: what every client's payload becomes once its
//! adapter is done with it, and the only thing the shared pipeline reads.
//!
//! Six of its fields are what the capture pipeline consumes — which event
//! this is, whose session it belongs to, which audience inside that session
//! is listening, which directory to discover a repository from, what the
//! snapshot's subject should say, and the shell command the tool carried
//! when it carried one, which is what the raw-git correction reads. The
//! last two, the tool's name and the file it named, are what the fan-out
//! puts on the wire for a subscriber to act on. A vendor field that nothing
//! downstream reads would be a field to keep in sync for nobody.

use std::path::{Path, PathBuf};

/// The events fufu has a use for, named after what they mean rather than
/// after any one vendor's spelling.
///
/// `SubagentStart` and `SessionEnd` are mapped by the adapters and consumed
/// by nothing but the capture floor — declared so an adapter has somewhere
/// honest to put an event it receives, on the same discipline that defers
/// editor hooks. `TurnEnd` is the next one with a use beyond capture: the
/// daily auto-trim rides `ContextStart` today, which puts an inline walk on
/// the agent's critical path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventKind {
    /// The context was built or rebuilt — a startup, a resume, a `/clear`,
    /// a fork, a compaction. Whatever was injected into the old context is
    /// gone, which is why this is not a `ContextStart`: a turn begins
    /// inside a context that already exists, and a boundary replaces one.
    SessionStart,
    /// A turn is starting and context can be injected.
    ContextStart,
    /// A tool is about to run. This is the one capture cannot miss.
    BeforeTool,
    SubagentStart,
    /// A turn ended — or a subagent's did. The last edit of a turn is only
    /// durable because this fires after it.
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
            "sessionstart" => EventKind::SessionStart,
            "contextstart" | "userpromptsubmit" | "beforesubmitprompt" => EventKind::ContextStart,
            "beforetool" | "pretooluse" | "beforeshellexecution" => EventKind::BeforeTool,
            "subagentstart" | "presubagent" => EventKind::SubagentStart,
            "turnend" | "stop" | "subagentstop" | "posttooluse" => EventKind::TurnEnd,
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
///
/// The fan-out spells the event's `path` field with it too: a subscriber
/// reads the same path the subject shows, and one spelling is easier to act
/// on than two.
pub(super) fn rela(path: &Path, workdir: Option<&Path>) -> String {
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
    /// Who inside that session is listening: a subagent's id, or empty for
    /// the main thread. A subagent inherits the parent's session id and is
    /// still an audience of its own, because nothing the parent was told
    /// reaches it.
    pub agent: String,
    /// The directory the repository is discovered from.
    pub cwd: PathBuf,
    pub label: Label,
    /// The tool's shell command, when the tool had one. `None` for
    /// everything else — a prompt, a file edit, a bare prompt hook.
    pub command: Option<String>,
    /// The tool's name as the client spelled it, on `BeforeTool` and `None`
    /// on every other kind. The capture floor reads the label; this is for
    /// the fan-out, where it is the name a subscription's matcher is tested
    /// against — so an event carrying no tool name, a shell prompt or a
    /// hand-taken snapshot, matches nothing.
    pub tool: Option<String>,
    /// The file the tool named, by whichever of the field names that client
    /// uses for it, and `None` when the tool named none. The label already
    /// renders it into a subject; a subscriber that had only the subject
    /// would be parsing one to find out which file moved.
    pub path: Option<PathBuf>,
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
        for spelling in ["UserPromptSubmit", "BeforeSubmitPrompt", "contextStart"] {
            assert_eq!(
                EventKind::from_hint(spelling),
                Some(EventKind::ContextStart)
            );
        }
        // A boundary is its own event: the runtime has to be able to tell
        // a rebuilt context from a turn inside one.
        for spelling in ["SessionStart", "sessionStart", "session_start"] {
            assert_eq!(
                EventKind::from_hint(spelling),
                Some(EventKind::SessionStart)
            );
        }
        for spelling in ["Stop", "SubagentStop", "PostToolUse"] {
            assert_eq!(EventKind::from_hint(spelling), Some(EventKind::TurnEnd));
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
