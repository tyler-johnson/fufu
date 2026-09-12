//! `ff absorb` and `ff lift` — one move, two spellings. Content leaves a run
//! of commits, `--from`, and lands in one commit, `--into`; each verb's word
//! is its defaults: absorb from the open change into the commit under it,
//! lift from the commit under the open change into the open change. A path
//! filter chooses which of the sources' files move.

use ff_core::absorb::{Endpoint, MoveOptions, MoveVerb};
use ff_core::revset::{Rev, Revset};
use ff_core::{MoveOutcome, Result};

use crate::ctx::Ctx;

pub fn run(
    ctx: &Ctx,
    verb: MoveVerb,
    from: Option<String>,
    into: Option<String>,
    message: Option<String>,
    paths: Vec<String>,
    no_verify: bool,
) -> Result<()> {
    let repo = ff_core::discover(".")?;

    let endpoint = |rev: Rev| match rev {
        Rev::Open(_) => Endpoint::Open,
        Rev::Commit(id) => Endpoint::Commit(id.object_id()),
    };
    let from = match &from {
        Some(src) => Some(
            Revset::parse(src)?
                .members(&repo)?
                .into_iter()
                .map(endpoint)
                .collect(),
        ),
        None => None,
    };
    let into = match &into {
        Some(src) => Some(endpoint(Revset::parse(src)?.point(&repo)?.rev)),
        None => None,
    };

    let (outcome, verb_ctx) = ff_core::absorb::move_change(
        &repo,
        &MoveOptions {
            verb,
            from,
            into,
            paths,
            message,
            verify: crate::verify(no_verify),
            now: None,
            argv: std::env::args().collect(),
        },
        &crate::provenance::pre_ff(ctx),
    )?;
    crate::render::init_palette(&repo);
    crate::render::reconcile_notice(&verb_ctx.reconcile);

    let cmd = verb.as_str();
    match outcome {
        MoveOutcome::Moved(report) => {
            if ctx.json {
                let payload = serde_json::json!({
                    "move": report,
                    "undo": "ff undo",
                });
                crate::machine::emit(cmd, &payload)?;
                return Ok(());
            }
            let colored = crate::pager::color_enabled();
            println!(
                "moved {} file(s) from {} into {}",
                report.files.len(),
                sources(&report, colored),
                target(&report, colored)
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
            // The target's own drop is in the headline; a source's is not,
            // so each one is named here.
            for line in
                crate::render::dropped_lines(&report.dropped, Some(&report.into.id), colored)
            {
                println!("{line}");
            }
            if report.published > 0 {
                // Disclosure, not a warning, on the same rule as reword.
                let upstream_name = ff_core::upstream(&repo)?
                    .map(|u| u.r#ref)
                    .unwrap_or_else(|| "the remote".to_string());
                println!(
                    "{} of the rewritten commits are already on {}",
                    report.published, upstream_name
                );
            }
            if !report.paths.is_empty() {
                println!("limited to {} path(s)", report.paths.len());
            }
            if report.still_open {
                println!("the rest of your change is still open");
            }
            // The branches stacked above, in the lines every cascading verb
            // prints. A hold above does not change the exit: the move
            // landed, and `ff status` shows the branch waiting.
            for line in crate::render::cascade_lines(&report.cascade, colored) {
                println!("{line}");
            }
            println!("{}", crate::render::paint_dim("undo: ff undo", colored));
        }
        MoveOutcome::Held(report) => {
            if ctx.json {
                let payload = serde_json::json!({
                    "move": serde_json::Value::Null,
                    "held": report,
                });
                crate::machine::emit(cmd, &payload)?;
                crate::exit::held();
                return Ok(());
            }
            let colored = crate::pager::color_enabled();
            println!("{}", crate::render::held_block(&report, colored));
            crate::exit::held();
        }
        MoveOutcome::Nothing { verb, branch } => {
            if ctx.json {
                let payload = serde_json::json!({
                    "move": serde_json::Value::Null,
                    "branch": branch,
                    "nothing": true,
                });
                crate::machine::emit(cmd, &payload)?;
                return Ok(());
            }
            println!("nothing to {verb} on {branch}");
        }
    }
    Ok(())
}

/// The sources as the headline names them: the open change, one commit
/// with its subject, a run by its ends — and the open change on top of a
/// run when it was among them.
fn sources(report: &ff_core::MoveReport, colored: bool) -> String {
    let closed: Vec<&ff_core::MoveSource> = report.from.iter().filter(|s| s.id != "@").collect();
    let open = report.from.iter().any(|s| s.id == "@");
    let run = match closed.as_slice() {
        [] => String::new(),
        [one] => format!(
            "{} \"{}\"",
            crate::render::paint_sha(ff_core::sha::short(&one.id), colored),
            one.subject.as_deref().unwrap_or_default()
        ),
        [lo, .., hi] => format!(
            "{} commits ({}..{})",
            closed.len(),
            crate::render::paint_sha(ff_core::sha::short(&lo.id), colored),
            crate::render::paint_sha(ff_core::sha::short(&hi.id), colored)
        ),
    };
    match (run.is_empty(), open) {
        (true, _) => "the open change".to_string(),
        (false, false) => run,
        (false, true) => format!("{run} and the open change"),
    }
}

/// The target as the headline names it: the open change, the commit's new
/// identity with its subject, or — when the fold left it introducing
/// nothing — the identity it was, and the fact that it is gone.
fn target(report: &ff_core::MoveReport, colored: bool) -> String {
    let into = &report.into;
    if into.id == "@" {
        return "the open change".to_string();
    }
    let subject = into.subject.as_deref().unwrap_or_default();
    match &into.new {
        Some(new) => format!(
            "{}: {subject}",
            crate::render::paint_sha(ff_core::sha::short(new), colored)
        ),
        None => format!(
            "{} \"{subject}\": the commit introduces nothing now and is gone",
            crate::render::paint_sha(ff_core::sha::short(&into.id), colored)
        ),
    }
}
