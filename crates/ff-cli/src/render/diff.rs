use ff_core::{ChangeKind, ChangeStat, FileStat};

use super::palette::{BOLD, DIM, paint, palette};

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

/// The patch block: every file that carries hunks, in git's unified diff.
///
/// The body is git's format verbatim — `diff --git`, `index`, `---`/`+++`,
/// `@@`, and `+`/`-`/space — because a patch format is not fufu's to spell
/// and output that `git apply` takes is worth more than a dialect of our
/// own. What fufu contributes is the *content*: the same tree diff every
/// stat surface walks, which sees the untracked sweep git's own `diff` is
/// blind to.
///
/// Color rides on top of that format and grows nothing: `+` and `-` lines
/// spend the palette's existing `ins` and `del`, the `@@` header is dim and
/// the header block is bold. Dim and bold are modifiers rather than colors,
/// so DESIGN's one-color-per-meaning rule has nothing new to rule on.
///
/// Files whose `hunks` is `None` are skipped: nobody asked for their
/// content, so there is none to print.
pub(crate) fn patch_block(files: &[FileStat], colored: bool) -> String {
    let mut out = String::new();
    for f in files {
        let Some(hunks) = &f.hunks else { continue };

        // A rename's two paths; every other kind names one path twice, the
        // way git does.
        let old_path = f.from.as_deref().unwrap_or(&f.path);
        let new_path = f.path.as_str();
        let mut head = vec![format!("diff --git a/{old_path} b/{new_path}")];
        match f.kind {
            ChangeKind::Added | ChangeKind::IntentToAdd => {
                if let Some(mode) = &f.new_mode {
                    head.push(format!("new file mode {mode}"));
                }
            }
            ChangeKind::Deleted => {
                if let Some(mode) = &f.old_mode {
                    head.push(format!("deleted file mode {mode}"));
                }
            }
            ChangeKind::Renamed | ChangeKind::Copied => {
                let verb = if f.kind == ChangeKind::Copied {
                    "copy"
                } else {
                    "rename"
                };
                head.push(format!("{verb} from {old_path}"));
                head.push(format!("{verb} to {new_path}"));
            }
            ChangeKind::Modified | ChangeKind::TypeChange => {}
        }
        // A mode that moved is its own two lines, on any kind where both
        // sides exist — the executable bit is content as far as a checkout
        // is concerned.
        if let (Some(old), Some(new)) = (&f.old_mode, &f.new_mode)
            && old != new
        {
            head.push(format!("old mode {old}"));
            head.push(format!("new mode {new}"));
        }
        if let Some(line) = index_line(f) {
            head.push(line);
        }
        for line in head {
            out.push_str(&paint(&line, BOLD, colored));
            out.push('\n');
        }

        if f.binary {
            // git's own wording, and its own null-side spelling, so a reader
            // who has seen one has seen both.
            let a = match f.kind {
                ChangeKind::Added | ChangeKind::IntentToAdd => "/dev/null".to_string(),
                _ => format!("a/{old_path}"),
            };
            let b = match f.kind {
                ChangeKind::Deleted => "/dev/null".to_string(),
                _ => format!("b/{new_path}"),
            };
            out.push_str(&format!("Binary files {a} and {b} differ\n"));
            continue;
        }

        // No hunks and not binary: a rename or a mode change that moved no
        // content. The header already said everything there is to say.
        if hunks.is_empty() {
            continue;
        }

        let minus = match f.kind {
            ChangeKind::Added | ChangeKind::IntentToAdd => "--- /dev/null".to_string(),
            _ => format!("--- a/{old_path}"),
        };
        let plus = match f.kind {
            ChangeKind::Deleted => "+++ /dev/null".to_string(),
            _ => format!("+++ b/{new_path}"),
        };
        out.push_str(&paint(&minus, BOLD, colored));
        out.push('\n');
        out.push_str(&paint(&plus, BOLD, colored));
        out.push('\n');

        for hunk in hunks {
            out.push_str(&paint(&hunk.header, DIM, colored));
            out.push('\n');
            for line in &hunk.lines {
                let (mark, style) = match line.kind {
                    ff_core::patch::LineKind::Context => (' ', anstyle::Style::new()),
                    ff_core::patch::LineKind::Insert => ('+', palette().ins),
                    ff_core::patch::LineKind::Delete => ('-', palette().del),
                };
                out.push_str(&paint(&format!("{mark}{}", line.text), style, colored));
                out.push('\n');
                if line.no_newline {
                    out.push_str(&paint("\\ No newline at end of file", DIM, colored));
                    out.push('\n');
                }
            }
        }
    }
    out
}

/// git's `index <old>..<new> [mode]`. The abbreviation is fufu's one short
/// length rather than git's, since the line is informational — `git apply`
/// reads it only for a three-way merge, which needs the objects present.
/// The mode rides along only when it did not move; git spells a moved mode
/// on its own two lines instead.
fn index_line(f: &FileStat) -> Option<String> {
    let zeros = |len: usize| "0".repeat(len);
    let (old, new) = match (&f.old_id, &f.new_id) {
        (Some(old), Some(new)) => (
            ff_core::sha::short(old).to_string(),
            ff_core::sha::short(new).to_string(),
        ),
        (None, Some(new)) => {
            let new = ff_core::sha::short(new).to_string();
            (zeros(new.len()), new)
        }
        (Some(old), None) => {
            let old = ff_core::sha::short(old).to_string();
            let len = old.len();
            (old, zeros(len))
        }
        (None, None) => return None,
    };
    // The mode rides here only when it did not move and both sides exist.
    // git spells a moved mode on its own two lines, and a created or deleted
    // file's mode on the `new file mode` / `deleted file mode` line above —
    // repeating it here would be the one difference from git's own output.
    let mode = match (&f.old_mode, &f.new_mode) {
        (Some(old), Some(new)) if old == new => Some(new.clone()),
        _ => None,
    };
    Some(match mode {
        Some(mode) => format!("index {old}..{new} {mode}"),
        None => format!("index {old}..{new}"),
    })
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
