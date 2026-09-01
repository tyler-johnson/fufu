//! The two verbs that aim a tree change at [`crate::rewrite::plan`]:
//! `ff absorb` folds the open change into a commit at a distance, and
//! `ff lift` runs the same reach backwards, taking paths out of a commit
//! and back into the open change.
//!
//! Neither verb writes a single file: nothing is added to or removed from
//! the working tree, and content is only reattributed between the open
//! change and a commit. The worktree is byte-identical before and after —
//! only refs, the index, and the operation log move.

use gix::bstr::ByteSlice;

use crate::error::{Error, Result};
use crate::futures::At;
use crate::held::{self, Held, Intent};
use crate::hooks;
use crate::model::{AbsorbOutcome, AbsorbReport, HeadState, HeldReport, LiftOutcome, LiftReport};
use crate::ops::record::observe_refs;
use crate::ops::{OpKind, OpRecord, verb};
use crate::refs;
use crate::rewrite;
use crate::snapshot::Provenance;
use crate::snapshot::tree as snaptree;

/// The branch HEAD sits on and its tip commit.
fn head_branch(repo: &gix::Repository, verb_noun: &str) -> Result<(String, gix::ObjectId)> {
    let head = crate::head::head_state(repo)?;
    match head {
        HeadState::Branch { name, commit, .. } => {
            let tip = gix::ObjectId::from_hex(commit.as_bytes()).map_err(Error::repo)?;
            Ok((name, tip))
        }
        HeadState::Unborn { .. } => Err(Error::coded(
            "target/unresolvable",
            format!("nothing is committed yet: there is nothing to {verb_noun}"),
            vec!["ff commit -m <msg>".into()],
        )),
        HeadState::Detached { .. } => Err(Error::coded(
            "repo/detached",
            "detached HEAD: there is no branch to carry the rewrite",
            vec!["ff switch <branch>".into()],
        )),
    }
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

/// The first parent's tree, or the empty tree for a root commit.
fn parent_tree_of(repo: &gix::Repository, commit: gix::ObjectId) -> Result<gix::ObjectId> {
    let obj = repo.find_object(commit).map_err(Error::repo)?;
    let commit_ref = gix::objs::CommitRef::from_bytes(&obj.data).map_err(Error::repo)?;
    match commit_ref.parents.first() {
        Some(hex) => tree_of(repo, gix::ObjectId::from_hex(hex).map_err(Error::repo)?),
        None => Ok(gix::ObjectId::empty_tree(repo.object_hash())),
    }
}

/// The exact worktree tree: the tip's tree with the scan assembled onto it,
/// nothing size-capped out — an absorb must be exact for the same reason a
/// commit is. The second result says the tree is clean, and the scan comes
/// back with it because the hook window needs the paths a filter left behind.
fn open_tree(
    repo: &gix::Repository,
    tip_tree: gix::ObjectId,
) -> Result<(gix::ObjectId, bool, snaptree::Scan)> {
    let scan = snaptree::scan(repo)?;
    if scan.is_empty() {
        return Ok((tip_tree, true, scan));
    }
    let (tree_id, _skipped) = snaptree::assemble(repo, tip_tree, &scan, u64::MAX)?;
    Ok((tree_id, false, scan))
}

/// `base` with the selected paths taking `other`'s content. An empty
/// `selectors` list selects everything, so the result is `other`.
fn filtered(
    repo: &gix::Repository,
    base: gix::ObjectId,
    other: gix::ObjectId,
    selectors: &[String],
) -> Result<gix::ObjectId> {
    if base == other {
        return Ok(base);
    }
    let lhs = repo.find_object(base).map_err(Error::repo)?.detach();
    let rhs = repo.find_object(other).map_err(Error::repo)?.detach();
    let mut recorder = gix::diff::tree::Recorder::default();
    gix::diff::tree(
        gix::objs::TreeRefIter::from_bytes(&lhs.data),
        gix::objs::TreeRefIter::from_bytes(&rhs.data),
        gix::diff::tree::State::default(),
        &repo.objects,
        &mut recorder,
    )
    .map_err(Error::repo)?;

    let mut editor = repo.edit_tree(base).map_err(Error::repo)?;
    use gix::diff::tree::recorder::Change as Rec;
    for record in recorder.records {
        match record {
            Rec::Addition {
                entry_mode,
                oid,
                path,
                ..
            } => {
                if entry_mode.is_tree() {
                    continue; // directories are not entries
                }
                let path = path.to_string();
                if !selectors.is_empty() && !crate::restore::path_selected(&path, selectors) {
                    continue;
                }
                editor
                    .upsert(path.as_str(), entry_mode.kind(), oid)
                    .map_err(Error::repo)?;
            }
            Rec::Deletion {
                entry_mode, path, ..
            } => {
                if entry_mode.is_tree() {
                    continue;
                }
                let path = path.to_string();
                if !selectors.is_empty() && !crate::restore::path_selected(&path, selectors) {
                    continue;
                }
                editor.remove(path.as_str()).map_err(Error::repo)?;
            }
            Rec::Modification {
                entry_mode,
                oid,
                path,
                ..
            } => {
                if entry_mode.is_tree() {
                    continue;
                }
                let path = path.to_string();
                if !selectors.is_empty() && !crate::restore::path_selected(&path, selectors) {
                    continue;
                }
                editor
                    .upsert(path.as_str(), entry_mode.kind(), oid)
                    .map_err(Error::repo)?;
            }
        }
    }
    Ok(editor.write().map_err(Error::repo)?.detach())
}

/// A three-way tree merge, resolved but not yet written: the caller probes
/// with a handle that writes nothing, and only then merges for real.
fn merge_into<'a>(
    repo: &'a gix::Repository,
    base: gix::ObjectId,
    ours: gix::ObjectId,
    theirs: gix::ObjectId,
    labels: Option<&(String, String)>,
) -> Result<gix::merge::tree::Outcome<'a>> {
    let options = repo.tree_merge_options().map_err(Error::repo)?;
    let labels = match labels {
        Some((ours_label, theirs_label)) => gix::merge::blob::builtin_driver::text::Labels {
            ancestor: None,
            current: Some(ours_label.as_bytes().as_bstr()),
            other: Some(theirs_label.as_bytes().as_bstr()),
        },
        None => Default::default(),
    };
    repo.merge_trees(base, ours, theirs, labels, options)
        .map_err(Error::repo)
}

