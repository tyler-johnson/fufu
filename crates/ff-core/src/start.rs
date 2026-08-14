//! `ff start` — always begins a new line of work on a fresh branch. A tree
//! belongs to its branch, and every arrival materializes the destination's
//! own: starting is travel, so the open change parks where it was and the
//! new branch opens clean. Bare forks at trunk's tip; a `<rev>` target
//! forks there instead — a branch name resolved among revisions forks at
//! that branch's tip rather than continuing it. No invocation of `start`
//! produces a commit.

use crate::branch;
use crate::branchmeta;
use crate::error::{Error, Result};
use crate::journal::{self, OpKind, OpRecord, RefTransition};
use crate::model::StartReport;
use crate::refs;
use crate::snapshot::Provenance;

#[derive(Debug, Clone, Default)]
pub struct StartOptions {
    /// None = bare (trunk). Some(rev) = fork there.
    pub target: Option<String>,
    /// -m: pending description for the change being OPENED.
    pub message: Option<String>,
    /// -b: name for the minted branch; None mints an anonymous one.
    pub branch: Option<String>,
    /// Clock injection for tests.
    pub now: Option<i64>,
    pub argv: Vec<String>,
}

/// Where the new branch forks from.
struct ForkPoint {
    at: gix::ObjectId,
    /// A branch name when the fork point came from one, else a short sha.
    forked_from: String,
}

/// Resolve the fork point, never guessing: a branch name forks at its tip;
/// anything else is a revision. `@` is rejected before either is tried.
fn resolve_fork_point(repo: &gix::Repository, target: Option<&str>) -> Result<ForkPoint> {
    match target {
        None => {
            let t = crate::trunk::trunk(repo)?;
            let at = refs::ref_target(repo, &t.full_ref)?
                .ok_or_else(|| Error::msg(format!("trunk ref {} has no target", t.full_ref)))?;
            Ok(ForkPoint {
                at,
                forked_from: t.name,
            })
        }
        Some("@") => Err(Error::msg(
            "ff: @ is not a start target — ff start always opens a clean branch; to move the open change onto its own branch, use ff commit -b <name>",
        )),
        Some(raw) => {
            let names = crate::switch::branch_names(repo)?;
            if names.iter().any(|n| n == raw) {
                let at = refs::ref_target(repo, &format!("refs/heads/{raw}"))?
                    .ok_or_else(|| Error::msg(format!("no branch named {raw}")))?;
                return Ok(ForkPoint {
                    at,
                    forked_from: raw.to_string(),
                });
            }
            // Not a branch name: try a revision.
            let commit = repo
                .rev_parse_single(raw)
                .map_err(|_| Error::msg(format!("{raw} is neither a branch nor a revision")))?
                .object()
                .map_err(Error::repo)?
                .peel_to_kind(gix::objs::Kind::Commit)
                .map_err(|_| Error::msg(format!("{raw} does not point at a commit")))?
                .id;
            let forked_from = {
                use gix::prelude::ObjectIdExt;
                commit
                    .attach(repo)
                    .shorten()
                    .map_err(Error::repo)?
                    .to_string()
            };
            Ok(ForkPoint {
                at: commit,
                forked_from,
            })
        }
    }
}

/// Open a new change on a fresh branch. See the module docs.
pub fn start(
    repo: &gix::Repository,
    opts: &StartOptions,
    prov: &Provenance,
) -> Result<(StartReport, journal::VerbContext)> {
    let fork = resolve_fork_point(repo, opts.target.as_deref())?;

    let name = match &opts.branch {
        Some(name) => {
            branch::validate_name(name)?;
            if refs::ref_target(repo, &format!("refs/heads/{name}"))?.is_some() {
                return Err(Error::msg(format!("a branch named {name} already exists")));
            }
            name.clone()
        }
        None => crate::petname::mint(repo)?,
    };

    mint_branch(
        repo,
        &name,
        fork.at,
        &fork.forked_from,
        resolve_now(opts.now),
        &opts.argv,
    )?;

    // Park the open change on the branch it was open on, and materialize
    // the new branch's own (empty) tree.
    let (switch_report, ctx) = crate::switch::switch(
        repo,
        &crate::switch::SwitchOptions {
            target: name.clone(),
            now: opts.now,
            argv: opts.argv.clone(),
        },
        prov,
    )?;

    finish_with_description(repo, &name, opts, prov)?;

    Ok((
        StartReport {
            minted: name,
            forked_from: fork.forked_from,
            parked: switch_report.parked,
        },
        ctx,
    ))
}

/// Mint a branch at a commit, journaled, with its fork base recorded once.
fn mint_branch(
    repo: &gix::Repository,
    name: &str,
    at: gix::ObjectId,
    forked_from: &str,
    now: i64,
    argv: &[String],
) -> Result<()> {
    let mut planned = journal::observe_refs(repo)?;
    planned
        .refs
        .insert(format!("refs/heads/{name}"), at.to_string());
    let mut record = OpRecord::new(
        OpKind::Op,
        "start",
        format!("mint branch {name} at {}", &at.to_string()[..8]),
        now,
    );
    record.argv = argv.to_vec();
    record.branch = Some(name.to_string());
    record.refs = vec![RefTransition {
        name: format!("refs/heads/{name}"),
        old: None,
        new: Some(at.to_string()),
    }];
    let index_tree = crate::index::tree_from_index(repo)?;
    record.index_tree = Some(index_tree.to_string());
    journal::append(repo, &record, &planned, index_tree, &[at], now)?;

    branch::create_at(
        repo,
        name,
        at,
        now,
        &format!("branch: forked from {forked_from}"),
    )?;
    branchmeta::write(
        repo,
        name,
        &branchmeta::BranchMeta {
            pending_description: None,
            forked_from: Some(forked_from.to_string()),
        },
    )?;
    Ok(())
}

/// `-m` on start: the pending description of the opened change.
fn finish_with_description(
    repo: &gix::Repository,
    opened: &str,
    opts: &StartOptions,
    prov: &Provenance,
) -> Result<()> {
    if let Some(text) = &opts.message {
        let head = crate::head::head_state(repo)?;
        let current = crate::snapshot::chain::chain_name(&head);
        debug_assert_eq!(&current, opened);
        crate::describe::set_pending(repo, Some(text.clone()), prov, opts.now, opts.argv.to_vec())?;
    }
    Ok(())
}

fn resolve_now(now: Option<i64>) -> i64 {
    now.unwrap_or_else(|| {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0)
    })
}
