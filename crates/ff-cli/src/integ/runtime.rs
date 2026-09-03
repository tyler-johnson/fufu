//! The shared capture pipeline: everything after an adapter has parsed a
//! payload, and nothing that knows which vendor sent it.
//!
//! Discover the repository from the event's own `cwd`, capture with the
//! source's provenance, brief the audience in front of it if it has not
//! been briefed since the last context boundary, then ride the two
//! throttled ambient lanes. The manual source enters here too, with a
//! synthesized event, so there is one capture path and not two.

use std::io::Read;

use ff_core::{Error, Result};

use serde::{Deserialize, Serialize};

use super::briefing::NOTICE;
use super::{AgentEvent, AgentProtocol, EventKind, Reply, skill};
use crate::ctx::Ctx;

/// A payload larger than this is refused rather than read into memory.
const MAX_PAYLOAD: u64 = 8 * 1024 * 1024;

/// How many audiences one session's marker remembers. A session with more
/// subagents than this re-briefs the oldest, which costs one copy of the
/// notice; an unbounded list costs a file that grows forever.
const MAX_AUDIENCES: usize = 32;

/// What the pipeline did, for the one source that reports on itself.
pub struct Landed {
    pub repo: ff_core::gix::Repository,
    pub outcome: ff_core::CaptureOutcome,
}

/// The agent trigger's absolute contract, in one place: it always exits 0,
/// it says nothing about a failure unless `FF_DEBUG=1`, and it never vetoes
/// the action it fired on except where config said to — `fufu.gitPolicy
/// strict` and `fufu.toolPolicy strict` are the two vetoes there are, and
/// each travels as JSON the client may ignore rather than as an exit code.
pub fn agent(ctx: &Ctx, slug: &'static str, proto: &dyn AgentProtocol, forced: Option<EventKind>) {
    let payload = match read_payload() {
        Ok(payload) => payload,
        Err(err) => return complain(slug, &err),
    };
    agent_payload(ctx, slug, proto, forced, &payload);
}

/// The same runtime against a payload somebody else already read. The
/// legacy `ff hook claude` shim needs this: it has to look at stdin to tell
/// an install from a trigger, and stdin can only be read once.
pub fn agent_payload(
    ctx: &Ctx,
    slug: &'static str,
    proto: &dyn AgentProtocol,
    forced: Option<EventKind>,
    payload: &[u8],
) {
    if let Err(err) = run_agent(ctx, slug, proto, forced, payload) {
        complain(slug, &err);
    }
}

pub(super) fn complain(slug: &str, err: &Error) {
    if std::env::var_os("FF_DEBUG").is_some() {
        eprintln!("ff[debug]: {slug} trigger failed: {err}");
    }
}

fn run_agent(
    ctx: &Ctx,
    slug: &'static str,
    proto: &dyn AgentProtocol,
    forced: Option<EventKind>,
    payload: &[u8],
) -> Result<()> {
    let Some(event) = proto.parse(payload, forced)? else {
        return Ok(());
    };
    pipeline(ctx, slug, &event, Some(proto))?;
    Ok(())
}

/// Read the client's payload from stdin, capped.
fn read_payload() -> Result<Vec<u8>> {
    let mut buf = Vec::new();
    std::io::stdin()
        .lock()
        .take(MAX_PAYLOAD + 1)
        .read_to_end(&mut buf)
        .map_err(Error::repo)?;
    if buf.len() as u64 > MAX_PAYLOAD {
        return Err(Error::msg("hook payload exceeds 8MiB"));
    }
    Ok(buf)
}

/// Everything after the parse. Vendor-blind by construction: the only thing
/// it knows about the source is its name, which goes in the subject.
pub fn pipeline(
    ctx: &Ctx,
    source: &str,
    event: &AgentEvent,
    proto: Option<&dyn AgentProtocol>,
) -> Result<Landed> {
    let repo = ff_core::discover(&event.cwd)?;
    let detail = event.label.render(repo.workdir());
    let prov = crate::provenance::agent(ctx, source, &event.session, detail);
    // Contended and NoOp are outcomes, not failures; only a real error is
    // one, and for a client source even that stays silent.
    let outcome = ff_core::capture(&repo, &prov)?;

    // Everything fufu says, after the capture and never conditional on it:
    // a briefing or a correction fufu could not compute must never cost a
    // snapshot.
    //
    // The fan-out is on this side of the adapter rather than inside `speak`,
    // because a subscription buys being told and not being printed: a source
    // with no protocol at all — a shell prompt, a hand-taken snapshot — hands
    // the event out too, and every subscriber runs whether or not this client
    // has a channel for what they said.
    let subscribed = super::fanout::run(event, source, repo.workdir(), ctx.session.as_deref());
    if let Some(proto) = proto {
        speak(
            &repo,
            source,
            event,
            proto,
            ctx.session.as_deref(),
            subscribed,
        );
    }

    crate::selfupdate::notify::maybe_spawn_check(&repo);
    // A client trigger is often the only thing feeding a repository, so it
    // carries the auto-trim lane too — a daily inline walk is the price of
    // an engine that maintains itself.
    crate::autotrim::maybe_trim(&repo);

    Ok(Landed { repo, outcome })
}

