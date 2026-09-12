//! `ff branch` — the branch family: bare is the list (named and anonymous
//! segregated), `<name> [<rev>]` creates one where you are not, `-d` takes
//! one away, `--prune` takes away every one whose shared copy is gone.
//! Naming the branch you are on is not here; `ff describe -b` is the one
//! verb that does it.

use ff_core::{Kept, KeptBranch, PrunedBranch, Result};

use crate::ctx::Ctx;

/// The family's flags, as the parser hands them over.
pub struct Args {
    pub name: Option<String>,
    pub rev: Option<String>,
    pub delete: Option<String>,
    pub shared: bool,
    pub prune: bool,
    pub dry_run: bool,
    pub all: bool,
}

pub fn run(ctx: &Ctx, args: Args) -> Result<()> {
    // The parser has already refused every pairing that crosses shapes, so
    // the order here only says which field names the shape.
    if args.prune {
        prune(ctx, args.dry_run)
    } else if let Some(target) = args.delete {
        delete_branch(ctx, &target, args.shared)
    } else if let Some(name) = args.name {
        create(ctx, &name, args.rev.as_deref())
    } else {
        list(ctx, args.all)
    }
}

/// `ff branch --prune`: fetch, then one operation deleting every branch
/// whose shared copy is gone. The fetch is pull's — foreground, pruning,
/// stamping the lane's cadence — and `--no-fetch` prunes from the refs as
/// they stand.
fn prune(ctx: &Ctx, dry_run: bool) -> Result<()> {
    let repo = ff_core::discover(".")?;
    crate::render::init_palette(&repo);
    let colored = crate::pager::color_enabled();

    // The guards, and the remote: the same ground pull reads before its
    // fetch.
    let pre = ff_core::preflight::preflight(&repo, ff_core::preflight::Verb::Prune)?;
    let cwd = repo
        .workdir()
        // Uncoded on purpose: preflight already refused a bare repository.
        .ok_or_else(|| ff_core::Error::msg("no working directory: internal inconsistency"))?
        .to_path_buf();
    let mut fetched = false;
    if !ctx.no_fetch
        && let Some(remote) = pre.remote.clone()
    {
        if !ctx.json {
            println!(
                "{}",
                crate::render::paint_dim(&format!("fetching from {remote}"), colored)
            );
        }
        let result = crate::net::fetch(
            &cwd,
            &remote,
            &crate::net::FetchOptions {
                deadline: None,
                interactive: true,
                prune: true,
                tags: true,
            },
        );
        crate::autofetch::stamp(&repo, &remote, result.as_ref().map(|_| ()));
        result?;
        fetched = true;
    }

    let (report, verb_ctx) = ff_core::prune::prune(
        &repo,
        ff_core::prune::PruneOptions { dry_run, fetched },
        &crate::provenance::pre_ff(ctx),
        None,
        std::env::args().collect(),
    )?;
    if let Some(verb_ctx) = &verb_ctx {
        crate::render::reconcile_notice(&verb_ctx.reconcile);
    }

    if ctx.json {
        let undo = if dry_run || report.pruned.is_empty() {
            serde_json::Value::Null
        } else {
            serde_json::Value::from("ff undo")
        };
        let payload = serde_json::json!({
            "prune": report,
            "undo": undo,
        });
        crate::machine::emit("branch prune", &payload)?;
        return Ok(());
    }

    for line in prune_lines(&report.pruned, &report.kept, dry_run, colored) {
        println!("{line}");
    }
    if report.pruned.is_empty() && report.kept.is_empty() {
        println!("{}", crate::render::paint_dim("nothing to prune", colored));
    }
    if !report.pruned.is_empty() {
        let tail = if dry_run {
            "nothing was written — drop --dry-run to prune"
        } else {
            "undo: ff undo"
        };
        println!("{}", crate::render::paint_dim(tail, colored));
    }
    Ok(())
}

