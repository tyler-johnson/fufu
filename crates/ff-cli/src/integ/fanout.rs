//! The neutral agent event, handed to the extensions that subscribed to it.
//!
//! It runs after the capture and never before it, because a subscriber that
//! fails must not cost a snapshot. Each declared extension whose manifest
//! subscribed to this event is run as `ff-<name> trigger`, in the event's own
//! directory, with the event as one JSON object on one line on stdin
//! followed by EOF, and with the same three variables an extension is handed
//! anywhere else. Subscribing adds nothing to the environment: the event is
//! the payload. One handler covers all four clients, because the vendor's
//! spelling is gone by the time the event reaches it.
//!
//! Everything the spawn costs is [`crate::ext::ask_at`]'s: the child is
//! drained on threads of its own so it cannot deadlock fufu, and every way
//! it can fail — a binary that has left PATH, one that will not start, one
//! that exits nonzero, hangs, or prints something that is not one envelope —
//! is the same answer, which is nothing to say. That is `ff trigger`'s
//! doctrine applied to a subscriber, and it is why nothing here returns an
//! error.

use std::path::Path;
use std::time::{Duration, Instant};

use serde::Serialize;

use super::AgentEvent;
use crate::registry::Declared;

/// How long the whole fan-out has, across every subscriber.
///
/// The box is fufu's rather than an extension's, and it is one box rather
/// than one per subscriber: an agent pays this on every event, and on
/// `BeforeTool` that is a spawn per tool call on its critical path, so what
/// it pays has to be bounded by fufu and not by how many extensions somebody
/// declared. Half a second is two orders of magnitude more than printing a
/// line costs, short enough to disappear inside a tool call the agent was
/// making anyway, and half the briefing's, which is paid once per audience
/// per session rather than per event.
pub const BUDGET: Duration = Duration::from_millis(500);

/// Ask every subscriber, in the registry's order, and answer with what they
/// said — one entry per subscriber that had something to say.
///
/// The order is the order they were declared, which is what puts them in a
/// stable order in the agent's context. The common answer is an empty
/// vector: most machines declare nothing, and most events are ones a
/// subscriber has nothing to say about.
pub fn run(
    event: &AgentEvent,
    source: &str,
    workdir: Option<&Path>,
    session: Option<&str>,
) -> Vec<String> {
    // `Other` has no name in `events`, so nothing can subscribe to it and
    // there is nothing to spell on the wire either.
    let Some(kind) = crate::manifest::kind_name(event.kind) else {
        return Vec::new();
    };
    let subscribers: Vec<&Declared> = crate::registry::read()
        .declared()
        .iter()
        .filter(|declared| subscribed(declared, event))
        .collect();
    if subscribers.is_empty() {
        return Vec::new();
    }
    // Serialized once: every subscriber is handed the same bytes, and the
    // shape cannot drift between two of them.
    let Ok(wire) = serde_json::to_string(&wire(event, kind, source, workdir)) else {
        return Vec::new();
    };

    let deadline = Instant::now() + BUDGET;
    let mut left = subscribers.len();
    let mut said = Vec::new();
    for declared in subscribers {
        // What is left of the box, split evenly over the subscribers that
        // have not been asked yet. A share rather than the whole remainder
        // because the registry's order decides who is asked first and must
        // not also decide who is heard: one extension having a bad day would
        // otherwise starve every extension declared after it. One that
        // answers in a few milliseconds hands almost all of its share on.
        let share = deadline.saturating_duration_since(Instant::now()) / left as u32;
        left -= 1;
        if share.is_zero() {
            break;
        }
        let Some(path) = declared.resolve() else {
            continue;
        };
        let reply = crate::ext::ask_at(
            &path,
            &crate::ext::Ask {
                name: declared.name(),
                verb: "trigger",
                rest: &[],
                cwd: &event.cwd,
                repo: workdir,
                session,
                stdin: wire.as_bytes(),
                budget: share,
            },
        );
        if let Some(context) = reply.as_deref().and_then(context_of) {
            said.push(context);
        }
    }
    said
}

/// Whether this extension asked to be woken for this event.
///
/// The kind is most of the test: an extension names the kinds it wants and
/// fufu spawns it on those. A `BeforeTool` subscription carries the tool
/// names it wants as well, which is what keeps a spawn per tool call
/// proportionate to what the extension is for, and one subscription matching
/// is the whole of it — an extension declaring two is woken once.
fn subscribed(declared: &Declared, event: &AgentEvent) -> bool {
    let tool = event.tool.as_deref();
    declared
        .manifest
        .events
        .iter()
        .any(|sub| sub.wants(event.kind, tool))
}

/// The event as a subscriber reads it.
///
/// Every field is always present and a field that does not apply is `null`,
/// so a handler indexes rather than probes. `ff` is the contract version, the
/// same number `FF_CONTRACT` holds, so a handler can check what it is reading
/// without reaching for the environment; the event is deliberately not the
/// envelope itself, since an envelope is an answer and an event has no
/// failure branch to report.
#[derive(Serialize)]
struct Wire<'a> {
    ff: u32,
    kind: &'static str,
    source: &'a str,
    session: &'a str,
    agent: &'a str,
    cwd: String,
    label: String,
    tool: Option<&'a str>,
    command: Option<&'a str>,
    path: Option<String>,
}

