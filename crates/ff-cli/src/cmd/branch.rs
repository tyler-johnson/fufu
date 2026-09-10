//! `ff branch` — the branch family: bare is the list (named and anonymous
//! segregated), `<name> [<rev>]` creates one where you are not, `-d` takes
//! one away. Naming the branch you are on is not here; `ff describe -b` is
//! the one verb that does it.

use ff_core::Result;

use crate::ctx::Ctx;

pub fn run(
    ctx: &Ctx,
    name: Option<String>,
    rev: Option<String>,
    delete: Option<String>,
    shared: bool,
    all: bool,
) -> Result<()> {
    // The parser has already refused every pairing that crosses shapes, so
    // the order here only says which field names the shape.
    if let Some(target) = delete {
        delete_branch(ctx, &target, shared)
    } else if let Some(name) = name {
        create(ctx, &name, rev.as_deref())
    } else {
        list(ctx, all)
    }
}

fn create(ctx: &Ctx, name: &str, rev: Option<&str>) -> Result<()> {
    let repo = ff_core::discover(".")?;
    let (report, verb_ctx) = ff_core::branch::create(
        &repo,
        name,
        rev,
        &crate::provenance::pre_ff(ctx),
        None,
        std::env::args().collect(),
    )?;
    crate::render::init_palette(&repo);
    crate::render::reconcile_notice(&verb_ctx.reconcile);
    let colored = crate::pager::color_enabled();

    if ctx.json {
        let payload = serde_json::json!({
            "create": report,
            "reconcile": verb_ctx.reconcile,
            "undo": "ff undo",
        });
        crate::machine::emit("branch create", &payload)?;
        return Ok(());
    }
    println!(
        "created {} at {} (forked from {})",
        report.name,
        crate::render::paint_sha(ff_core::sha::short(report.at.as_str()), colored),
        report.forked_from
    );
    println!("{}", crate::render::paint_dim("undo: ff undo", colored));
    Ok(())
}