/// The one place fufu prints on a client's stream.
///
/// The lanes contribute to one [`Reply`], the adapter renders it once, and
/// the briefing marker is stamped only if that rendering produced
/// something — three adapters answer `None` on `BeforeTool`, and a marker
/// stamped against a reply that never printed would lose that repository's
/// briefing permanently.
///
/// The stamp still lands *before* the print, so a crash between the two can
/// only under-notify. That direction is the right one: a missing briefing
/// costs the agent one context of spelling, and a repeated one costs
/// context on every turn.
fn speak(
    repo: &ff_core::gix::Repository,
    slug: &str,
    event: &AgentEvent,
    proto: &dyn AgentProtocol,
    session: Option<&str>,
    subscribed: Vec<String>,
) {
    let mut reply = Reply::new(event.kind);

    // The marker is loaded once, and two lanes may move it: the briefing
    // stamps an audience, and the tool steer's coach spends its one line.
    let mut marker = load_briefed(repo, slug);
    let mut dirty = false;
    if let Some(next) = briefing_due(&marker, event.kind, &event.session, &event.agent) {
        marker = next;
        dirty = true;
        // The skill line joins the notice before the envelope rather than
        // after it, because a client that wants JSON wants one field and
        // not two. It is asked of the adapter at print time: an install
        // and a disk can disagree, and naming a skill that is not there is
        // worse than saying nothing at all.
        let mut text = NOTICE.to_string();
        if proto.has_skill() {
            text.push_str(skill::LINE);
        }
        reply.context.push(text);
        // A declared extension's line is briefing, so it rides this same
        // boundary and this same marker rather than a lane of its own. Each
        // is its own entry, which is what puts each on its own line when the
        // adapter joins them.
        for line in super::briefing::extension_lines(&event.cwd, repo.workdir(), session) {
            reply.context.push(line);
        }
    }

    if event.kind == EventKind::BeforeTool
        && let Some(command) = event.command.as_deref()
    {
        correct(repo, &event.session, command, &mut reply);
        dirty |= steer(repo, &mut marker, command, &mut reply);
    }

    // A subscriber speaks after fufu does, in the registry's order. fufu's
    // own lines are the ones the agent has to have; an extension's are the
    // ones it asked for, and they are merged into the one reply this client
    // was going to get rather than printed beside it.
    reply.context.extend(subscribed);

    if reply.is_empty() {
        return;
    }
    let Some(envelope) = proto.reply_envelope(&reply) else {
        return;
    };
    if dirty && save_briefed(repo, slug, &marker).is_err() {
        return;
    }
    println!("{envelope}");
}

/// The raw-git correction: what the tier has to say about a `git …` the
/// agent is about to run.
///
/// Tallying happens under every tier, including `observe` — that is what
/// `observe` is for, and what `ff doctor`'s row reads. Only the speaking is
/// gated, and only a client with a documented channel is spoken to.
///
/// Every failure inside is swallowed the way the other ambient lanes'
/// failures are: this rides an event whose job is the snapshot.
fn correct(repo: &ff_core::gix::Repository, session: &str, command: &str, reply: &mut Reply) {
    let crate::rawgit::Shape::Write(word, _) = crate::rawgit::classify_command(command) else {
        return;
    };
    let policy = crate::gitpolicy::read(repo);
    let deny = policy == crate::gitpolicy::Policy::Strict;
    let fresh = crate::gitpolicy::record(repo, session, word.git, deny);
    if policy == crate::gitpolicy::Policy::Observe {
        return;
    }
    // A refusal is the answer and prints every time; a coach is a nudge and
    // spends itself the first time the word comes up this session.
    if !deny && !fresh {
        return;
    }
    if deny {
        reply.deny = Some(format!(
            "fufu.gitPolicy is strict here: run {} instead of git {} — {}",
            word.ff, word.git, word.why
        ));
    } else {
        reply.context.push(format!(
            "fufu: {} is what fufu has for git {} — {}",
            word.ff, word.git, word.why
        ));
    }
}

