//! `ff restack` replays a branch's commits onto a different base — the one
//! recorded for it, or the one `--onto` names — and carries the open change
//! onto the new tip. It is the primitive `ff sync` and `ff done` will aim at,
//! and it is offline: one branch moves, never a cascade of branches.
//!
//! The one rule that organizes this file: the worktree moves if and only if
//! the branch HEAD stands on is carried by the rewrite — always so for the
//! branch you are on, never so for one you are not, and the difference
//! between writing files and touching none at all.

use gix::prelude::ObjectIdExt;

use crate::branchmeta;
use crate::error::{Error, Result};
use crate::futures::{self, At, UnknownReason, Verdict};
use crate::model::{HeadState, Parked, RestackOutcome, RestackReport};
use crate::ops::record::{ParentTransition, RefTransition, observe_refs};
use crate::ops::{OpKind, OpRecord, verb};
use crate::refs;
use crate::rewrite;
use crate::snapshot::Provenance;
use crate::snapshot::tree as snaptree;
use crate::switch;

/// A 7-hex-character-ish abbreviation, git's own minimal-unique-prefix
/// shortening with a fixed fallback.
fn short(repo: &gix::Repository, id: gix::ObjectId) -> String {
    id.attach(repo)
        .shorten()
        .map(|p| p.to_string())
        .unwrap_or_else(|_| id.to_string()[..7].to_string())
}

/// The tree of a commit, resolved through whichever repository handle is
/// given.
fn tree_of(repo: &gix::Repository, commit: gix::ObjectId) -> Result<gix::ObjectId> {
    Ok(repo
        .find_object(commit)
        .map_err(Error::repo)?
        .into_commit()
        .tree_id()
        .map_err(Error::repo)?
        .detach())
}

/// The subject of a commit, through the object handle — the raw `CommitRef`
/// message has no summary.
fn subject(repo: &gix::Repository, commit: gix::ObjectId) -> Result<String> {
    let commit = repo.find_object(commit).map_err(Error::repo)?.into_commit();
    Ok(commit.message().map_err(Error::repo)?.summary().to_string())
}

/// The exact worktree tree: the tip's tree with the scan assembled onto it,
/// nothing size-capped out. The second result says the tree is clean.
fn open_tree(repo: &gix::Repository, tip_tree: gix::ObjectId) -> Result<(gix::ObjectId, bool)> {
    let scan = snaptree::scan(repo)?;
    if scan.is_empty() {
        return Ok((tip_tree, true));
    }
    let (tree_id, _skipped) = snaptree::assemble(repo, tip_tree, &scan, u64::MAX)?;
    Ok((tree_id, false))
}

/// A three-way tree merge, resolved but not yet written: the caller probes
/// with a handle that writes nothing, and only then merges for real.
fn merge_into(
    repo: &gix::Repository,
    base: gix::ObjectId,
    ours: gix::ObjectId,
    theirs: gix::ObjectId,
) -> Result<gix::merge::tree::Outcome<'_>> {
    let options = repo.tree_merge_options().map_err(Error::repo)?;
    repo.merge_trees(base, ours, theirs, Default::default(), options)
        .map_err(Error::repo)
}

/// The `held/rewrite-conflict` refusal, naming the paths the way the rest of
/// the tool prints them.
fn conflict_error(repo: &gix::Repository, at: &At, base_name: &str, paths: &[String]) -> Error {
    let what = match at {
        At::Commit { id, subject } => format!(
            "replaying {} \"{}\" onto {base_name} conflicts",
            short(
                repo,
                gix::ObjectId::from_hex(id.as_bytes()).expect("the probe's ids are shas")
            ),
            subject
        ),
        At::OpenChange => format!("replaying your open change onto {base_name} conflicts"),
    };
    Error::coded(
        "held/rewrite-conflict",
        format!(
            "{what} in {}: nothing was restacked",
            rewrite::join_paths(paths)
        ),
        vec!["ff status".into(), "ff log -r <rev>".into()],
    )
}

