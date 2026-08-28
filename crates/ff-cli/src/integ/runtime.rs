//! The shared capture pipeline: everything after an adapter has parsed a
//! payload, and nothing that knows which vendor sent it.
//!
//! Discover the repository from the event's own `cwd`, capture with the
//! source's provenance, brief once per session when the event is a context
//! start, then ride the two throttled ambient lanes. The manual source
//! enters here too, with a synthesized event, so there is one capture path
//! and not two.

use std::io::Read;

use ff_core::{Error, Result};

use super::briefing::NOTICE;
use super::{AgentEvent, AgentProtocol, Correction, EventKind, skill};
use crate::ctx::Ctx;

/// A payload larger than this is refused rather than read into memory.
const MAX_PAYLOAD: u64 = 8 * 1024 * 1024;

/// What the pipeline did, for the one source that reports on itself.
pub struct Landed {
    pub repo: ff_core::gix::Repository,
    pub outcome: ff_core::CaptureOutcome,
}

/// The agent trigger's absolute contract, in one place: it always exits 0,
/// it says nothing about a failure unless `FF_DEBUG=1`, and it never vetoes
/// the action it fired on except where config said to — `fufu.gitPolicy
/// strict` is the only veto there is, and it travels as JSON the client may
/// ignore rather than as an exit code.
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

    if event.kind == EventKind::ContextStart
        && let Some(proto) = proto
    {
        brief(&repo, source, &event.session, proto)?;
    }

    // The raw-git lane, after the capture and never conditional on it: a
    // correction fufu could not compute must never cost a snapshot.
    if event.kind == EventKind::BeforeTool
        && let Some(command) = event.command.as_deref()
    {
        correct(&repo, &event.session, command, proto);
    }

    crate::selfupdate::notify::maybe_spawn_check(&repo);
    // A client trigger is often the only thing feeding a repository, so it
    // carries the auto-trim lane too — a daily inline walk is the price of
    // an engine that maintains itself.
    crate::autotrim::maybe_trim(&repo);

    Ok(Landed { repo, outcome })
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
fn correct(
    repo: &ff_core::gix::Repository,
    session: &str,
    command: &str,
    proto: Option<&dyn AgentProtocol>,
) {
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
    let text = if deny {
        format!(
            "fufu.gitPolicy is strict here: run {} instead of git {} — {}",
            word.ff, word.git, word.why
        )
    } else {
        format!(
            "fufu: {} is what fufu has for git {} — {}",
            word.ff, word.git, word.why
        )
    };
    let Some(proto) = proto else { return };
    if let Some(envelope) = proto.correction_envelope(&Correction { text, deny }) {
        println!("{envelope}");
    }
}

/// Print the briefing at most once per session, per client. Answers
/// whether this invocation is the one that printed it.
///
/// The marker is per-slug — `.git/fufu/session/<slug>` — because two
/// clients working in one repository would otherwise each clobber the
/// other's id and re-brief forever.
///
/// The marker is written durably *before* the briefing prints, so a crash
/// between the two can only under-notify. That direction is the right one:
/// a missing briefing costs the agent one session of spelling, and a
/// repeated one costs context on every turn.
fn brief(
    repo: &ff_core::gix::Repository,
    slug: &str,
    session: &str,
    proto: &dyn AgentProtocol,
) -> Result<bool> {
    let marker = repo.git_dir().join("fufu/session").join(slug);
    if std::fs::read_to_string(&marker).is_ok_and(|prev| prev == session) {
        return Ok(false);
    }
    if let Some(parent) = marker.parent() {
        std::fs::create_dir_all(parent).map_err(Error::repo)?;
    }
    let tmp = marker.with_extension("tmp");
    {
        use std::io::Write;
        // Sync through the write handle: Windows refuses to flush a handle
        // opened read-only.
        let mut file = std::fs::File::create(&tmp).map_err(Error::repo)?;
        file.write_all(session.as_bytes()).map_err(Error::repo)?;
        file.sync_all().map_err(Error::repo)?;
    }
    std::fs::rename(&tmp, &marker).map_err(Error::repo)?;
    // The skill line joins the notice before the envelope rather than
    // after it, because a client that wants JSON wants one field and not
    // two. It is asked of the adapter at print time: an install and a
    // disk can disagree, and naming a skill that is not there is worse
    // than saying nothing at all.
    let mut text = NOTICE.to_string();
    if proto.has_skill() {
        text.push_str(skill::LINE);
    }
    print!("{}", proto.briefing_envelope(&text));
    Ok(true)
}