/// Fill it in. Both paths are spelled the way every other fufu surface
/// spells one — forward slashes regardless of the host's — and `path` is
/// relative to the worktree when it is inside one, which is the spelling the
/// snapshot's subject already shows.
fn wire<'a>(
    event: &'a AgentEvent,
    kind: &'static str,
    source: &'a str,
    workdir: Option<&Path>,
) -> Wire<'a> {
    Wire {
        ff: crate::machine::CONTRACT,
        kind,
        source,
        session: &event.session,
        agent: &event.agent,
        cwd: ff_core::linked::path::as_git(&event.cwd),
        label: event.label.render(workdir),
        tool: event.tool.as_deref(),
        command: event.command.as_deref(),
        path: event
            .path
            .as_deref()
            .map(|path| super::event::rela(path, workdir)),
    }
}

/// The one field fufu reads out of a subscriber's reply.
///
/// stdout is read as one envelope on one line and nothing else, so a banner,
/// a progress line, a pretty-printed envelope, and an envelope carrying
/// `error` are all a subscriber with nothing to say. Any other field in
/// `data` is ignored rather than refused, so a later contract can define one
/// without breaking a handler written against this one.
fn context_of(said: &[u8]) -> Option<String> {
    let text = std::str::from_utf8(said).ok()?.trim();
    if text.contains('\n') {
        return None;
    }
    let envelope: serde_json::Value = serde_json::from_str(text).ok()?;
    if envelope.get("error").is_some_and(|err| !err.is_null()) {
        return None;
    }
    let context = envelope.get("data")?.get("context")?.as_str()?.trim();
    (!context.is_empty()).then(|| context.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::integ::{EventKind, Label};

    fn event(kind: EventKind) -> AgentEvent {
        AgentEvent {
            kind,
            session: "s-1".into(),
            agent: String::new(),
            cwd: "/repo/crates".into(),
            label: Label::Path {
                tool: "Edit".into(),
                path: "/repo/src/lib.rs".into(),
            },
            command: None,
            tool: Some("Edit".into()),
            path: Some("/repo/src/lib.rs".into()),
        }
    }

    /// The documented shape, field for field: ten of them, in order, with
    /// `null` where one does not apply.
    #[test]
    fn the_wire_carries_every_field_and_nulls_the_rest() {
        let event = event(EventKind::BeforeTool);
        let wire = wire(&event, "BeforeTool", "claude", Some(Path::new("/repo")));
        assert_eq!(
            serde_json::to_string(&wire).expect("serializes"),
            r#"{"ff":1,"kind":"BeforeTool","source":"claude","session":"s-1","agent":"","cwd":"/repo/crates","label":"Edit(src/lib.rs)","tool":"Edit","command":null,"path":"src/lib.rs"}"#
        );
    }

    /// A file outside the worktree keeps its whole path, exactly as the
    /// subject does.
    #[test]
    fn a_path_outside_the_worktree_stays_whole() {
        let event = event(EventKind::BeforeTool);
        let wire = wire(
            &event,
            "BeforeTool",
            "claude",
            Some(Path::new("/elsewhere")),
        );
        assert_eq!(wire.path.as_deref(), Some("/repo/src/lib.rs"));
    }

    /// One envelope on one line, and `data.context` out of it.
    #[test]
    fn the_reply_is_one_envelope_and_one_field() {
        assert_eq!(
            context_of(br#"{"ff":1,"cmd":"tower trigger","data":{"context":"tower: #73 is in progress."}}"#)
                .as_deref(),
            Some("tower: #73 is in progress.")
        );
        // A trailing newline is the normal case for a binary that echoed.
        assert_eq!(
            context_of(b"{\"ff\":1,\"data\":{\"context\":\"a line\"}}\n").as_deref(),
            Some("a line")
        );
        // Every other field in `data` is ignored rather than refused.
        assert_eq!(
            context_of(br#"{"ff":1,"data":{"later":7,"context":"a line"}}"#).as_deref(),
            Some("a line")
        );
    }

    /// Anything that is not one parsable envelope is a subscriber with
    /// nothing to say, and none of it is an error a caller has to handle.
    #[test]
    fn anything_else_says_nothing() {
        for said in [
            // Not JSON at all.
            &b"working..."[..],
            // A banner in front of the envelope.
            &b"ff-tower 0.1.0\n{\"ff\":1,\"data\":{\"context\":\"a line\"}}"[..],
            // Pretty-printed, so more than one line.
            &b"{\n  \"ff\": 1,\n  \"data\": {\"context\": \"a line\"}\n}"[..],
            // An envelope reporting a failure.
            &br#"{"ff":1,"cmd":"tower trigger","error":{"message":"no board"}}"#[..],
            // An envelope with nothing in the field fufu reads.
            &br#"{"ff":1,"data":{"other":"a line"}}"#[..],
            &br#"{"ff":1,"data":{"context":"   "}}"#[..],
            &br#"{"ff":1,"data":{"context":7}}"#[..],
            // Not text.
            &[0xff, 0xfe][..],
        ] {
            assert_eq!(context_of(said), None, "{}", String::from_utf8_lossy(said));
        }
    }
}
