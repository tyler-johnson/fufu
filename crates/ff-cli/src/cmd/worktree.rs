//! `ff worktree` — the worktrees of this repository, and the chains of the
//! ones that are gone.
//!
//! The earn over `git worktree list` is the second section: a chain lives in
//! the shared ref namespace, so it outlives its checkout, and git cannot
//! know a chain whose worktree is gone. That row is the front door for a bay
//! somebody deleted with work in it — its tip is what `ff restore --at-op`
//! takes.

use std::path::Path;

use ff_core::Result;

use crate::cli::WorktreeAction;
use crate::ctx::Ctx;

pub fn run(ctx: &Ctx, action: Option<WorktreeAction>) -> Result<()> {
    // Bare `ff worktree` is the list, on the same rule as bare `ff branch`.
    match action {
        None => list(ctx),
        Some(WorktreeAction::List { .. }) => list(ctx),
        Some(WorktreeAction::Add { path, branch }) => add(ctx, &path, branch.as_deref()),
        Some(WorktreeAction::Remove { target }) => remove(ctx, &target),
    }
}

pub(crate) fn add(ctx: &Ctx, path: &Path, branch: Option<&str>) -> Result<()> {
    let repo = ff_core::discover(".")?;
    let (report, verb_ctx) = ff_core::add_worktree(
        &repo,
        path,
        branch,
        &crate::provenance::pre_ff(ctx),
        None,
        std::env::args().collect(),
    )?;
    crate::render::init_palette(&repo);
    crate::render::reconcile_notice(&verb_ctx.reconcile);

    if ctx.json {
        let payload = serde_json::json!({
            "added": report,
            "undo": "ff undo",
        });
        return crate::machine::emit("worktree add", &payload);
    }

    let colored = crate::pager::color_enabled();
    println!(
        "made {} at {} on {}",
        report.id,
        report.path.display(),
        report.branch
    );
    if report.created_branch {
        println!("{}", crate::render::paint_dim("  on a new branch", colored));
    }
    println!(
        "{}",
        crate::render::paint_dim(&format!("  its log is {}", report.chain), colored)
    );
    // Warnings, not failures: the checkout stands either way, and each one
    // says what it costs.
    for warning in &report.warnings {
        eprintln!(
            "{}",
            crate::render::paint_warn(&format!("ff: {warning}"), colored)
        );
    }
    Ok(())
}

fn remove(ctx: &Ctx, target: &str) -> Result<()> {
    let repo = ff_core::discover(".")?;
    let id = resolve(&repo, target)?;
    let (report, verb_ctx) = ff_core::remove_worktree(
        &repo,
        &id,
        &crate::provenance::pre_ff(ctx),
        None,
        std::env::args().collect(),
    )?;
    crate::render::init_palette(&repo);
    crate::render::reconcile_notice(&verb_ctx.reconcile);

    if ctx.json {
        let payload = serde_json::json!({
            "removed": report,
            "undo": "ff undo",
        });
        return crate::machine::emit("worktree remove", &payload);
    }

    let colored = crate::pager::color_enabled();
    match &report.branch {
        Some(branch) => println!("removed {} (was on {})", report.id, branch),
        None => println!("removed {}", report.id),
    }
    // The line the verb earns: git needs --force for a dirty worktree
    // because it has nowhere to put the work, and this is where fufu says
    // where it put it.
    match &report.capture {
        Some(capture) => {
            // The same 12-letter prefix `ff op log` prints; the full id is
            // in `--json`.
            let id = capture.chars().take(12).collect::<String>();
            println!(
                "  {}{}{}",
                crate::render::paint_dim("captured first as ", colored),
                crate::render::paint_id(&id, colored),
                crate::render::paint_dim(&format!(" — ff restore <path> --at-op {id}"), colored)
            );
        }
        None => {
            println!(
                "{}",
                crate::render::paint_dim("  it held nothing to capture", colored)
            );
        }
    }
    println!(
        "{}",
        crate::render::paint_dim(&format!("  its log stays at {}", report.chain), colored)
    );
    Ok(())
}

