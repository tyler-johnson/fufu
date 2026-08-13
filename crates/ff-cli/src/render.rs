//! Human-readable rendering: plain rows, no TUI, no color yet.

use ff_core::{ChangeKind, HeadState, LogEntry, Operation, Status, StatusEntry, Upstream};

pub fn status_human(status: &Status) -> String {
    let mut out = String::new();
    out.push_str(&header_line(status));
    out.push('\n');

    let clean = status.staged.is_empty()
        && status.unstaged.is_empty()
        && status.untracked.is_empty()
        && status.conflicts.is_empty();
    if clean {
        out.push_str("clean\n");
        return out;
    }

    section(&mut out, "conflicts", &status.conflicts, |p| {
        format!("  {p}")
    });
    entry_section(&mut out, "staged", &status.staged);
    entry_section(&mut out, "unstaged", &status.unstaged);
    section(&mut out, "untracked", &status.untracked, |p| {
        format!("  {p}")
    });
    out
}

fn header_line(status: &Status) -> String {
    let mut parts: Vec<String> = Vec::new();
    parts.push(match &status.head {
        HeadState::Unborn { r#ref } => {
            let name = r#ref.strip_prefix("refs/heads/").unwrap_or(r#ref);
            format!("on {name} (no commits yet)")
        }
        HeadState::Branch { name, .. } => format!("on {name}"),
        HeadState::Detached { commit } => format!("detached at {}", &commit[..commit.len().min(8)]),
    });
    if let Some(upstream) = &status.upstream {
        parts.push(upstream_phrase(upstream));
    }
    if let Some(op) = &status.operation {
        parts.push(operation_phrase(*op).to_string());
    }
    parts.join(" · ")
}

fn upstream_phrase(u: &Upstream) -> String {
    if u.gone {
        return format!("{} is gone", u.r#ref);
    }
    match (u.ahead, u.behind) {
        (0, 0) => format!("in sync with {}", u.r#ref),
        (a, 0) => format!("ahead {a} of {}", u.r#ref),
        (0, b) => format!("behind {b} of {}", u.r#ref),
        (a, b) => format!("ahead {a}, behind {b} of {}", u.r#ref),
    }
}

fn operation_phrase(op: Operation) -> &'static str {
    match op {
        Operation::ApplyMailbox => "applying mailbox",
        Operation::ApplyMailboxRebase => "applying mailbox (rebase)",
        Operation::Bisect => "bisecting",
        Operation::CherryPick | Operation::CherryPickSequence => "cherry-picking",
        Operation::Merge => "merging",
        Operation::Rebase | Operation::RebaseInteractive => "rebasing",
        Operation::Revert | Operation::RevertSequence => "reverting",
    }
}

fn entry_section(out: &mut String, title: &str, entries: &[StatusEntry]) {
    section(out, title, entries, |e| match (&e.from, e.kind) {
        (Some(from), _) => format!("  {}  {} -> {}", kind_letter(e.kind), from, e.path),
        (None, _) => format!("  {}  {}", kind_letter(e.kind), e.path),
    });
}

fn section<T>(out: &mut String, title: &str, items: &[T], row: impl Fn(&T) -> String) {
    if items.is_empty() {
        return;
    }
    out.push_str(title);
    out.push_str(":\n");
    for item in items {
        out.push_str(&row(item));
        out.push('\n');
    }
}

fn kind_letter(kind: ChangeKind) -> char {
    match kind {
        ChangeKind::Added => 'A',
        ChangeKind::Modified => 'M',
        ChangeKind::Deleted => 'D',
        ChangeKind::TypeChange => 'T',
        ChangeKind::Renamed => 'R',
        ChangeKind::Copied => 'C',
        ChangeKind::IntentToAdd => 'I',
    }
}

pub fn log_row(entry: &LogEntry, now: i64) -> String {
    format!(
        "{}  {:>8}  {}  {}",
        entry.short_id,
        relative_age(now, entry.time),
        entry.author_name,
        entry.subject
    )
}

fn relative_age(now: i64, then: i64) -> String {
    let delta = now - then;
    if delta < 0 {
        return "future".into();
    }
    const STEPS: &[(i64, &str)] = &[
        (60, "s"),
        (60 * 60, "m"),
        (60 * 60 * 24, "h"),
        (60 * 60 * 24 * 7, "d"),
        (60 * 60 * 24 * 30, "w"),
        (60 * 60 * 24 * 365, "mo"),
    ];
    let mut prev = 1;
    for &(limit, unit) in STEPS {
        if delta < limit {
            return format!("{}{unit} ago", delta / prev);
        }
        prev = limit;
    }
    format!("{}y ago", delta / (60 * 60 * 24 * 365))
}

#[cfg(test)]
mod tests {
    use super::relative_age;

    #[test]
    fn ages() {
        assert_eq!(relative_age(1000, 990), "10s ago");
        assert_eq!(relative_age(10_000, 100), "2h ago");
        assert_eq!(relative_age(1_000_000, 100), "1w ago");
        assert_eq!(relative_age(100_000_000, 100), "3y ago");
    }
}
