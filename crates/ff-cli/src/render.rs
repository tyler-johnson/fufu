//! Human-readable rendering: plain rows, no TUI; the log family carries
//! ANSI color when the stream says so (see the palette below).

use ff_core::{
    ChangeKind, ChangeStat, FileStat, HeadState, LogEntry, OpenChange, Operation, ReconcileReport,
    SnapEntry, Status, Upstream,
};

/// Render a reconcile pass to stderr, loudly, before any verb output —
/// silent when there is nothing to say. Foreign motion is a fact the user
/// deserves to see exactly once per absorption (and pinned in `ff status`
/// until the next fufu op).
pub fn reconcile_notice(report: &ReconcileReport) {
    if report.is_quiet() {
        return;
    }
    let colored = !matches!(
        anstream::AutoStream::choice(&std::io::stderr()),
        anstream::ColorChoice::Never
    );
    for warning in &report.warnings {
        eprintln!("{}", paint_warn(&format!("ff: {warning}"), colored));
    }
    if report.bootstrapped && !report.reinitialized {
        eprintln!("ff: journal initialized; operations from here on are undoable");
    }
    if !report.foreign.is_empty() {
        eprintln!("ff: absorbed changes made outside fufu:");
        for change in &report.foreign {
            let what = match (&change.old, &change.new) {
                (Some(_), Some(new)) => {
                    let sha = &new[..new.len().min(8)];
                    format!("moved to {}", paint_sha(sha, colored))
                }
                (None, Some(new)) => {
                    let sha = &new[..new.len().min(8)];
                    format!("created at {}", paint_sha(sha, colored))
                }
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

/// The foreign-changes shape: `(ref_name, old_oid?, new_oid?)`.
pub type ForeignChanges = Option<Vec<(String, Option<String>, Option<String>)>>;

/// Bundled render inputs for `status_human` to avoid too-many-arguments.
pub struct StatusView<'a> {
    pub status: &'a Status,
    pub open: &'a OpenChange,
    pub change_stat: &'a ChangeStat,
    pub lens: &'a std::collections::HashMap<String, usize>,
    pub parent: Option<&'a LogEntry>,
    pub now: i64,
    pub colored: bool,
    pub foreign: ForeignChanges,
}

/// Render the new status view: header, open change row, diffstat, parent commit,
/// then conflicts and foreign blocks if present.
pub fn status_human(view: &StatusView<'_>) -> String {
    let StatusView {
        status,
        open,
        change_stat,
        lens,
        parent,
        now,
        colored,
        foreign,
    } = &view;
    let now = *now;
    let colored = *colored;
    let mut out = String::new();

    // Header line
    out.push_str(&status_header(status, colored));
    out.push('\n');

    // Open change row
    out.push_str(&change_row(open, lens, now, colored));
    out.push('\n');

    // Diffstat rows (rail rows) when files exist
    if !change_stat.files.is_empty() {
        out.push_str(&render_diffstat(change_stat, colored));
        out.push('\n');
    }

    // Parent commit row (only when born)
    if let Some(p) = parent {
        out.push_str(&commit_row(p, None, lens, now, colored));
        out.push('\n');
    }

    // Conflicts block
    if !status.conflicts.is_empty() {
        out.push_str("conflicts:\n");
        for c in &status.conflicts {
            out.push_str("  ");
            out.push_str(c);
            out.push('\n');
        }
    }

    // Foreign changes block
    if let Some(foreign) = foreign
        && !foreign.is_empty()
    {
        out.push_str("changes made outside fufu (absorbed; ff undo can roll them back):\n");
        for (name, old, new) in foreign {
            let what = match (old, new) {
                (_, Some(new)) => format!("moved to {}", &new[..new.len().min(8)]),
                (Some(_), None) => "deleted".to_string(),
                (None, None) => continue,
            };
            out.push_str("  ");
            out.push_str(name);
            out.push(' ');
            out.push_str(&what);
            out.push('\n');
        }
    }

    out
}

/// Build the header line: branch + upstream phrase + operation.
/// The upstream phrase is colored by state.
fn status_header(status: &Status, colored: bool) -> String {
    let mut parts: Vec<String> = Vec::new();
    parts.push(match &status.head {
        HeadState::Unborn { r#ref } => {
            let name = r#ref.strip_prefix("refs/heads/").unwrap_or(r#ref);
            format!("on {name} (no commits yet)")
        }
        HeadState::Branch { name, .. } => format!("on {name}"),
        HeadState::Detached { commit } => {
            format!("detached at {}", &commit[..commit.len().min(8)])
        }
    });
    if let Some(upstream) = &status.upstream {
        parts.push(colored_upstream_phrase(upstream, colored));
    }
    if let Some(op) = &status.operation {
        parts.push(operation_phrase(*op).to_string());
    }
    parts.join(" · ")
}

/// Build the upstream phrase, colored by state.
fn colored_upstream_phrase(u: &Upstream, colored: bool) -> String {
    let phrase = upstream_phrase(u);
    if !colored {
        return phrase;
    }
    let style = if u.gone {
        None
    } else if u.ahead == 0 && u.behind == 0 {
        Some(DIM)
    } else if u.ahead > 0 && u.behind == 0 {
        Some(palette().ahead)
    } else {
        None
    };
    match style {
        Some(s) => format!("{}{phrase}{}", s.render(), s.render_reset()),
        None => phrase,
    }
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

/// Render the diffstat block: file rows with kind, path, counts, bar, then summary.
fn render_diffstat(stat: &ChangeStat, colored: bool) -> String {
    let files = &stat.files;
    let rail = paint("│", DIM, colored);

    // Signed count strings: insertions get +, deletions get -
    let ins_str = |n: u32| format!("+{n}");
    let del_str = |n: u32| format!("-{n}");

    // Compute column widths from every value that appears in the column
    // (file rows AND the summary row), so the widest value dictates width.
    let max_path = files
        .iter()
        .map(|f| file_path_for_stat(f).chars().count())
        .max()
        .unwrap_or(0);
    let max_ins = files
        .iter()
        .map(|f| ins_str(f.insertions).chars().count())
        .max()
        .unwrap_or(ins_str(stat.insertions).chars().count())
        .max(ins_str(stat.insertions).chars().count());
    let max_del = files
        .iter()
        .map(|f| del_str(f.deletions).chars().count())
        .max()
        .unwrap_or(del_str(stat.deletions).chars().count())
        .max(del_str(stat.deletions).chars().count());

    // Determine max_total for bar scaling
    let max_total = files
        .iter()
        .map(|f| f.insertions + f.deletions)
        .max()
        .unwrap_or(0);

    let mut lines = Vec::new();

    for f in files {
        let path_display = file_path_for_stat(f);
        let path_padded = {
            let pad = " ".repeat(max_path.saturating_sub(path_display.chars().count()));
            format!("{path_display}{pad}")
        };

        let kind = paint(&format!("{}", kind_letter(f.kind)), DIM, colored);

        if f.binary {
            // Binary: dim "binary" in place of counts, no bar
            let bin = paint("binary", DIM, colored);
            let bin_col = {
                let width = max_ins + 2 + max_del; // ins + gap + del
                let pad = " ".repeat(width.saturating_sub(bin.chars().count()));
                format!("{bin}{pad}")
            };
            lines.push(format!("{rail}  {kind} {path_padded} {bin_col}"));
        } else {
            let ins = ins_str(f.insertions);
            let del = del_str(f.deletions);

            // Pad counts (right-aligned within column)
            let ins_padded = {
                let pad = " ".repeat(max_ins.saturating_sub(ins.chars().count()));
                format!("{pad}{ins}")
            };
            let del_padded = {
                let pad = " ".repeat(max_del.saturating_sub(del.chars().count()));
                format!("{pad}{del}")
            };

            // Color the counts
            let ins_colored = paint(&ins_padded, palette().ins, colored);
            let del_colored = paint(&del_padded, palette().del, colored);

            // Bar
            let bar = if max_total > 0 && (f.insertions > 0 || f.deletions > 0) {
                let total = f.insertions + f.deletions;
                let bar_len = (total as f64 * 20.0 / max_total as f64).round() as usize;
                let bar_len = bar_len.clamp(1, 20);

                let mut plus =
                    (bar_len as f64 * f.insertions as f64 / total as f64).round() as usize;
                let mut minus = bar_len - plus;

                // Force at least 1 on each side if that side has counts
                if f.insertions > 0 && plus == 0 {
                    plus = 1;
                    minus = bar_len.saturating_sub(1);
                }
                if f.deletions > 0 && minus == 0 {
                    minus = 1;
                    plus = bar_len.saturating_sub(1);
                }

                let plus_str = "+".repeat(plus);
                let minus_str = "-".repeat(minus);
                let plus_c = paint(&plus_str, palette().ins, colored);
                let minus_c = paint(&minus_str, palette().del, colored);
                format!("{plus_c}{minus_c}")
            } else {
                String::new()
            };

            let bar_prefix = if bar.is_empty() {
                String::new()
            } else {
                "  ".to_string()
            };

            lines.push(format!(
                "{rail}  {kind} {path_padded} {ins_colored}  {del_colored}{bar_prefix}{bar}"
            ));
        }
    }

    // Summary row — label starts in the path column (blank where kind letter goes)
    let file_count = files.len();
    let label = if file_count == 1 {
        "1 file"
    } else {
        &format!("{file_count} files")
    };
    let label_dim = paint(label, DIM, colored);
    // Pad label to max_path width
    let label_padded = {
        let width = label.chars().count();
        let pad = " ".repeat(max_path.saturating_sub(width));
        format!("{label_dim}{pad}")
    };

    let total_ins = ins_str(stat.insertions);
    let total_del = del_str(stat.deletions);
    let total_ins_padded = {
        let pad = " ".repeat(max_ins.saturating_sub(total_ins.chars().count()));
        format!("{pad}{total_ins}")
    };
    let total_del_padded = {
        let pad = " ".repeat(max_del.saturating_sub(total_del.chars().count()));
        format!("{pad}{total_del}")
    };
    let total_ins_colored = paint(&total_ins_padded, palette().ins, colored);
    let total_del_colored = paint(&total_del_padded, palette().del, colored);

    lines.push(format!(
        "{rail}    {label_padded} {total_ins_colored}  {total_del_colored}"
    ));

    lines.join("\n")
}

/// The display path for a stat row: "from => path" for renames/copies, else "path".
fn file_path_for_stat(f: &FileStat) -> String {
    if let Some(from) = &f.from {
        format!("{from} => {}", f.path)
    } else {
        f.path.clone()
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

/// Dim is a modifier, not a color — it is never themed.
const DIM: anstyle::Style = anstyle::Style::new().dimmed();

/// Nine semantic roles, each an ANSI style. Three themes are provided; the
/// process-global palette defaults to `MUTED` so every path works without
/// explicit initialization (tests, callers that forget, color-off pipes).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Palette {
    pub snap: anstyle::Style,
    pub sha: anstyle::Style,
    pub age: anstyle::Style,
    pub at: anstyle::Style,
    pub ins: anstyle::Style,
    pub del: anstyle::Style,
    pub ok: anstyle::Style,
    pub warn: anstyle::Style,
    pub ahead: anstyle::Style,
}

impl Palette {
    /// Desaturated 256-color — the default.
    pub const MUTED: Palette = Palette {
        snap: anstyle::Ansi256Color(139).on_default().bold(),
        sha: anstyle::Ansi256Color(67).on_default(),
        age: anstyle::Ansi256Color(73).on_default(),
        at: anstyle::Ansi256Color(71).on_default().bold(),
        ins: anstyle::Ansi256Color(71).on_default(),
        del: anstyle::Ansi256Color(167).on_default(),
        ok: anstyle::Ansi256Color(71).on_default(),
        warn: anstyle::Ansi256Color(173).on_default(),
        ahead: anstyle::Ansi256Color(67).on_default(),
    };

    /// Saturated 256-color — brighter, higher contrast.
    pub const VIVID: Palette = Palette {
        snap: anstyle::Ansi256Color(170).on_default().bold(),
        sha: anstyle::Ansi256Color(39).on_default(),
        age: anstyle::Ansi256Color(44).on_default(),
        at: anstyle::Ansi256Color(41).on_default().bold(),
        ins: anstyle::Ansi256Color(41).on_default(),
        del: anstyle::Ansi256Color(203).on_default(),
        ok: anstyle::Ansi256Color(41).on_default(),
        warn: anstyle::Ansi256Color(208).on_default(),
        ahead: anstyle::Ansi256Color(39).on_default(),
    };

    /// Base sixteen colors — lets the user's terminal theme decide the actual hues.
    pub const TERMINAL: Palette = Palette {
        snap: anstyle::AnsiColor::Magenta.on_default().bold(),
        sha: anstyle::AnsiColor::Blue.on_default(),
        age: anstyle::AnsiColor::Cyan.on_default(),
        at: anstyle::AnsiColor::Green.on_default().bold(),
        ins: anstyle::AnsiColor::Green.on_default(),
        del: anstyle::AnsiColor::Red.on_default(),
        ok: anstyle::AnsiColor::Green.on_default(),
        warn: anstyle::AnsiColor::Yellow.on_default(),
        ahead: anstyle::AnsiColor::Blue.on_default(),
    };
}

static PALETTE: std::sync::OnceLock<Palette> = std::sync::OnceLock::new();

/// Store the palette for the process. First call wins; subsequent calls are
/// silently ignored so a caller that initializes twice does not panic.
pub fn set_palette(p: Palette) {
    // OnceLock::set returns Err if already initialized — we drop it because
    // the first winner stands and a double-init is harmless.
    let _ = PALETTE.set(p);
}

/// The current palette, or `MUTED` when nothing was set.
pub fn palette() -> &'static Palette {
    PALETTE.get().unwrap_or(&Palette::MUTED)
}

/// Map a config string to a palette. Unrecognized values and `None` yield `MUTED`.
pub fn palette_for(name: Option<&str>) -> Palette {
    match name {
        Some(n) => match n.to_lowercase().as_str() {
            "vivid" => Palette::VIVID,
            "terminal" => Palette::TERMINAL,
            _ => Palette::MUTED,
        },
        None => Palette::MUTED,
    }
}

/// Read `fufu.theme` from the repo config and install the matching palette.
pub fn init_palette(repo: &ff_core::gix::Repository) {
    let theme = repo
        .config_snapshot()
        .string("fufu.theme")
        .map(|s| s.to_string());
    set_palette(palette_for(theme.as_deref()));
}

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
        col(base, SHA_WIDTH, palette().sha, colored),
        col_right(
            &relative_age(now, snap.time),
            AGE_WIDTH,
            palette().age,
            colored
        ),
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
    let sym = paint("@", palette().at, colored);
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
    let sha = col(pending_short, SHA_WIDTH, palette().sha, colored);
    let age = col_right(
        &open.time.map(|t| relative_age(now, t)).unwrap_or_default(),
        AGE_WIDTH,
        palette().age,
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
    let sha = col(short7(&entry.id), SHA_WIDTH, palette().sha, colored);
    let age = col_right(
        &relative_age(now, entry.time),
        AGE_WIDTH,
        palette().age,
        colored,
    );
    let sym = paint("●", palette().sha, colored);
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
        paint(head, palette().snap, colored),
        paint(tail, DIM, colored)
    )
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

/// Painter helpers — one-liners that command files use instead of touching
/// `anstyle` directly. Each takes `(text, colored)` and delegates to the
/// private `paint` with the correct semantic role from the current palette.
pub fn paint_sha(text: &str, colored: bool) -> String {
    paint(text, palette().sha, colored)
}

pub fn paint_ok(text: &str, colored: bool) -> String {
    paint(text, palette().ok, colored)
}

pub fn paint_warn(text: &str, colored: bool) -> String {
    paint(text, palette().warn, colored)
}

pub fn paint_dim(text: &str, colored: bool) -> String {
    paint(text, DIM, colored)
}

#[cfg(test)]
mod tests {
    use super::{Palette, palette_for, relative_age, styled_id};

    #[test]
    fn ages() {
        assert_eq!(relative_age(1000, 990), "10s ago");
        assert_eq!(relative_age(10_000, 100), "2h ago");
        assert_eq!(relative_age(1_000_000, 100), "1w ago");
        assert_eq!(relative_age(100_000_000, 100), "3y ago");
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

    #[test]
    fn palette_defaults_to_muted() {
        assert_eq!(palette_for(None), Palette::MUTED);
        assert_eq!(palette_for(Some("nonsense")), Palette::MUTED);
    }

    #[test]
    fn palette_parses_case_insensitively() {
        assert_eq!(palette_for(Some("VIVID")), Palette::VIVID);
        assert_eq!(palette_for(Some("Terminal")), Palette::TERMINAL);
        assert_eq!(palette_for(Some("muted")), Palette::MUTED);
    }

    // The OnceLock behind `palette()` is process-global, so a test that sets
    // it races every other test in the same binary — cargo runs them as
    // threads. `palette_for` is where the real mapping logic lives and it is
    // pure; the set-once plumbing is std behavior and is covered end-to-end by
    // the `ff config theme` integration tests instead.
}
