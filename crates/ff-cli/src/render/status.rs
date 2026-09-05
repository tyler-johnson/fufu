use ff_core::{ChangeStat, HeadState, InProgress, ReconcileReport, Status};

use super::diff::render_diffstat;
use super::palette::{paint_ahead, paint_dim, paint_ok, paint_sha, paint_warn};
use super::rows::{ChangeRowDisplay, CommitRowDisplay, SigMark, change_row, commit_row};

/// A ref table value for display. Shas shorten to eight characters; a
/// `ref:<full-name>` value — a symbolic or unborn HEAD — is a ref, not a sha,
/// and shows the branch it names, untruncated (`ref:` and, when present,
/// `refs/heads/` stripped). `None` marks a real sha, the case the caller
/// handles with `ff_core::sha::short`.
fn ref_display(value: &str) -> Option<String> {
    let rest = value.strip_prefix("ref:")?;
    Some(rest.strip_prefix("refs/heads/").unwrap_or(rest).to_string())
}

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
        eprintln!("{}", absorbed_line(&report.foreign, colored));
    }
}

/// How one foreign ref moved, read off its old and new values. `(None,
/// None)` is a change with no shape — it counts, and joins no kind.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Motion {
    Moved,
    Created,
    Deleted,
}

impl Motion {
    /// The order the shape lists kinds in.
    const ALL: [Motion; 3] = [Motion::Moved, Motion::Created, Motion::Deleted];

    fn classify(old: Option<&str>, new: Option<&str>) -> Option<Motion> {
        match (old, new) {
            (Some(_), Some(_)) => Some(Motion::Moved),
            (None, Some(_)) => Some(Motion::Created),
            (Some(_), None) => Some(Motion::Deleted),
            (None, None) => None,
        }
    }

    fn word(self) -> &'static str {
        match self {
            Motion::Moved => "moved",
            Motion::Created => "created",
            Motion::Deleted => "deleted",
        }
    }
}

/// The shape of several foreign changes as counts by kind — `2 moved, 1
/// created` — listing only the kinds that occurred, moved first, then
/// created, then deleted. Empty when no change has a shape.
fn shape<'a>(changes: impl IntoIterator<Item = (Option<&'a str>, Option<&'a str>)>) -> String {
    let mut counts = [0usize; 3];
    for (old, new) in changes {
        if let Some(motion) = Motion::classify(old, new) {
            counts[motion as usize] += 1;
        }
    }
    Motion::ALL
        .iter()
        .filter(|m| counts[**m as usize] > 0)
        .map(|m| format!("{} {}", counts[*m as usize], m.word()))
        .collect::<Vec<_>>()
        .join(", ")
}

/// One ref's motion as a phrase: `moved to <name|sha>`, `created at …`,
/// `deleted`. A sha is short and painted; a `ref:` value shows the branch
/// it names. Both surfaces that speak of a single foreign change use this,
/// so they cannot drift apart in wording.
fn motion_phrase(old: Option<&str>, new: Option<&str>, colored: bool) -> String {
    let target = |new: &str| match ref_display(new) {
        Some(name) => name,
        None => paint_sha(ff_core::sha::short(new), colored),
    };
    match (Motion::classify(old, new), new) {
        (Some(Motion::Moved), Some(new)) => format!("moved to {}", target(new)),
        (Some(Motion::Created), Some(new)) => format!("created at {}", target(new)),
        (Some(Motion::Deleted), _) => "deleted".to_string(),
        _ => "changed".to_string(),
    }
}

/// `N change(s)`, painted as a warning: the count is the fact, and it wears
/// the role the preamble's other warnings already wear.
fn change_count(n: usize, colored: bool) -> String {
    paint_warn(&format!("{n} {}", noun(n, "change", "changes")), colored)
}

/// The one preamble line for absorbed foreign motion. A single change is
/// the common interactive case — a reset, an amend, a teammate's commit —
/// and there the ref is the shape, so it stays on the line with git's
/// reflog hint. More than one folds to counts by kind: one `git fetch`
/// moves every remote ref, and a line per ref is a wall. `ff op show @`
/// has the per-ref detail.
fn absorbed_line(foreign: &[ff_core::ForeignChange], colored: bool) -> String {
    let count = change_count(foreign.len(), colored);
    let what = match foreign {
        [only] => {
            let phrase = motion_phrase(only.old.as_deref(), only.new.as_deref(), colored);
            match &only.hint {
                Some(hint) => format!("{} {phrase} ({hint})", only.name),
                None => format!("{} {phrase}", only.name),
            }
        }
        many => shape(many.iter().map(|c| (c.old.as_deref(), c.new.as_deref()))),
    };
    if what.is_empty() {
        format!("ff: absorbed {count} made outside fufu")
    } else {
        format!("ff: absorbed {count} made outside fufu: {what}")
    }
}

