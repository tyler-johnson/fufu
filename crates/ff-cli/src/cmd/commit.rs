//! `ff commit` — close the open change. The pre-verb snapshot is mandatory
//! and owned by core; contention aborts before anything is written.

use ff_core::{CloseOptions, CommitOutcome, Error, Result};

pub fn run(
    message: Option<String>,
    no_verify: bool,
    branch: Option<String>,
    json: bool,
) -> Result<()> {
    let repo = ff_core::discover(".")?;
    let (outcome, ctx) = ff_core::close(
        &repo,
        &CloseOptions {
            message,
            no_verify,
            branch,
            now: None,
            argv: std::env::args().collect(),
        },
        &crate::provenance::pre_ff(),
    )?;

    crate::render::reconcile_notice(&ctx.reconcile);

    if json {
        let body = serde_json::to_string(&serde_json::json!({
            "commit": outcome,
            "reconcile": ctx.reconcile,
            "undo": "ff undo",
        }))
        .map_err(Error::repo)?;
        println!("{body}");
        return Ok(());
    }

    match &outcome {
        CommitOutcome::Closed {
            short_id,
            branch,
            subject,
            files_changed,
            claimed_from,
            ..
        } => {
            if let Some(old) = claimed_from {
                println!("claimed {old} as {branch}");
            }
            let described = if subject.is_empty() {
                "(no description)".to_string()
            } else {
                subject.clone()
            };
            println!("closed {short_id} on {branch}: {described} ({files_changed} file(s))");
            println!("undo: ff undo");
        }
        CommitOutcome::NothingToClose { branch } => {
            println!("nothing to close on {branch}: the working tree is clean");
        }
    }
    Ok(())
}
