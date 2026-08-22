//! Human-readable rendering: plain rows, no TUI; the log family carries
//! ANSI color when the stream says so (see the palette below).

use ff_core::{
    BranchInfo, ChangeKind, ChangeStat, FileStat, HeadState, InProgress, LogEntry, ReconcileReport,
    SnapEntry, Status,
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
                    let sha = ff_core::sha::short(new.as_str());
                    format!("moved to {}", paint_sha(sha, colored))
                }
                (None, Some(new)) => {
                    let sha = ff_core::sha::short(new.as_str());
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

    // The resolution outranks the hold, and both outrank the session block:
    // the more urgent fact about where you are standing goes highest, and a
    // tree full of markers is the most urgent of the three — a hold is only
    // waiting, and a session is only editing.
    if let Some(resolving) = &model.resolving {
        // Singular keeps the grammar honest: one conflict "is" in the tree.
        let (noun, be) = if resolving.conflicts == 1 {
            ("conflict", "is")
        } else {
            ("conflicts", "are")
        };
        // One painted line, no paint nested inside it — an inner reset would
        // end the warn colour for the rest of the line.
        out.push_str(&paint_warn(
            &format!(
                "resolving: {} {} from ff {} {} in your working tree",
                resolving.conflicts, noun, resolving.verb, be,
            ),
            colored,
        ));
        out.push('\n');
        out.push_str(&format!(
            "    {hint}\n",
            hint = paint_dim(
                "fix the markers, then ff done · ff resolve --abandon to drop it",
                colored
            )
        ));
    }
    if let Some(held) = &model.held {
        let where_it_stopped = match &held.at {
            ff_core::futures::At::Commit { id, subject } => format!(
                "{} \"{}\"",
                ff_core::sha::short(id),
                truncate_subject(subject)
            ),
            ff_core::futures::At::OpenChange => "your open change".to_string(),
        };
        // One painted line, no paint nested inside it — an inner reset would
        // end the warn colour for the rest of the line.
        out.push_str(&paint_warn(
            &format!(
                "held: ff {} conflicts at {} in {} {}",
                held.verb,
                where_it_stopped,
                held.paths.len(),
                noun(held.paths.len(), "file", "files"),
            ),
            colored,
        ));
        out.push('\n');
        out.push_str(&format!(
            "    {hint}\n",
            hint = paint_dim(
                "ff resolve to fix them · ff resolve --abandon to drop it",
                colored
            )
        ));
        // The third of the three held-rewrite disciplines is exits blocked:
        // sync refuses to publish while a hold stands, and a guard nobody is
        // told about is a guard that surprises people.
        out.push_str(&format!(
            "    {note}\n",
            note = paint_dim(
                "exits are blocked: ff sync will not publish while this stands",
                colored
            )
        ));
    }

    // A running session is the least urgent of the three, so it goes below
    // them and above the change.
    if let Some(session) = &model.session {
        let sha = ff_core::sha::short(session.editing.as_str());
        out.push_str(&format!(
            "editing {sha_p} \"{subject}\" — lands back on {onto}\n",
            sha_p = paint_sha(sha, colored),
            subject = session.subject,
            onto = session.onto,
        ));
        out.push_str(&format!(
            "    {hint}\n",
            hint = paint_dim("ff done to finish · ff done --abandon to drop it", colored),
        ));
    }

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
        out.push_str(&commit_row(
            &commit_display,
            parent.segment.as_deref(),
            lens,
            now,
            colored,
        ));
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
                (_, Some(new)) => format!("moved to {}", ff_core::sha::short(new.as_str())),
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
/// contributes nothing. When the remote axis is unnameable — remotes exist
/// but none of them answers to this branch — a third part, `remote unnamed`,
/// stands in for the missing axis so an empty axis never reads as a settled
/// one. `ff status` and the ambient shell channel share this renderer, so a
/// prompt can never word a verdict differently from the command.
pub fn sync_parts(futures: &ff_core::futures::Futures, colored: bool) -> Vec<String> {
    let mut parts: Vec<String> = [futures.base.as_ref(), futures.remote.as_ref()]
        .into_iter()
        .flatten()
        .filter_map(|f| axis_phrase(f, colored))
        .collect();
    if futures.remote_unnamed {
        parts.push(paint_warn("remote unnamed", colored));
    }
    parts
}

/// `{n} to publish[ to <ref>]`, pending work headed for the remote.
///
/// The verb, not git's word for it: `ff push` is refused, so a status line
/// that said "to push" would name the one thing a reader cannot then type.
/// `to_sync` is its mirror and carries the rest of the reasoning.
///
/// `ff branch list` walks every branch and must not pay a merge simulation
/// per row, so it spells the remote axis off `BranchInfo.upstream`'s cheap
/// local counts — and it must spell it in these exact words, so the two
/// callers read one definition.
fn to_publish(n: usize, toward: Option<&str>, colored: bool) -> String {
    let phrase = match toward {
        Some(ref_name) => format!("{n} to publish to {ref_name}"),
        None => format!("{n} to publish"),
    };
    paint_ahead(&phrase, colored)
}

/// `{n} to sync[ from <ref>]`, pending work the remote already has.
///
/// Each half names the verb that handles it — `{n} to sync`, `{n} to publish`
/// — so a count is always something you can act on. Not "to pull", which
/// names a verb fufu refuses. The verbs' own output is where "take" and
/// "send" live: sync says it took commits in, publish says it sent them. A
/// status line is shorter than a sentence and wants the verb, not the motion.
///
/// `ff branch list` walks every branch and must not pay a merge simulation
/// per row, so it spells the remote axis off `BranchInfo.upstream`'s cheap
/// local counts — and it must spell it in these exact words, so the two
/// callers read one definition.
fn to_sync(n: usize, toward: Option<&str>, colored: bool) -> String {
    let phrase = match toward {
        Some(ref_name) => format!("{n} to sync from {ref_name}"),
        None => format!("{n} to sync"),
    };
    paint_ahead(&phrase, colored)
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
    let alias = matches!(role, Role::RemoteAlias).then_some(f.against.name.as_str());

    Some(match &f.verdict {
        // Sync never merges you into your base, so unmerged work is a
        // branch's permanent condition rather than pending work — and a line
        // that reported it every time would teach people to stop reading it.
        Verdict::UpToDate { .. } if role.is_base() => return None,
        // Against the remote the same verdict means the opposite: these are
        // precisely the commits sync will send.
        Verdict::UpToDate { ahead: 0 } => return None,
        Verdict::UpToDate { ahead } => to_publish(*ahead, alias, colored),
        Verdict::FastForward { behind } if !role.is_base() => to_sync(*behind, alias, colored),
        Verdict::FastForward { .. } => paint_ok(&format!("{which} moved — fast-forwards"), colored),
        Verdict::Clean { replayed, dropped } => {
            // Zero dropped stays byte-identical to the line before the
            // field existed; a drop is named only when the replay would
            // actually drop.
            let dropped_clause = (*dropped > 0).then(|| {
                format!(
                    ", {} {} dropped as empty",
                    dropped,
                    noun(*dropped, "commit", "commits")
                )
            });
            paint_ok(
                &format!(
                    "{which} moved — rebases cleanly ({replayed} {} replayed{})",
                    noun(*replayed, "commit", "commits"),
                    dropped_clause.as_deref().unwrap_or("")
                ),
                colored,
            )
        }
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
        // A phrase rather than a count-plus-verb, like "is gone" above: what
        // is out there is not work to take in, and the number is how much of
        // your own is still standing on the shared copy.
        Verdict::Undone { behind } => {
            paint_warn(&format!("{which} holds {behind} you undid"), colored)
        }
        // Dim rather than warn: nothing was lost, there has simply never
        // been a copy. This is a fresh clone's first status line.
        Verdict::Unpublished => paint_dim(&format!("{which} has no copy yet"), colored),
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

/// The block a rewrite verb prints when it holds. A hold reads like a report
/// and not a refusal, because that is what it is: the intent is recorded, it
/// survives until it lands or is dropped, and the two ways out are the last
/// thing on screen.
pub(crate) fn held_block(report: &ff_core::HeldReport, colored: bool) -> String {
    let where_it_stopped = match &report.at {
        ff_core::futures::At::Commit { id, subject } => format!(
            "replaying {} \"{}\"",
            ff_core::sha::short(id),
            truncate_subject(subject)
        ),
        // No verb of its own: a restack replays the open change onto a new
        // base and an absorb folds it into a commit, and the block would have
        // to know which. What it conflicts with is the same news either way.
        ff_core::futures::At::OpenChange => "your open change".to_string(),
    };
    let mut out = paint_warn(
        &format!(
            "held: {where_it_stopped} conflicts in {}",
            join_paths(&report.paths)
        ),
        colored,
    );
    // A fold never reaches a replay, so it has no commit count to give —
    // saying "of 0 commits" would be answering a question nobody asked.
    let scale = if report.of == 0 {
        String::new()
    } else {
        format!(" of {} {}", report.of, noun(report.of, "commit", "commits"))
    };
    out.push_str(&format!(
        "\n    the {}{scale} on {} is waiting — nothing was written",
        report.verb, report.branch,
    ));
    out.push_str(&format!(
        "\n    {}",
        paint_dim(
            "ff resolve to fix them, all at once · ff resolve --abandon to drop it",
            colored
        )
    ));
    out
}

/// Paths the way the rest of the tool prints them: all of them up to three,
/// then the first three and a count. Mirrors `rewrite::join_paths`, which is
/// crate-private to the engine and so cannot be shared.
fn join_paths(paths: &[String]) -> String {
    if paths.len() <= 3 {
        paths.join(", ")
    } else {
        format!("{}, and {} more", paths[..3].join(", "), paths.len() - 3)
    }
}

/// The one line the rewrite verbs print for commits a rewrite dropped —
/// `None` when nothing was dropped, so the caller prints nothing. Names a
/// single drop; counts several and shows the first three, the
/// `rewrite::join_paths` shape.
///
/// `already_named` is the commit the caller's own headline has just spoken
/// about — the target a lift emptied, the anchor a session emptied. Saying
/// it a second time here would be the same sentence twice, so it is left
/// out; `None` from a verb whose headline names no commit, like `ff restack`.
pub(crate) fn dropped_line(
    dropped: &[ff_core::rewrite::Dropped],
    already_named: Option<&str>,
    colored: bool,
) -> Option<String> {
    let rest: Vec<&ff_core::rewrite::Dropped> = dropped
        .iter()
        .filter(|d| already_named != Some(d.old.as_str()))
        .collect();
    let first = rest.first()?;
    let sha = |d: &&ff_core::rewrite::Dropped| paint_sha(ff_core::sha::short(&d.old), colored);
    if rest.len() == 1 {
        return Some(format!(
            "dropped {} \"{}\" — it changes nothing",
            sha(first),
            truncate_subject(&first.subject)
        ));
    }
    let names: Vec<String> = rest.iter().take(3).map(&sha).collect();
    let tail = if rest.len() > 3 {
        format!(", and {} more", rest.len() - 3)
    } else {
        String::new()
    };
    Some(format!(
        "dropped {} commit(s) that change nothing: {}{tail}",
        rest.len(),
        names.join(", ")
    ))
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
            format!("detached at {}", ff_core::sha::short(commit.as_str()))
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

/// Dim is a modifier, not a color — it is never themed.
const DIM: anstyle::Style = anstyle::Style::new().dimmed();

/// Bold is the other unthemed modifier, and it already means one thing in
/// this tool: *what you can type*. Op id columns bold their shortest unique
/// prefix and dim the rest for exactly that reason. A branch name is the
/// other typeable token on a row — it is what `ff switch` takes — so it wears
/// the same encoding rather than a tenth palette color. The current branch
/// adds the `at` green on top: bold says "you could go here", green says
/// "you are here".
const BOLD: anstyle::Style = anstyle::Style::new().bold();

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
    use super::{Palette, axis_phrase, palette_for, relative_age, styled_id, to_publish, to_sync};

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

    #[test]
    fn the_row_and_the_axis_word_pending_work_alike() {
        // The guard against the drift this change fixes: `ff branch list`
        // spells the remote axis off the cheap upstream counts, `ff status`
        // off the simulation — the two must never say different words for
        // the same fact, and a literal on each side keeps a refactor that
        // changed both from passing.
        use ff_core::futures::{Future, Role, SyncRef, Verdict};
        let remote = |verdict| Future {
            against: SyncRef {
                name: "origin/main".into(),
                r#ref: "refs/remotes/origin/main".into(),
                tip: "0".repeat(40),
                role: Role::Remote,
            },
            verdict,
        };
        let push = axis_phrase(&remote(Verdict::UpToDate { ahead: 6 }), false).unwrap();
        assert_eq!(push, to_publish(6, None, false));
        assert_eq!(push, "6 to publish");
        let pull = axis_phrase(&remote(Verdict::FastForward { behind: 2 }), false).unwrap();
        assert_eq!(pull, to_sync(2, None, false));
        assert_eq!(pull, "2 to sync");
    }

    // The OnceLock behind `palette()` is process-global, so a test that sets
    // it races every other test in the same binary — cargo runs them as
    // threads. `palette_for` is where the real mapping logic lives and it is
    // pure; the set-once plumbing is std behavior and is covered end-to-end by
    // the `ff config theme` integration tests instead.
}
