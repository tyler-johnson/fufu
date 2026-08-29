use ff_core::{BranchInfo, LogEntry, SnapEntry};

use super::age::relative_age;
use super::palette::{BOLD, DIM, col, col_right, paint, paint_dim, paint_warn, palette, styled_id};
use super::status::{sync_parts, to_publish, to_sync};

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
    let base = snap
        .base
        .as_deref()
        .map(ff_core::sha::short)
        .unwrap_or_default();
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

/// One `ff history` row: `<marker> <letters8> <age> <landing>  <summary>`.
///
/// Deliberately `op_row`'s column shape with the kind and branch columns
/// spent differently — the id, the age, and the styled prefix are in the same
/// places, so the two views read as siblings rather than as two dialects. What
/// replaces the kind is the *move*: `now`, `undo`, `redo`, which is the whole
/// difference between the two views. The marker ahead of it is the same fact
/// as a number, because "press undo twice" is the thing a person came here to
/// find out and counting rows to learn it is a tax.
pub fn history_row(step: &ff_core::history::Step, now: i64, colored: bool) -> String {
    let marker = match step.distance {
        0 => "@".to_string(),
        d if d > 0 => format!("↓{d}"),
        d => format!("↑{}", -d),
    };
    let marker_style = if step.distance == 0 {
        palette().at
    } else {
        DIM
    };
    let letters: String = step.id.chars().take(ID_WIDTH).collect();
    let unique = step.short_id.chars().count();
    let mut tail = step.summary.clone();
    // What the keystroke covers, not what the row is: a run of captures is
    // one undo, and the count is the part that cannot be inferred from a
    // view that shows one row for it.
    if step.collapsed > 1 {
        tail = format!("{tail} · {} captures", step.collapsed);
    }
    if let Some(session) = &step.session {
        tail = format!("{tail} [{session}]");
    }
    format!(
        "{} {}  {}  {}  {}",
        col(&marker, MARKER_WIDTH, marker_style, colored),
        styled_id(&letters, unique, ID_WIDTH, colored),
        col_right(
            &relative_age(now, step.time),
            AGE_WIDTH,
            palette().age,
            colored
        ),
        col(step.landing.as_str(), MOVE_WIDTH, DIM, colored),
        tail
    )
}

const ID_WIDTH: usize = 8;
/// `↓12` is the widest marker anyone reaches by hand; past that the column
/// simply grows and the row still lines up with itself.
const MARKER_WIDTH: usize = 3;
const MOVE_WIDTH: usize = 4;
const SHA_WIDTH: usize = ff_core::sha::SHORT;
const AGE_WIDTH: usize = 8;
const KIND_WIDTH: usize = 7;
const BRANCH_WIDTH: usize = 12;
/// The op-id column with nothing to put in it: one em dash where the id would
/// start, then the column's remaining width in spaces. A mark rather than a
/// blank, so the sha beside it reads as the second column and not as an
/// accident of indentation; one mark rather than a filled run, so a page of
/// them stays quiet. Dim, because it is furniture, not data. It means one
/// thing on every surface that draws the column: no capture ever recorded
/// that commit's content, so there is nothing to drill into.
const BLANK_ID: &str = "\u{2014}       ";

/// The letters, sha and age columns taken together — what `no changes` fills
/// on the map's `@` row, so the branch label beside it lands in the column
/// every other row puts it in.
const QUIET_WIDTH: usize = ID_WIDTH + 1 + SHA_WIDTH + 1 + AGE_WIDTH;

/// `BLANK_ID`, dimmed for display.
fn blank_id(colored: bool) -> String {
    paint(BLANK_ID, DIM, colored)
}

/// The sha column with nothing to put in it — an unborn branch's missing
/// tip: one em dash where the sha would start, then the column's remaining
/// width in spaces. A mark rather than a blank, so the column reads as a
/// column and not as an accident of indentation; dim, because it is
/// furniture, not data.
const BLANK_SHA: &str = "\u{2014}       "; // em dash + 7 spaces = SHA_WIDTH (8)

/// `BLANK_SHA`, dimmed for display.
fn blank_sha(colored: bool) -> String {
    paint(BLANK_SHA, DIM, colored)
}

