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
use crate::model::StartReport;
use crate::ops::record::observe_refs;
use crate::ops::{OpKind, OpRecord, RefTransition, verb};
use crate::refs;
use crate::revset::{Rev, Revset};
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
#[derive(Debug)]
struct ForkPoint {
    at: gix::ObjectId,
    /// A branch name when the fork point came from one, else a short sha.
    forked_from: String,
    /// The local branch the user explicitly forked from, when the target
    /// named one. `None` for a bare (trunk) start and for a target that
    /// resolved to a bare commit.
    parent: Option<String>,
}

/// Resolve the fork point, never guessing: the target is a revset that has to
/// name exactly one revision, and the revset resolver is the only thing here
/// that reads it.
///
/// It used to try branch names first and hand anything else to git's own
/// parser, which meant a name that was both a branch and a commit forked at
/// the branch and said nothing about the commit it ignored. That precedence
/// is the bug the revset resolver exists to refuse: it looks a base up in
/// both address spaces unconditionally and names both candidates rather than
/// ranking them. (This file mentions git's parser by description rather than
/// by name on purpose — the guard test in `revset::resolve` greps for it.)
fn resolve_fork_point(repo: &gix::Repository, target: Option<&str>) -> Result<ForkPoint> {
    match target {
        None => {
            let t = crate::trunk::trunk(repo)?;
            let at = refs::ref_target(repo, &t.full_ref)?.ok_or_else(|| {
                Error::coded(
                    "target/unresolvable",
                    format!("trunk ref {} has no target", t.full_ref),
                    vec!["ff branch".into()],
                )
            })?;
            Ok(ForkPoint {
                at,
                forked_from: t.name,
                parent: None,
            })
        }
        Some(raw) => {
            let point = Revset::parse(raw)?.point(repo)?;
            // Refused on the *resolved* revision rather than on the literal
            // "@": `latest(@)` and `heads(@)` were never a different request,
            // and a check on the spelling would have let them through.
            let at = match point.rev {
                Rev::Open => return Err(open_is_not_a_start_target()),
                Rev::Commit(id) => id.object_id(),
            };
            // A branch name reports the branch; anything else reports the
            // commit it landed on, in the spelling `ff log` prints. The
            // resolver decides which — it already knows whether the whole
            // expression was one branch's tip.
            // `point.name` is already exactly a local branch name (None for
            // a sha, tag, or compound expression); clone it before the match
            // below consumes it.
            let parent = point.name.clone();
            let forked_from = match point.name {
                Some(name) => name,
                None => crate::sha::short_oid(at),
            };
            Ok(ForkPoint {
                at,
                forked_from,
                parent,
            })
        }
    }
}

fn open_is_not_a_start_target() -> Error {
    Error::coded(
        "target/unresolvable",
        "@ is not a start target — ff start always opens a clean branch; \
         to move the open change onto its own branch, use ff commit -b <name>",
        vec!["ff commit -b <name>".into()],
    )
}

