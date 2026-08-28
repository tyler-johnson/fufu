//! The payload shape three of the four clients share, and the tool-label
//! rendering all four do.
//!
//! Claude Code and Codex are field-for-field compatible — Codex even
//! aliases `CLAUDE_PLUGIN_ROOT` — so they parse through one struct. Gemini
//! CLI names the same fields and differs only in its event and tool
//! vocabulary, which the neutral `EventKind` already absorbs. Cursor is the
//! one that needs a struct of its own.
//!
//! Labels are derived from what the tool input *holds* rather than from the
//! tool's name, because the names are the part that differs per vendor and
//! the shapes are the part that does not: a tool carrying a command reads
//! as a command, a tool carrying a path reads as a path, and a tool
//! carrying neither is named honestly and snapshotted anyway.

use ff_core::{Error, Result};
use serde::Deserialize;

use super::{AgentEvent, EventKind, Label};

/// A command is a subject line, not a transcript.
const MAX_COMMAND: usize = 64;
/// A prompt is shorter still: it is the least informative of the three.
const MAX_PROMPT: usize = 60;

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct Payload {
    pub hook_event_name: String,
    pub session_id: String,
    pub cwd: String,
    pub tool_name: String,
    pub tool_input: ToolInput,
    pub prompt: String,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct ToolInput {
    pub command: String,
    pub file_path: String,
    pub notebook_path: String,
    /// Gemini's `replace` and Cursor's file tools spell it this way.
    pub path: String,
}

impl ToolInput {
    /// The path this tool names, whichever of the three spellings it used.
    pub fn any_path(&self) -> Option<&str> {
        [
            self.file_path.as_str(),
            self.notebook_path.as_str(),
            self.path.as_str(),
        ]
        .into_iter()
        .find(|p| !p.is_empty())
    }
}

/// Read the payload, refusing one with no `cwd` — there is no repository to
/// discover without it, so there is nothing honest to do.
pub fn parse_json<T: serde::de::DeserializeOwned>(stdin: &[u8]) -> Result<T> {
    serde_json::from_slice(stdin).map_err(Error::repo)
}

/// The label for one tool call.
pub fn tool_label(tool_name: &str, input: &ToolInput) -> Label {
    if !input.command.is_empty() {
        return Label::text(format!(
            "{tool_name}({})",
            crate::provenance::truncate(&input.command, MAX_COMMAND)
        ));
    }
    if let Some(path) = input.any_path() {
        return Label::Path {
            tool: tool_name.to_string(),
            path: path.into(),
        };
    }
    // A tool fufu has never heard of, or one whose input says nothing
    // useful: name it honestly. The snapshot happens either way, which is
    // the whole point of a floor.
    Label::text(format!("tool {tool_name}"))
}

/// The label for anything that is not a tool call.
pub fn event_label(kind: EventKind, event_name: &str, prompt: &str) -> Label {
    if kind == EventKind::ContextStart && !prompt.is_empty() {
        return Label::text(format!(
            "prompt \"{}\"",
            crate::provenance::truncate(prompt, MAX_PROMPT)
        ));
    }
    Label::text(format!("event {event_name}"))
}

/// The tool's shell command, verbatim and untruncated — the label's copy is
/// cut to a subject line, and a classifier reading a cut command line would
/// be reading a different command.
pub fn command_of(input: &ToolInput) -> Option<String> {
    (!input.command.is_empty()).then(|| input.command.clone())
}

/// The shared translation for the three clients that speak this dialect.
pub fn to_event(payload: &Payload, forced: Option<EventKind>) -> Result<Option<AgentEvent>> {
    if payload.cwd.is_empty() {
        return Err(Error::msg("hook payload has no cwd"));
    }
    // The name's hint wins when it was given: a `<vendor>-<event>` trigger
    // exists precisely for clients whose payload cannot say, or says the
    // wrong thing.
    let kind = forced
        .or_else(|| EventKind::from_hint(&payload.hook_event_name))
        .unwrap_or(EventKind::Other);
    let label = match kind {
        EventKind::BeforeTool => tool_label(&payload.tool_name, &payload.tool_input),
        _ => event_label(kind, &payload.hook_event_name, &payload.prompt),
    };
    Ok(Some(AgentEvent {
        kind,
        session: payload.session_id.clone(),
        cwd: payload.cwd.clone().into(),
        label,
        command: command_of(&payload.tool_input),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tool_carrying_a_command_reads_as_a_command() {
        let input = ToolInput {
            command: "rm -rf build && make".into(),
            ..Default::default()
        };
        assert_eq!(
            tool_label("Bash", &input),
            Label::text("Bash(rm -rf build && make)")
        );
    }

    #[test]
    fn a_tool_carrying_a_path_reads_as_a_path_whichever_field_it_used() {
        for input in [
            ToolInput {
                file_path: "/repo/a.rs".into(),
                ..Default::default()
            },
            ToolInput {
                notebook_path: "/repo/a.rs".into(),
                ..Default::default()
            },
            ToolInput {
                path: "/repo/a.rs".into(),
                ..Default::default()
            },
        ] {
            assert_eq!(
                tool_label("Edit", &input),
                Label::Path {
                    tool: "Edit".into(),
                    path: "/repo/a.rs".into()
                }
            );
        }
    }

    #[test]
    fn a_tool_saying_nothing_useful_is_named_honestly() {
        assert_eq!(
            tool_label("FutureTool", &ToolInput::default()),
            Label::text("tool FutureTool")
        );
    }

    #[test]
    fn a_payload_with_no_cwd_is_refused() {
        assert!(to_event(&Payload::default(), None).is_err());
    }
}
