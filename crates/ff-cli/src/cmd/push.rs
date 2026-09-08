//! `ff push` — send a branch to its remote, under a lease.
//!
//! The whole of the outgoing half. It does not fetch, does not replay, and
//! does not touch a ref that is not the shared copy of a branch in the run:
//! `ff pull` is what takes anything in. What this command owns is the
//! network call the core deliberately will not make, and the one honest
//! line afterwards — the push left the machine, and `ff undo` cannot reach
//! across the wire to take it back.
//!
//! Which branches the run sends is decided first, before the network is
//! paid for: the branch underfoot, or the ones named. A name that resolves
//! to nothing is refused here, ahead of the wire, and so is every refusal
//! preflight has for any branch in the run. Then one capture, one plan per
//! branch, and one send per branch, each under its own lease, since each
//! has its own shared copy. A refusal on one is that branch's: the rest of
//! the run still goes out, the report says which, and the exit is 1. A run
//! of one branch is the verb as it always was, and its refusal is the
//! run's error.
//!
//! `--dry-run` is the answer to the honest line: the only way to see which
//! push this would be *before* it becomes unrecallable. It writes nothing
//! and sends nothing, so every sentence below switches to the conditional.
//!
//! What `ff undo` cannot reach, `ff push` can: undo the commit and
//! push again, and the lease rolls the shared copy back to where the
//! branch now stands. That is not erasure — other clones may hold the
//! commits, CI ran, a webhook fired — which is why the tail line still says
//! the push left the machine. But it is a way back, and the log recording
//! the push is what lets fufu stop pointing the other way.

use ff_core::{Push, PushReport, PushShape, Result};

use crate::ctx::Ctx;

/// One branch in the run: what push planned for it, and what the wire
/// said. The plan is the core's; `pushed` and `error` are this command's,
/// because the core never reaches the network.
struct Row {
    branch: String,
    push: Push,
    pushed: bool,
    error: Option<ff_core::Error>,
}

pub fn run(ctx: &Ctx, branches: Vec<String>, dry_run: bool, to: Option<&str>) -> Result<()> {
    let repo = ff_core::discover(".")?;
    crate::render::init_palette(&repo);
    let colored = crate::pager::color_enabled();

    // Every refusal before any network: the guards on HEAD, the names, and
    // then each branch's own preflight, `--to` included.
    let head = ff_core::preflight::head_branch(&repo, ff_core::preflight::Verb::Push)?;
    let scope = if branches.is_empty() {
        ff_core::push::Scope::Current
    } else {
        ff_core::push::Scope::Named(branches)
    };
    let chosen = ff_core::push::choose(&repo, &head, &scope)?;
    let pres = chosen
        .branches
        .iter()
        .map(|branch| {
            ff_core::preflight::preflight_branch(&repo, ff_core::preflight::Verb::Push, branch, to)
        })
        .collect::<Result<Vec<_>>>()?;
    let cwd = repo
        .workdir()
        // Uncoded on purpose: preflight already refused a bare repository, so
        // nobody can reach this and there is nothing to tell them.
        .ok_or_else(|| ff_core::Error::msg("no working directory: internal inconsistency"))?
        .to_path_buf();

    let prov = crate::provenance::pre_ff(ctx);
    let verb_ctx = ff_core::push::begin(&repo, dry_run, None, &prov)?;
    if let Some(verb_ctx) = &verb_ctx {
        crate::render::reconcile_notice(&verb_ctx.reconcile);
    }
    let mut rows = pres
        .iter()
        .map(|pre| {
            Ok(Row {
                branch: pre.branch.clone(),
                push: ff_core::push::plan(&repo, pre)?,
                pushed: false,
                error: None,
            })
        })
        .collect::<Result<Vec<_>>>()?;

    // The push, before any report line about it: the report says what
    // happened and not what was planned. A dry run is the one case where
    // those differ, and it says "would" throughout rather than pretending.
    let alone = rows.len() == 1;
    for (row, pre) in rows.iter_mut().zip(&pres) {
        if dry_run || !matches!(row.push, Push::Create { .. } | Push::Push { .. }) {
            continue;
        }
        // Named before the wire agrees: a push that fails to an
        // unreachable URL still leaves `ff pull` working. Only when there
        // was no upstream at all — a branch that already tracks this
        // remote is set correctly, and rewriting it could clobber a
        // legitimately multi-valued `merge`.
        if let Some(remote) = to.filter(|_| pre.tracking.is_none()) {
            ff_core::snapshot::config::set_branch_upstream(&repo, &row.branch, remote)?;
        }
        match crate::net::push(&cwd, &row.branch, &row.push) {
            Ok(()) => row.pushed = true,
            // One branch is the whole run, and the wire's refusal is the
            // run's error, as it always was. Among several it is that
            // branch's alone: the lease it failed was its own, and the next
            // branch's lease says nothing about it.
            Err(err) if alone => return Err(err),
            Err(err) => {
                row.error = Some(err);
                continue;
            }
        }
        // Recorded after the wire agreed, not before: push moves no local
        // ref, so there is nothing a write-ahead claim could be diffed
        // against and an append-before would be a claim nothing could
        // falsify.
        if let Some(verb_ctx) = &verb_ctx {
            let report = PushReport {
                branch: row.branch.clone(),
                push: row.push.clone(),
                dry_run,
            };
            ff_core::push::record(&repo, pre, &report, verb_ctx, &prov).map_err(|err| {
                ff_core::Error::coded(
                    "push/unrecorded",
                    format!("the push landed and the operation log could not record it: {err}"),
                    vec!["ff op log".into(), "ff status".into()],
                )
            })?;
        }
    }

    // The branch underfoot leads the report, as it always has, and reads
    // `NotNamed` when names left it out of the run.
    let (push, pushed) = rows
        .iter()
        .find(|row| row.branch == head)
        .map(|row| (row.push.clone(), row.pushed))
        .unwrap_or((Push::NotNamed, false));
    let report = PushReport {
        branch: head.clone(),
        push,
        dry_run,
    };
    let refused = rows.iter().any(|row| row.error.is_some());
    let blocked = rows.iter().any(|row| matches!(row.push, Push::Blocked));

    if ctx.json {
        let branches: Vec<serde_json::Value> = rows
            .iter()
            .map(|row| {
                serde_json::json!({
                    "branch": row.branch,
                    "push": row.push,
                    "pushed": row.pushed,
                    "error": row.error.as_ref().map(crate::machine::error_object),
                })
            })
            .collect();
        let payload = serde_json::json!({
            "push": report,
            "pushed": pushed,
            "branches": branches,
        });
        crate::machine::emit("push", &payload)?;
        exit(refused, blocked);
        return Ok(());
    }

    // The branch underfoot leads and reads as it always has; every other
    // branch in the run is a block after it, its name on a line of its own
    // and what happened to it indented, the way `ff pull` files the
    // branches it reached.
    let sent = rows.iter().any(|row| {
        matches!(row.push, Push::Create { .. } | Push::Push { .. }) && row.error.is_none()
    });
    let underfoot = rows.iter().filter(|row| row.branch == head);
    let others = rows.iter().filter(|row| row.branch != head);
    for row in underfoot.chain(others) {
        let lines = row_lines(row, dry_run, colored);
        if row.branch == head {
            for line in &lines {
                println!("{line}");
            }
            continue;
        }
        println!("{}", row.branch);
        for line in &lines {
            for l in line.lines() {
                println!("    {l}");
            }
        }
    }
    if sent {
        tail(dry_run, colored);
    }
    exit(refused, blocked);
    Ok(())
}