fn delete_branch(ctx: &Ctx, target: &str, shared: bool) -> Result<()> {
    let repo = ff_core::discover(".")?;

    // The wire's cwd is resolved before the local delete; the copy `--shared`
    // will remove is read from the report once the delete is done, and an
    // upstream under another branch's name is not one — it is the base the
    // branch was cut from, and the report leaves it unnamed.
    let cwd = if shared {
        let cwd = repo
            .workdir()
            // Uncoded on purpose: this is not a bare repository, so there is
            // a working directory, and reaching here without one is an
            // internal inconsistency rather than a state to name.
            .ok_or_else(|| ff_core::Error::msg("no working directory: internal inconsistency"))?
            .to_path_buf();
        Some(cwd)
    } else {
        None
    };

    let (report, verb_ctx) = ff_core::branch::delete(
        &repo,
        target,
        &crate::provenance::pre_ff(ctx),
        None,
        std::env::args().collect(),
    )?;
    crate::render::init_palette(&repo);
    crate::render::reconcile_notice(&verb_ctx.reconcile);
    let colored = crate::pager::color_enabled();

    // The wire, and only then its local traces: local first, so a failed
    // push degrades to the plain-delete outcome — the delete done and
    // undoable, the copy intact — and the tracking ref, the config section
    // and the published note come off only after the wire agreed.
    let shared_removed = if let (Some(cwd), Some(shared)) = (&cwd, &report.shared) {
        if !shared.tip.is_empty() {
            crate::net::push_delete(cwd, &shared.remote, &shared.remote_branch, &shared.tip)?;
            ff_core::branch::forget_shared(&repo, target, &shared.r#ref, verb_ctx.now)?;
            true
        } else {
            false
        }
    } else {
        false
    };

    if ctx.json {
        let payload = serde_json::json!({
            "deleted": report,
            "undo": "ff undo",
            "shared_removed": shared_removed,
        });
        crate::machine::emit("branch delete", &payload)?;
        return Ok(());
    }
    println!(
        "deleted {} (was {})",
        report.name,
        crate::render::paint_sha(ff_core::sha::short(report.tip.as_str()), colored)
    );
    if let Some(trash) = &report.trash_ref {
        println!("  its timeline moved to {trash}");
    }
    if report.parked_demoted.is_some() {
        println!("  its parked change stays in the stash (git stash list)");
    }
    match (&report.shared, shared) {
        (Some(shared), true) => {
            if shared_removed {
                println!(
                    "  removed the shared copy {}, and the tracking ref and upstream with it",
                    shared.name
                );
            } else {
                println!("  there was no shared copy to remove");
            }
        }
        (Some(shared), false) => {
            if shared.tip.is_empty() {
                println!(
                    "  its upstream {} is configured and not there — nothing to remove",
                    shared.name
                );
            } else {
                // The way to it is a pair, not a verb: this branch is already
                // gone here, so `--shared` has nothing left to stand on until
                // the undo puts it back. `ff push`'s tail says the same
                // shape for the same reason.
                println!(
                    "  the shared copy {} is still there — ff undo then ff branch -d {} --shared removes it too",
                    shared.name, report.name
                );
            }
        }
        (None, true) => println!("  there was no shared copy to remove"),
        (None, false) => {}
    }
    if shared_removed {
        println!(
            "{}",
            crate::render::paint_dim(
                "the delete left the machine — ff undo cannot reach it",
                colored
            )
        );
        println!(
            "{}",
            crate::render::paint_dim(
                "ff undo brings the branch back; ff push sends the copy again",
                colored
            )
        );
    } else {
        println!("{}", crate::render::paint_dim("undo: ff undo", colored));
    }
    Ok(())
}

fn list(ctx: &Ctx, all: bool) -> Result<()> {
    // Listing branches as of a past operation needs that operation's ref
    // table threaded through the walk, which is the follow-up this plan
    // named rather than the flag being absent.
    ctx.refuse_past("ff branch")?;
    let repo = ff_core::discover(".")?;
    // Reads don't reconcile here; `ff status` owns loudness.
    // Ten is the map's own default branch bound — the bound keeps a clone
    // of a large repository from scrolling your own branches off the top,
    // and `--all` is that wish spelled out.
    let remote_limit = if all { None } else { Some(10) };
    let list = ff_core::branch::list(&repo, &ff_core::BranchListOptions { remote_limit })?;
    if ctx.json {
        crate::machine::emit("branch list", &list)?;
        return Ok(());
    }
    crate::render::init_palette(&repo);
    let colored = crate::pager::color_enabled();
    // One name column across the whole listing — the widest label, not the
    // widest name, since sigil and brackets ride the column too, and the
    // remote labels ride the same column, two narrower for wanting no
    // brackets — floored so a listing of short names does not look cramped.
    let local_width = list
        .named
        .iter()
        .chain(list.anonymous.iter())
        .map(|info| crate::render::branch_label_width(&info.name))
        .max()
        .unwrap_or(0);
    let remote_width = list
        .remote_only
        .iter()
        .map(|info| crate::render::remote_label_width(&info.name))
        .max()
        .unwrap_or(0);
    let label_width = local_width.max(remote_width).max(14);
    let mut gap = false;
    for (section, header) in [(&list.named, ""), (&list.anonymous, "anonymous:")] {
        if section.is_empty() {
            continue;
        }
        if !header.is_empty() {
            if !list.named.is_empty() {
                println!();
            }
            println!("{}", crate::render::paint_dim(header, colored));
            // The section separator is already the air; never double it.
            gap = false;
        }
        for info in section {
            if gap {
                println!();
            }
            let lines = crate::render::branch_row(info, label_width, colored);
            // A row that hung a note gets air beneath it before the next
            // branch's head line; an all-quiet listing stays single-spaced.
            gap = lines.len() > 1;
            for line in lines {
                println!("{line}");
            }
        }
    }
    if !list.remote_only.is_empty() {
        if !list.named.is_empty() || !list.anonymous.is_empty() {
            println!();
        }
        println!("{}", crate::render::paint_dim("remote only:", colored));
        for info in &list.remote_only {
            println!(
                "{}",
                crate::render::remote_branch_row(info, label_width, colored)
            );
        }
        // `remote_more` cannot be non-zero when the bucket is empty, so no
        // orphan count row is possible here.
        if list.remote_more > 0 {
            println!(
                "{}",
                crate::render::remote_more_row(list.remote_more, colored)
            );
        }
    }
    Ok(())
}
