//! `ff new` — pure composition: `[ff switch <target> +] ff commit`. Bare
//! `ff new` closes the open change; with a target it parks what's here,
//! arrives there, and closes whatever is then open — a fresh slate either
//! way. Target resolution never guesses: a branch name (or the tip of
//! exactly one branch) continues that branch; a tip shared by several
//! errors, listing them; anything else — a commit with children, mid-stack,
//! a raw sha — mints an anonymous branch.

use crate::branch;
use crate::branchmeta;
use crate::error::{Error, Result};
use crate::journal::{self, OpKind, OpRecord, RefTransition};
use crate::model::{CommitOutcome, NewReport};
use crate::refs;
use crate::snapshot::Provenance;

#[derive(Debug, Clone, Default)]
pub struct NewOptions {
    /// Where to open the next change; `None` = here.
    pub target: Option<String>,
    /// Pending description for the OPENED change (consumed at its close).
    pub message: Option<String>,
    /// Name for the minted branch (with a rev target), the claim (on a
    /// placeholder), or the fork at the resulting tip (bare).
    pub branch: Option<String>,
    /// Clock injection for tests.
    pub now: Option<i64>,
    pub argv: Vec<String>,
}

/// Where a target resolved to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NewTarget {
    /// An existing branch (by name, or the tip of exactly this one).
    Existing(String),
    /// Mint a branch at this commit.
    Mint {
        at: gix::ObjectId,
        name: String,
        forked_from: String,
    },
}

/// Resolve a `ff new` target, never guessing.
pub fn resolve_target(
    repo: &gix::Repository,
    raw: &str,
    dash_b: Option<&str>,
) -> Result<NewTarget> {
    let names = crate::switch::branch_names(repo)?;
    if names.iter().any(|n| n == raw) {
        if dash_b.is_some() {
            return Err(Error::msg(format!(
                "-b names a new branch, but {raw} already names one"
            )));
        }
        return Ok(NewTarget::Existing(raw.to_string()));
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

    // Branch tips carrying exactly this commit?
    let mut at_tip = Vec::new();
    for name in &names {
        if refs::ref_target(repo, &format!("refs/heads/{name}"))? == Some(commit) {
            at_tip.push(name.clone());
        }
    }
    match at_tip.as_slice() {
        [one] => {
            if dash_b.is_some() {
                return Err(Error::msg(format!(
                    "-b names a new branch, but {raw} is the tip of {one}"
                )));
            }
            Ok(NewTarget::Existing(one.clone()))
        }
        [] => {
            let name = match dash_b {
                Some(name) => {
                    branch::validate_name(name)?;
                    if refs::ref_target(repo, &format!("refs/heads/{name}"))?.is_some() {
                        return Err(Error::msg(format!("a branch named {name} already exists")));
                    }
                    name.to_string()
                }
                None => crate::petname::mint(repo)?,
            };
            Ok(NewTarget::Mint {
                at: commit,
                name,
                forked_from: commit.to_string(),
            })
        }
        many => {
            let list: Vec<&str> = many.iter().map(|n| n.as_str()).collect();
            Err(Error::msg(format!(
                "{raw} is the tip of several branches ({}); name the one to continue",
                list.join(", ")
            )))
        }
    }
}

/// Open a new change. See the module docs for the composition.
pub fn new(
    repo: &gix::Repository,
    opts: &NewOptions,
    prov: &Provenance,
) -> Result<(NewReport, journal::VerbContext)> {
    match &opts.target {
        None => new_here(repo, opts, prov),
        Some(raw) => new_at(repo, raw, opts, prov),
    }
}

/// Bare `ff new`: close. With `-b`: the close advances the current branch
/// (or claims a placeholder), then a fresh branch forks at the resulting
/// tip and the slate opens there.
fn new_here(
    repo: &gix::Repository,
    opts: &NewOptions,
    prov: &Provenance,
) -> Result<(NewReport, journal::VerbContext)> {
    let head = crate::head::head_state(repo)?;
    let current = crate::snapshot::chain::chain_name(&head);
    let claim = match &opts.branch {
        Some(name) if branch::is_anonymous(&current) => {
            // Placeholder: claim first, close lands on the new name.
            let (_report, _ctx) =
                branch::claim_current(repo, name, prov, opts.now, opts.argv.clone())?;
            true
        }
        _ => false,
    };

    let (commit, ctx) = crate::close::close(
        repo,
        &crate::close::CloseOptions {
            message: None,
            no_verify: false,
            branch: None,
            now: opts.now,
            argv: opts.argv.clone(),
        },
        prov,
    )?;

    let mut opened = match &commit {
        CommitOutcome::Closed { branch, .. } => branch.clone(),
        CommitOutcome::NothingToClose { branch } => branch.clone(),
    };
    let mut minted = None;

    if let Some(name) = &opts.branch
        && !claim
    {
        // Fork at the resulting tip and move there.
        let tip = refs::ref_target(repo, &format!("refs/heads/{opened}"))?
            .ok_or_else(|| Error::msg("no tip to fork from (unborn branch)"))?;
        mint_branch(repo, name, tip, &opened, ctx.now, &opts.argv)?;
        branch::retarget_head(repo, &format!("refs/heads/{name}"), ctx.now)?;
        minted = Some(name.clone());
        opened = name.clone();
    }

    finish_with_description(repo, &opened, opts, prov)?;
    Ok((
        NewReport {
            switch: None,
            commit,
            opened,
            minted,
        },
        ctx,
    ))
}

/// `ff new <target>`: park here via the switch, arrive there, close what
/// is then open.
fn new_at(
    repo: &gix::Repository,
    raw: &str,
    opts: &NewOptions,
    prov: &Provenance,
) -> Result<(NewReport, journal::VerbContext)> {
    let target = resolve_target(repo, raw, opts.branch.as_deref())?;
    let (branch_name, minted) = match target {
        NewTarget::Existing(name) => (name, None),
        NewTarget::Mint {
            at,
            name,
            forked_from,
        } => {
            mint_branch(
                repo,
                &name,
                at,
                &forked_from,
                resolve_now(opts.now),
                &opts.argv,
            )?;
            (name.clone(), Some(name))
        }
    };

    let (switch_report, _switch_ctx) = crate::switch::switch(
        repo,
        &crate::switch::SwitchOptions {
            target: branch_name.clone(),
            now: opts.now,
            argv: opts.argv.clone(),
        },
        prov,
    )?;

    let (commit, ctx) = crate::close::close(
        repo,
        &crate::close::CloseOptions {
            message: None,
            no_verify: false,
            branch: None,
            now: opts.now,
            argv: opts.argv.clone(),
        },
        prov,
    )?;

    finish_with_description(repo, &branch_name, opts, prov)?;
    Ok((
        NewReport {
            switch: Some(switch_report),
            commit,
            opened: branch_name,
            minted,
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
        "new",
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

/// `-m` on new: the pending description of the opened change.
fn finish_with_description(
    repo: &gix::Repository,
    opened: &str,
    opts: &NewOptions,
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