/// The one line `ff status` pins while the log's tip is foreign: the same
/// count and shape as the preamble, with the undo hint in place of git's,
/// which the status record does not carry.
fn pinned_line(foreign: &[crate::cmd::status::ForeignEntry], colored: bool) -> String {
    let count = change_count(foreign.len(), colored);
    let undo = format!(
        "(absorbed; ff undo can roll {} back)",
        noun(foreign.len(), "it", "them")
    );
    match foreign {
        [only] => {
            let phrase = motion_phrase(only.old.as_deref(), only.new.as_deref(), colored);
            format!("{count} made outside fufu: {} {phrase} {undo}", only.r#ref)
        }
        many => {
            let shape = shape(many.iter().map(|e| (e.old.as_deref(), e.new.as_deref())));
            if shape.is_empty() {
                format!("{count} made outside fufu {undo}")
            } else {
                format!("{count} made outside fufu, {shape} {undo}")
            }
        }
    }
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
                "resolving: {} {} from ff {} {} in your working copy",
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
            // The same free mark `ff log` shows. Verifying it would be a
            // spawn nobody asked for, but saying it is signed costs nothing
            // and the two views agreeing is worth more than the blank.
            signature: if parent.signed {
                SigMark::Signed
            } else {
                SigMark::None
            },
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

    // Foreign changes line
    if let Some(foreign) = &model.foreign
        && !foreign.is_empty()
    {
        out.push_str(&pinned_line(foreign, colored));
        out.push('\n');
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
pub(super) fn sync_parts(futures: &ff_core::futures::Futures, colored: bool) -> Vec<String> {
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
pub(super) fn to_publish(n: usize, toward: Option<&str>, colored: bool) -> String {
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
pub(super) fn to_sync(n: usize, toward: Option<&str>, colored: bool) -> String {
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

/// The lines a rewrite verb prints for the branches stacked above the one it
/// moved: one per branch that followed, then each that held, then each that
/// was skipped. Nothing for a branch with nothing of its own to replay — it
/// did not move, and the verb's own divergence line already names it when
/// it sits inside the replayed range.
pub(crate) fn cascade_lines(cascade: &ff_core::Cascade, colored: bool) -> Vec<String> {
    let mut out = Vec::new();
    for m in &cascade.moved {
        out.push(format!(
            "{} followed {}: replayed {} commit(s)",
            m.branch, m.base, m.replayed
        ));
        if let Some(line) = dropped_line(&m.dropped, None, colored) {
            out.push(format!("    {line}"));
        }
        if !m.diverged.is_empty() {
            let sits = if m.diverged.len() == 1 { "sits" } else { "sit" };
            out.push(paint_warn(
                &format!(
                    "    {} now {sits} on commits this replay replaced",
                    m.diverged.join(", ")
                ),
                colored,
            ));
        }
        if m.published > 0 {
            let on = m.published_on.as_deref().unwrap_or("the remote");
            out.push(format!(
                "    {} of {}'s rewritten commits are already on {on}",
                m.published, m.branch
            ));
        }
    }
    for h in &cascade.held {
        let where_it_stopped = match &h.report.at {
            ff_core::futures::At::Commit { id, subject } => format!(
                "replaying {} \"{}\"",
                ff_core::sha::short(id),
                truncate_subject(subject)
            ),
            ff_core::futures::At::OpenChange => "your open change".to_string(),
        };
        out.push(paint_warn(
            &format!(
                "{} held: {where_it_stopped} conflicts in {}",
                h.branch,
                join_paths(&h.report.paths)
            ),
            colored,
        ));
        out.push(format!(
            "    the restack onto {} is waiting — nothing was written there{}",
            h.base,
            left_alone(&h.left_alone)
        ));
        out.push(paint_dim(
            &format!(
                "    ff switch {} · ff resolve to fix them, all at once · ff resolve --abandon to \
                 drop it",
                h.branch
            ),
            colored,
        ));
    }
    for s in &cascade.skipped {
        out.push(paint_warn(
            &format!(
                "{} skipped: {}{}",
                s.branch,
                skip_reason(&s.reason, &s.base),
                left_alone(&s.left_alone)
            ),
            colored,
        ));
    }
    out
}

/// Why a branch was left where it stands, in the words every verb uses;
/// `base` is the branch it sits on, named when the reason is about it.
pub(crate) fn skip_reason(reason: &ff_core::SkipReason, base: &str) -> String {
    match reason {
        ff_core::SkipReason::Worktree { path } => format!("checked out in {path}"),
        ff_core::SkipReason::AlreadyHeld => "a rewrite is already held there".to_string(),
        ff_core::SkipReason::MergeInRange => {
            "its commits hold a merge, and replaying a merge is ambiguous".to_string()
        }
        ff_core::SkipReason::Unrelated => format!("it shares no history with {base}"),
    }
}

/// The tail a held or skipped branch's line carries for the branches above
/// it, which stay where they stood because their base did not move.
fn left_alone(names: &[String]) -> String {
    if names.is_empty() {
        String::new()
    } else {
        format!("; above it, {} left alone", names.join(", "))
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

#[cfg(test)]
mod tests {
    use super::{absorbed_line, axis_phrase, pinned_line, shape, to_publish, to_sync};
    use crate::cmd::status::ForeignEntry;
    use ff_core::ForeignChange;

    fn change(
        name: &str,
        old: Option<&str>,
        new: Option<&str>,
        hint: Option<&str>,
    ) -> ForeignChange {
        ForeignChange {
            name: name.into(),
            old: old.map(String::from),
            new: new.map(String::from),
            hint: hint.map(String::from),
        }
    }

    fn entry(name: &str, old: Option<&str>, new: Option<&str>) -> ForeignEntry {
        ForeignEntry {
            r#ref: name.into(),
            old: old.map(String::from),
            new: new.map(String::from),
        }
    }

    const A: &str = "3dbae0c0aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const B: &str = "5a56e702bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

    #[test]
    fn the_shape_lists_kinds_in_order_and_skips_the_empty_ones() {
        // Deleted before created in the input, and a `(None, None)` change
        // that counts toward nothing: the shape is still moved, created,
        // deleted, and names only the kinds that happened.
        let changes = [
            (Some(A), None),
            (None, Some(B)),
            (Some(A), Some(B)),
            (None, None),
            (Some(B), Some(A)),
        ];
        assert_eq!(shape(changes), "2 moved, 1 created, 1 deleted");
        assert_eq!(shape([(None, Some(A)), (None, Some(B))]), "2 created");
        assert_eq!(shape([(None, None)]), "");
    }

    #[test]
    fn a_single_change_keeps_its_ref_and_hint() {
        let one = [change(
            "refs/heads/parser",
            Some(B),
            Some(A),
            Some("reset: moving to HEAD~2"),
        )];
        assert_eq!(
            absorbed_line(&one, false),
            "ff: absorbed 1 change made outside fufu: refs/heads/parser moved to 3dbae0c0 (reset: \
             moving to HEAD~2)"
        );
        let pinned = [entry("refs/heads/parser", Some(B), Some(A))];
        assert_eq!(
            pinned_line(&pinned, false),
            "1 change made outside fufu: refs/heads/parser moved to 3dbae0c0 (absorbed; ff undo \
             can roll it back)"
        );
        // A symbolic value shows the branch it names, as the table does.
        let unborn = [entry("HEAD", Some(A), Some("ref:refs/heads/next"))];
        assert_eq!(
            pinned_line(&unborn, false),
            "1 change made outside fufu: HEAD moved to next (absorbed; ff undo can roll it back)"
        );
    }

    #[test]
    fn several_changes_fold_to_counts_and_name_no_ref() {
        let many = [
            change("refs/remotes/origin/main", Some(A), Some(B), Some("fetch")),
            change(
                "refs/remotes/origin/parser",
                Some(B),
                Some(A),
                Some("fetch"),
            ),
            change("refs/remotes/origin/new", None, Some(A), Some("fetch")),
            change("refs/remotes/origin/old", Some(B), None, None),
        ];
        let line = absorbed_line(&many, false);
        assert_eq!(
            line,
            "ff: absorbed 4 changes made outside fufu: 2 moved, 1 created, 1 deleted"
        );
        assert!(!line.contains("refs/"), "no ref names: {line:?}");
        let pinned: Vec<ForeignEntry> = many
            .iter()
            .map(|c| entry(&c.name, c.old.as_deref(), c.new.as_deref()))
            .collect();
        let line = pinned_line(&pinned, false);
        assert_eq!(
            line,
            "4 changes made outside fufu, 2 moved, 1 created, 1 deleted (absorbed; ff undo can \
             roll them back)"
        );
        assert!(!line.contains("refs/"), "no ref names: {line:?}");
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
}
