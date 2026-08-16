//! `ff op` — the operation log as objects.
//!
//! Six verbs under one name, and every envelope carries the full path
//! (`"op log"`, `"op show"`) rather than the bare family. `ff session` is the
//! anti-precedent: a listing and a diffstat both went out stamped `session`,
//! so a consumer had to read the payload to learn which shape it had.

use std::io::Write as _;

use ff_core::{Error, OpVerbOptions, Result};

use crate::cli::OpAction;
use crate::ctx::{At, Ctx};

pub fn run(ctx: &Ctx, action: OpAction) -> Result<()> {
    match action {
        OpAction::Log {
            count,
            revisions,
            captures,
            ..
        } => log(ctx, count, revisions, captures),
        OpAction::Show { op, .. } => show(ctx, op),
        OpAction::Diff { a, b, .. } => diff(ctx, a, b),
        OpAction::Restore { op, force } => restore(ctx, op, force),
        OpAction::Revert { op } => revert(ctx, op),
    }
}

/// The operation `--at-op` / `--at` placed this command at, if either was
/// given. One kind per door, resolved through the resolver that owns it:
/// `OpLog::resolve` for an id (which refuses hex), the clock for a time.
fn placed_at(ctx: &Ctx, repo: &ff_core::gix::Repository) -> Result<Option<ff_core::OpId>> {
    let Some(at) = &ctx.at else { return Ok(None) };
    let log = ff_core::ops::OpLog::open(repo)?;
    Ok(Some(match at {
        At::Op(spec) => log.resolve(spec)?,
        At::Time(raw) => {
            let now = now_secs();
            let at = ff_core::restore_time(raw, now)?;
            let mut found = None;
            for op in log.iter() {
                let op = op?;
                if op.time() <= at {
                    found = Some(op.id());
                    break;
                }
            }
            found.ok_or_else(|| {
                Error::coded(
                    "op/not-found",
                    format!("no operation on the log at or before {raw}"),
                    vec!["ff op log".into()],
                )
            })?
        }
    }))
}

/// The operation an argument names, defaulting to `@` — the newest.
fn resolve(
    ctx: &Ctx,
    repo: &ff_core::gix::Repository,
    spec: Option<&str>,
) -> Result<ff_core::OpId> {
    match spec {
        Some(spec) => ff_core::ops::OpLog::open(repo)?.resolve(spec),
        // No positional: the context flags fill the slot, and `@` when
        // neither was given.
        None => match placed_at(ctx, repo)? {
            Some(id) => Ok(id),
            None => ff_core::ops::OpLog::open(repo)?.resolve("@"),
        },
    }
}

fn log(ctx: &Ctx, count: usize, revisions: Option<String>, captures: bool) -> Result<()> {
    // Parsed before the repository is even opened: the grammar is pure, so a
    // misspelled revset fails the same way in a repo and out of one.
    let revs = match &revisions {
        Some(src) => Some(ff_core::revset::Revset::parse(src)?),
        None => None,
    };
    let repo = ff_core::discover(".")?;
    // Bounded at a past operation rather than filtered: operations behind a
    // point never change, so the log as it read then is this log with its
    // head cut off.
    let start = placed_at(ctx, &repo)?;
    let entries = match &revs {
        // `-r` replaces where the rows come from and nothing else, so it
        // composes with --captures and with the context flags: a bounded log
        // is the set the expression is evaluated against.
        Some(revs) => {
            let bound = start;
            let members = revs.evaluate_ops(&repo)?;
            let ids: Vec<ff_core::Result<ff_core::OpId>> = match bound {
                None => members.map(|m| m.map(|m| m.id)).collect(),
                Some(at) => {
                    let visible: std::collections::HashSet<ff_core::OpId> =
                        ff_core::ops::OpLog::open(&repo)?
                            .iter_from(at)
                            .map(|op| op.map(|op| op.id()))
                            .collect::<ff_core::Result<_>>()?;
                    members
                        .map(|m| m.map(|m| m.id))
                        .filter(|id| id.as_ref().is_ok_and(|id| visible.contains(id)))
                        .collect()
                }
            };
            ff_core::ops::read_ops_of(&repo, ids.into_iter(), count, captures)?
        }
        None => ff_core::ops::read_ops_from(&repo, start, count, captures)?,
    };
    crate::render::init_palette(&repo);
    let mut out = crate::pager::LogOut::new(&repo, ctx.json);
    let colored = out.colored();
    let result = (|| -> std::io::Result<()> {
        if ctx.json {
            let payload = serde_json::json!({ "ops": entries });
            crate::machine::write(&mut out, "op log", &payload).map_err(std::io::Error::other)?;
            return Ok(());
        }
        if entries.is_empty() {
            writeln!(out, "no operations recorded yet")?;
            return Ok(());
        }
        let now = now_secs();
        for op in &entries {
            writeln!(out, "{}", crate::render::op_row(op, now, colored))?;
        }
        Ok(())
    })();
    out.finish();
    result.map_err(Error::repo)
}