/// The lines a prune says, the verb's and pull's alike: what was pruned,
/// with each re-aim inline, then one line per kept branch and its way out.
/// Under a dry run the pruned line reads in the conditional; a kept branch
/// is as kept either way.
pub(crate) fn prune_lines(
    pruned: &[PrunedBranch],
    kept: &[KeptBranch],
    dry_run: bool,
    colored: bool,
) -> Vec<String> {
    let mut out = Vec::new();
    if !pruned.is_empty() {
        let names: Vec<String> = pruned
            .iter()
            .map(|p| {
                let reaims: Vec<String> = p
                    .reaimed
                    .iter()
                    .map(|r| match &r.onto {
                        Some(onto) => format!("{} now sits on {onto}", r.branch),
                        None => format!("{} now sits on nothing", r.branch),
                    })
                    .collect();
                if reaims.is_empty() {
                    p.name.clone()
                } else {
                    format!("{} ({})", p.name, reaims.join(", "))
                }
            })
            .collect();
        let verb = if dry_run { "would prune" } else { "pruned" };
        let noun = if pruned.len() == 1 {
            "branch"
        } else {
            "branches"
        };
        out.push(format!(
            "{verb} {} {noun} whose shared copy is gone: {}",
            pruned.len(),
            names.join(", ")
        ));
    }
    for k in kept {
        let line = match &k.reason {
            Kept::Current => format!("kept {}: the branch you are on", k.name),
            Kept::Elsewhere { path } => format!("kept {}: checked out in {path}", k.name),
            Kept::Held { verb } => format!(
                "kept {}: a held {verb} stands on it — ff switch {}, then ff resolve",
                k.name, k.name
            ),
            Kept::Ahead { count } => format!(
                "kept {}: {count} commit{} its copy never held — ff branch -d {}",
                k.name,
                if *count == 1 { "" } else { "s" },
                k.name
            ),
        };
        out.push(crate::render::paint_warn(&line, colored));
    }
    out
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
    if let Some(sha) = &report.carried {
        println!(
            "carried the open change onto {} ({})",
            report.name,
            crate::render::paint_sha(ff_core::sha::short(sha.as_str()), colored)
        );
    }
    println!("{}", crate::render::paint_dim("undo: ff undo", colored));
    Ok(())
}

fn delete_branch(ctx: &Ctx, target: &str, shared: bool) -> Result<()> {
    let repo = ff_core::discover(".")?;

    // The wire's cwd and the lease are resolved before the local delete.
    // The lease is the tip fufu last showed the copy standing at, checked
    // against the tracking ref now: a copy that moved since, or one fufu
    // never showed, is refused here with the branch still standing, since
    // a refusal after the local delete would leave the branch gone with the
    // refusal unspoken. The copy itself is read from the report once the
    // delete is done, and an upstream under another branch's name is not
    // one — it is the base the branch was cut from, and the report leaves
    // it unnamed.
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
    let lease = if shared {
        ff_core::branch::shared_copy(&repo, target)?
            .map(|copy| ff_core::branch::shared_lease(target, &copy))
            .transpose()?
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
    // undoable, the copy intact — and the tracking ref, the config section,
    // the published note, and the seen record come off only after the wire
    // agreed. The lease was checked before the local delete, so a refusal
    // there left the branch standing; the wire still holds the lease, and
    // catches a move that landed in between.
    let shared_removed = match (&cwd, &report.shared, lease.as_deref()) {
        (Some(cwd), Some(shared), Some(lease)) if !lease.is_empty() => {
            crate::net::push_delete(cwd, &shared.remote, &shared.remote_branch, lease)?;
            ff_core::branch::forget_shared(&repo, target, &shared.r#ref, verb_ctx.now)?;
            true
        }
        _ => false,
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
    if let Some(open) = &report.open_left {
        println!(
            "  its open change ({}) stays pinned by its timeline in trash",
            crate::render::paint_sha(ff_core::sha::short(open.as_str()), colored)
        );
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
    // remote rows wear the same label — floored so a listing of short names
    // does not look cramped.
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
        .map(|info| crate::render::branch_label_width(&info.name))
        .max()
        .unwrap_or(0);
    let label_width = local_width.max(remote_width).max(14);
    let mut gap = false;
    for (section, header) in [(&list.named, ""), (&list.anonymous, "anonymous:")] {
        if section.is_empty() && !(header.is_empty() && list.gone > 0) {
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
        // The way out, on the screen that shows the state: one dim line
        // after the named section when any branch's shared copy is gone.
        if header.is_empty() && list.gone > 0 {
            if gap {
                println!();
            }
            println!(
                "{}",
                crate::render::paint_dim(
                    &format!(
                        "{} whose shared copy is gone — ff branch --prune",
                        list.gone
                    ),
                    colored
                )
            );
            gap = false;
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
