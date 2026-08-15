//! Provenance strings — the CLI↔core contract for snapshot subjects.
//! Core caps subjects at 120 chars and collapses whitespace; the per-detail
//! truncation (commands ≤64, prompts ≤60) happens here.

use std::ffi::OsString;

use ff_core::Provenance;

/// `pre: ff <args>` — rebuilt from this process's own argv.
pub fn pre_ff() -> Provenance {
    let args: Vec<String> = std::env::args().collect();
    let mut summary = String::from("ff");
    for arg in args.iter().skip(1) {
        summary.push(' ');
        summary.push_str(arg);
    }
    Provenance::new("pre", Some(summary))
}

/// `pre: git <args>` for the passthrough.
pub fn pre_git(args: &[OsString]) -> Provenance {
    let mut summary = String::from("git");
    for arg in args {
        summary.push(' ');
        summary.push_str(&arg.to_string_lossy());
    }
    Provenance::new("pre", Some(summary))
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
pub fn claude(session_id: &str, detail: String) -> Provenance {
    let sess: String = session_id.chars().take(8).collect();
    let source = if sess.is_empty() {
        "claude".to_string()
    } else {
        format!("claude[{sess}]")
    };
    Provenance::new(source, Some(detail))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncates_with_ellipsis() {
        assert_eq!(truncate("short", 10), "short");
        assert_eq!(truncate("abcdefghij", 5), "abcd…");
    }

    #[test]
    fn claude_source_includes_session_prefix() {
        let p = claude("0123456789abcdef", "Bash(ls)".into());
        assert_eq!(p.source, "claude[01234567]");
        assert_eq!(p.subject(), "claude[01234567]: Bash(ls)");
        let p = claude("", "prompt \"hi\"".into());
        assert_eq!(p.subject(), "claude: prompt \"hi\"");
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
