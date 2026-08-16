//! Human-readable rendering: plain rows, no TUI; the log family carries
//! ANSI color when the stream says so (see the palette below).

use ff_core::{
    ChangeKind, ChangeStat, FileStat, HeadState, InProgress, LogEntry, ReconcileReport, SnapEntry,
    Status,
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
        eprintln!("ff: operation log initialized; operations from here on are undoable");
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

/// The data `change_row` needs, extracted from whatever source holds it
/// (the status model or an ff-core \`OpenChange\`).
pub struct ChangeRowDisplay<'a> {
    pub subject: Option<&'a str>,
    pub born: bool,
    pub clean: bool,
    pub id: Option<&'a str>,
    pub pending: Option<&'a str>,
    pub time: Option<i64>,
}

/// The data `commit_row` needs, extracted from a \`LogEntry\` or the status model.
pub struct CommitRowDisplay<'a> {
    pub id: &'a str,
    pub subject: &'a str,
    pub time: i64,
}

/// Bundled render inputs for `status_human`: presentation parameters and a
/// borrow of the shared data model.
pub struct StatusView<'a> {
    pub model: &'a crate::cmd::status::StatusModel,
    pub lens: &'a std::collections::HashMap<String, usize>,
    pub now: i64,
    pub colored: bool,
}

/// Render the new status view: header, open change row, diffstat, parent commit,
/// then conflicts and foreign blocks if present.
pub fn status_human(view: &StatusView<'_>) -> String {
    let StatusView {
        model,
        lens,
        now,
        colored,
    } = view;
    let now = *now;
    let colored = *colored;

    // Reconstruct the types the helper functions expect from model fields.
    let status = Status {
        head: model.head.clone(),
        operation: model.operation,
        upstream: model.upstream.clone(),
        staged: vec![],
        unstaged: vec![],
        untracked: vec![],
        conflicts: model.conflicts.clone(),
    };
    let change_stat = ChangeStat {
        files: model.changes.clone(),
        insertions: model.insertions,
        deletions: model.deletions,
    };

    let mut out = String::new();

    // Header line
    out.push_str(&status_header(&status, &model.futures, colored));
    out.push('\n');

    // Open change row
    let change_row_display = ChangeRowDisplay {
        subject: model.open.subject.as_deref(),
        born: model.open.base.is_some(),
        clean: model.open.clean,
        id: model.open.id.as_deref(),
        pending: model.open.pending.as_deref(),
        time: model.open.time,
    };
    out.push_str(&change_row(&change_row_display, lens, now, colored));
    out.push('\n');

    // Diffstat rows (rail rows) when files exist
    if !model.changes.is_empty() {
        out.push_str(&render_diffstat(&change_stat, colored));
        out.push('\n');
    }

    // Parent commit row (only when born)
    if let Some(parent) = &model.parent {
        let commit_display = CommitRowDisplay {
            id: &parent.id,
            subject: &parent.subject,
            time: parent.time,
        };
        out.push_str(&commit_row(&commit_display, None, lens, now, colored));
        out.push('\n');
    }

    // Conflicts block
    if !model.conflicts.is_empty() {
        out.push_str("conflicts:\n");
        for c in &model.conflicts {
            out.push_str("  ");
            out.push_str(c);
            out.push('\n');
        }
    }

    // Foreign changes block
    if let Some(foreign) = &model.foreign
        && !foreign.is_empty()
    {
        out.push_str("changes made outside fufu (absorbed; ff undo can roll them back):\n");
        for entry in foreign {
            let what = match (&entry.old, &entry.new) {
                (_, Some(new)) => format!("moved to {}", &new[..new.len().min(8)]),
                (Some(_), None) => "deleted".to_string(),
                (None, None) => continue,
            };
            out.push_str("  ");
            out.push_str(&entry.r#ref);
            out.push(' ');
            out.push_str(&what);
            out.push('\n');
        }
    }

    out
}

/// What `ff sync` would do, in the two nouns a person learns once: the
/// **base** this work sits on, and the **remote** copy of this same branch.
/// One part per axis, in that order; an axis sync would not act on
/// contributes nothing. `ff status` and the ambient shell channel share this
/// renderer, so a prompt can never word a verdict differently from the
/// command.
pub fn sync_parts(futures: &ff_core::futures::Futures, colored: bool) -> Vec<String> {
    [futures.base.as_ref(), futures.remote.as_ref()]
        .into_iter()
        .flatten()
        .filter_map(|f| axis_phrase(f, colored))
        .collect()
}

/// One axis's phrase, or `None` when `ff sync` would not act on it.
fn axis_phrase(f: &ff_core::futures::Future, colored: bool) -> Option<String> {
    use ff_core::futures::{At, Role, Verdict};

    let role = f.against.role;
    // The role word, carrying a name only when the name is news — a base that
    // is not trunk, a remote that is not this branch's own copy. Every other
    // time the name is noise. Ref syntax never appears: `origin/feature` is a
    // cache of what a remote held at last fetch wearing a branch's name, and
    // making a person reconcile that by hand is the confusion fufu deletes.
    let which = match role {
        Role::Trunk => "base".to_string(),
        Role::Parent => format!("base {}", f.against.name),
        Role::Remote => "remote".to_string(),
        Role::RemoteAlias => format!("remote {}", f.against.name),
    };
    // Push and pull count against a place, so an aliased remote names it
    // inline rather than wearing the `remote <name>` prefix.
    let toward = |preposition: &str| match role {
        Role::RemoteAlias => format!(" {preposition} {}", f.against.name),
        _ => String::new(),
    };

    Some(match &f.verdict {
        // Sync never merges you into your base, so unmerged work is a
        // branch's permanent condition rather than pending work — and a line
        // that reported it every time would teach people to stop reading it.
        Verdict::UpToDate { .. } if role.is_base() => return None,
        // Against the remote the same verdict means the opposite: these are
        // precisely the commits sync will send.
        Verdict::UpToDate { ahead: 0 } => return None,
        Verdict::UpToDate { ahead } => {
            paint_ahead(&format!("{ahead} to push{}", toward("to")), colored)
        }
        Verdict::FastForward { behind } if !role.is_base() => {
            paint_ahead(&format!("{behind} to pull{}", toward("from")), colored)
        }
        Verdict::FastForward { .. } => paint_ok(&format!("{which} moved — fast-forwards"), colored),
        Verdict::Clean { replayed } => paint_ok(
            &format!(
                "{which} moved — rebases cleanly ({replayed} {} replayed)",
                noun(*replayed, "commit", "commits")
            ),
            colored,
        ),
        Verdict::Conflict {
            at: At::Commit { subject, .. },
            paths,
        } => paint_warn(
            &format!(
                "{which} moved — conflicts at \"{}\" in {} {}",
                truncate_subject(subject),
                paths.len(),
                noun(paths.len(), "file", "files")
            ),
            colored,
        ),
        Verdict::Conflict {
            at: At::OpenChange,
            paths,
        } => paint_warn(
            &format!(
                "{which} moved — conflicts with your open change in {} {}",
                paths.len(),
                noun(paths.len(), "file", "files")
            ),
            colored,
        ),
        Verdict::Unknown { reason } => paint_dim(
            &format!("{which} moved — can't simulate ({})", reason.text()),
            colored,
        ),
        Verdict::Gone => paint_warn(&format!("{which} is gone"), colored),
    })
}

/// Pick the singular or plural noun for a count: `1 commit`, `3 commits`.
fn noun(n: usize, singular: &'static str, plural: &'static str) -> &'static str {
    if n == 1 { singular } else { plural }
}