/// The sigil that leads a branch name on the map.
const BRANCH_SIGIL: &str = "\u{25b8} ";

/// A branch name rendered as what it is: a place you could jump to, and the
/// thing the map exists to help you find.
///
/// The palette has no color left to spend here — magenta, blue, cyan and
/// green are all already on a map row, and the only unused roles mean
/// *trouble* — so the emphasis is shape and modifiers instead. Three cues
/// stack, each independent of the others:
///
///   * the `BRANCH_SIGIL`, which survives `NO_COLOR`, a pipe, a monochrome
///     terminal and a screen reader — color is redundant encoding here, never
///     the only encoding;
///   * brackets, `git log --decorate`'s own shape, free everywhere;
///   * an underline on the name, because a branch name is a jump target and
///     that is what an underline means everywhere else.
///
/// Under all three, bold carries "this is what you can type", exactly as it
/// does on an op id's shortest unique prefix, and the current branch adds the
/// `at` green on top. Bold reads "you could go here"; green reads "you are
/// here".
fn branch_label(name: &str, current: bool, colored: bool) -> String {
    let base = if current { palette().at } else { BOLD };
    let mut out = String::new();
    out.push_str(&paint(BRANCH_SIGIL, base, colored));
    out.push_str(&paint("[", base, colored));
    out.push_str(&paint(name, base.underline(), colored));
    out.push_str(&paint("]", base, colored));
    out
}

/// The display width of `branch_label`'s output — sigil, brackets, name —
/// with no escape bytes counted, so a caller can size the column before
/// painting anything.
pub fn branch_label_width(name: &str) -> usize {
    BRANCH_SIGIL.chars().count() + 2 + name.chars().count()
}

/// One `ff branch list` row (one or two lines, never a trailing empty
/// string): the map's row grammar laid out as a table — the same
/// `branch_label`, the same `@` you-are-here glyph, and the note the map
/// hangs on a second line, so a verdict can never scroll off the right edge
/// behind a long subject. The note's base half comes from `sync_parts`, the
/// same renderer `ff status` calls, so the two surfaces cannot word it
/// differently.
pub fn branch_row(info: &BranchInfo, label_width: usize, colored: bool) -> Vec<String> {
    let marker = if info.current {
        format!("{} ", paint("@", palette().at, colored))
    } else {
        "  ".to_string()
    };
    let label = branch_label(&info.name, info.current, colored);
    // Pad after painting — `col`'s doc comment says why format-width would
    // count escape bytes.
    let pad = " ".repeat(label_width.saturating_sub(branch_label_width(&info.name)));
    let sha = match &info.tip {
        Some(tip) => col(ff_core::sha::short(tip), SHA_WIDTH, palette().sha, colored),
        None => blank_sha(colored),
    };
    let head = format!(
        "{marker}{label}{pad}  {sha}  {}",
        info.subject.as_deref().unwrap_or("")
    );

    // The note line: the base axis first, off the shared renderer —
    // `sync_parts` returns nothing for a settled base, so silence follows
    // for free — then the remote axis off the cheap local counts (no probe,
    // and no ref name here: the row already names the branch), then the
    // change's own state.
    let mut notes = sync_parts(
        &ff_core::futures::Futures {
            base: info.future.clone(),
            remote: None,
            remote_unnamed: false,
        },
        colored,
    );
    if let Some(up) = &info.upstream {
        if up.gone {
            // The exact string `axis_phrase` produces for `Verdict::Gone` at
            // `Role::Remote`.
            notes.push(paint_warn("remote is gone", colored));
        } else {
            if up.ahead > 0 {
                notes.push(to_publish(up.ahead, None, colored));
            }
            if up.behind > 0 {
                notes.push(to_sync(up.behind, None, colored));
            }
        }
    }
    // A held rewrite and an open resolution are worth the same notice an
    // unfinished session is — standing work on that branch — and more
    // urgent, so they lead the notes; a resolution outranks a hold, the
    // same ordering `ff status` runs.
    if info.resolving {
        notes.push(paint_dim("resolving", colored));
    } else if info.held {
        notes.push(paint_dim("held", colored));
    }
    if let Some(onto) = &info.session {
        notes.push(paint_dim(
            &format!("editing session, lands on {onto}"),
            colored,
        ));
    }
    if info.parked {
        notes.push(paint_dim("parked change", colored));
    }
    if let Some(desc) = &info.pending_description {
        notes.push(paint_dim(&format!("pending: {desc}"), colored));
    }

    let mut lines = vec![head.trim_end().to_string()];
    if !notes.is_empty() {
        lines.push(format!("    {}", notes.join(" · ")));
    }
    lines
}

