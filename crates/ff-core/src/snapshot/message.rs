//! Snapshot commit messages: a provenance subject plus an optional body
//! listing oversize-skipped files. No timestamp or branch in the message —
//! the commit and the ref already carry those.

/// Subjects are capped so `log --oneline`-style rendering stays sane.
pub const MAX_SUBJECT: usize = 120;

/// Collapse whitespace runs to single spaces and cap at `max` characters,
/// appending `…` when truncated.
pub fn clean_subject(raw: &str, max: usize) -> String {
    let mut out = String::with_capacity(raw.len().min(max));
    let mut last_space = true; // leading whitespace is dropped
    for ch in raw.chars() {
        if ch.is_whitespace() {
            if !last_space {
                out.push(' ');
                last_space = true;
            }
        } else {
            out.push(ch);
            last_space = false;
        }
    }
    while out.ends_with(' ') {
        out.pop();
    }
    if out.chars().count() > max {
        out = out.chars().take(max.saturating_sub(1)).collect();
        while out.ends_with(' ') {
            out.pop();
        }
        out.push('…');
    }
    out
}

/// Build the full commit message: subject, plus a body listing skipped files.
pub fn build(subject: &str, skipped: &[String]) -> String {
    let subject = clean_subject(subject, MAX_SUBJECT);
    if skipped.is_empty() {
        return subject;
    }
    let mut msg = subject;
    msg.push_str("\n\nSkipped (fufu.maxFileSize):\n");
    for path in skipped {
        msg.push_str(path);
        msg.push('\n');
    }
    msg
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collapses_and_caps() {
        assert_eq!(clean_subject("  a\t\nb   c ", 120), "a b c");
        let long = "x".repeat(200);
        let capped = clean_subject(&long, 120);
        assert_eq!(capped.chars().count(), 120);
        assert!(capped.ends_with('…'));
    }

    #[test]
    fn body_lists_skips() {
        let msg = build("manual", &["big.bin".into()]);
        assert_eq!(msg, "manual\n\nSkipped (fufu.maxFileSize):\nbig.bin\n");
    }
}
