//! `ff restore` — pull worktree state back from somewhere else. No
//! best-effort pre-capture here: restore's own pre-restore capture is
//! mandatory, and its failure aborts the restore.

use ff_core::{Error, RestoreOptions, RestoreSource, Result};

use crate::ctx::{At, Ctx};

pub fn run(ctx: &Ctx, from: Option<String>, all: bool, paths: Vec<String>) -> Result<()> {
    let source = source_of(ctx, from)?;
    let repo = ff_core::discover(".")?;
    crate::render::init_palette(&repo);
    let report = ff_core::restore(
        &repo,
        &RestoreOptions {
            source,
            paths,
            all,
            now: None,
        },
        &crate::provenance::pre_ff(ctx),
    )?;

    if ctx.json {
        let payload = serde_json::json!({
            "origin": report.origin,
            "restored": report.restored,
            "deleted": report.deleted,
            "skipped_gitlinks": report.skipped_gitlinks,
            "pre_op": report.pre_op,
            "undo": "ff undo",
        });
        crate::machine::emit("restore", &payload)?;
        return Ok(());
    }

    let colored = crate::pager::color_enabled();

    // The id wears the color of the space it came from: magenta for an
    // operation, blue for a commit. One palette, and it colors roles.
    let id = if report.origin.space == "operation" {
        crate::render::paint_id(&report.origin.short_id, colored)
    } else {
        crate::render::paint_sha(&report.origin.short_id, colored)
    };
    println!("restored from {id} ({})", report.origin.subject);
    for path in &report.restored {
        println!("  restored  {path}");
    }
    for path in &report.deleted {
        println!("  deleted   {path}");
    }
    for path in &report.skipped_gitlinks {
        println!("  skipped   {path} (embedded repository)");
    }
    if report.restored.is_empty() && report.deleted.is_empty() {
        println!("  (no files differed)");
    }
    println!("{}", crate::render::paint_dim("undo: ff undo", colored));
    Ok(())
}

/// Three flags, one kind each, and no more than one of them at a time.
///
/// `--at-op` and `--at` arrive through `Ctx`, which already refused the pair;
/// `--from` is restore's own, and pairing it with either is refused here for
/// the same reason — three ways to name a source is three, not one ranked
/// list.
fn source_of(ctx: &Ctx, from: Option<String>) -> Result<RestoreSource> {
    match (from, &ctx.at) {
        (Some(_), Some(_)) => Err(Error::coded(
            "usage/bad-flags",
            "--from names a revision and --at-op/--at name an operation: a restore has one \
             source, and which one is not something fufu will rank for you",
            vec![
                "ff restore <path> --from <rev>".into(),
                "ff restore <path> --at-op <op>".into(),
            ],
        )),
        (Some(rev), None) => Ok(RestoreSource::Rev(rev)),
        (None, Some(At::Op(op))) => Ok(RestoreSource::Op(op.clone())),
        (None, Some(At::Time(time))) => Ok(RestoreSource::Time(time.clone())),
        (None, None) => Ok(RestoreSource::Open),
    }
}
