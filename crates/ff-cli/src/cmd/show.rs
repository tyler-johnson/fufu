//! `ff show` — one commit, header and patch.
//!
//! The message prints whole: the subject, then the body under a blank line
//! when there is one, each line indented like the subject.
//!
//! The revision half of the patch layer. `ff diff` is the open change;
//! this is anything the revset grammar names, including `@`, which is the
//! open change again — so the two verbs share one renderer rather than
//! wording the same body twice.
//!
//! The file view is the set `ff diff` and `ff log` share: `--stat`,
//! `--name-only`, and `--no-patch` shorten the patch to the diffstat, the
//! paths, or nothing, and `-U` sets its context.
//!
//! Resolution goes through `Revset::parse(raw)?.point(repo)?`, the same
//! single-member resolver `ff restore --from` uses, which means the
//! address-space refusal is already written: an operation id typed in a
//! revision position raises `usage/op-in-rev-position` and names `ff op
//! show`. Blob and tree reads (`HEAD:file.txt`, `<tree>`) stay git's, the
//! same call `ff blame` got.

use std::io::Write as _;

use ff_core::revset::{Rev, Revset};
use ff_core::{Against, ChangeStat, Error, Result};

use crate::cmd::fileview::{self, Depth, FileView, Flags};
use crate::ctx::Ctx;

pub fn run(ctx: &Ctx, rev: Option<String>, flags: Flags, paths: Vec<String>) -> Result<()> {
    // `@` reads the open change against the branch's newest operation, so it
    // needs the same capture `ff diff` needs. A commit read does not, but it
    // is the same verb and a capture-first floor that depended on which
    // argument you typed would be a floor with a hole in it.
    let repo = ff_core::discover(".")?;

    let raw = rev.as_deref().unwrap_or("@");
    let point = Revset::parse(raw)?.point(&repo)?;
    // The revision is first, so a bad revision has already won; what is
    // left in the path slot must name a path. A second revision there, as
    // in `ff show HEAD <sha>`, would otherwise read as a filter that
    // matches nothing and answer "it changed no files".
    crate::cmd::paths::require(
        &repo,
        "show",
        "takes one revision first, then paths",
        &paths,
        |_| vec!["ff show <rev>".into(), "ff log -r <revset>".into()],
    )?;
    let view = FileView::resolve(flags, Depth::Patch)?;

    match point.rev {
        Rev::Open(_) => open(ctx, &repo, view, paths),
        Rev::Commit(id) => commit(ctx, &repo, id.object_id(), view, paths),
    }
}

/// The files under the message, in the view's form. A note follows a
/// one-line message directly; after a body, which ended on a blank line, it
/// stands apart. The patch keeps its one blank line either way, and the
/// rail rows of `--stat` and `--name-only` hang directly under a one-line
/// message the way `ff status` hangs them under `@`.
///
/// `note` is the lines that stand in the note slot before the files: how a
/// merge was measured. They take the slot's gap; the rows follow directly
/// under them, and a patch keeps its blank line.
fn write_files(
    out: &mut impl std::io::Write,
    stat: &ChangeStat,
    view: FileView,
    body: &str,
    colored: bool,
    note: &[String],
    empty_note: &[String],
) -> std::io::Result<()> {
    let note_gap = !body.is_empty();
    if note_gap && (!note.is_empty() || stat.files.is_empty()) {
        writeln!(out)?;
    }
    for line in note {
        writeln!(out, "  {line}")?;
    }
    if stat.files.is_empty() {
        for line in empty_note {
            writeln!(out, "  {line}")?;
        }
        return Ok(());
    }
    if view.depth == Depth::Patch || (note_gap && note.is_empty()) {
        writeln!(out)?;
    }
    write!(out, "{}", fileview::text(stat, view.depth, colored))
}