/// The tool steer: what `fufu.toolPolicy` has to say about an `ff …` the
/// agent is about to run in its shell while the `ff` tool is up for it.
///
/// Every check fails open, in this order: the command runs no `ff` the
/// tool serves; the tier is `observe`; the client did not say who it is
/// (`CLAUDE_PID`, which only Claude Code sets, and only Claude Code has a
/// deny channel, so no client sniffing is needed — a Cursor-launched
/// server writes a marker under Cursor's pid that nothing ever reads);
/// fufu's own server is not provably up for that client, which is the only
/// server asked about, since the tool is the only place this refusal sends
/// anything; or the reply already carries a refusal, since one reply
/// carries one reason and the git refusal stands.
///
/// Answers whether the marker changed — the coach spends its one line per
/// session on it, and the caller saves.
fn steer(
    repo: &ff_core::gix::Repository,
    marker: &mut Briefed,
    command: &str,
    reply: &mut Reply,
) -> bool {
    let Some(call) = crate::toolpolicy::classify(command) else {
        return false;
    };
    let policy = crate::toolpolicy::read(repo);
    if policy == crate::toolpolicy::Policy::Observe {
        return false;
    }
    let Some(client) = std::env::var("CLAUDE_PID")
        .ok()
        .and_then(|pid| pid.trim().parse::<u32>().ok())
    else {
        return false;
    };
    if !crate::cmd::mcp::presence::serving(client, crate::integ::mcp::NAME) {
        return false;
    }
    if reply.deny.is_some() {
        return false;
    }
    let args = serde_json::json!({ "args": call.args });
    let tool = crate::cmd::mcp::describe::CLAUDE_TOOL;
    if policy == crate::toolpolicy::Policy::Strict {
        // A declared extension is named, because the agent has no other way
        // to tell it from the undeclared one beside it that does run here.
        reply.deny = Some(match &call.extension {
            Some(name) => format!(
                "fufu.toolPolicy is strict here and the ff tool is up: {name} is a declared \
                 extension the tool serves, so call the ff tool ({tool}) with {args} instead \
                 of running ff in the shell — load the tool's schema first if it is deferred"
            ),
            None => format!(
                "fufu.toolPolicy is strict here and the ff tool is up: call the ff tool \
                 ({tool}) with {args} instead of running ff in the shell — load the tool's \
                 schema first if it is deferred"
            ),
        });
        return false;
    }
    if marker.tool_coached {
        return false;
    }
    marker.tool_coached = true;
    reply.context.push(format!(
        "fufu: the ff tool is up — call it with {args} instead of running ff in the shell"
    ));
    true
}

// ---- who has been briefed --------------------------------------------------

/// `.git/fufu/session/<slug>` — the audiences this client has briefed, and
/// the session they were briefed in.
///
/// Per-slug because two clients working in one repository would otherwise
/// each clobber the other's id and re-brief forever. Per-audience because a
/// subagent inherits the parent's session id and yet was told nothing: it
/// is a context of its own, so it is an entry of its own, with the main
/// thread under the empty name.
///
/// On the `gitpolicy.rs` template: serde struct, atomic temp-and-rename,
/// and every read failure yielding [`Briefed::default`] — which briefs.
/// A marker that could not be read must cost a duplicate notice and never
/// a missing one.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
struct Briefed {
    session: String,
    /// The audiences briefed this session: `""` is the main thread, the
    /// rest are agent ids.
    agents: Vec<String>,
    /// Whether `fufu.toolPolicy coach` has spent its one line this session.
    /// It resets with the audiences: a context boundary loses it too.
    tool_coached: bool,
}

fn marker_path(repo: &ff_core::gix::Repository, slug: &str) -> std::path::PathBuf {
    repo.git_dir().join("fufu/session").join(slug)
}

fn load_briefed(repo: &ff_core::gix::Repository, slug: &str) -> Briefed {
    std::fs::read(marker_path(repo, slug))
        .ok()
        .and_then(|data| serde_json::from_slice(&data).ok())
        .unwrap_or_default()
}

fn save_briefed(repo: &ff_core::gix::Repository, slug: &str, marker: &Briefed) -> Result<()> {
    let path = marker_path(repo, slug);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(Error::repo)?;
    }
    let body = serde_json::to_vec(marker).map_err(Error::repo)?;
    let tmp = path.with_extension("ff-tmp");
    {
        use std::io::Write;
        // Sync through the write handle: Windows refuses to flush a handle
        // opened read-only.
        let mut file = std::fs::File::create(&tmp).map_err(Error::repo)?;
        file.write_all(&body).map_err(Error::repo)?;
        file.sync_all().map_err(Error::repo)?;
    }
    std::fs::rename(&tmp, &path).map_err(Error::repo)?;
    Ok(())
}