/// Truncate a long subject so the futures line stays one line: cut to 40
/// characters and append an ellipsis, trimming trailing whitespace first so
/// the ellipsis never floats after a stray space.
fn truncate_subject(subject: &str) -> String {
    let chars: Vec<char> = subject.chars().collect();
    if chars.len() <= 40 {
        return subject.to_string();
    }
    let mut truncated: String = chars[..40].iter().collect();
    while truncated.ends_with(char::is_whitespace) {
        truncated.pop();
    }
    truncated.push('\u{2026}');
    truncated
}

/// Build the header line: branch + what syncing would cost + operation.
fn status_header(status: &Status, futures: &ff_core::futures::Futures, colored: bool) -> String {
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
    // What the header reports is what `ff sync` would do — which is also what
    // decides whether it speaks at all. The upstream's raw ahead/behind used
    // to sit here; the remote axis says the same thing in the vocabulary the
    // base already uses, and saying it twice in two dialects was the whole
    // problem.
    let sync = sync_parts(futures, colored);
    if sync.is_empty() {
        // Both axes settled — or the only one fufu could name did. One dim
        // phrase stands for both, and never "in sync", which a reader can
        // hear as "merged". With no axis at all (detached, unborn, no
        // nameable trunk) there is nothing honest to claim, so say nothing.
        if futures.base.is_some() || futures.remote.is_some() {
            parts.push(paint_dim("nothing to sync", colored));
        }
    } else {
        parts.extend(sync);
    }
    if let Some(op) = &status.operation {
        parts.push(operation_phrase(*op).to_string());
    }
    parts.join(" · ")
}

fn operation_phrase(op: InProgress) -> &'static str {
    match op {
        InProgress::ApplyMailbox => "applying mailbox",
        InProgress::ApplyMailboxRebase => "applying mailbox (rebase)",
        InProgress::Bisect => "bisecting",
        InProgress::CherryPick | InProgress::CherryPickSequence => "cherry-picking",
        InProgress::Merge => "merging",
        InProgress::Rebase | InProgress::RebaseInteractive => "rebasing",
        InProgress::Revert | InProgress::RevertSequence => "reverting",
    }
}