/// The labels a fold writes when it conflicts. A conflicted fold is handed
/// straight to `chain` as step one's tree, so its markers have to be fufu's
/// own: `regions` and `attribute` only see a block whose closer carries a
/// step, and a block nobody can attribute is a block that lands inside a
/// commit.
fn fold_labels(
    repo: &gix::Repository,
    target: gix::ObjectId,
    tip: gix::ObjectId,
) -> Result<(String, String)> {
    let n = rewrite::stack_size(repo, target, tip)?;
    Ok(rewrite::chain_labels(&subject(repo, target)?, 1, n))
}

/// A rewrite that conflicts is an outcome, not an error: record the hold the
/// caller assembled as a slim operation and report it. Nothing moves — no
/// ref, no file, no futures cache — so the whole path is the operation's
/// append and the branch's metadata, the way `ff describe` records a pending
/// description. Shared by `absorb` and `lift`: they differ only in the
/// `held` they assembled and the report's verb, which is why both travel as
/// arguments rather than being special-cased.
fn hold(
    repo: &gix::Repository,
    rec: held::Recording<'_>,
    verb: &str,
    branch: &str,
    held: &Held,
    summary: String,
    of: usize,
) -> Result<HeldReport> {
    let verb_past = match verb {
        "absorb" => "absorbed",
        "lift" => "lifted",
        other => other,
    };
    held::refuse_if_held(repo, branch, verb_past)?;
    held::record(repo, rec, branch, held, summary)?;
    Ok(HeldReport {
        verb: verb.to_string(),
        branch: branch.to_string(),
        at: held.at.clone(),
        paths: held.paths.clone(),
        of,
    })
}

