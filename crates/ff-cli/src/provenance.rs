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

/// `claude[<sess8>]: <detail>` — the agent-hook provenance. Unknown tools and
/// events are labeled honestly; the snapshot happens regardless.
///
/// The session id from the hook payload is used verbatim as the session
/// trailer (it says *which run*). The subject prefix stays unchanged — it
/// says *who*.
pub fn claude(ctx: &Ctx, session_id: &str, detail: String) -> Provenance {
    let sess: String = session_id.chars().take(8).collect();
    let source = if sess.is_empty() {
        "claude".to_string()
    } else {
        format!("claude[{sess}]")
    };
    let prov = Provenance::new(source, Some(detail));

    // Attach the agent's own session id as the session trailer.
    let session = if !session_id.is_empty() {
        crate::session::parse(session_id).ok()
    } else {
        None
    };
    // If the agent's id is unusable or empty, fall back to the invocation's
    // own session — the flag, or the environment behind it.
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
            command: "hook",
            at: None,
        }
    }

    #[test]
    fn claude_source_includes_session_prefix() {
        let p = claude(&ctx(), "0123456789abcdef", "Bash(ls)".into());
        assert_eq!(p.source, "claude[01234567]");
        assert_eq!(p.subject(), "claude[01234567]: Bash(ls)");
        let p = claude(&ctx(), "", "prompt \"hi\"".into());
        assert_eq!(p.subject(), "claude: prompt \"hi\"");
    }

    #[test]
    fn claude_prefers_the_hook_payload_id_over_the_invocation_session() {
        let ctx = Ctx {
            session: Some("from-the-flag".into()),
            ..ctx()
        };
        // A usable payload id is the session trailer, verbatim.
        let p = claude(&ctx, "0123456789abcdef", "Bash(ls)".into());
        assert_eq!(p.session.as_deref(), Some("0123456789abcdef"));
        // An empty or unusable one falls back to the invocation's session.
        assert_eq!(
            claude(&ctx, "", "Bash(ls)".into()).session.as_deref(),
            Some("from-the-flag")
        );
        assert_eq!(
            claude(&ctx, "a\nb", "Bash(ls)".into()).session.as_deref(),
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
