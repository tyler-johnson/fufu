//! `ff start` — always begins a new line of work on a fresh branch. A tree
//! belongs to its branch, and every arrival materializes the destination's
//! own: starting is travel, so the open change parks where it was and the
//! new branch opens clean. Bare forks at trunk's tip; a `<rev>` target
//! forks there instead — a branch name resolved among revisions forks at
//! that branch's tip rather than continuing it. The one target that carries
//! anything is `@`: the fork lands under the open change and the new branch
//! receives a copy of it — the same open commit, id, birth, and description —
//! while the branch left behind keeps its own, parked. No invocation of
//! `start` produces a commit, and every invocation is one operation: the
//! mint, the copy, and the switch go back together under one `ff undo`.

use crate::branch;
use crate::branchmeta;
use crate::error::{Error, Result};
use crate::model::StartReport;
use crate::open;
use crate::ops::record::observe_refs;
use crate::ops::{
    ChangeIdTransition, DescriptionTransition, OpKind, OpRecord, RefTransition, verb,
};
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
pub(crate) struct ForkPoint {
    pub at: gix::ObjectId,
    /// A branch name when the fork point came from one, else a short sha.
    pub forked_from: String,
    /// The branch the user explicitly forked from, when the target named one
    /// — local, or someone else's by way of a tracking ref. `None` for a bare
    /// (trunk) start and for a target that resolved to a bare commit.
    pub parent: Option<String>,
    /// The target resolved to `@`, however it was spelled: the fork lands
    /// under the open change, and the caller carries a copy of it.
    pub open: bool,
}

/// Resolve the fork point, never guessing: the target is a revset that has to
/// name exactly one revision, and the revset resolver is the only thing here
/// that reads it.
///
/// `under` is the commit under the open change, what `@` forks at; `None`
/// on an unborn branch, where `@` has nothing under it and is refused.
///
/// It used to try branch names first and hand anything else to git's own
/// parser, which meant a name that was both a branch and a commit forked at
/// the branch and said nothing about the commit it ignored. That precedence
/// is the bug the revset resolver exists to refuse: it looks a base up in
/// both address spaces unconditionally and names both candidates rather than
/// ranking them. (This file mentions git's parser by description rather than
/// by name on purpose — the guard test in `revset::resolve` greps for it.)
pub(crate) fn resolve_fork_point(
    repo: &gix::Repository,
    target: Option<&str>,
    under: Option<gix::ObjectId>,
) -> Result<ForkPoint> {
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
                open: false,
            })
        }
        Some(raw) => {
            let point = Revset::parse(raw)?.point(repo)?;
            // Decided on the *resolved* revision rather than on the literal
            // "@": `latest(@)` and `heads(@)` were never a different request,
            // and a check on the spelling would have let them through.
            let (at, open) = match (point.rev, under) {
                (Rev::Open(_), Some(id)) => (id, true),
                (Rev::Open(_), None) => {
                    return Err(Error::coded(
                        "target/unresolvable",
                        "@ has no commit under it yet",
                        vec!["ff commit".into()],
                    ));
                }
                (Rev::Commit(id), _) => (id.object_id(), false),
            };
            // A branch name reports the branch; anything else reports the
            // commit it landed on, in the spelling `ff log` prints. The
            // resolver decides which — it already knows whether the whole
            // expression was one branch's tip.
            //
            // `point.name` is exactly a *local* branch name; a tracking ref
            // earns only `full_ref`, and it earns a name here too. Forking
            // from someone else's branch records them as the parent, so the
            // minted branch has a base to be measured against — shortened to
            // `origin/feature`, which is how every report spells it.
            let named = point.name.clone().or_else(|| {
                point
                    .full_ref
                    .as_deref()
                    .and_then(|full| full.strip_prefix("refs/remotes/"))
                    .map(str::to_string)
            });
            let parent = named.clone();
            let forked_from = named.unwrap_or_else(|| crate::sha::short_oid(at));
            Ok(ForkPoint {
                at,
                forked_from,
                parent,
                open,
            })
        }
    }
}