/// The target must still sit in the branch's history — a hold recorded
/// earlier may name a commit a later rewrite has since moved out of reach,
/// and re-planning it would fold into nothing.
fn in_history(repo: &gix::Repository, target: gix::ObjectId, tip: gix::ObjectId) -> Result<()> {
    let bases: Vec<gix::ObjectId> = repo
        .merge_bases_many(target, &[tip])
        .map_err(Error::repo)?
        .into_iter()
        .map(|id| id.detach())
        .collect();
    if !bases.contains(&target) {
        return Err(Error::coded(
            "rewrite/not-in-history",
            format!(
                "{} is no longer in the branch's history",
                crate::sha::short_oid(target)
            ),
            vec!["ff log".into()],
        ));
    }
    Ok(())
}

/// The triple an absorb replays: the target takes the open change folded
/// into it, and its descendants follow.
///
/// The fold is computed against the working tree as it stands now, so a hold
/// re-planned later sees whatever has been done since it was recorded. When
/// the target is not the tip the fold is a three-way merge that can leave
/// unresolved paths; the merged tree is returned either way, conflicts and
/// all. Deciding what a conflicted fold means — holding, refusing, resolving —
/// is the caller's job, not the replan's.
pub(crate) fn replan_absorb(
    repo: &gix::Repository,
    into: Option<gix::ObjectId>,
    paths: &[String],
    open: Option<gix::ObjectId>,
) -> Result<held::Replan> {
    let (_branch, tip) = head_branch(repo, "absorb into")?;
    let target = into.unwrap_or(tip);
    in_history(repo, target, tip)?;
    let tip_tree = repo.head_tree_id_or_empty().map_err(Error::repo)?.detach();
    // `open`, when given, is the working tree a resolution session recorded
    // before it wrote the markers over it — the same change this read
    // otherwise takes from disk.
    let open_tree = match open {
        Some(tree) => tree,
        None => open_tree(repo, tip_tree)?.0,
    };
    let theirs = filtered(repo, tip_tree, open_tree, paths)?;
    let target_tree = tree_of(repo, target)?;
    let new_target_tree = if target == tip {
        theirs
    } else {
        let labels = fold_labels(repo, target, tip)?;
        let mut outcome = merge_into(repo, tip_tree, target_tree, theirs, Some(&labels))?;
        outcome.tree.write().map_err(Error::repo)?.detach()
    };
    let change = rewrite::Change::Tree {
        tree: new_target_tree,
        message: None,
    };
    Ok(held::Replan {
        target,
        tip,
        change,
    })
}

/// The triple a lift replays: the target loses the selected paths back to
/// its parent's content, and its descendants follow.
pub(crate) fn replan_lift(
    repo: &gix::Repository,
    from: Option<gix::ObjectId>,
    paths: &[String],
) -> Result<held::Replan> {
    let (_branch, tip) = head_branch(repo, "lift from")?;
    let target = from.unwrap_or(tip);
    in_history(repo, target, tip)?;
    let target_tree = tree_of(repo, target)?;
    let parent_tree = parent_tree_of(repo, target)?;
    let lifted = filtered(repo, target_tree, parent_tree, paths)?;
    let change = rewrite::Change::Tree {
        tree: lifted,
        message: None,
    };
    Ok(held::Replan {
        target,
        tip,
        change,
    })
}

/// Fold the open change — or the part of it a path filter selected — into a
/// commit at a distance: `HEAD` by default, or the one named by `into`.
pub fn absorb(
    repo: &gix::Repository,
    into: Option<gix::ObjectId>,
    paths: Vec<String>,
    verify: hooks::Verify,
    prov: &Provenance,
    now: Option<i64>,
    argv: Vec<String>,
) -> Result<(AbsorbOutcome, verb::VerbContext)> {
    absorb_with(
        repo,
        into,
        paths,
        verify,
        prov,
        (now, argv),
        &rewrite::Decided::none(),
    )
}

