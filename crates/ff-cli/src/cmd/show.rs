//! `ff show` — one commit, header and patch.
//!
//! The revision half of the patch layer. `ff diff` is the open change;
//! this is anything the revset grammar names, including `@`, which is the
//! open change again — so the two verbs share one renderer rather than
//! wording the same body twice.
//!
//! Resolution goes through `Revset::parse(raw)?.point(repo)?`, the same
//! single-member resolver `ff restore --from` uses, which means the
//! address-space refusal is already written: an operation id typed in a
//! revision position raises `usage/op-in-rev-position` and names `ff op
//! show`. Blob and tree reads (`HEAD:file.txt`, `<tree>`) stay git's, the
//! same call `ff blame` got.

use std::io::Write as _;

use ff_core::revset::{Rev, Revset};
use ff_core::{DiffOptions, Error, Result};

use crate::ctx::Ctx;

pub fn run(ctx: &Ctx, rev: Option<String>, paths: Vec<String>) -> Result<()> {
    // `@` reads the open change against the branch's newest operation, so it
    // needs the same capture `ff diff` needs. A commit read does not, but it
    // is the same verb and a capture-first floor that depended on which
    // argument you typed would be a floor with a hole in it.
    let repo = ff_core::discover(".")?;

    let raw = rev.as_deref().unwrap_or("@");
    let point = Revset::parse(raw)?.point(&repo)?;
    let opts = DiffOptions { hunks: true, paths };

    match point.rev {
        Rev::Open => open(ctx, &repo, &opts),
        Rev::Commit(id) => commit(ctx, &repo, id.object_id(), &opts),
    }
}

/// The open change: the same body `ff diff` prints, under a header that says
/// what it is. Nothing here is a commit yet, so there is no id to name.
fn open(ctx: &Ctx, repo: &ff_core::gix::Repository, opts: &DiffOptions) -> Result<()> {
    let change = ff_core::open_change(repo)?;
    let stat = ff_core::change_diff(repo, opts)?;

    if ctx.json {
        let payload = serde_json::json!({
            "kind": "open",
            "branch": change.branch,
            "subject": change.subject,
            "pending": change.pending,
            "base": change.base,
            "time": change.time,
            "merge": false,
            "changes": stat.files,
            "insertions": stat.insertions,
            "deletions": stat.deletions,
        });
        return crate::machine::emit("show", &payload);
    }

    crate::render::init_palette(repo);
    let mut out = crate::pager::LogOut::new(repo, ctx.json);
    let colored = out.colored();
    let result = (|| -> std::io::Result<()> {
        writeln!(
            out,
            "@  the open change on {}{}",
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
        if stat.files.is_empty() {
            writeln!(out, "  (nothing is open)")?;
            return Ok(());
        }
        writeln!(out)?;
        write!(out, "{}", crate::render::patch_block(&stat.files, colored))
    })();
    out.finish();
    result.map_err(Error::repo)
}

/// One commit: its furniture, then what it did — its tree against its first
/// parent's.
fn commit(
    ctx: &Ctx,
    repo: &ff_core::gix::Repository,
    id: ff_core::gix::ObjectId,
    opts: &DiffOptions,
) -> Result<()> {
    let commit = repo.find_commit(id).map_err(Error::repo)?;
    let parents: Vec<ff_core::gix::ObjectId> = commit.parent_ids().map(|p| p.detach()).collect();
    let merge = parents.len() > 1;
    let author = commit.author().map_err(Error::repo)?;
    let subject = commit.message().map_err(Error::repo)?.summary().to_string();
    let time = author.time().map_err(Error::repo)?.seconds;

    // One commit, one verification, always: this is the verb that shows a
    // commit whole, and its signature is part of what it is. An unsigned
    // commit costs no spawn — the header is not there to read.
    let signature = ff_core::sign::verify::verify(repo, id)?;

    // A merge has no single "what it did": which parent to measure against
    // is a choice, and making it silently would report a diff nobody asked
    // for. git prints nothing here either — this at least says why.
    let stat = if merge {
        None
    } else {
        let before = match parents.first() {
            Some(parent) => repo
                .find_commit(*parent)
                .map_err(Error::repo)?
                .tree_id()
                .map_err(Error::repo)?
                .detach(),
            None => ff_core::gix::ObjectId::empty_tree(repo.object_hash()),
        };
        Some(ff_core::tree_diff(
            repo,
            before,
            commit.tree_id().map_err(Error::repo)?.detach(),
            opts,
        )?)
    };

    if ctx.json {
        let payload = serde_json::json!({
            "kind": "commit",
            "id": id.to_string(),
            "short_id": ff_core::sha::short(&id.to_string()),
            "subject": subject,
            "author_name": author.name.to_string(),
            "author_email": author.email.to_string(),
            "time": time,
            "parents": parents.iter().map(|p| p.to_string()).collect::<Vec<_>>(),
            "merge": merge,
            "changes": stat.as_ref().map(|s| s.files.clone()).unwrap_or_default(),
            "insertions": stat.as_ref().map(|s| s.insertions).unwrap_or(0),
            "deletions": stat.as_ref().map(|s| s.deletions).unwrap_or(0),
            "signature": signature,
        });
        return crate::machine::emit("show", &payload);
    }

    crate::render::init_palette(repo);
    let mut out = crate::pager::LogOut::new(repo, ctx.json);
    let colored = out.colored();
    let result = (|| -> std::io::Result<()> {
        writeln!(
            out,
            "{}  {}  {}",
            crate::render::paint_sha(ff_core::sha::short(&id.to_string()), colored),
            author.name,
            crate::render::relative_age(now_secs(), time)
        )?;
        writeln!(out, "  {subject}")?;
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
        match &stat {
            None => {
                writeln!(
                    out,
                    "  (a merge — which parent to diff against is a choice)"
                )?;
                writeln!(
                    out,
                    "  ff git show -m {} shows it against each",
                    ff_core::sha::short(&id.to_string())
                )
            }
            Some(stat) if stat.files.is_empty() => writeln!(out, "  (it changed no files)"),
            Some(stat) => {
                writeln!(out)?;
                write!(out, "{}", crate::render::patch_block(&stat.files, colored))
            }
        }
    })();
    out.finish();
    result.map_err(Error::repo)
}

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}
