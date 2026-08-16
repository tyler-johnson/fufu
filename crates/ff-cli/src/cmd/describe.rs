//! `ff describe` — pending description edit, or `-b` to name the branch you
//! are on (claiming a petname and renaming a chosen name alike). Bare form
//! opens $EDITOR seeded with the current pending text — a sanctioned spawn,
//! exactly like git's editor behavior.

use ff_core::{Error, Result};

use crate::ctx::Ctx;

pub fn run(ctx: &Ctx, message: Option<String>, branch: Option<String>) -> Result<()> {
    let repo = ff_core::discover(".")?;

    // Non-interactive gate: when no terminal and no -m, fail early instead
    // of trying to spawn an editor.
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

    let text = match message {
        Some(text) => Some(text),
        None => edit_in_editor(&repo)?,
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

/// Seed a temp file with the current pending description, open $EDITOR
/// (fall back to vi), and read the result. Empty result clears.
fn edit_in_editor(repo: &ff_core::gix::Repository) -> Result<Option<String>> {
    let head = ff_core::head_state(repo)?;
    let branch = match &head {
        ff_core::HeadState::Branch { name, .. } => name.clone(),
        ff_core::HeadState::Unborn { r#ref } => r#ref
            .strip_prefix("refs/heads/")
            .unwrap_or(r#ref)
            .to_string(),
        ff_core::HeadState::Detached { .. } => {
            return Err(Error::coded(
                "repo/detached",
                "detached HEAD: there is no change to describe",
                vec!["ff switch <branch>".into()],
            ));
        }
    };
    let current = ff_core::branchmeta::read(repo, &branch)?
        .pending_description
        .unwrap_or_default();
    let path = repo.git_dir().join("FF_DESCRIBE_MSG");
    let seed = format!(
        "{current}\n# Describe the open change on {branch}.\n# Lines starting with '#' are dropped; an empty description clears it.\n"
    );
    std::fs::write(&path, seed).map_err(Error::repo)?;

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
    let text: String = raw
        .lines()
        .filter(|line| !line.starts_with('#'))
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string();
    Ok((!text.is_empty()).then_some(text))
}