/// Whether this event briefs, and the marker to stamp if it does. Split
/// from the disk the way `gitpolicy::mark` is, so the rule is testable
/// without one.
///
/// Three kinds carry a briefing, and they answer three different questions.
/// `SessionStart` is a context boundary — a startup, a resume, a `/clear`,
/// a fork, a compaction — and everything injected into the old context went
/// with it, so it resets unconditionally and briefs. It fires once per
/// boundary, so it cannot spam, and this needs no reading of the vendor's
/// `source` field to tell the five apart. `ContextStart` briefs the main
/// thread if nothing has. `BeforeTool` briefs whoever is making the call,
/// which is what reaches a subagent — and what reaches a repository the
/// agent has just `cd`'d into, since the marker lives in that repository's
/// own `.git` and it has none.
fn briefing_due(marker: &Briefed, kind: EventKind, session: &str, agent: &str) -> Option<Briefed> {
    let audience = match kind {
        // A boundary and a turn are both the main thread talking.
        EventKind::SessionStart | EventKind::ContextStart => "",
        EventKind::BeforeTool => agent,
        _ => return None,
    };
    let mut marker = marker.clone();
    if kind == EventKind::SessionStart || marker.session != session {
        marker.session = session.to_string();
        marker.agents.clear();
        marker.tool_coached = false;
    } else if marker.agents.iter().any(|seen| seen == audience) {
        return None;
    }
    marker.agents.push(audience.to_string());
    if marker.agents.len() > MAX_AUDIENCES {
        marker.agents.remove(0);
    }
    Some(marker)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The main thread hears it once, and a subagent inside the same
    /// session is an audience of its own.
    #[test]
    fn each_audience_is_briefed_once_per_session() {
        let mut marker = Briefed::default();
        marker = briefing_due(&marker, EventKind::ContextStart, "s1", "").expect("the first turn");
        assert!(
            briefing_due(&marker, EventKind::ContextStart, "s1", "").is_none(),
            "the main thread was already told"
        );
        assert!(
            briefing_due(&marker, EventKind::BeforeTool, "s1", "").is_none(),
            "and a tool call from the main thread is the same audience"
        );

        marker = briefing_due(&marker, EventKind::BeforeTool, "s1", "sub-1")
            .expect("a subagent was told nothing");
        assert!(
            briefing_due(&marker, EventKind::BeforeTool, "s1", "sub-1").is_none(),
            "and it hears it once too"
        );
        assert!(
            briefing_due(&marker, EventKind::BeforeTool, "s1", "sub-2").is_some(),
            "a second subagent is a second context"
        );
        // A fresh session id starts everyone over.
        assert!(briefing_due(&marker, EventKind::ContextStart, "s2", "").is_some());
    }

    /// A boundary rebuilt the context, so everything briefed into the old
    /// one is gone — even on a session id the marker already holds.
    #[test]
    fn a_boundary_rebriefs_a_session_it_already_holds() {
        let mut marker = Briefed::default();
        marker = briefing_due(&marker, EventKind::ContextStart, "s1", "").expect("the first turn");
        marker = briefing_due(&marker, EventKind::BeforeTool, "s1", "sub-1").expect("a subagent");
        marker = briefing_due(&marker, EventKind::SessionStart, "s1", "")
            .expect("a boundary briefs regardless");
        assert_eq!(marker.agents, vec![String::new()], "and it reset the rest");
        assert!(
            briefing_due(&marker, EventKind::ContextStart, "s1", "").is_none(),
            "the turn after it is silent again"
        );
    }

    /// Capture-only events have no briefing channel at all.
    #[test]
    fn a_capture_only_event_never_briefs() {
        let marker = Briefed::default();
        for kind in [
            EventKind::TurnEnd,
            EventKind::SubagentStart,
            EventKind::SessionEnd,
            EventKind::Other,
        ] {
            assert!(briefing_due(&marker, kind, "s1", "").is_none());
        }
    }

    /// A long session must not grow the marker without bound.
    #[test]
    fn the_audience_list_is_bounded() {
        let mut marker = Briefed::default();
        for n in 0..MAX_AUDIENCES + 4 {
            marker = briefing_due(&marker, EventKind::BeforeTool, "s1", &format!("sub-{n}"))
                .expect("a new audience");
        }
        assert_eq!(marker.agents.len(), MAX_AUDIENCES);
        assert_eq!(marker.agents.last().unwrap(), "sub-35");
        // The oldest went, and re-briefing it is the harmless direction.
        assert!(!marker.agents.iter().any(|a| a == "sub-0"));
    }
}