/// `-m`'s text through the normalization `ff describe` applies: trailing
/// whitespace dropped, and an empty message is no message.
fn normalize_message(text: Option<&str>) -> Option<String> {
    text.map(|t| t.trim_end().to_string())
        .filter(|t| !t.is_empty())
}

/// Open a new change on a fresh branch. See the module docs.
///
/// One operation, in `ff commit -b`'s shape: recorded on the branch it
/// creates, with the ref it mints, the HEAD move, and — under `@` — the
/// description and id the copy wears, so the append plans the new branch's
/// open commit from them and one `ff undo` takes the whole of it back.
pub fn start(
    repo: &gix::Repository,
    opts: &StartOptions,
    prov: &Provenance,
) -> Result<(StartReport, verb::VerbContext)> {
    if repo.workdir().is_none() {
        return Err(Error::coded(
            "repo/bare",
            "bare repository: nothing to start from",
            vec![],
        ));
    }
    if let Some(op) = crate::head::operation(repo) {
        return Err(Error::coded(
            "repo/mid-operation",
            format!("a {op:?} is in progress: finish or abort it with git before starting"),
            vec![],
        ));
    }

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
    let head = crate::head::head_state(repo)?;
    let head_commit = crate::snapshot::chain::base_commit(&head)?;
    let fork = resolve_fork_point(repo, opts.target.as_deref(), head_commit)?;

    // The preamble: a dirty tree's park is its open commit, and the capture
    // is what writes one — the append below finds it there when the copy
    // reuses it, and `park_of` refuses when there is none to stand.
    let ctx = verb::begin_verb(repo, prov, opts.now)?;
    let now = ctx.now;
    let current = crate::snapshot::chain::chain_name(&head);
    let parked = crate::switch::park_of(repo, &head, &current, ctx.pre_tree)?;
    let carry = match (fork.open, parked) {
        (true, Some(open)) => Some(open),
        _ => None,
    };

    // What the opened change wears. `-m` wins; a carried copy otherwise keeps
    // the original's description, and keeps its id and birth either way —
    // it is the same change on two branches. A described change has an
    // identity from the start, as `ff describe` rules.
    let current_meta = branchmeta::read(repo, &current)?;
    let description = match (normalize_message(opts.message.as_deref()), carry) {
        (Some(text), _) => Some(text),
        (None, Some(_)) => current_meta.pending_description.clone(),
        (None, None) => None,
    };
    let (change_id, change_born) = match (carry, &description) {
        (Some(_), _) => (current_meta.change_id.clone(), current_meta.change_born),
        (None, Some(_)) => (
            Some(crate::changeid::ChangeId::mint()?.letters()),
            Some(now),
        ),
        (None, None) => (None, None),
    };

    let fork_tree = repo
        .find_commit(fork.at)
        .map_err(Error::repo)?
        .tree_id()
        .map_err(Error::repo)?
        .detach();
    let target_ref = format!("refs/heads/{name}");
    let mut planned = observe_refs(repo)?;
    let head_old = planned.head.clone();
    planned.refs.insert(target_ref.clone(), fork.at.to_string());
    planned.head = format!("ref:{target_ref}");

    let short = crate::sha::short_oid(fork.at);
    let summary = match carry {
        Some(_) => format!("start {name} at {short}, carrying the open change"),
        None => format!("start {name} at {short}"),
    };
    let mut record = OpRecord::new("start", summary, now);
    record.argv = opts.argv.clone();
    record.refs = vec![RefTransition {
        name: target_ref.clone(),
        old: None,
        new: Some(fork.at.to_string()),
    }];
    record.head = Some((head_old, format!("ref:{target_ref}")));
    record.description = description.clone().map(|new| DescriptionTransition {
        branch: name.clone(),
        old: None,
        new: Some(new),
    });
    record.change_id = change_id.clone().map(|new| ChangeIdTransition {
        branch: name.clone(),
        old: None,
        new: Some(new),
        old_born: None,
        new_born: change_born,
    });
    // The end state: the fork's tree, or the worktree as it stands when the
    // copy rides along — the index is the fork's either way.
    let end_tree = match carry {
        Some(_) => ctx.pre_tree,
        None => fork_tree,
    };
    let mut pins = vec![fork.at];
    pins.extend(parked);
    pins.extend(ctx.pre_op.map(|id| id.object_id()));
    clear_stale_open(repo, &name, now)?;
    verb::append_op_hinted(
        repo,
        OpKind::Op,
        verb::VerbOp {
            record,
            planned,
            tree: end_tree,
            index_tree: fork_tree,
            // Recorded on the destination, as a switch is: the pointer that
            // moves is the one the next capture on this worktree will read,
            // and the append writes the new branch's open commit from it.
            branch: name.clone(),
            base: head_commit,
            session: prov.session.clone(),
            pins: &pins,
        },
        carry,
        now,
    )?;

    // Mutate: the branch and its metadata, then HEAD, index, and worktree.
    // No arrival: the branch is fresh, and what it holds is the copy the
    // append already wrote.
    mint_branch(
        repo,
        &Mint {
            name: &name,
            at: fork.at,
            forked_from: &fork.forked_from,
            parent: fork.parent.as_deref(),
            opened: Opened {
                description,
                change_id,
                born: change_born,
            },
        },
        now,
    )?;
    crate::switch::move_worktree(repo, &target_ref, ctx.pre_tree, fork_tree, end_tree, now)?;

    let carried = match carry {
        Some(_) => refs::ref_target(repo, &open::open_ref(&name))?.map(|id| id.to_string()),
        None => None,
    };
    // The park line names where the change *was*, which is the branch
    // underfoot and not the fork source: standing on `alpha` and forking
    // from `main`, what got parked was alpha's.
    let parked_from = parked.is_some().then(|| current.clone());

    Ok((
        StartReport {
            minted: name,
            forked_from: fork.forked_from,
            parked: parked.map(|id| id.to_string()),
            parked_from,
            carried,
        },
        ctx,
    ))
}

