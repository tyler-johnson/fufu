//! Human-readable rendering: plain rows, no TUI; the log family carries
//! ANSI color when the stream says so (see the palette below).

use ff_core::{
    ChangeKind, HeadState, LogEntry, OpenChange, Operation, ReconcileReport, SnapEntry, Status,
    StatusEntry, Upstream,
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

/// The palette, jj-adjacent: snapshot ids magenta (bold unique prefix, dim
/// rest), commit shas blue and plain, ages cyan, the working-copy `@` green,
/// rails dim — so a metadata line and a subject line never read as one.
const SNAP_UNIQUE: anstyle::Style = anstyle::AnsiColor::Magenta.on_default().bold();
const SHA: anstyle::Style = anstyle::AnsiColor::Blue.on_default();
const AGE: anstyle::Style = anstyle::AnsiColor::Cyan.on_default();
const AT: anstyle::Style = anstyle::AnsiColor::Green.on_default().bold();
const DIM: anstyle::Style = anstyle::Style::new().dimmed();

/// Paint `text`, or hand it back untouched when color is off or it's empty.
fn paint(text: &str, style: anstyle::Style, colored: bool) -> String {
    if !colored || text.is_empty() {
        return text.to_string();
    }
    format!("{}{text}{}", style.render(), style.render_reset())
}

/// A left-aligned column: pad FIRST (format-width would count escape bytes).
fn col(text: &str, width: usize, style: anstyle::Style, colored: bool) -> String {
    let pad = " ".repeat(width.saturating_sub(text.chars().count()));
    format!("{}{pad}", paint(text, style, colored))
}

/// A right-aligned column, same escape-safe padding.
fn col_right(text: &str, width: usize, style: anstyle::Style, colored: bool) -> String {
    let pad = " ".repeat(width.saturating_sub(text.chars().count()));
    format!("{pad}{}", paint(text, style, colored))
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
        "{} {} {}  {}",
        styled_id(&letters, unique, ID_WIDTH, colored),
        col(base, SHA_WIDTH, SHA, colored),
        col_right(&relative_age(now, snap.time), AGE_WIDTH, AGE, colored),
        snap.subject
    )
}

fn short7(hex: &str) -> &str {
    &hex[..hex.len().min(7)]
}

const ID_WIDTH: usize = 8;
const SHA_WIDTH: usize = 7;
const AGE_WIDTH: usize = 8;
const BLANK_ID: &str = "        ";

/// The `@` row (two lines): the open change. The sha column is the pending
/// commit hash (the change's own identity). Letters id = chain tip (via
/// `lens`), age = tip snapshot time. Clean + undescribed collapses to
/// `@  no changes`.
pub fn change_row(
    open: &OpenChange,
    lens: &std::collections::HashMap<String, usize>,
    now: i64,
    colored: bool,
) -> String {
    let sym = paint("@", AT, colored);
    let rail = paint("│", DIM, colored);
    let subject = match open.subject.as_deref() {
        Some(text) => text.to_string(),
        None => paint("(no description)", DIM, colored),
    };

    // Born + clean + no description: collapsed "no changes" line.
    if open.base.is_some() && open.clean && open.subject.is_none() {
        let head = format!("{sym}  {}", paint("no changes", DIM, colored));
        return format!("{}\n{rail}  {subject}", head.trim_end());
    }

    // Full layout: letters + pending sha + age + optional marker.
    let letters = match &open.id {
        Some(id) => styled_id(
            &ff_core::snapid::encode(&id[..id.len().min(ID_WIDTH)]),
            lens.get(id).copied().unwrap_or(1),
            ID_WIDTH,
            colored,
        ),
        None => BLANK_ID.to_string(),
    };
    let pending_short = open.pending.as_deref().map(short7).unwrap_or_default();
    let sha = col(pending_short, SHA_WIDTH, SHA, colored);
    let age = col_right(
        &open.time.map(|t| relative_age(now, t)).unwrap_or_default(),
        AGE_WIDTH,
        AGE,
        colored,
    );
    let marker = if open.base.is_none() {
        format!("  {}", paint("(no commits yet)", DIM, colored))
    } else {
        String::new()
    };
    let head = format!("{sym}  {letters} {sha} {age}{marker}");
    format!("{}\n{rail}  {subject}", head.trim_end())
}

/// One `●` commit row (two lines). The letters column is the commit's
/// chain-segment tip — the newest snapshot based on it, the evolog drill-in
/// anchor — blank when no snapshot was ever taken on this commit.
pub fn commit_row(
    entry: &LogEntry,
    segment: Option<&str>,
    lens: &std::collections::HashMap<String, usize>,
    now: i64,
    colored: bool,
) -> String {
    let letters = match segment {
        Some(id) => styled_id(
            &ff_core::snapid::encode(&id[..id.len().min(ID_WIDTH)]),
            lens.get(id).copied().unwrap_or(1),
            ID_WIDTH,
            colored,
        ),
        None => BLANK_ID.to_string(),
    };
    let sha = col(short7(&entry.id), SHA_WIDTH, SHA, colored);
    let age = col_right(&relative_age(now, entry.time), AGE_WIDTH, AGE, colored);
    let sym = paint("●", SHA, colored);
    let rail = paint("│", DIM, colored);
    let head = format!("{sym}  {letters} {sha} {age}");
    format!("{}\n{rail}  {}", head.trim_end(), entry.subject)
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

/// A snapshot id column: pad FIRST (format-width would count escape bytes),
/// then brighten the shortest-unique prefix and dim the rest — "the bold
/// part is what you can type". Snapshot ids only; commit shas are plain.
pub fn styled_id(display: &str, unique: usize, width: usize, colored: bool) -> String {
    let pad = " ".repeat(width.saturating_sub(display.chars().count()));
    if !colored {
        return format!("{display}{pad}");
    }
    let (head, tail) = display.split_at(unique.min(display.len()));
    format!(
        "{}{}{pad}",
        paint(head, SNAP_UNIQUE, colored),
        paint(tail, DIM, colored)
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