/// Replay the branch's commits onto its base, carrying the open change onto
/// the new tip when the head branch is carried along.
pub fn restack(
    repo: &gix::Repository,
    branch: Option<String>,
    onto: Option<String>,
    prov: &Provenance,
    now: Option<i64>,
    argv: Vec<String>,
) -> Result<(RestackOutcome, verb::VerbContext)> {
    // 1. Guards.
    if repo.workdir().is_none() {
        return Err(Error::coded(
            "repo/bare",
            "bare repository: nothing to restack",
            vec![],
        ));
    }

    if let Some(op) = crate::head::operation(repo) {
        return Err(Error::coded(
            "repo/mid-operation",
            format!(
                "a {op:?} is in progress: finish it with git (git rebase --abort / git merge \
                 --abort); fufu owns merges in a later phase"
            ),
            vec![],
        ));
    }

    // 2.
    let ctx = verb::begin_verb(repo, prov, now)?;
    let now = ctx.now;

    // 3. HEAD, and the branch to move. HEAD's own state is only fatal when
    // the verb needs it: a named branch does not.
    let head = crate::head::head_state(repo)?;
    let (head_branch, head_tip) = match &head {
        HeadState::Branch { name, commit, .. } => {
            let tip = gix::ObjectId::from_hex(commit.as_bytes()).map_err(Error::repo)?;
            (Some(name.clone()), Some(tip))
        }
        HeadState::Unborn { .. } | HeadState::Detached { .. } => (None, None),
    };

    let branch = match branch {
        Some(raw) => {
            let name = switch::resolve_branch(repo, &raw)?;
            // Restacking a branch another worktree has checked out would
            // rewrite that worktree's tree underfoot.
            crate::branch::guard_other_worktrees(repo, &name)?;
            name
        }
        None => match &head {
            HeadState::Branch { name, .. } => name.clone(),
            HeadState::Unborn { .. } => {
                return Err(Error::coded(
                    "target/unresolvable",
                    "nothing is committed yet: there is nothing to restack",
                    vec!["ff commit -m <msg>".into()],
                ));
            }
            HeadState::Detached { .. } => {
                return Err(Error::coded(
                    "repo/detached",
                    "detached HEAD: name the branch to restack",
                    vec!["ff restack <branch>".into()],
                ));
            }
        },
    };

    let branch_tip = refs::ref_target(repo, &format!("refs/heads/{branch}"))?.ok_or_else(|| {
        Error::coded(
            "branch/not-found",
            format!("no branch named {branch}"),
            vec![],
        )
    })?;

    // 4. The base.
    let (base_name, reaimed) = match onto {
        Some(raw) => {
            let name = switch::resolve_branch(repo, &raw)?;
            if name == branch {
                return Err(Error::coded(
                    "usage/restack-onto-self",
                    format!("{branch} cannot be restacked onto itself"),
                    vec![format!("ff restack {branch} --onto <base>")],
                ));
            }
            (name, true)
        }
        None => {
            let sync_ref = futures::base_for(repo, &branch)?.ok_or_else(|| {
                Error::coded(
                    "restack/no-base",
                    format!("{branch} has no base to replay onto"),
                    vec![
                        format!("ff restack {branch} --onto <base>"),
                        "ff status".into(),
                    ],
                )
            })?;
            (sync_ref.name, false)
        }
    };

    let base_tip =
        refs::ref_target(repo, &format!("refs/heads/{base_name}"))?.ok_or_else(|| {
            Error::coded(
                "branch/not-found",
                format!("no branch named {base_name}"),
                vec![],
            )
        })?;

    let recorded_parent = branchmeta::read(repo, &branch)?.parent;
    let previous_parent = recorded_parent.clone();
    // `--onto` naming the parent that is already recorded changes nothing.
    let parent_changes = reaimed && recorded_parent.as_deref() != Some(base_name.as_str());

    // 5. The range, and whether the worktree moves.
    let bases: Vec<gix::ObjectId> = repo
        .merge_bases_many(branch_tip, &[base_tip])
        .map_err(Error::repo)?
        .into_iter()
        .map(|id| id.detach())
        .collect();
    if bases.is_empty() {
        return Err(Error::coded(
            "restack/unrelated",
            format!(
                "{branch} and {base_name} have no common ancestor: there is nothing to \
                     replay onto"
            ),
            vec!["ff log".into()],
        ));
    }

    // Up-to-date before fast-forward: when the tips are equal both are true,
    // and the honest answer is "already there", not "fast-forwarded by
    // nothing". futures.rs makes the same choice for the same reason.
    let up_to_date = bases.contains(&base_tip);
    let fast_forward = !up_to_date && bases.contains(&branch_tip);

    if up_to_date && !parent_changes {
        return Ok((
            RestackOutcome::NothingToRestack {
                branch,
                base: base_name,
            },
            ctx,
        ));
    }

    let mut range: Vec<gix::ObjectId> = Vec::new();
    if !up_to_date && !fast_forward {
        // Newest-first from the walk; reversed below. A restack is something
        // a person asked for, so the range is walked in full — no depth cap.
        let walk = repo
            .rev_walk(Some(branch_tip))
            .with_boundary(bases.iter().copied())
            .all()
            .map_err(Error::repo)?;
        for info in walk {
            let info = info.map_err(Error::repo)?;
            if info.parent_ids().count() > 1 {
                return Err(Error::coded(
                    "rewrite/merge-in-range",
                    format!(
                        "{} \"{}\" is a merge, and replaying a merge is ambiguous: nothing was \
                         rewritten",
                        short(repo, info.id),
                        subject(repo, info.id)?
                    ),
                    vec!["ff log".into()],
                ));
            }
            range.push(info.id);
        }
        range.reverse(); // oldest-first; the target is the first element
    }

    let head_carried = if up_to_date {
        false
    } else if fast_forward {
        head_branch.as_deref() == Some(branch.as_str())
    } else {
        head_branch
            .as_deref()
            .is_some_and(|h| h == branch || head_tip.is_some_and(|t| range.contains(&t)))
    };

    // 6. The open change — only when the head branch is carried.
    let (mut open, mut head_tip_tree) = (None, None);
    if head_carried && let Some(ht) = head_tip {
        let ht_tree = tree_of(repo, ht)?;
        open = Some(open_tree(repo, ht_tree)?.0);
        head_tip_tree = Some(ht_tree);
    }

    // 7. The conflict verdict — probes only, nothing written. A verb the user
    // asked for pays the full replay cost, unlike thrifty `ff status`.
    let mut replayed = 0usize;
    if !up_to_date && !fast_forward {
        let probe_open = if head_carried && head_branch.as_deref() == Some(branch.as_str()) {
            open
        } else {
            None
        };
        match futures::probe_to_depth(repo, base_tip, branch_tip, probe_open, usize::MAX)? {
            Verdict::Clean { replayed: n } => {
                replayed = n;
                // Standing mid-stack: the open change belongs to the head
                // branch's tip, not the target's, so it needs its own probe.
                // Replaying bases..head_tip is a prefix of the range just
                // proven clean, so only the open-change step can still fail.
                if let (Some(open_t), Some(hb), Some(ht)) = (open, head_branch.as_deref(), head_tip)
                    && hb != branch
                    && let Verdict::Conflict {
                        at: at @ At::OpenChange,
                        paths,
                    } = futures::probe_to_depth(repo, base_tip, ht, Some(open_t), usize::MAX)?
                {
                    return Err(conflict_error(repo, &at, &base_name, &paths));
                }
            }
            Verdict::Conflict { at, paths } => {
                return Err(conflict_error(repo, &at, &base_name, &paths));
            }
            // Unreachable in practice — §5 already refused a merge in the
            // range — but handled rather than panicked, named the way §5
            // names it.
            Verdict::Unknown {
                reason: UnknownReason::MergeCommits,
            } => {
                let merge = range
                    .iter()
                    .copied()
                    .find(|&id| {
                        repo.find_object(id).is_ok_and(|obj| {
                            gix::objs::CommitRef::from_bytes(&obj.data)
                                .is_ok_and(|c| c.parents.len() > 1)
                        })
                    })
                    .unwrap_or(branch_tip);
                return Err(Error::coded(
                    "rewrite/merge-in-range",
                    format!(
                        "{} \"{}\" is a merge, and replaying a merge is ambiguous: nothing was \
                         rewritten",
                        short(repo, merge),
                        subject(repo, merge).unwrap_or_default()
                    ),
                    vec!["ff log".into()],
                ));
            }
            verdict => {
                return Err(Error::msg(format!(
                    "the probe and the range walk disagree ({verdict:?}): internal inconsistency"
                )));
            }
        }
    }

    // 8. Plan, and the new worktree.
    let mut carried: Vec<RefTransition> = Vec::new();
    let mut rewrites: Vec<rewrite::Rewrite> = Vec::new();
    let mut new_tip = branch_tip;
    let mut published = 0usize;
    let mut new_head_tip: Option<gix::ObjectId> = None;
    let mut new_worktree: Option<gix::ObjectId> = None;

    if !up_to_date && !fast_forward {
        let plan = rewrite::plan(
            repo,
            range[0],
            branch_tip,
            &rewrite::Change::Onto(base_tip),
            now,
        )?;
        published = rewrite::published_count(repo, &branch, &plan.rewrites)?;
        carried = plan.carried;
        rewrites = plan.rewrites;
        new_tip = plan.new_tip;

        // The head branch's new tip comes out of the carried table either
        // way, so there is one code path for head and non-head.
        if head_carried && let Some(hb) = head_branch.as_deref() {
            let hb_ref = format!("refs/heads/{hb}");
            let entry = carried
                .iter()
                .find(|t| t.name == hb_ref)
                .ok_or_else(|| Error::msg(format!("{hb_ref} was not carried: internal error")))?;
            let new_hex = entry
                .new
                .as_deref()
                .ok_or_else(|| Error::msg(format!("{hb_ref} has no new end: internal error")))?;
            new_head_tip = Some(gix::ObjectId::from_hex(new_hex.as_bytes()).map_err(Error::repo)?);
        }

        // The worktree the verb will leave behind. §7 already proved this
        // merge clean, so this pass only makes it real.
        if let (Some(open_t), Some(ht_tree), Some(new_ht)) = (open, head_tip_tree, new_head_tip) {
            if open_t == ht_tree {
                new_worktree = Some(tree_of(repo, new_ht)?);
            } else {
                let mut outcome = merge_into(repo, ht_tree, tree_of(repo, new_ht)?, open_t)?;
                new_worktree = Some(outcome.tree.write().map_err(Error::repo)?.detach());
            }
        }
    } else if fast_forward {
        // The base already contains the branch: no commit is rewritten, the
        // ref simply moves. No plan, no rewrite map.
        carried = vec![RefTransition {
            name: format!("refs/heads/{branch}"),
            old: Some(branch_tip.to_string()),
            new: Some(base_tip.to_string()),
        }];
        new_tip = base_tip;
        if head_carried && let (Some(open_t), Some(ht_tree)) = (open, head_tip_tree) {
            new_head_tip = Some(base_tip);
            // The FastForward verdict fires before the probe ever reaches the
            // open-change step, so it is checked here — purely.
            let base_tree = ht_tree;
            let ours_tree = tree_of(repo, base_tip)?;
            let paths = futures::conflict_paths(repo, base_tree, ours_tree, open_t)?;
            if !paths.is_empty() {
                return Err(conflict_error(repo, &At::OpenChange, &base_name, &paths));
            }
            if open_t == ht_tree {
                new_worktree = Some(ours_tree);
            } else {
                let mut outcome = merge_into(repo, base_tree, ours_tree, open_t)?;
                new_worktree = Some(outcome.tree.write().map_err(Error::repo)?.detach());
            }
        }
    }

    // 11. Write-ahead: the planned table is the post-restack world.
    let mut planned = observe_refs(repo)?;
    for t in &carried {
        if let Some(new) = &t.new {
            planned.refs.insert(t.name.clone(), new.clone());
        }
    }

    let mut record = OpRecord::new("restack", format!("restack {branch} onto {base_name}"), now);
    record.argv = argv;
    record.refs = carried.clone();
    record.rewrites = rewrites.clone();
    if parent_changes {
        record.parent = Some(ParentTransition {
            branch: branch.clone(),
            old: recorded_parent.clone(),
            new: Some(base_name.clone()),
        });
    }

    let mut pins: Vec<gix::ObjectId> = rewrites
        .iter()
        .map(|r| gix::ObjectId::from_hex(r.new.as_bytes()).map_err(Error::repo))
        .collect::<Result<_>>()?;
    pins.push(branch_tip);
    if let Some(head_tip) = head_tip
        && head_tip != branch_tip
    {
        pins.push(head_tip);
    }

    verb::append_op(
        repo,
        OpKind::Op,
        verb::VerbOp {
            record,
            planned,
            // Absorb passes ctx.pre_tree because it writes no files; restack
            // does when the head branch is carried. The recorded end tree
            // must be what the tree will actually hold, or an undo of the
            // next operation throws the carried change away (switch.rs:203-214).
            tree: new_worktree.unwrap_or(ctx.pre_tree),
            // The new tip's tree, so the next foreign `git status` sees the
            // open change against the commit it now sits on.
            index_tree: new_head_tip
                .map(|id| tree_of(repo, id))
                .transpose()?
                .unwrap_or(ctx.pre_tree),
            // The chain the op leaves you on: an off-branch restack leaves
            // you exactly where you stood, so HEAD's branch — and only when
            // HEAD is detached or unborn is the restacked one the honest
            // answer.
            branch: head_branch.clone().unwrap_or_else(|| branch.clone()),
            base: Some(branch_tip),
            session: prov.session.clone(),
            pins: &pins,
        },
        now,
    )?;

    // 12. Mutate.
    // 12.1 Refs: one atomic transaction over every carried head.
    let reflog_msg = format!("restack: onto {base_name}");
    let mut edits = Vec::new();
    for t in &carried {
        let (Some(old), Some(new)) = (&t.old, &t.new) else {
            continue;
        };
        let old_id = gix::ObjectId::from_hex(old.as_bytes()).map_err(Error::repo)?;
        let new_id = gix::ObjectId::from_hex(new.as_bytes()).map_err(Error::repo)?;
        edits.push(refs::update_edit(
            &t.name,
            new_id,
            gix::refs::transaction::PreviousValue::MustExistAndMatch(gix::refs::Target::Object(
                old_id,
            )),
            &reflog_msg,
        )?);
    }
    match refs::commit_edits(repo, edits, now)? {
        refs::EditOutcome::Applied => {}
        refs::EditOutcome::Contended => {
            return Err(Error::coded(
                "ref/contended",
                "refs moved while restacking; nothing was rewritten (re-run to restack on the \
                 new tips)",
                vec![],
            ));
        }
    }

    // 12.2 The recorded parent.
    if parent_changes {
        let mut meta = branchmeta::read(repo, &branch)?;
        meta.parent = Some(base_name.clone());
        branchmeta::write(repo, &branch, &meta)?;
    }

    // 12.3 Worktree: index first, then the files — the order switch.rs uses.
    let mut files = 0usize;
    if let (Some(open_t), Some(new_wt), Some(new_ht)) = (open, new_worktree, new_head_tip) {
        crate::index::write_index_for_tree(repo, tree_of(repo, new_ht)?)?;
        let everything = |_: &str| true;
        let transition = crate::worktree::apply_tree_transition(repo, open_t, new_wt, &everything)?;
        files = transition.written.len() + transition.deleted.len();
    }

    // 12.4 Futures caches: the restacked branch and every branch it carried.
    // Best-effort — it costs recomputation and nothing else.
    let _ = futures::cache::remove(repo, &branch);
    for t in &carried {
        if let Some(name) = t.name.strip_prefix("refs/heads/")
            && name != branch
        {
            let _ = futures::cache::remove(repo, name);
        }
    }

    // 13. The parked disclosure: say so, never move it. Skipped for the
    // branch underfoot — a branch you are standing on has no parked entry to
    // speak of, its change being open rather than parked.
    let parked = if head_carried && head_branch.as_deref() == Some(branch.as_str()) {
        None
    } else {
        match crate::stash::parked_entry(repo, &branch)? {
            Some(id) => {
                let sc = crate::stash::read_stash_commit(repo, id)?;
                let new_tip_tree = tree_of(repo, new_tip)?;
                let applies =
                    futures::conflict_paths(repo, sc.base_tree, new_tip_tree, sc.wip_tree)?
                        .is_empty();
                Some(Parked {
                    stash: id.to_string(),
                    applies,
                })
            }
            None => None,
        }
    };

    // 14. The report.
    let behind = crate::upstream::count_exclusive(repo, base_tip, &bases)?;
    let branch_ref = format!("refs/heads/{branch}");
    let moved: Vec<String> = carried
        .iter()
        .filter(|t| t.name != branch_ref)
        .map(|t| {
            t.name
                .strip_prefix("refs/heads/")
                .unwrap_or(&t.name)
                .to_string()
        })
        .collect(); // carried is already sorted by ref name

    // The exact open tree, never ctx.pre_tree: the capture floor may have
    // size-capped a blob out of pre_tree while the exact tree kept it.
    let still_open = match (open, new_head_tip.map(|id| tree_of(repo, id)).transpose()?) {
        (Some(open_t), Some(t)) => open_t != t,
        _ => false,
    };

    let published_on = rewrite::tracking_name(repo, &branch)?;

    Ok((
        RestackOutcome::Restacked(RestackReport {
            branch,
            base: base_name,
            onto: base_tip.to_string(),
            reaimed,
            previous_parent,
            replayed,
            behind,
            fast_forward,
            new_tip: new_tip.to_string(),
            moved,
            published,
            published_on,
            parked,
            files,
            still_open,
        }),
        ctx,
    ))
}
