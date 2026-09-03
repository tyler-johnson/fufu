//! `ff describe` — pending description edit, `-b` to name the branch you are
//! on, or a target revision to reword a commit that has already closed.
//! Bare form (and a bare `@`) opens $EDITOR seeded with the current pending
//! text; naming a rev opens it seeded with that commit's message instead —
//! both are sanctioned spawns, exactly like git's editor behavior.

use ff_core::gix;
use ff_core::revset::{Rev, Revset};
use ff_core::{Error, Result};

use crate::ctx::Ctx;

pub fn run(
    ctx: &Ctx,
    rev: Option<String>,
    message: Option<String>,
    branch: Option<String>,
    no_verify: bool,
) -> Result<()> {
    let repo = ff_core::discover(".")?;

    // Non-interactive gate: when no terminal and no -m, fail early instead
    // of trying to spawn an editor. A rev with no -m already trips this —
    // the refusal belongs to the resolved revision, not to whichever
    // spelling of "the open change" was typed.
    if message.is_none() && branch.is_none() && !crate::machine::interactive() {
        return Err(Error::coded(
            "usage/needs-message",
            "no description given and no terminal to open an editor on",
            vec!["ff describe -m <msg>".into()],
        ));
    }

    if let Some(new_name) = branch {
        let (report, verb_ctx) = ff_core::branch::rename_current(
            &repo,
            &new_name,
            &crate::provenance::pre_ff(ctx),
            None,
            std::env::args().collect(),
        )?;
        crate::render::init_palette(&repo);
        crate::render::reconcile_notice(&verb_ctx.reconcile);
        let colored = crate::pager::color_enabled();
        if ctx.json {
            let payload = serde_json::json!({
                "renamed": report,
                "undo": "ff undo",
            });
            crate::machine::emit("describe", &payload)?;
            return Ok(());
        }
        // One act, two readings: a petname had no name worth keeping, so
        // taking one is a claim; a chosen name being replaced is a rename.
        if ff_core::branch::is_anonymous(&report.from) {
            println!("claimed {} as {}", report.from, report.to);
        } else {
            println!("renamed {} to {}", report.from, report.to);
        }
        println!("{}", crate::render::paint_dim("undo: ff undo", colored));
        return Ok(());
    }

    // Resolve the rev: no rev, or one that points at the open change,
    // both mean the pending-description path below. Anything else is a
    // closed commit to reword.
    let target = match &rev {
        Some(src) => match Revset::parse(src)?.point(&repo)?.rev {
            Rev::Open => None,
            Rev::Commit(id) => Some(id.object_id()),
        },
        None => None,
    };

    if let Some(id) = target {
        return reword(ctx, &repo, id, message, no_verify);
    }

    let text = match message {
        Some(text) => Some(text),
        None => edit_pending(&repo)?,
    };

    let (report, verb_ctx) = ff_core::describe::set_pending(
        &repo,
        text,
        &crate::provenance::pre_ff(ctx),
        None,
        std::env::args().collect(),
    )?;
    crate::render::init_palette(&repo);
    crate::render::reconcile_notice(&verb_ctx.reconcile);

    if ctx.json {
        let payload = serde_json::json!({
            "describe": report,
            "undo": "ff undo",
        });
        crate::machine::emit("describe", &payload)?;
        return Ok(());
    }
    match (&report.old, &report.new) {
        (_, Some(new)) => println!("pending description on {}: {new}", report.branch),
        (Some(_), None) => println!("cleared the pending description on {}", report.branch),
        (None, None) => println!("no pending description on {}", report.branch),
    }
    Ok(())
}