/// The change a minted branch opens with: what `-m` said, or the copy of
/// the open change it carries. All `None` for a branch that opens clean.
#[derive(Clone, Default)]
pub(crate) struct Opened {
    pub description: Option<String>,
    pub change_id: Option<String>,
    pub born: Option<i64>,
}

/// What one minted branch is: the name and where it lands, the fork base
/// its metadata records, and the change it opens with.
#[derive(Clone)]
pub(crate) struct Mint<'a> {
    pub name: &'a str,
    pub at: gix::ObjectId,
    pub forked_from: &'a str,
    pub parent: Option<&'a str>,
    pub opened: Opened,
}

/// Mint a branch at a commit, with its fork base written once. The
/// mutation only: the caller has already appended the operation that
/// records it, write-ahead, so the planned table already contains the
/// branch this is about to create.
pub(crate) fn mint_branch(repo: &gix::Repository, mint: &Mint<'_>, now: i64) -> Result<()> {
    let Mint {
        name,
        at,
        forked_from,
        parent,
        opened,
    } = mint;
    branch::create_at(
        repo,
        name,
        *at,
        now,
        &format!("branch: forked from {forked_from}"),
    )?;
    branchmeta::write(
        repo,
        name,
        &branchmeta::BranchMeta {
            pending_description: opened.description.clone(),
            change_id: opened.change_id.clone(),
            change_born: opened.born,
            forked_from: Some((*forked_from).to_string()),
            parent: parent.map(str::to_string),
            session: None,
            held: None,
            resolving: None,
        },
    )?;
    Ok(())
}

