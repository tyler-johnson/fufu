//! `ff worktree` — the worktrees of this repository, and the chains of the
//! ones that are gone.
//!
//! The earn over `git worktree list` is the second section: a chain lives in
//! the shared ref namespace, so it outlives its checkout, and git cannot
//! know a chain whose worktree is gone. That row is the front door for a bay
//! somebody deleted with work in it — its tip is what `ff restore --at-op`
//! takes.

use ff_core::Result;

use crate::cli::WorktreeAction;
use crate::ctx::Ctx;

pub fn run(ctx: &Ctx, action: Option<WorktreeAction>) -> Result<()> {
    // Bare `ff worktree` is the list, on the same rule as bare `ff branch`.
    match action {
        None => list(ctx),
        Some(WorktreeAction::List { .. }) => list(ctx),
    }
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