/// `absorb`, with some rewritten commits' trees decided in advance. When the
/// target's tree is among them the fold itself is decided: the merge that
/// would fold the open change in, and the conflict check guarding it, are
/// both skipped.
pub fn absorb_with(
    repo: &gix::Repository,
    into: Option<gix::ObjectId>,
    paths: Vec<String>,
    verify: hooks::Verify,
    prov: &Provenance,
    invocation: (Option<i64>, Vec<String>),
    decided: &rewrite::Decided,
) -> Result<(AbsorbOutcome, verb::VerbContext)> {
    let (now, argv) = invocation;
    if repo.workdir().is_none() {
        return Err(Error::coded(
            "repo/bare",
            "bare repository: nothing to absorb",
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

    let ctx = verb::begin_verb(repo, prov, now)?;
    let now = ctx.now;
    let (branch, tip) = head_branch(repo, "absorb into")?;

    let tip_tree = repo.head_tree_id_or_empty().map_err(Error::repo)?.detach();
    let (mut open_tree, clean, scan) = open_tree(repo, tip_tree)?;
    if clean || open_tree == tip_tree {
        return Ok((
            AbsorbOutcome::NothingToAbsorb {
                branch: branch.clone(),
            },
            ctx,
        ));
    }

    // The tip's tree, with the selected paths taken from the worktree.
    let mut theirs = filtered(repo, tip_tree, open_tree, &paths)?;
    if theirs == tip_tree {
        return Ok((
            AbsorbOutcome::NothingToAbsorb {
                branch: branch.clone(),
            },
            ctx,
        ));
    }

    // The pre-commit gate. `theirs` is precisely what is folding in — the
    // tip's tree with the selected paths taken from the worktree — so a
    // hook-runner asking `git diff --cached` against the tip is told exactly
    // those paths, and a partial `ff absorb <path>` shows it exactly that
    // slice. Emptiness refuses above, before any hook runs, matching close.
    //
    // A resolution landing has already run the gate in `finish_resolution`:
    // this re-entry must not run it a second time.
    let mut window = None;
    if decided.clearing.is_none()
        && verify == hooks::Verify::Run
        && hooks::will_run(repo, &["pre-commit"])?
    {
        let differs = snaptree::unselected_paths(&scan, &paths);
        let (opened, ran) = hooks::Window::open(repo, theirs, &differs, verify, "absorb")?;
        window = Some(opened);
        if ran {
            // A formatter's fixes are part of what folds in, the same way
            // they are part of a close — so re-read the worktree and put
            // both emptiness refusals again, since the hook may have
            // reverted the change it was handed. `self::` because the local
            // binding shadows the helper's name from here on.
            let (reread, clean, _scan) = self::open_tree(repo, tip_tree)?;
            open_tree = reread;
            if clean || open_tree == tip_tree {
                return Ok((
                    AbsorbOutcome::NothingToAbsorb {
                        branch: branch.clone(),
                    },
                    ctx,
                ));
            }
            theirs = filtered(repo, tip_tree, open_tree, &paths)?;
            if theirs == tip_tree {
                return Ok((
                    AbsorbOutcome::NothingToAbsorb {
                        branch: branch.clone(),
                    },
                    ctx,
                ));
            }
        }
    }

    let target = into.unwrap_or(tip);
    let target_tree = tree_of(repo, target)?;
    let target_subject = subject(repo, target)?;

    // The fold itself can conflict before a single descendant is replayed.
    // `replan_absorb` returns the folded tree either way, so the verb decides
    // what a conflicted fold means here rather than in the replan. Skipped
    // when the target's tree is decided: the fold's result is already in
    // `decided`, so this merge is no longer going to happen.
    if target != tip && !decided.trees.contains_key(&target) {
        let memory = repo.clone().with_object_memory();
        let probe = merge_into(&memory, tip_tree, target_tree, theirs, None)?;
        let conflicted = crate::futures::unresolved(&probe);
        if !conflicted.is_empty() {
            // The fold itself cannot apply the open change to the target —
            // `at` is the open change, not a commit — and the absorb never
            // reaches a replay, so `of` is 0: the size of the stack it would
            // have restacked is unknown here, and we do not invent one.
            let held = Held {
                intent: Intent::Absorb {
                    into: target.to_string(),
                    paths: paths.clone(),
                },
                at: At::OpenChange,
                paths: conflicted.clone(),
                time: now,
            };
            return Ok((
                AbsorbOutcome::Held(hold(
                    repo,
                    held::Recording {
                        ctx: &ctx,
                        prov,
                        argv,
                        now,
                    },
                    "absorb",
                    &branch,
                    &held,
                    format!("hold absorb into {}", crate::sha::short_oid(target)),
                    0,
                )?),
                ctx,
            ));
        }
    }

    // The target's new tree: a decided landing carries it in `decided` —
    // `chain` already folded the resolution in — so the fold's merge is
    // skipped along with its probe above, and only the guard that the target
    // still sits in the branch's history runs. Otherwise the fold's merge
    // computes it, the same triple `held::replan` re-derives, so the verb and
    // the replan cannot disagree.
    let new_target_tree = if let Some(tree) = decided.trees.get(&target) {
        in_history(repo, target, tip)?;
        *tree
    } else {
        let replan = replan_absorb(repo, into, &paths, None)?;
        match &replan.change {
            rewrite::Change::Tree { tree, .. } => *tree,
            other => {
                return Err(Error::msg(format!(
                    "internal: an absorb replan is not a tree change: {other:?}"
                )));
            }
        }
    };
    // If the fold changes nothing there is nothing to absorb.
    if new_target_tree == target_tree {
        return Ok((
            AbsorbOutcome::NothingToAbsorb {
                branch: branch.clone(),
            },
            ctx,
        ));
    }
    let change = rewrite::Change::Tree {
        tree: new_target_tree,
        message: None,
    };

    // Pre-flight the descendant replay with the same `change` `plan` will get:
    // a conflict is a hold, and after a clean pre-flight `plan` cannot
    // conflict. Skipped for a decided landing: its trees are already known,
    // so the replay has nothing left to conflict on.
    if decided.is_empty()
        && let Some(conflict) = rewrite::conflict(repo, target, tip, &change)?
    {
        let held = Held {
            intent: Intent::Absorb {
                into: target.to_string(),
                paths: paths.clone(),
            },
            at: conflict.at.clone(),
            paths: conflict.paths.clone(),
            time: now,
        };
        return Ok((
            AbsorbOutcome::Held(hold(
                repo,
                held::Recording {
                    ctx: &ctx,
                    prov,
                    argv,
                    now,
                },
                "absorb",
                &branch,
                &held,
                format!("hold absorb into {}", crate::sha::short_oid(target)),
                conflict.of,
            )?),
            ctx,
        ));
    }
    let plan = rewrite::plan_with(repo, target, tip, &change, now, &decided.trees)?;
    let published = rewrite::published_count(repo, &branch, &plan)?;

    // Write-ahead: the planned table is the post-absorb world. HEAD does not
    // move — it stays symbolic on the same branch.
    let mut planned = observe_refs(repo)?;
    for t in &plan.carried {
        if let Some(new) = &t.new {
            planned.refs.insert(t.name.clone(), new.clone());
        }
    }

    let target_short = crate::sha::short_oid(target);
    let mut record = OpRecord::new(
        "absorb",
        format!("absorb into {target_short} on {branch}"),
        now,
    );
    record.argv = argv;
    record.refs = plan.carried.clone();
    record.rewrites = plan.rewrites.clone();
    record.dropped = plan.dropped.clone();
    if let Some(clearing) = &decided.clearing {
        let (held, resolving) = crate::held::clearing_transitions(clearing);
        record.held = held;
        record.resolving = resolving;
    }

    let mut pins: Vec<gix::ObjectId> = plan
        .rewrites
        .iter()
        .map(|r| gix::ObjectId::from_hex(r.new.as_bytes()).map_err(Error::repo))
        .collect::<Result<_>>()?;
    pins.push(tip);

    // Absorb writes no files, so the planned worktree is the one already
    // there, and the index is about to be rewritten to match the new tip.
    // A resolution landing is the exception: `ff resolve` put the chain's
    // markers in the working tree, and a chain that stopped at a tangle put
    // the TARGET's tree there rather than the tip's — so the landing has to
    // bring the tree to the tip it just wrote, or files the descendants
    // reintroduce would read as deleted.
    let new_tip_tree = tree_of(repo, plan.new_tip)?;
    verb::append_op(
        repo,
        OpKind::Op,
        verb::VerbOp {
            record,
            planned,
            tree: if decided.clearing.is_some() {
                new_tip_tree
            } else {
                ctx.pre_tree
            },
            index_tree: new_tip_tree,
            branch: branch.clone(),
            base: Some(tip),
            session: prov.session.clone(),
            pins: &pins,
        },
        now,
    )?;

    // Move the refs: one atomic transaction over every carried head.
    let reflog_msg = format!("absorb: into {target_short}");
    let mut edits = Vec::new();
    for t in &plan.carried {
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
                "refs moved while absorbing; nothing was rewritten (re-run to absorb on the new \
                 tips)",
                vec![],
            ));
        }
    }

    // The fold has landed: the staged index is no longer provisional, and
    // putting the old one back would contradict the refs that just moved.
    // Every exit before this point — a declining hook, a hold, `ref/contended`,
    // any `?` on the way — drops the window armed and gets the index back
    // byte-for-byte.
    if let Some(window) = window.take() {
        window.landed();
    }

    crate::index::write_index_for_tree(repo, new_tip_tree)?;

    // A resolution landing: bring the working tree to the tip just written —
    // the markers were standing in it and the reader's fixes are already in
    // the commits — and clear the hold and the session it resolved, so one
    // `ff undo` of this op takes the whole resolution back.
    if let Some(clearing) = &decided.clearing {
        let everything = |_: &str| true;
        crate::worktree::apply_tree_transition(repo, open_tree, new_tip_tree, &everything)?;
        crate::held::set(repo, &clearing.branch, None)?;
        crate::held::set_resolving(repo, &clearing.branch, None)?;
    }

    let branch_ref = format!("refs/heads/{branch}");
    let moved: Vec<String> = plan
        .carried
        .iter()
        .filter(|t| t.name != branch_ref)
        .map(|t| {
            t.name
                .strip_prefix("refs/heads/")
                .unwrap_or(&t.name)
                .to_string()
        })
        .collect();

    // The target's new identity — or the fact that the rewrite dropped it.
    // Absent from `rewrites` legitimately only when the plan names it in
    // `dropped`; anywhere else it is an ordering bug, not a drop.
    let new_target = match plan.rewrites.iter().find(|r| r.old == target.to_string()) {
        Some(r) => Some(r.new.clone()),
        None if plan.dropped.iter().any(|d| d.old == target.to_string()) => None,
        None => return Err(Error::msg("the target was not in the rewrite plan")),
    };

    Ok((
        AbsorbOutcome::Absorbed(AbsorbReport {
            branch,
            into: target.to_string(),
            // The target is in `rewrites` exactly when it survived, so it is
            // subtracted from the restack exactly when it is there. Read
            // before `new_target` is moved into `new`.
            restacked: plan
                .rewrites
                .len()
                .saturating_sub(usize::from(new_target.is_some())),
            new: new_target,
            subject: target_subject,
            moved,
            published,
            paths,
            // The exact open tree from above, not `ctx.pre_tree`: the
            // capture floor may have size-capped a blob out of `pre_tree`
            // while the exact tree kept it, and comparing the capped tree
            // would report work still open when there is none. A resolution
            // landing left the tree standing on the new tip, so nothing is
            // open there by construction.
            still_open: decided.clearing.is_none() && open_tree != new_tip_tree,
            dropped: plan.dropped.clone(),
        }),
        ctx,
    ))
}