/// A stale open ref under a name whose branch is gone — left by a delete
/// that never resynced — would be read as the branch's park by the append;
/// the name is being minted fresh, so it holds nothing yet.
pub(crate) fn clear_stale_open(repo: &gix::Repository, name: &str, now: i64) -> Result<()> {
    if refs::ref_target(repo, &format!("refs/heads/{name}"))?.is_none() {
        open::clear(repo, name, now)?;
    }
    Ok(())
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

    fn oid(sha: &str) -> gix::ObjectId {
        gix::ObjectId::from_hex(sha.as_bytes()).unwrap()
    }

    #[test]
    fn a_branch_name_reports_the_branch_name() {
        let (fx, sha) = one_commit();
        let repo = fx.repo();
        let fork = resolve_fork_point(&repo, Some("main"), Some(oid(&sha))).expect("main resolves");
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
            let fork = resolve_fork_point(&repo, Some(target), Some(oid(&sha))).expect("resolves");
            assert_eq!(fork.at.to_string(), sha);
            assert!(
                sha.starts_with(&fork.forked_from) && fork.forked_from.len() < sha.len(),
                "{target} reported {:?}, not a short sha of {sha}",
                fork.forked_from
            );
        }
    }

    /// Someone else's branch is a fork point with a name, and a parent: a
    /// branch minted from a tracking ref has a base to be measured against,
    /// where a short sha would have left it answering to trunk.
    #[test]
    fn a_tracking_ref_reports_and_records_the_remote_name() {
        let (fx, sha) = one_commit();
        fx.git(&["update-ref", "refs/remotes/origin/feature", &sha]);
        let repo = fx.repo();
        let fork =
            resolve_fork_point(&repo, Some("origin/feature"), Some(oid(&sha))).expect("resolves");
        assert_eq!(fork.at.to_string(), sha);
        assert_eq!(fork.forked_from, "origin/feature");
        assert_eq!(fork.parent.as_deref(), Some("origin/feature"));
    }

    /// The precedence this routing exists to delete: a name that is both a
    /// branch and an object used to fork at the branch and say nothing.
    #[test]
    fn a_name_that_is_both_refuses_and_names_both() {
        let (fx, sha) = one_commit();
        let both = &sha[..8];
        fx.git(&["branch", both]);
        let repo = fx.repo();

        let err = resolve_fork_point(&repo, Some(both), Some(oid(&sha))).expect_err("must refuse");
        assert_eq!(err.id(), "usage/revset-ambiguous");
        let text = err.to_string();
        assert!(
            text.contains(&format!("refs/heads/{both}")),
            "must name the branch: {text}"
        );
        assert!(text.contains(&sha), "must name the object: {text}");
    }

    /// However the open change is spelled: the fork lands under it, and the
    /// point says so. The decision is about the resolved revision, so a
    /// function wrapper does not smuggle it past.
    #[test]
    fn the_open_change_forks_under_itself_and_says_so() {
        let (fx, sha) = one_commit();
        let repo = fx.repo();
        for target in ["@", "latest(@)", "heads(@)"] {
            let fork = resolve_fork_point(&repo, Some(target), Some(oid(&sha))).expect("resolves");
            assert_eq!(fork.at.to_string(), sha, "{target}");
            assert!(fork.open, "{target} is the open change");
            assert_eq!(fork.parent, None, "{target}");
        }
        let fork = resolve_fork_point(&repo, Some("main"), Some(oid(&sha))).expect("resolves");
        assert!(!fork.open, "a branch name is not the open change");
    }

    /// An unborn branch has nothing under its open change to fork at.
    #[test]
    fn the_open_change_on_an_unborn_branch_is_refused() {
        let (fx, _) = one_commit();
        let repo = fx.repo();
        let err = resolve_fork_point(&repo, Some("@"), None).expect_err("must refuse");
        assert_eq!(err.id(), "target/unresolvable");
        assert_eq!(err.to_string(), "@ has no commit under it yet");
    }

    /// A target that denotes nothing is the revset's refusal now, which names
    /// the exits; `target/unresolvable` used to swallow it and name none.
    #[test]
    fn an_unresolvable_target_is_the_revsets_refusal() {
        let (fx, sha) = one_commit();
        let repo = fx.repo();
        let err = resolve_fork_point(&repo, Some("nosuchthing"), Some(oid(&sha)))
            .expect_err("must refuse");
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
        let err = resolve_fork_point(&repo, Some("::main"), None).expect_err("must refuse");
        assert_eq!(err.id(), "usage/revset-not-a-point");
    }
}
