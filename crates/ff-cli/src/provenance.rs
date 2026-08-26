//! Provenance strings — the CLI↔core contract for snapshot subjects.
//! Core caps subjects at 120 chars and collapses whitespace; the per-detail
//! truncation (commands ≤64, prompts ≤60) happens here.

use std::ffi::OsString;

use ff_core::Provenance;

use crate::ctx::Ctx;

/// `pre: ff <args>` — rebuilt from this process's own argv.
pub fn pre_ff(ctx: &Ctx) -> Provenance {
    let args: Vec<String> = std::env::args().collect();
    let mut summary = String::from("ff");
    for arg in args.iter().skip(1) {
        summary.push(' ');
        summary.push_str(arg);
    }
    let prov = Provenance::new("pre", Some(summary));
    prov.with_session(ctx.session.clone())
}

/// `pre: ff <name> <args…>` for a PATH-dispatched extension. Takes the
/// session directly because the command line failed to parse, so no `Ctx`
/// exists when it is called.
pub fn pre_ext(session: Option<String>) -> Provenance {
    let args: Vec<String> = std::env::args().collect();
    let mut summary = String::from("ff");
    for arg in args.iter().skip(1) {
        summary.push(' ');
        summary.push_str(arg);
    }
    let prov = Provenance::new("pre", Some(summary));
    prov.with_session(session)
}

/// `pre: git <args>` for the passthrough.
pub fn pre_git(ctx: &Ctx, args: &[OsString]) -> Provenance {
    let mut summary = String::from("git");
    for arg in args {
        summary.push(' ');
        summary.push_str(&arg.to_string_lossy());
    }
    let prov = Provenance::new("pre", Some(summary));
    prov.with_session(ctx.session.clone())
}

/// Truncate to at most `max` characters, appending `…` when cut.
pub fn truncate(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    let mut out: String = text.chars().take(max.saturating_sub(1)).collect();
    out.push('…');
    out
}

/// `<source>[<sess8>]: <detail>` — the provenance every trigger source
/// stamps on its capture.
///
/// The source keeps its own name — `claude[a1b2c3d4]: Edit(src/x.rs)`,
/// `codex[…]: Bash(…)`, `manual: before the risky bit` — because the
/// subject says *who*, and flattening four clients to one `agent[…]` would
/// lose the thing a subject is for.
///
/// The session id from the payload is used verbatim as the session trailer
/// (it says *which run*). An empty detail means the source has nothing to
/// add beyond its own name, which is what the bare manual snapshot is.
pub fn agent(ctx: &Ctx, source: &str, session_id: &str, detail: String) -> Provenance {
    let sess: String = session_id.chars().take(8).collect();
    let name = if sess.is_empty() {
        source.to_string()
    } else {
        format!("{source}[{sess}]")
    };
    let prov = Provenance::new(name, (!detail.is_empty()).then_some(detail));

    // Attach the client's own session id as the session trailer.
    let session = if !session_id.is_empty() {
        crate::session::parse(session_id).ok()
    } else {
        None
    };
    // If the client's id is unusable or empty, fall back to the
    // invocation's own session — the flag, or the environment behind it.
    let session = session.or_else(|| ctx.session.clone());
    prov.with_session(session)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncates_with_ellipsis() {
        assert_eq!(truncate("short", 10), "short");
        assert_eq!(truncate("abcdefghij", 5), "abcd…");
    }

    fn ctx() -> Ctx {
        Ctx {
            json: false,
            session: None,
            command: "trigger",
            at: None,
        }
    }

    #[test]
    fn the_source_keeps_its_own_name_and_its_session_prefix() {
        let p = agent(&ctx(), "claude", "0123456789abcdef", "Bash(ls)".into());
        assert_eq!(p.source, "claude[01234567]");
        assert_eq!(p.subject(), "claude[01234567]: Bash(ls)");
        let p = agent(&ctx(), "codex", "", "prompt \"hi\"".into());
        assert_eq!(p.subject(), "codex: prompt \"hi\"");
    }

    /// The manual source renders `manual` alone, or `manual: <label>` when
    /// `-m` was given.
    #[test]
    fn the_manual_source_is_bare_without_a_label() {
        assert_eq!(
            agent(&ctx(), "manual", "", String::new()).subject(),
            "manual"
        );
        assert_eq!(
            agent(&ctx(), "manual", "", "before the risky bit".into()).subject(),
            "manual: before the risky bit"
        );
    }

    #[test]
    fn a_trigger_prefers_the_payload_id_over_the_invocation_session() {
        let ctx = Ctx {
            session: Some("from-the-flag".into()),
            ..ctx()
        };
        // A usable payload id is the session trailer, verbatim.
        let p = agent(&ctx, "claude", "0123456789abcdef", "Bash(ls)".into());
        assert_eq!(p.session.as_deref(), Some("0123456789abcdef"));
        // An empty or unusable one falls back to the invocation's session.
        assert_eq!(
            agent(&ctx, "claude", "", "Bash(ls)".into())
                .session
                .as_deref(),
            Some("from-the-flag")
        );
        assert_eq!(
            agent(&ctx, "claude", "a\nb", "Bash(ls)".into())
                .session
                .as_deref(),
            Some("from-the-flag")
        );
    }

    #[test]
    fn provenance_subject_ignores_the_session() {
        let p = Provenance::new("manual", Some("hello".into()))
            .with_session(Some("refactor-parser".into()));
        assert_eq!(p.subject(), "manual: hello");
        // Session-only provenance also unaffected.
        let p2 = Provenance::new("pre", None).with_session(Some("x".into()));
        assert_eq!(p2.subject(), "pre");
    }
}