/// The worktree a removal names, as an id. A person thinks in paths and the
/// listing prints ids, so both are taken: an exact id match wins over a
/// path, and a path is made absolute before it is compared — a checkout
/// somebody deleted by hand still has an entry to remove, and
/// `canonicalize` would refuse it.
fn resolve(repo: &ff_core::gix::Repository, target: &str) -> Result<String> {
    let survey = ff_core::survey(repo)?;
    if let Some(row) = survey.worktrees.iter().find(|row| row.id == target) {
        return Ok(row.id.clone());
    }
    let absolute = std::path::absolute(target).unwrap_or_else(|_| std::path::PathBuf::from(target));
    if let Some(row) = survey
        .worktrees
        .iter()
        .find(|row| row.path.as_deref() == Some(absolute.as_path()))
    {
        return Ok(row.id.clone());
    }
    Err(ff_core::Error::coded(
        "worktree/not-found",
        format!("no worktree at {target}"),
        vec!["ff worktree list".into()],
    ))
}

fn list(ctx: &Ctx) -> Result<()> {
    // Reading the worktrees as of a past operation would need a past-state
    // view of the layout on disk, which does not exist — the same refusal
    // `ff remote` and `ff branch list` make, for the same reason.
    ctx.refuse_past("ff worktree list")?;

    let repo = ff_core::discover(".")?;
    let survey = ff_core::survey(&repo)?;

    if ctx.json {
        // Survey already derives Serialize, so the whole shape goes out for
        // free: the live rows and the orphan chains under one name.
        return crate::machine::emit("worktree list", &survey);
    }

    crate::render::init_palette(&repo);
    let colored = crate::pager::color_enabled();

    // Live rows: id, checkout, branch. The id is the key of the worktree's
    // own chain, so it lines up with the orphan ids in the section below.
    let id_width = survey
        .worktrees
        .iter()
        .map(|w| w.id.chars().count())
        .max()
        .unwrap_or_default()
        .max(8);
    let path_width = survey
        .worktrees
        .iter()
        .map(|w| match &w.path {
            Some(path) => path.display().to_string().chars().count(),
            None => "no working tree".chars().count(),
        })
        .max()
        .unwrap_or_default();
    for w in &survey.worktrees {
        // The current worktree wears the star immediately before its id;
        // the others wear a blank, so the column still lines up.
        let marker = if w.current { "* " } else { "  " };
        let path = match &w.path {
            Some(path) => cell(&path.display().to_string(), path_width, false, colored),
            None => cell("no working tree", path_width, true, colored),
        };
        let branch = match &w.branch {
            Some(branch) => branch.clone(),
            None => crate::render::paint_dim("detached", colored),
        };
        println!(
            "{}{}  {}  {}",
            marker,
            cell(&w.id, id_width, false, colored),
            path,
            branch
        );
    }

    if !survey.orphans.is_empty() {
        println!();
        println!(
            "{}",
            crate::render::paint_dim("chains whose worktree is gone", colored)
        );
        let orphan_id_width = survey
            .orphans
            .iter()
            .map(|o| o.id.chars().count())
            .max()
            .unwrap_or_default()
            .max(8);
        let orphan_branch_width = survey
            .orphans
            .iter()
            .map(|o| {
                o.branch
                    .as_deref()
                    .map(|b| b.chars().count())
                    .unwrap_or_else(|| "detached".chars().count())
            })
            .max()
            .unwrap_or_default();
        let now = now_secs();
        for o in &survey.orphans {
            let branch = match &o.branch {
                Some(branch) => cell(branch, orphan_branch_width, false, colored),
                None => cell("detached", orphan_branch_width, true, colored),
            };
            // The same 12-letter prefix `ff op log` prints: enough to
            // resolve, short enough to read. The full id is in `--json`.
            let tip = o
                .tip
                .as_deref()
                .map(|id| {
                    crate::render::paint_id(&id.chars().take(12).collect::<String>(), colored)
                })
                .unwrap_or_default();
            // A None time shows nothing rather than a fabricated age.
            let age = o
                .time
                .map(|t| crate::render::relative_age(now, t))
                .unwrap_or_default();
            let tail = if age.is_empty() {
                tip
            } else {
                format!("{tip}  {age}")
            };
            println!(
                "  {}  {}  {}",
                cell(&o.id, orphan_id_width, false, colored),
                branch,
                tail
            );
        }
        // The tip is the point of the row: it is what the restore takes.
        println!(
            "{}",
            crate::render::paint_dim(
                "ff restore <path> --at-op <op>  brings a file back from one",
                colored
            )
        );
    }
    Ok(())
}

/// A left-aligned cell. The pad is measured on the raw text and applied
/// after the paint, because `format!` width would count the ANSI bytes.
fn cell(text: &str, width: usize, dim: bool, colored: bool) -> String {
    let body = if dim {
        crate::render::paint_dim(text, colored)
    } else {
        text.to_string()
    };
    let pad = " ".repeat(width.saturating_sub(text.chars().count()));
    format!("{body}{pad}")
}

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}