/// The open change: the same body `ff diff` prints, under a header that says
/// what it is. The sha it names is the open commit's — the one the close
/// lands — when there is one; blank on a clean tree, or under signing.
fn open(
    ctx: &Ctx,
    repo: &ff_core::gix::Repository,
    view: FileView,
    paths: Vec<String>,
) -> Result<()> {
    let change = ff_core::open_change(repo)?;
    // Under `--no-patch` the question is not asked, so the keys are absent
    // rather than empty: absent says nobody looked, empty would say clean.
    let stat = if view.depth == Depth::Skip {
        None
    } else {
        Some(ff_core::change_diff(repo, &view.diff_options(paths))?)
    };

    if ctx.json {
        let mut payload = serde_json::json!({
            "kind": "open",
            "branch": change.branch,
            "subject": change.subject,
            "body": change.body,
            "pending": change.pending,
            "base": change.base,
            "time": change.time,
            "merge": false,
        });
        if let (Some(stat), serde_json::Value::Object(map)) = (&stat, &mut payload) {
            map.insert("against".into(), serde_json::json!(Against::Parent.word()));
            for (key, value) in fileview::json_keys(stat, view.depth) {
                map.insert(key.into(), value);
            }
        }
        return crate::machine::emit("show", &payload);
    }

    crate::render::init_palette(repo);
    let mut out = crate::pager::LogOut::new(repo, ctx.json);
    let colored = out.colored();
    let result = (|| -> std::io::Result<()> {
        writeln!(
            out,
            "@  {}the open change on {}{}",
            match change.pending.as_deref() {
                Some(sha) => format!(
                    "{} ",
                    crate::render::paint_sha(ff_core::sha::short(sha), colored)
                ),
                None => String::new(),
            },
            change.branch,
            match change.time {
                Some(time) => format!("  {}", crate::render::relative_age(now_secs(), time)),
                None => String::new(),
            }
        )?;
        writeln!(
            out,
            "  {}",
            change
                .subject
                .as_deref()
                .unwrap_or("(no description yet — ff describe -m)")
        )?;
        write_body(&mut out, &change.body)?;
        match &stat {
            None => Ok(()),
            Some(stat) => write_files(
                &mut out,
                stat,
                view,
                &change.body,
                colored,
                &[],
                &["(nothing is open)".to_string()],
            ),
        }
    })();
    out.finish();
    result.map_err(Error::repo)
}