/// Take paths out of a commit — `HEAD` by default, or the one named by
/// `from` — and back into the open change. The same reach as [`absorb`] run
/// backwards: the target keeps what it still introduces, its descendants
/// are restacked, and no file moves. A fully-lifted commit stays, as an
/// empty commit.
pub fn lift(
    repo: &gix::Repository,
    from: Option<gix::ObjectId>,
    paths: Vec<String>,
    prov: &Provenance,
    now: Option<i64>,
    argv: Vec<String>,
) -> Result<(LiftOutcome, verb::VerbContext)> {
    lift_with(
        repo,
        from,
        paths,
        prov,
        (now, argv),
        &rewrite::Decided::none(),
    )
}

/// `lift`, with some rewritten commits' trees decided in advance: those
/// skip the three-way merge and take what they are given, and the pre-flight
/// — a question about merges that are no longer going to happen — is asked
/// only when nothing is decided.
pub fn lift_with(
    repo: &gix::Repository,
    from: Option<gix::ObjectId>,
    paths: Vec<String>,
    prov: &Provenance,
    invocation: (Option<i64>, Vec<String>),
    decided: &rewrite::Decided,
) -> Result<(LiftOutcome, verb::VerbContext)> {
    let (now, argv) = invocation;
    if repo.workdir().is_none() {
        return Err(Error::coded(
            "repo/bare",
            "bare repository: nothing to lift",
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

    let ctx = verb::begin_verb(repo, prov, now)?;
    let now = ctx.now;
    let (branch, tip) = head_branch(repo, "lift from")?;

    let target = from.unwrap_or(tip);
    let target_tree = tree_of(repo, target)?;

    // The triple the lift replays — the same one `held::replan` re-derives, so
    // the verb and the replan cannot disagree.
    let replan = replan_lift(repo, from, &paths)?;
    // The target's new tree: its own, with the selected paths reverted to the
    // parent's content. If the revert changes nothing there is nothing to lift.
    let lifted = match &replan.change {
        rewrite::Change::Tree { tree, .. } => *tree,
        other => {
            return Err(Error::msg(format!(
                "internal: a lift replan is not a tree change: {other:?}"
            )));
        }
    };
    if lifted == target_tree {
        return Ok((
            LiftOutcome::NothingToLift {
                from: target.to_string(),
            },
            ctx,
        ));
    }
    let change = replan.change;

    // Pre-flight the descendant replay with the same `change` `plan` will get:
    // a conflict is a hold, and after a clean pre-flight `plan` cannot
    // conflict. Skipped for a decided landing: its trees are already known,
    // so the replay has nothing left to conflict on.
    if decided.is_empty()
        && let Some(conflict) = rewrite::conflict(repo, target, tip, &change)?
    {
        let held = Held {
            intent: Intent::Lift {
                from: target.to_string(),
                paths: paths.clone(),
            },
            at: conflict.at.clone(),
            paths: conflict.paths.clone(),
            time: now,
        };
        return Ok((
            LiftOutcome::Held(hold(
                repo,
                held::Recording {
                    ctx: &ctx,
                    prov,
                    argv,
                    now,
                },
                "lift",
                &branch,
                &held,
                format!("hold lift from {}", crate::sha::short_oid(target)),
                conflict.of,
            )?),
            ctx,
        ));
    }
    let plan = rewrite::plan_with(repo, target, tip, &change, now, &decided.trees)?;
    let published = rewrite::published_count(repo, &branch, &plan)?;

    // Write-ahead: the planned table is the post-lift world. HEAD does not
    // move — it stays symbolic on the same branch.
    let mut planned = observe_refs(repo)?;
    for t in &plan.carried {
        if let Some(new) = &t.new {
            planned.refs.insert(t.name.clone(), new.clone());
        }
    }

    let target_short = crate::sha::short_oid(target);
    let mut record = OpRecord::new(
        "lift",
        format!("lift out of {target_short} on {branch}"),
        now,
    );
    record.argv = argv;
    record.refs = plan.carried.clone();
    record.rewrites = plan.rewrites.clone();
    record.dropped = plan.dropped.clone();
    if let Some(clearing) = &decided.clearing {
        let (held, resolving) = crate::held::clearing_transitions(clearing);
        record.held = held;
        record.resolving = resolving;
    }

    let mut pins: Vec<gix::ObjectId> = plan
        .rewrites
        .iter()
        .map(|r| gix::ObjectId::from_hex(r.new.as_bytes()).map_err(Error::repo))
        .collect::<Result<_>>()?;
    pins.push(tip);

    // Lift writes no files, so the planned worktree is the one already
    // there, and the index is about to be rewritten to match the new tip.
    verb::append_op(
        repo,
        OpKind::Op,
        verb::VerbOp {
            record,
            planned,
            tree: ctx.pre_tree,
            index_tree: tree_of(repo, plan.new_tip)?,
            branch: branch.clone(),
            base: Some(tip),
            session: prov.session.clone(),
            pins: &pins,
        },
        now,
    )?;

    // Move the refs: one atomic transaction over every carried head.
    let reflog_msg = format!("lift: out of {target_short}");
    let mut edits = Vec::new();
    for t in &plan.carried {
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
                "refs moved while lifting; nothing was rewritten (re-run to lift on the new \
                 tips)",
                vec![],
            ));
        }
    }

    crate::index::write_index_for_tree(repo, tree_of(repo, plan.new_tip)?)?;

    // A resolution landing: clear the hold and the session it resolved, so
    // one `ff undo` of this op takes the whole resolution back. The tree is
    // left standing, as every lift leaves it: what the lift took out of the
    // commit is exactly what is open in it now.
    if let Some(clearing) = &decided.clearing {
        crate::held::set(repo, &clearing.branch, None)?;
        crate::held::set_resolving(repo, &clearing.branch, None)?;
    }

    let branch_ref = format!("refs/heads/{branch}");
    let moved: Vec<String> = plan
        .carried
        .iter()
        .filter(|t| t.name != branch_ref)
        .map(|t| {
            t.name
                .strip_prefix("refs/heads/")
                .unwrap_or(&t.name)
                .to_string()
        })
        .collect();

    // The target's new identity — or the fact that the rewrite dropped it.
    // Absent from `rewrites` legitimately only when the plan names it in
    // `dropped`; anywhere else it is an ordering bug, not a drop.
    let new_target = match plan.rewrites.iter().find(|r| r.old == target.to_string()) {
        Some(r) => Some(r.new.clone()),
        None if plan.dropped.iter().any(|d| d.old == target.to_string()) => None,
        None => return Err(Error::msg("the target was not in the rewrite plan")),
    };

    Ok((
        LiftOutcome::Lifted(LiftReport {
            branch,
            from: target.to_string(),
            // The target is in `rewrites` exactly when it survived, so it is
            // subtracted from the restack exactly when it is there. Read
            // before `new_target` is moved into `new`.
            restacked: plan
                .rewrites
                .len()
                .saturating_sub(usize::from(new_target.is_some())),
            new: new_target,
            subject: subject(repo, target)?,
            moved,
            published,
            paths,
            dropped: plan.dropped.clone(),
        }),
        ctx,
    ))
}