/// The code a run that reported still owes the shell: 1 when the wire
/// refused any branch, else 3 when a hold blocked any, else 0.
fn exit(refused: bool, blocked: bool) {
    if refused {
        crate::exit::refused();
    } else if blocked {
        crate::exit::held();
    }
}

/// What one branch's block says: the push it was, or why nothing went, or
/// what the wire said when it refused.
fn row_lines(row: &Row, dry_run: bool, colored: bool) -> Vec<String> {
    let branch = &row.branch;
    let would = if dry_run { "would " } else { "" };
    if let Some(err) = &row.error {
        // The failure the run of this branch alone would have exited with,
        // filed under the branch instead: the message, and the way out.
        let mut out = vec![crate::render::paint_warn(
            &format!("not pushed: {err}"),
            colored,
        )];
        let exits = crate::explain::exits_for(err);
        if !exits.is_empty() {
            out.push("try:".to_string());
            out.extend(exits.into_iter().map(|hint| format!("  {hint}")));
        }
        return out;
    }
    match &row.push {
        Push::NotNamed => vec![],
        Push::NoRemote => vec![crate::render::paint_dim(
            "nowhere to push: this repository has no remote",
            colored,
        )],
        Push::Blocked => vec![crate::render::paint_warn(
            &format!(
                "nothing sent: a rewrite is held on {branch} — the exit stays blocked until it lands"
            ),
            colored,
        )],
        Push::UpToDate => vec![crate::render::paint_dim("nothing to push", colored)],
        Push::Create {
            remote,
            remote_branch,
            ..
        } => vec![crate::render::paint_ok(
            &format!(
                "{would}create{} {remote}/{remote_branch} and set {branch} to track it",
                if dry_run { "" } else { "d" },
            ),
            colored,
        )],
        Push::Push {
            remote,
            remote_branch,
            shape,
            ..
        } => {
            // The lease is empty for two of these and only the sentence
            // differs. It used to be the whole test, which is why a clone of
            // an empty remote was told its shared copy was gone.
            let line = match shape {
                PushShape::Recreate => format!(
                    "{would}re-create{} {remote}/{remote_branch}, which is gone",
                    if dry_run { "" } else { "d" }
                ),
                PushShape::First => format!(
                    "{would}create{} {remote}/{remote_branch}",
                    if dry_run { "" } else { "d" }
                ),
                // Not a send at all: the tip is an ancestor of what the
                // remote holds, so this takes commits off the shared copy.
                PushShape::Retract => format!(
                    "{would}roll{} {remote}/{remote_branch} back to {branch}",
                    if dry_run { "" } else { "ed" },
                ),
                PushShape::Replace => format!(
                    "{would}push{} {branch} to {remote}/{remote_branch}",
                    if dry_run { "" } else { "ed" },
                ),
            };
            vec![crate::render::paint_ok(&line, colored)]
        }
    }
}

/// The tail under a push. A dry run has not spent the irreversible act yet,
/// and saying it did would be the one lie this verb cannot afford — so it
/// gets the conditional line and nothing else. A real push gets both: what
/// left the machine, and the way back that is not a way to erase it. Once
/// per run, however many branches went.
fn tail(dry_run: bool, colored: bool) {
    if dry_run {
        println!(
            "{}",
            crate::render::paint_dim("nothing was sent — drop --dry-run to send it", colored)
        );
        return;
    }
    println!(
        "{}",
        crate::render::paint_dim(
            "the push left the machine — ff undo cannot reach it",
            colored
        )
    );
    println!(
        "{}",
        crate::render::paint_dim(
            "ff undo then ff push rolls the shared copy back, under a lease",
            colored
        )
    );
}