/// One commit: its furniture, then what it did — its tree against what
/// `measure` says it is measured against: its parent, or for a merge the
/// auto-merge of its parents.
fn commit(
    ctx: &Ctx,
    repo: &ff_core::gix::Repository,
    id: ff_core::gix::ObjectId,
    view: FileView,
    paths: Vec<String>,
) -> Result<()> {
    let commit = repo.find_commit(id).map_err(Error::repo)?;
    let parents: Vec<ff_core::gix::ObjectId> = commit.parent_ids().map(|p| p.detach()).collect();
    let merge = parents.len() > 1;
    let author = commit.author().map_err(Error::repo)?;
    let (subject, body) = ff_core::message::split(commit.message_raw_sloppy());
    let time = author.time().map_err(Error::repo)?.seconds;
    let change_id = ff_core::changeid::of_commit(&commit.data, &id).letters();

    // One commit, one verification, always: this is the verb that shows a
    // commit whole, and its signature is part of what it is. An unsigned
    // commit costs no spawn — the header is not there to read.
    let signature = ff_core::sign::verify::verify(repo, id)?;

    // A merge is measured against the auto-merge of its parents, so a clean
    // merge shows nothing and a merge that resolved a conflict or carried an
    // edit shows that; `diff.against` says which measure applied. Under
    // `--no-patch` the question is not asked at all, and the keys are
    // absent, `against` with them.
    let diff = if view.depth == Depth::Skip {
        None
    } else {
        Some(ff_core::commit_diff(repo, id, &view.diff_options(paths))?)
    };

    if ctx.json {
        let mut payload = serde_json::json!({
            "kind": "commit",
            "id": id.to_string(),
            "short_id": ff_core::sha::short(&id.to_string()),
            "change_id": change_id,
            "subject": subject,
            "body": body,
            "author_name": author.name.to_string(),
            "author_email": author.email.to_string(),
            "time": time,
            "parents": parents.iter().map(|p| p.to_string()).collect::<Vec<_>>(),
            "merge": merge,
        });
        if let serde_json::Value::Object(map) = &mut payload {
            if let Some(diff) = &diff {
                map.insert("against".into(), serde_json::json!(diff.against.word()));
                for (key, value) in fileview::json_keys(&diff.stat, view.depth) {
                    map.insert(key.into(), value);
                }
            }
            map.insert("signature".into(), serde_json::json!(signature));
        }
        return crate::machine::emit("show", &payload);
    }

    crate::render::init_palette(repo);
    let mut out = crate::pager::LogOut::new(repo, ctx.json);
    let colored = out.colored();
    let result = (|| -> std::io::Result<()> {
        // The sha, then the change id whole: `ff log` shows its first eight
        // letters, and this is the verb that shows a commit whole.
        writeln!(
            out,
            "{}  {}  {}  {}",
            crate::render::paint_sha(ff_core::sha::short(&id.to_string()), colored),
            crate::render::paint_id(&change_id, colored),
            author.name,
            crate::render::relative_age(now_secs(), time)
        )?;
        // The signature is furniture, so it sits with the header; the
        // subject and body stay contiguous under it, git's layout.
        if signature.present {
            // One line: the verdict, who, and the least that names the key.
            // Enough to know what happened, which is all a header owes.
            let mut line = format!("  signature: {} — {}", signature.word(), signature.summary);
            if let Some(key) = signature.short_key() {
                line.push_str(&format!(" ({} {key})", signature.tool()));
            }
            writeln!(
                out,
                "{}",
                if signature.code == 'G' {
                    crate::render::paint_ok(&line, colored)
                } else {
                    crate::render::paint_warn(&line, colored)
                }
            )?;
        }
        writeln!(out, "  {subject}")?;
        write_body(&mut out, &body)?;
        let Some(diff) = &diff else {
            // Nothing was asked, so nothing is said.
            return Ok(());
        };
        let short = ff_core::sha::short_oid(id);
        // A merge says how it was measured. Against its first parent: the
        // reason, then its files as any commit's. A clean auto-merge: that
        // it did nothing of its own, and the two commands for the
        // per-parent view. An auto-merge with files, or a plain commit:
        // the files, no note.
        let (note, empty_note): (Vec<String>, Vec<String>) = match &diff.against {
            Against::FirstParent(why) => (
                vec![format!(
                    "(a merge {}: measured against its first parent {})",
                    crate::cmd::diff::fallback_words(why),
                    parents
                        .first()
                        .map(|p| ff_core::sha::short_oid(*p))
                        .unwrap_or_default()
                )],
                vec!["(it changed no files)".to_string()],
            ),
            Against::AutoMerge => (
                Vec::new(),
                vec![
                    "(a clean merge: nothing beyond its parents)".to_string(),
                    format!("ff diff --from {short}^ --to {short}"),
                    format!("ff diff --from {short}^2 --to {short}"),
                ],
            ),
            Against::Parent => (Vec::new(), vec!["(it changed no files)".to_string()]),
        };
        write_files(
            &mut out,
            &diff.stat,
            view,
            &body,
            colored,
            &note,
            &empty_note,
        )
    })();
    out.finish();
    result.map_err(Error::repo)
}

/// The body under its subject: a blank line, then each line indented the
/// two spaces the subject has. Nothing for a one-line message.
fn write_body(out: &mut impl std::io::Write, body: &str) -> std::io::Result<()> {
    if body.is_empty() {
        return Ok(());
    }
    writeln!(out)?;
    for line in body.lines() {
        if line.is_empty() {
            writeln!(out)?;
        } else {
            writeln!(out, "  {line}")?;
        }
    }
    Ok(())
}

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}