fn show(ctx: &Ctx, spec: Option<String>) -> Result<()> {
    let repo = ff_core::discover(".")?;
    let id = resolve(ctx, &repo, spec.as_deref())?;
    let log = ff_core::ops::OpLog::open(&repo)?;
    let op = log.get(id)?;

    // What the operation changed in the worktree: its own tree against the
    // one before it. Every operation has a tree, which is what makes this
    // uniform across the four kinds.
    let before = match op.prev() {
        Some(prev) => log.get(prev)?.tree(),
        None => ff_core::gix::ObjectId::empty_tree(repo.object_hash()),
    };
    let stat = ff_core::tree_diff_stat(&repo, before, op.tree())?;
    let refs: Vec<_> = op
        .record()?
        .map(|record| record.refs.clone())
        .unwrap_or_default();

    if ctx.json {
        let payload = serde_json::json!({
            "id": id.to_string(),
            "kind": op.kind().as_str(),
            "summary": op.summary(),
            "time": op.time(),
            "branch": op.branch(),
            "session": op.session(),
            "base": op.base().map(|b| b.to_string()),
            "prev": op.prev().map(|p| p.to_string()),
            "tree": op.tree().to_string(),
            "refs": refs,
            "changes": stat.files,
            "insertions": stat.insertions,
            "deletions": stat.deletions,
        });
        return crate::machine::emit("op show", &payload);
    }

    crate::render::init_palette(&repo);
    let colored = crate::pager::color_enabled();
    let now = now_secs();
    println!(
        "{}  {}  {}",
        crate::render::paint_id(&id.short(12), colored),
        op.kind().as_str(),
        crate::render::relative_age(now, op.time())
    );
    println!("  {}", op.summary());
    if let Some(branch) = op.branch() {
        println!("  on        {branch}");
    }
    if let Some(session) = op.session() {
        println!("  session   {session}");
    }
    if let Some(base) = op.base() {
        println!(
            "  base      {}",
            crate::render::paint_sha(&base.to_string()[..7], colored)
        );
    }
    for t in &refs {
        let what = match (&t.old, &t.new) {
            (_, Some(new)) => format!("→ {}", crate::render::paint_sha(&new[..7], colored)),
            (Some(_), None) => "deleted".to_string(),
            (None, None) => continue,
        };
        println!("  {} {what}", t.name);
    }
    if stat.files.is_empty() {
        println!("  (the worktree is unchanged across it)");
    } else {
        println!("{}", crate::render::render_diffstat(&stat, colored));
    }
    Ok(())
}

fn diff(ctx: &Ctx, a: String, b: Option<String>) -> Result<()> {
    let repo = ff_core::discover(".")?;
    let log = ff_core::ops::OpLog::open(&repo)?;
    let a_id = log.resolve(&a)?;
    let b_id = resolve(ctx, &repo, b.as_deref())?;
    let stat = ff_core::tree_diff_stat(&repo, log.get(a_id)?.tree(), log.get(b_id)?.tree())?;

    if ctx.json {
        let payload = serde_json::json!({
            "a": a_id.to_string(),
            "b": b_id.to_string(),
            "changes": stat.files,
            "insertions": stat.insertions,
            "deletions": stat.deletions,
        });
        return crate::machine::emit("op diff", &payload);
    }

    crate::render::init_palette(&repo);
    let colored = crate::pager::color_enabled();
    // Twelve letters, the same length the ambiguity refusal lists candidates
    // at: long enough to be sure of, short enough to read side by side. The
    // full ids are on the machine surface for anything that needs them.
    println!(
        "{} → {}",
        crate::render::paint_id(&a_id.short(12), colored),
        crate::render::paint_id(&b_id.short(12), colored)
    );
    if stat.files.is_empty() {
        println!("  (no files differed)");
    } else {
        println!("{}", crate::render::render_diffstat(&stat, colored));
    }
    Ok(())
}

fn restore(ctx: &Ctx, op: String, force: bool) -> Result<()> {
    let repo = ff_core::discover(".")?;
    crate::render::init_palette(&repo);
    let (report, verb_ctx) = ff_core::rewind(
        &repo,
        &ff_core::Landing::At(op),
        &ff_core::RewindOptions {
            force,
            now: None,
            argv: std::env::args().collect(),
        },
        &crate::provenance::pre_ff(ctx),
    )?;
    crate::render::reconcile_notice(&verb_ctx.reconcile);
    crate::cmd::undo::report_move(ctx, &report, "op restore")
}

fn revert(ctx: &Ctx, op: String) -> Result<()> {
    let repo = ff_core::discover(".")?;
    crate::render::init_palette(&repo);
    let (report, verb_ctx) = ff_core::revert(
        &repo,
        &op,
        &OpVerbOptions {
            now: None,
            argv: std::env::args().collect(),
        },
        &crate::provenance::pre_ff(ctx),
    )?;
    crate::render::reconcile_notice(&verb_ctx.reconcile);

    if ctx.json {
        let payload = serde_json::json!({
            "revert": report,
            "undo": "ff undo",
        });
        return crate::machine::emit("op revert", &payload);
    }

    let colored = crate::pager::color_enabled();
    println!(
        "reverted {}: {}",
        crate::render::paint_id(&report.reverted, colored),
        report.reverted_summary
    );
    for t in &report.refs {
        let what = match (&t.old, &t.new) {
            (_, Some(new)) => format!("→ {}", crate::render::paint_sha(&new[..7], colored)),
            (Some(_), None) => "deleted".to_string(),
            (None, None) => continue,
        };
        println!("  {} {what}", t.name);
    }
    println!("{}", crate::render::paint_dim("undo: ff undo", colored));
    Ok(())
}

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}