/// Reword `target`, a commit that has already closed, and restack whatever
/// sits on top of it.
fn reword(
    ctx: &Ctx,
    repo: &gix::Repository,
    target: gix::ObjectId,
    message: Option<String>,
    no_verify: bool,
) -> Result<()> {
    let text = match message {
        Some(text) => text,
        // An empty editor result is not turned into a clear here — core
        // refuses it with usage/needs-message, so it is passed straight
        // through and left to raise. One authority for that check.
        None => edit_reword(repo, target)?,
    };

    let (report, verb_ctx) = ff_core::describe::reword(
        repo,
        target,
        text,
        crate::verify(no_verify),
        &crate::provenance::pre_ff(ctx),
        None,
        std::env::args().collect(),
    )?;
    crate::render::init_palette(repo);
    crate::render::reconcile_notice(&verb_ctx.reconcile);

    if ctx.json {
        let payload = serde_json::json!({
            "reword": report,
            "undo": "ff undo",
        });
        crate::machine::emit("describe", &payload)?;
        return Ok(());
    }

    let colored = crate::pager::color_enabled();
    let short_new = ff_core::sha::short(&report.new);
    println!(
        "reworded {} on {}: {}",
        crate::render::paint_sha(short_new, colored),
        report.branch,
        report.subject
    );
    if report.restacked > 0 {
        if report.moved.is_empty() {
            println!("restacked {} commit(s) above it", report.restacked);
        } else {
            println!(
                "restacked {} commit(s) above it; moved {}",
                report.restacked,
                report.moved.join(", ")
            );
        }
    }
    // The branches stacked above this one. A reword moves no tree, so the
    // cascade replays clean or skips; the lines name what followed.
    for line in crate::render::cascade_lines(&report.cascade, colored) {
        println!("{line}");
    }
    if report.published > 0 {
        // Disclosure, not a warning: naming where the rewritten commits
        // already live, with no "careful" and no suggested fix — a
        // rewrite is allowed to outrun what has been pushed.
        let upstream_name = ff_core::upstream(repo)?
            .map(|u| u.r#ref)
            .unwrap_or_else(|| "the remote".to_string());
        println!(
            "{} of the rewritten commits are already on {}",
            report.published, upstream_name
        );
    }
    println!("{}", crate::render::paint_dim("undo: ff undo", colored));
    Ok(())
}

/// Seed a temp file with the current pending description, open $EDITOR
/// (fall back to vi), and read the result. Empty result clears.
fn edit_pending(repo: &gix::Repository) -> Result<Option<String>> {
    let branch = current_branch_name(repo)?;
    let current = ff_core::branchmeta::read(repo, &branch)?
        .pending_description
        .unwrap_or_default();
    let comment = format!(
        "# Describe the open change on {branch}.\n# Lines starting with '#' are dropped; an empty description clears it.\n"
    );
    let text = run_editor(repo, &current, &comment)?;
    Ok((!text.is_empty()).then_some(text))
}

/// Seed a temp file with `target`'s current message, open $EDITOR (fall
/// back to vi), and read the result, trimmed. An empty result is returned
/// as-is — the caller decides what that means.
fn edit_reword(repo: &gix::Repository, target: gix::ObjectId) -> Result<String> {
    let branch = current_branch_name(repo)?;
    let short = ff_core::sha::short_oid(target);
    let commit = repo.find_object(target).map_err(Error::repo)?.into_commit();
    let seed = commit.message_raw().map_err(Error::repo)?.to_string();
    let comment = format!(
        "# Reword {short} on {branch}.\n# Lines starting with '#' are dropped; an empty description leaves it alone.\n"
    );
    run_editor(repo, &seed, &comment)
}

/// The current branch's short name, or the house `repo/detached` refusal —
/// shared by both editor seeders, since neither can name a branch to put
/// in the comment block without it.
fn current_branch_name(repo: &gix::Repository) -> Result<String> {
    match ff_core::head_state(repo)? {
        ff_core::HeadState::Branch { name, .. } => Ok(name),
        ff_core::HeadState::Unborn { r#ref } => Ok(r#ref
            .strip_prefix("refs/heads/")
            .unwrap_or(&r#ref)
            .to_string()),
        ff_core::HeadState::Detached { .. } => Err(Error::coded(
            "repo/detached",
            "detached HEAD: there is no change to describe",
            vec!["ff switch <branch>".into()],
        )),
    }
}

/// Write `seed` plus a trailing comment block to `.git/FF_DESCRIBE_MSG`,
/// spawn `$VISUAL`/`$EDITOR`/`vi` on it, strip `#` lines, and return the
/// trimmed result.
fn run_editor(repo: &gix::Repository, seed: &str, comment: &str) -> Result<String> {
    let path = repo.git_dir().join("FF_DESCRIBE_MSG");
    std::fs::write(&path, format!("{seed}\n{comment}")).map_err(Error::repo)?;

    let editor = std::env::var("VISUAL")
        .or_else(|_| std::env::var("EDITOR"))
        .unwrap_or_else(|_| "vi".to_string());
    let status = std::process::Command::new("sh")
        .arg("-c")
        .arg(format!("{editor} \"$1\"",))
        .arg("sh")
        .arg(&path)
        .status()
        .map_err(|err| {
            Error::coded(
                "editor/failed",
                format!("could not run editor {editor}: {err}"),
                vec!["ff describe -m <msg>".into()],
            )
        })?;
    if !status.success() {
        let _ = std::fs::remove_file(&path);
        return Err(Error::coded(
            "editor/failed",
            "editor exited non-zero; description unchanged",
            vec!["ff describe -m <msg>".into()],
        ));
    }
    let raw = std::fs::read_to_string(&path).map_err(Error::repo)?;
    let _ = std::fs::remove_file(&path);
    Ok(raw
        .lines()
        .filter(|line| !line.starts_with('#'))
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string())
}
