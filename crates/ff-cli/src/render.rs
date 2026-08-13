//! Human-readable rendering: plain rows, no TUI, no color yet.

use ff_core::{
    ChangeKind, HeadState, LogEntry, Operation, ReconcileReport, SnapEntry, Status, StatusEntry,
    Upstream,
};

/// Render a reconcile pass to stderr, loudly, before any verb output —
/// silent when there is nothing to say. Foreign motion is a fact the user
/// deserves to see exactly once per absorption (and pinned in `ff status`
/// until the next fufu op).
pub fn reconcile_notice(report: &ReconcileReport) {
    if report.is_quiet() {
        return;
    }
    for warning in &report.warnings {
        eprintln!("ff: {warning}");
    }
    if report.bootstrapped && !report.reinitialized {
        eprintln!("ff: journal initialized; operations from here on are undoable");
    }
    if !report.foreign.is_empty() {
        eprintln!("ff: absorbed changes made outside fufu:");
        for change in &report.foreign {
            let what = match (&change.old, &change.new) {
                (Some(_), Some(new)) => format!("moved to {}", &new[..new.len().min(8)]),
                (None, Some(new)) => format!("created at {}", &new[..new.len().min(8)]),
                (Some(_), None) => "deleted".to_string(),
                (None, None) => "changed".to_string(),
            };
            match &change.hint {
                Some(hint) => eprintln!("  {} {what} ({hint})", change.name),
                None => eprintln!("  {} {what}", change.name),
            }
        }
    }
}

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

/// One snapshot row: `<letters8> <base7|blank> <age>  <subject>`, the
/// letters id styled so its shortest-unique prefix is what you can type at
/// `ff restore --at`. Shared by `ff evolog` and bare `ff` so the two never
/// diverge.
pub fn snap_row(
    snap: &SnapEntry,
    lens: &std::collections::HashMap<String, usize>,
    now: i64,
    colored: bool,
) -> String {
    let letters = ff_core::snapid::encode(&snap.id[..snap.id.len().min(8)]);
    let unique = lens.get(&snap.id).copied().unwrap_or(1);
    let base = snap.base.as_deref().map(short7).unwrap_or_default();
    format!(
        "{} {:<8} {:>8}  {}",
        styled_id(&letters, unique, 8, colored),
        base,
        relative_age(now, snap.time),
        snap.subject
    )
}

fn short7(hex: &str) -> &str {
    &hex[..hex.len().min(7)]
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

/// Style an id column: pad FIRST (format-width would count escape bytes),
/// then brighten the shortest-unique prefix and dim the rest — "the bold
/// part is what you can type".
pub fn styled_id(display: &str, unique: usize, width: usize, colored: bool) -> String {
    let pad = " ".repeat(width.saturating_sub(display.chars().count()));
    if !colored {
        return format!("{display}{pad}");
    }
    let (head, tail) = display.split_at(unique.min(display.len()));
    let bold = anstyle::Style::new().bold();
    let dim = anstyle::Style::new().dimmed();
    format!(
        "{}{head}{}{}{tail}{}{pad}",
        bold.render(),
        bold.render_reset(),
        dim.render(),
        dim.render_reset()
    )
}

/// Shortest-unique-prefix length per id: sort, then each id needs one more
/// character than its longest common prefix with either neighbor. Bijective
/// per-character encodings (hex ↔ letters) preserve these lengths.
pub fn unique_prefix_lens(ids: &[String]) -> std::collections::HashMap<String, usize> {
    let mut sorted: Vec<&str> = ids.iter().map(String::as_str).collect();
    sorted.sort_unstable();
    let common = |a: &str, b: &str| a.bytes().zip(b.bytes()).take_while(|(x, y)| x == y).count();
    let mut lens = std::collections::HashMap::new();
    for (i, id) in sorted.iter().enumerate() {
        let prev = if i > 0 { common(sorted[i - 1], id) } else { 0 };
        let next = if i + 1 < sorted.len() {
            common(id, sorted[i + 1])
        } else {
            0
        };
        lens.insert(id.to_string(), (prev.max(next) + 1).min(id.len().max(1)));
    }
    lens
}

pub fn relative_age(now: i64, then: i64) -> String {
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
    use super::{relative_age, styled_id, unique_prefix_lens};

    #[test]
    fn ages() {
        assert_eq!(relative_age(1000, 990), "10s ago");
        assert_eq!(relative_age(10_000, 100), "2h ago");
        assert_eq!(relative_age(1_000_000, 100), "1w ago");
        assert_eq!(relative_age(100_000_000, 100), "3y ago");
    }

    #[test]
    fn unique_prefixes() {
        let ids: Vec<String> = ["abcd", "abxy", "zzzz"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let lens = unique_prefix_lens(&ids);
        assert_eq!(lens["abcd"], 3, "ab is shared, abc is unique");
        assert_eq!(lens["abxy"], 3);
        assert_eq!(lens["zzzz"], 1);

        let one: Vec<String> = vec!["solo".into()];
        assert_eq!(unique_prefix_lens(&one)["solo"], 1);

        // Duplicates cap at the full length instead of overflowing it.
        let dup: Vec<String> = vec!["same".into(), "same".into()];
        assert_eq!(unique_prefix_lens(&dup)["same"], 4);
    }

    #[test]
    fn styled_id_pads_before_ansi() {
        // Plain: padding only.
        assert_eq!(styled_id("abc", 2, 5, false), "abc  ");
        // Colored: trailing pad spaces sit outside the escapes, and the
        // visible text is intact.
        let styled = styled_id("abc", 2, 5, true);
        assert!(styled.ends_with("  "), "pad after reset: {styled:?}");
        assert!(styled.contains("ab"), "prefix present");
        assert!(styled.contains('c'), "tail present");
        // Width shorter than the id: no pad, no truncation.
        assert_eq!(styled_id("abcdef", 3, 4, false), "abcdef");
    }
}