/// Open a new change on a fresh branch. See the module docs.
pub fn start(
    repo: &gix::Repository,
    opts: &StartOptions,
    prov: &Provenance,
) -> Result<(StartReport, verb::VerbContext)> {
    let fork = resolve_fork_point(repo, opts.target.as_deref())?;

    let name = match &opts.branch {
        Some(name) => {
            branch::validate_name(name)?;
            if refs::ref_target(repo, &format!("refs/heads/{name}"))?.is_some() {
                return Err(Error::coded(
                    "branch/exists",
                    format!("a branch named {name} already exists"),
                    vec!["ff branch".into()],
                ));
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
        fork.parent.as_deref(),
        resolve_now(opts.now),
        &opts.argv,
        prov,
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

/// Mint a branch at a commit, recorded, with its fork base written once.
///
/// This runs BEFORE the switch that follows it, and therefore before any
/// preamble — so it reconciles nothing and captures nothing itself. That is
/// safe precisely because it is write-ahead: the planned table it records
/// already contains the branch it is about to create, so the switch's own
/// reconcile finds the world exactly where this operation said it would be.
#[allow(clippy::too_many_arguments)]
fn mint_branch(
    repo: &gix::Repository,
    name: &str,
    at: gix::ObjectId,
    forked_from: &str,
    parent: Option<&str>,
    now: i64,
    argv: &[String],
    prov: &Provenance,
) -> Result<()> {
    let head = crate::head::head_state(repo)?;
    let mut planned = observe_refs(repo)?;
    planned
        .refs
        .insert(format!("refs/heads/{name}"), at.to_string());
    let mut record = OpRecord::new(
        "start",
        format!("mint branch {name} at {}", &at.to_string()[..8]),
        now,
    );
    record.argv = argv.to_vec();
    record.refs = vec![RefTransition {
        name: format!("refs/heads/{name}"),
        old: None,
        new: Some(at.to_string()),
    }];
    let tree = crate::ops::verb::worktree_or_head(repo)?;
    verb::append_op(
        repo,
        OpKind::Op,
        verb::VerbOp {
            record,
            planned,
            // Minting a name touches neither the working tree nor the index;
            // the switch that follows is what moves them.
            tree,
            index_tree: crate::index::tree_from_index(repo)?,
            // Recorded against the branch it runs ON, not the one it creates:
            // the new branch has no pointer yet, and the switch is what will
            // open one.
            branch: crate::snapshot::chain::chain_name(&head),
            base: crate::snapshot::chain::base_commit(&head)?,
            session: prov.session.clone(),
            pins: &[at],
        },
        now,
    )?;

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
            parent: parent.map(str::to_string),
            session: None,
            held: None,
            resolving: None,
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

#[cfg(test)]
mod tests {
    use ff_testsupport::Fixture;

    use super::*;

    fn one_commit() -> (Fixture, String) {
        let fx = Fixture::new();
        fx.write("a.txt", "a\n");
        let sha = fx.commit("init");
        (fx, sha)
    }

    #[test]
    fn a_branch_name_reports_the_branch_name() {
        let (fx, sha) = one_commit();
        let repo = fx.repo();
        let fork = resolve_fork_point(&repo, Some("main")).expect("main resolves");
        assert_eq!(fork.forked_from, "main");
        assert_eq!(fork.at.to_string(), sha);
    }

    /// Anything that is not one branch's tip reports the commit it landed on,
    /// in the spelling `ff log` prints — which is what it reported before the
    /// revset, and what `branchmeta` has been storing all along.
    #[test]
    fn anything_else_reports_a_short_sha() {
        let (fx, sha) = one_commit();
        let repo = fx.repo();
        for target in [sha.as_str(), "main^{commit}", "HEAD"] {
            let fork = resolve_fork_point(&repo, Some(target)).expect("resolves");
            assert_eq!(fork.at.to_string(), sha);
            assert!(
                sha.starts_with(&fork.forked_from) && fork.forked_from.len() < sha.len(),
                "{target} reported {:?}, not a short sha of {sha}",
                fork.forked_from
            );
        }
    }

    /// The precedence this routing exists to delete: a name that is both a
    /// branch and an object used to fork at the branch and say nothing.
    #[test]
    fn a_name_that_is_both_refuses_and_names_both() {
        let (fx, sha) = one_commit();
        let both = &sha[..8];
        fx.git(&["branch", both]);
        let repo = fx.repo();

        let err = resolve_fork_point(&repo, Some(both)).expect_err("must refuse");
        assert_eq!(err.id(), "usage/revset-ambiguous");
        let text = err.to_string();
        assert!(
            text.contains(&format!("refs/heads/{both}")),
            "must name the branch: {text}"
        );
        assert!(text.contains(&sha), "must name the object: {text}");
    }

    /// However the open change is spelled. The refusal is about the resolved
    /// revision, so a function wrapper does not smuggle it past.
    #[test]
    fn the_open_change_is_never_a_start_target() {
        let (fx, _) = one_commit();
        let repo = fx.repo();
        for target in ["@", "latest(@)", "heads(@)"] {
            let err = resolve_fork_point(&repo, Some(target)).expect_err("must refuse");
            assert_eq!(err.id(), "target/unresolvable", "{target}");
            assert!(
                err.to_string().contains("ff commit -b"),
                "{target} must teach the exit"
            );
        }
    }

    /// A target that denotes nothing is the revset's refusal now, which names
    /// the exits; `target/unresolvable` used to swallow it and name none.
    #[test]
    fn an_unresolvable_target_is_the_revsets_refusal() {
        let (fx, _) = one_commit();
        let repo = fx.repo();
        let err = resolve_fork_point(&repo, Some("nosuchthing")).expect_err("must refuse");
        assert_eq!(err.id(), "usage/revset-unknown-revision");
    }

    /// A set with more than one member is not a fork point, and picking one
    /// would be the same guess the branch-first ladder used to make.
    #[test]
    fn a_target_naming_many_revisions_is_refused() {
        let fx = Fixture::new();
        fx.write("a.txt", "a\n");
        fx.commit("one");
        fx.write("a.txt", "b\n");
        fx.commit("two");
        let repo = fx.repo();
        let err = resolve_fork_point(&repo, Some("::main")).expect_err("must refuse");
        assert_eq!(err.id(), "usage/revset-not-a-point");
    }
}