/// A remote branch's name: `branch_label`'s shape with the brackets
/// removed, and the removal is the point. The brackets promise a name
/// `ff switch` moves you to, and `switch::resolve_branch` resolves local
/// names only — typed there, a remote name is read as a revision and mints
/// an anonymous branch at it, which is `ff start`'s job spelled the long
/// way (switch says so itself, and names `ff start`). So the brackets would
/// promise the wrong verb. The one that takes this name verbatim is
/// `ff start <remote>/<branch>`, and bold alone carries "a verb takes this".
fn remote_label(name: &str, colored: bool) -> String {
    let mut out = String::new();
    out.push_str(&paint(BRANCH_SIGIL, BOLD, colored));
    out.push_str(&paint(name, BOLD.underline(), colored));
    out
}

/// The display width of `remote_label`'s output — sigil and name — with no
/// escape bytes counted, so a caller can size the column before painting
/// anything: `branch_label_width` minus the two brackets.
pub fn remote_label_width(name: &str) -> usize {
    BRANCH_SIGIL.chars().count() + name.chars().count()
}

/// One `ff branch list` row for a branch that exists only on a remote:
/// `branch_row`'s head line with a blank marker, and nothing hung beneath —
/// a branch that is not local has no base axis and no upstream, so the note
/// line would be silence, and the row is one line, always. A tracking ref
/// is never unborn, so the tip always exists.
pub fn remote_branch_row(
    info: &ff_core::RemoteBranch,
    label_width: usize,
    colored: bool,
) -> String {
    let label = remote_label(&info.name, colored);
    // Pad after painting — `col`'s doc comment says why format-width would
    // count escape bytes.
    let pad = " ".repeat(label_width.saturating_sub(remote_label_width(&info.name)));
    let sha = col(
        ff_core::sha::short(&info.tip),
        SHA_WIDTH,
        palette().sha,
        colored,
    );
    format!(
        "  {label}{pad}  {sha}  {}",
        info.subject.as_deref().unwrap_or("")
    )
    .trim_end()
    .to_string()
}

/// The section's elision row — the map's own `~ N commits` grammar saying
/// the same thing about rows instead of commits — with the two leading
/// spaces that put it under the marker column, in line with the rows above
/// it.
pub fn remote_more_row(count: usize, colored: bool) -> String {
    paint_dim(&format!("  ~ {count} more"), colored)
}

/// The open change with nothing to say: born, clean, and undescribed. Every
/// surface that draws an `@` row collapses on exactly this, so the rule is
/// read from one place rather than restated per surface — the map restating
/// it is how it came to be the only one that never collapsed.
fn open_is_quiet(born: bool, clean: bool, subject: Option<&str>) -> bool {
    born && clean && subject.is_none()
}

/// The op-id column: the letters spelling of an operation, styled so its
/// shortest-unique prefix is what you can type, or the blank mark when no
/// operation answers. Shared by `ff log`'s commit rows and the map's, which
/// must never disagree about the same commit.
fn letters_col(
    anchor: Option<&str>,
    lens: &std::collections::HashMap<String, usize>,
    colored: bool,
) -> String {
    match anchor {
        Some(id) => styled_id(
            &ff_core::snapid::encode(&id[..id.len().min(ID_WIDTH)]),
            lens.get(id).copied().unwrap_or(1),
            ID_WIDTH,
            colored,
        ),
        None => blank_id(colored),
    }
}

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
    if open_is_quiet(open.born, open.clean, open.subject) {
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
        None => blank_id(colored),
    };
    let pending_short = open.pending.map(ff_core::sha::short).unwrap_or_default();
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
    let letters = letters_col(segment, lens, colored);
    let sha = col(
        ff_core::sha::short(entry.id),
        SHA_WIDTH,
        palette().sha,
        colored,
    );
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