/// Render the diffstat block: file rows with kind, path, counts, bar, then
/// summary. Shared so `ff op show` reuses the exact renderer `ff
/// status` does rather than a second one.
pub(crate) fn render_diffstat(stat: &ChangeStat, colored: bool) -> String {
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

/// One `ff op log` row: `<letters8> <age>  <kind> <branch>  <summary>`.
///
/// The prefix length comes off the row itself rather than out of a second
/// lookup — `read_ops_from` already priced abbreviation by the rows on
/// screen, and `short_id` *is* the shortest prefix `ff op` resolves
/// unambiguously. Two sources for one number is how highlighting and
/// resolution drift apart.
pub fn op_row(op: &ff_core::OpEntry, now: i64, colored: bool) -> String {
    let letters: String = op.id.chars().take(ID_WIDTH).collect();
    let unique = op.short_id.chars().count();
    let mut tail = op.summary.clone();
    if let Some(session) = &op.session {
        tail = format!("{tail} [{session}]");
    }
    format!(
        "{} {}  {} {}  {}",
        styled_id(&letters, unique, ID_WIDTH, colored),
        col_right(
            &relative_age(now, op.time),
            AGE_WIDTH,
            palette().age,
            colored
        ),
        col(&op.kind, KIND_WIDTH, DIM, colored),
        col(
            op.branch.as_deref().unwrap_or(""),
            BRANCH_WIDTH,
            DIM,
            colored
        ),
        tail
    )
}

fn short7(hex: &str) -> &str {
    &hex[..hex.len().min(7)]
}

const ID_WIDTH: usize = 8;
const SHA_WIDTH: usize = 7;
const AGE_WIDTH: usize = 8;
const KIND_WIDTH: usize = 7;
const BRANCH_WIDTH: usize = 12;
const BLANK_ID: &str = "        ";

/// The `@` row (two lines): the open change. The sha column is the pending
/// commit hash (the change's own identity). Letters id = chain tip (via
/// `lens`), age = tip snapshot time. Clean + undescribed collapses to
/// `@  no changes`.
pub fn change_row(
    open: &ChangeRowDisplay<'_>,
    lens: &std::collections::HashMap<String, usize>,
    now: i64,
    colored: bool,
) -> String {
    let sym = paint("@", palette().at, colored);
    let rail = paint("│", DIM, colored);
    let subject = match open.subject {
        Some(text) => text.to_string(),
        None => paint("(no description)", DIM, colored),
    };

    // Born + clean + no description: collapsed "no changes" line.
    if open.born && open.clean && open.subject.is_none() {
        let head = format!("{sym}  {}", paint("no changes", DIM, colored));
        return format!("{}\n{rail}  {subject}", head.trim_end());
    }

    // Full layout: letters + pending sha + age + optional marker.
    let letters = match open.id {
        Some(id) => styled_id(
            &ff_core::snapid::encode(&id[..id.len().min(ID_WIDTH)]),
            lens.get(id).copied().unwrap_or(1),
            ID_WIDTH,
            colored,
        ),
        None => BLANK_ID.to_string(),
    };
    let pending_short = open.pending.map(short7).unwrap_or_default();
    let sha = col(pending_short, SHA_WIDTH, palette().sha, colored);
    let age = col_right(
        &open.time.map(|t| relative_age(now, t)).unwrap_or_default(),
        AGE_WIDTH,
        palette().age,
        colored,
    );
    let marker = if !open.born {
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
    entry: &CommitRowDisplay<'_>,
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
    let sha = col(short7(entry.id), SHA_WIDTH, palette().sha, colored);
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

const AGE_STEPS: &[(i64, &str)] = &[
    (60, "s"),
    (60 * 60, "m"),
    (60 * 60 * 24, "h"),
    (60 * 60 * 24 * 7, "d"),
    (60 * 60 * 24 * 30, "w"),
    (60 * 60 * 24 * 365, "mo"),
];

/// A non-negative span of seconds, bucketed to its coarsest unit — the
/// shared core of `relative_age` (which appends " ago") and
/// `duration_human` (a plain elapsed span, no "ago").
fn bucketed(delta: i64) -> String {
    let mut prev = 1;
    for &(limit, unit) in AGE_STEPS {
        if delta < limit {
            return format!("{}{unit}", delta / prev);
        }
        prev = limit;
    }
    format!("{}y", delta / (60 * 60 * 24 * 365))
}

pub fn relative_age(now: i64, then: i64) -> String {
    let delta = now - then;
    if delta < 0 {
        return "future".into();
    }
    format!("{} ago", bucketed(delta))
}

/// Painter helpers — one-liners that command files use instead of touching
/// `anstyle` directly. Each takes `(text, colored)` and delegates to the
/// private `paint` with the correct semantic role from the current palette.
pub fn paint_sha(text: &str, colored: bool) -> String {
    paint(text, palette().sha, colored)
}

/// An operation id, whole. The log family's id role — the same magenta the
/// highlighted prefix wears, spent where there is no prefix to highlight.
pub fn paint_id(text: &str, colored: bool) -> String {
    paint(text, palette().snap, colored)
}

pub fn paint_ok(text: &str, colored: bool) -> String {
    paint(text, palette().ok, colored)
}

pub fn paint_warn(text: &str, colored: bool) -> String {
    paint(text, palette().warn, colored)
}

pub fn paint_ahead(text: &str, colored: bool) -> String {
    paint(text, palette().ahead, colored)
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