/// One map row's payload: the glyph that rides its lane, and the one or two
/// lines that hang beside it.
pub struct MapPayload {
    pub glyph: String,
    pub lines: Vec<String>,
}

/// The map's columns are `ff log`'s, so the two surfaces read as siblings;
/// the glyph and the rail the lines hang from are the graph renderer's, not
/// ours. `segments` is the commit → anchor-operation map `ff log` builds for
/// its own letters column, and `lens` prices every id in it plus the Open
/// row's.
pub fn map_payload(
    node: &ff_core::MapNode,
    segments: &std::collections::HashMap<String, String>,
    lens: &std::collections::HashMap<String, usize>,
    now: i64,
    colored: bool,
) -> MapPayload {
    match node {
        ff_core::MapNode::Open {
            branch,
            id,
            subject,
            pending,
            time,
            born,
            clean,
        } => {
            // The branch label rides the end of the first line either way,
            // so it is built once and appended to whichever line0 wins.
            let label = if branch == "@detached" {
                String::new()
            } else {
                format!("  {}", branch_label(branch, true, colored))
            };

            // Nothing open: the same two words `ff status` and `ff log` print,
            // filling the three columns at once so the branch name still lands
            // where every other row puts it. Showing the chain tip's operation
            // here instead would print the same id the `●` row below already
            // carries — one operation, twice, on a row about nothing.
            if open_is_quiet(*born, *clean, subject.as_deref()) {
                let line0 = format!("{}{label}", col("no changes", QUIET_WIDTH, DIM, colored));
                return MapPayload {
                    glyph: paint("@", palette().at, colored),
                    lines: vec![
                        line0.trim_end().to_string(),
                        paint("(no description)", DIM, colored),
                    ],
                };
            }

            let letters = letters_col(id.as_deref(), lens, colored);
            let sha = col(
                pending
                    .as_deref()
                    .map(ff_core::sha::short)
                    .unwrap_or_default(),
                SHA_WIDTH,
                palette().sha,
                colored,
            );
            let age = col_right(
                &time.map(|t| relative_age(now, t)).unwrap_or_default(),
                AGE_WIDTH,
                palette().age,
                colored,
            );
            let mut line0 = format!("{letters} {sha} {age}{label}");
            if !born {
                line0.push_str(&format!("  {}", paint("(no commits yet)", DIM, colored)));
            }
            let line1 = match subject {
                Some(text) => text.clone(),
                None => paint("(no description)", DIM, colored),
            };
            MapPayload {
                glyph: paint("@", palette().at, colored),
                lines: vec![line0.trim_end().to_string(), line1],
            }
        }
        ff_core::MapNode::Commit {
            id,
            short_id,
            subject,
            time,
            refs,
        } => {
            // The same chain-segment anchor `ff log` and `ff status` put on
            // this commit — one operation, one spelling, whichever surface
            // asks. Blank when the walk found none: `segment_anchors` reads
            // the current chain's operation log, so a commit whose captures
            // happened while HEAD sat on another chain has no anchor here.
            // That is already the answer `ff log -r <other-branch>` gives.
            let letters = letters_col(segments.get(id).map(String::as_str), lens, colored);
            let sha = col(short_id, SHA_WIDTH, palette().sha, colored);
            let age = col_right(&relative_age(now, *time), AGE_WIDTH, palette().age, colored);
            let mut line0 = format!("{letters} {sha} {age}");
            for r in refs {
                line0.push_str("  ");
                line0.push_str(&branch_label(&r.name, r.current, colored));
                if let Some(files) = r.parked {
                    let note = if files == 1 {
                        "(+ parked change, 1 file)"
                    } else {
                        &format!("(+ parked change, {files} files)")
                    };
                    line0.push_str(&format!("  {}", paint(note, DIM, colored)));
                }
            }
            // `pending_description` is --json only: a description with no open
            // row to hang on would be inventing a shape.
            MapPayload {
                glyph: paint("●", palette().sha, colored),
                lines: vec![line0.trim_end().to_string(), subject.clone()],
            }
        }
        ff_core::MapNode::Elided { count } => MapPayload {
            glyph: paint("~", DIM, colored),
            lines: vec![match count {
                Some(n) => paint(&format!("{n} commits"), DIM, colored),
                None => String::new(),
            }],
        },
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
