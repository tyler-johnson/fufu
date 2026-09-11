//! `ff fold` lands the branch you are standing on into another one: its
//! commits replay onto the target's tip, the target advances to the result,
//! the branch is taken away, and this worktree moves to the target with the
//! open change still open. Trunk is the target when none is named. One
//! operation, so one `ff undo` takes every move back.
//!
//! Nothing is merged. The target's history stays linear, the result `rebase
//! --onto` and then `merge --ff-only` would leave. The replay is
//! `restack`'s: `plan_restack` plans it as if the source were being
//! restacked onto the target, and fold reads the new tip off that plan,
//! gives it to the target, and deletes the source instead of carrying it.
//! A replay that would conflict is a refusal rather than a hold: `ff
//! restack --onto <target>` holds the same replay, and `ff fold` lands it
//! once it is clean.
//!
//! `--stay` is the other reading of a target another worktree holds: the
//! target advances in the tree that holds it, this worktree stays on the
//! source, and the source is kept at the new tip with the target as its
//! base. That is one operation on two chains, and each side's record names
//! the other so an undo of either says what the other tree still holds.

use crate::branch;
use crate::branchmeta::{self, BranchMeta};
use crate::cascade::{self, CascadePlan};
use crate::error::{Error, Result};
use crate::futures::{self, At};
use crate::held::{self, Intent};
use crate::linked::{self, Holder};
use crate::model::{FoldReport, HeadState, MovedTree, Parked, ReconcileReport};
use crate::ops::record::{
    ChangeIdTransition, Companion, DescriptionTransition, ParentTransition, PointerTransition,
    RefTransition, observe_refs,
};
use crate::ops::{OpKind, OpRecord, verb};
use crate::overlay::Overlay;
use crate::refs;
use crate::restack::{self, Aim, Onto, RestackPlan};
use crate::rewrite;
use crate::snapshot::Provenance;
use crate::stash;

/// Everything a fold decided, before anything is written. Commit objects
/// exist; no ref has moved.
struct FoldPlan {
    source: String,
    source_tip: gix::ObjectId,
    onto: Onto,
    /// Where the target lands: the replay's new tip, or the source's own
    /// tip when the target's history already held everything below it.
    new_tip: gix::ObjectId,
    rewrites: Vec<rewrite::Rewrite>,
    dropped: Vec<rewrite::Dropped>,
    replayed: usize,
    published: usize,
    diverged: Vec<String>,
    /// The open change as the plan found it and the tree this worktree will
    /// hold, both when the replay moves HEAD's commit and neither otherwise.
    open: Option<gix::ObjectId>,
    new_worktree: Option<gix::ObjectId>,
    cascade: CascadePlan,
    /// The branches that sat on the source, re-aimed at the target.
    reaims: Vec<ParentTransition>,
    parked_source: Option<gix::ObjectId>,
    source_meta: BranchMeta,
    target_meta: BranchMeta,
    /// How many commits the target moves ahead by.
    advanced: usize,
    holder: Option<Holder>,
    stay: bool,
}

/// The other worktree `--stay` advances the target in, prepared and checked
/// before anything is written on either chain.
struct OtherTree {
    holder: Holder,
    repo: gix::Repository,
    ctx: verb::VerbContext,
    /// Its worktree as a tree, exact.
    open: gix::ObjectId,
    new_tree: gix::ObjectId,
    /// The tree its worktree will hold: the new tip's, or its open change
    /// carried over it.
    merged: gix::ObjectId,
}

/// Land the branch HEAD stands on into `target`, trunk when `None`.
pub fn fold(
    repo: &gix::Repository,
    target: Option<String>,
    stay: bool,
    prov: &Provenance,
    now: Option<i64>,
    argv: Vec<String>,
) -> Result<(FoldReport, verb::VerbContext, Option<ReconcileReport>)> {
    // 1. Guards that need no capture.
    if repo.workdir().is_none() {
        return Err(Error::coded(
            "repo/bare",
            "bare repository: nothing to fold",
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

    let plan = plan_fold(repo, target, stay, now)?;
    let other = match &plan.holder {
        Some(holder) => Some(prepare_other(repo, &ctx, prov, holder, &plan)?),
        None => None,
    };

    if plan.stay {
        let (report, other_reconcile) = commit_fold_stay(repo, &ctx, prov, argv, plan, other)?;
        Ok((report, ctx, other_reconcile))
    } else {
        let report = commit_fold(repo, &ctx, prov, argv, plan)?;
        Ok((report, ctx, None))
    }
}

/// The planning half: every guard, the replay as `restack` plans it, and
/// what the fold adds on top — the target's move, the re-aims, and the
/// cascade's holds re-pointed at the target.
fn plan_fold(
    repo: &gix::Repository,
    target: Option<String>,
    stay: bool,
    now: i64,
) -> Result<FoldPlan> {
    // 2. HEAD: the source is the branch underfoot, and nothing else.
    let (source, source_tip) = match crate::head::head_state(repo)? {
        HeadState::Branch { name, commit, .. } => (
            name,
            gix::ObjectId::from_hex(commit.as_bytes()).map_err(Error::repo)?,
        ),
        HeadState::Unborn { .. } => {
            return Err(Error::coded(
                "target/unresolvable",
                "nothing is committed yet: there is nothing to fold",
                vec!["ff commit -m <msg>".into()],
            ));
        }
        HeadState::Detached { .. } => {
            return Err(Error::coded(
                "repo/detached",
                "detached HEAD: stand on the branch to fold",
                vec!["ff switch <branch>".into()],
            ));
        }
    };

    // 3. Trunk is what everything lands on; it lands on nothing.
    let trunk = crate::trunk::trunk(repo);
    if trunk.as_ref().is_ok_and(|t| t.name == source) {
        return Err(Error::coded(
            "fold/trunk-source",
            format!("{source} is trunk: it is what branches fold into, and folds into nothing"),
            vec!["ff switch <branch>".into(), "ff status".into()],
        ));
    }

    // 4. The source: not a session, not resolving, not held.
    let source_meta = branchmeta::read(repo, &source)?;
    if source_meta.session.is_some() {
        return Err(Error::coded(
            "session/open",
            format!(
                "{source} is an editing session: folding it would land the commit being edited"
            ),
            vec!["ff done".into(), "ff done --abandon".into()],
        ));
    }
    refuse_if_resolving(repo, &source)?;
    held::refuse_if_held(repo, &source, "folded")?;

    // 5. The target: a local branch that is not the source.
    let raw = match target {
        Some(raw) => raw,
        None => trunk?.name,
    };
    let onto = restack::resolve_onto(repo, &raw)?;
    if onto.full == format!("refs/heads/{source}") {
        return Err(Error::coded(
            "usage/fold-into-self",
            format!("{source} cannot be folded into itself"),
            vec!["ff fold <branch>".into(), "ff branch".into()],
        ));
    }
    if !onto.full.starts_with("refs/heads/") {
        return Err(Error::coded(
            "fold/remote-target",
            format!(
                "{} lives on a remote: fold lands into a local branch only",
                onto.name
            ),
            vec![
                format!("ff start {}", onto.name),
                "ff branch".into(),
                "ff push".into(),
            ],
        ));
    }
    let target = onto.name.clone();

    // 6. Who holds the target. Another worktree standing on it is refused
    // unless `--stay` says to advance it there.
    let holder = linked::holder_of(repo, &target)?;
    if let Some(holder) = &holder
        && !stay
    {
        return Err(Error::coded(
            "branch/checked-out-elsewhere",
            format!(
                "'{target}' is already used by worktree at '{}'",
                holder.path.display()
            ),
            vec![format!("ff fold {target} --stay"), "ff worktree".into()],
        ));
    }

    // 7. The target: not a session, not resolving. A plain hold on it is
    // allowed through, as restack allows one: the fold competes with
    // nothing the hold is waiting on.
    let target_meta = branchmeta::read(repo, &target)?;
    if target_meta.session.is_some() {
        return Err(Error::coded(
            "session/open",
            format!("{target} is an editing session: nothing folds into one"),
            vec!["ff done".into(), "ff done --abandon".into()],
        ));
    }
    refuse_if_resolving(repo, &target)?;

    // 8. The replay, as a restack of the source onto the target would plan
    // it. The aim is settled: the target is a branch a person named, and
    // never the source's own shared copy, since that lives under
    // `refs/remotes/` and was refused above.
    let plan = restack::plan_restack(
        repo,
        Some(source.clone()),
        Some(onto.full.clone()),
        now,
        &rewrite::Decided::none(),
        Aim::Settled,
        &Overlay::default(),
    )?;

    // 9. What the plan says the target becomes.
    let mut new_tip = source_tip;
    let mut rewrites = Vec::new();
    let mut dropped = Vec::new();
    let mut replayed = 0usize;
    let mut published = 0usize;
    let mut diverged = Vec::new();
    let mut open = None;
    let mut new_worktree = None;
    let mut cascade_plan = CascadePlan::default();
    match plan {
        RestackPlan::Held(hold) => {
            let where_held = match &hold.held.at {
                At::Commit { id, subject } => format!("{} \"{}\"", crate::sha::short(id), subject),
                At::OpenChange => "your open change".to_string(),
            };
            return Err(Error::coded(
                "fold/conflict",
                format!(
                    "{source} would conflict with {target} at {where_held} ({}); nothing was \
                     folded",
                    hold.held.paths.join(", ")
                ),
                vec![format!("ff restack --onto {target}"), "ff status".into()],
            ));
        }
        // The target's history already holds everything below the source:
        // the target moves to the source's tip, and nothing is rewritten.
        RestackPlan::Unchanged { .. } => {}
        RestackPlan::Replay(replay) if replay.new_tip == source_tip => {}
        RestackPlan::Replay(replay) => {
            new_tip = replay.new_tip;
            rewrites = replay.rewrites;
            dropped = replay.dropped;
            replayed = replay.replayed;
            published = replay.published;
            diverged = replay.diverged;
            open = replay.open;
            new_worktree = replay.new_worktree;
            cascade_plan = replay.cascade;
        }
    }

    // 10. The branches above sat on a branch that is going away, or that
    // will sit on the target with nothing of its own: every hold and every
    // report line that names the source as a base names the target instead,
    // so `ff resolve` on a held child replans onto a branch that exists.
    let source_ref = format!("refs/heads/{source}");
    let target_ref = format!("refs/heads/{target}");
    for hold in &mut cascade_plan.holds {
        if let Some(held) = &mut hold.new
            && let Intent::Restack { onto, .. } = &mut held.intent
            && *onto == source_ref
        {
            *onto = target_ref.clone();
        }
    }
    let report = &mut cascade_plan.report;
    for base in report
        .moved
        .iter_mut()
        .map(|m| &mut m.base)
        .chain(report.held.iter_mut().map(|h| &mut h.base))
        .chain(report.skipped.iter_mut().map(|s| &mut s.base))
    {
        if *base == source {
            *base = target.clone();
        }
    }

    let mut reaims = Vec::new();
    for child in cascade::Stack::read(repo)?.children(&source) {
        reaims.push(ParentTransition {
            branch: child.clone(),
            old: branchmeta::read(repo, child)?.parent,
            new: Some(target.clone()),
        });
    }
    let parked_source = stash::parked_entry(repo, &source)?;
    let advanced = crate::upstream::count_exclusive(repo, new_tip, &[onto.tip])?;

    Ok(FoldPlan {
        source,
        source_tip,
        onto,
        new_tip,
        rewrites,
        dropped,
        replayed,
        published,
        diverged,
        open,
        new_worktree,
        cascade: cascade_plan,
        reaims,
        parked_source,
        source_meta,
        target_meta,
        advanced,
        holder,
        stay,
    })
}

/// A branch whose hold has a resolution session open is being worked on
/// elsewhere: the way to it is a switch, not this verb.
fn refuse_if_resolving(repo: &gix::Repository, branch: &str) -> Result<()> {
    if let Some(open) = held::resolving(repo, branch)? {
        if open.session.is_empty() {
            return Err(held::predates_sessions(branch));
        }
        return Err(Error::coded(
            "held/resolving",
            format!("a resolution of {branch} is open on {}", open.session),
            vec![
                format!("ff switch {}", open.session),
                "ff resolve --abandon".into(),
                "ff status".into(),
            ],
        ));
    }
    Ok(())
}

impl FoldPlan {
    /// The commits this operation keeps reachable for its own replay: every
    /// rewritten commit, both old tips, and the source's parked change. The
    /// cascade's ride on the cascade plan.
    fn own_pins(&self) -> Result<Vec<gix::ObjectId>> {
        let mut pins: Vec<gix::ObjectId> = self
            .rewrites
            .iter()
            .map(|r| gix::ObjectId::from_hex(r.new.as_bytes()).map_err(Error::repo))
            .collect::<Result<_>>()?;
        pins.push(self.source_tip);
        pins.push(self.onto.tip);
        pins.extend(self.parked_source);
        Ok(pins)
    }

    /// The target's transition, absent when the target does not move.
    fn target_transition(&self) -> Option<RefTransition> {
        (self.new_tip != self.onto.tip).then(|| RefTransition {
            name: format!("refs/heads/{}", self.onto.name),
            old: Some(self.onto.tip.to_string()),
            new: Some(self.new_tip.to_string()),
        })
    }

    /// The source's transition to the new tip, absent when it stands there.
    fn source_transition(&self) -> Option<RefTransition> {
        (self.new_tip != self.source_tip).then(|| RefTransition {
            name: format!("refs/heads/{}", self.source),
            old: Some(self.source_tip.to_string()),
            new: Some(self.new_tip.to_string()),
        })
    }

    /// The update edit for one transition, expecting exactly its old value.
    fn edit_for(t: &RefTransition, reflog_msg: &str) -> Result<gix::refs::transaction::RefEdit> {
        let (Some(old), Some(new)) = (&t.old, &t.new) else {
            return Err(Error::msg("internal: a fold edit with no old or new value"));
        };
        refs::update_edit(
            &t.name,
            gix::ObjectId::from_hex(new.as_bytes()).map_err(Error::repo)?,
            gix::refs::transaction::PreviousValue::MustExistAndMatch(gix::refs::Target::Object(
                gix::ObjectId::from_hex(old.as_bytes()).map_err(Error::repo)?,
            )),
            reflog_msg,
        )
    }

    fn summary_suffix(&self) -> String {
        match self.cascade.report.moved.len() {
            0 => String::new(),
            n => format!(", and {n} above it"),
        }
    }

    /// Whether the replay moves this worktree: the open change and the tree
    /// it lands as are both planned, or neither is.
    fn worktree_move(&self) -> Option<(gix::ObjectId, gix::ObjectId)> {
        match (self.open, self.new_worktree) {
            (Some(open), Some(new)) => Some((open, new)),
            _ => None,
        }
    }

    /// The worktree, after the refs: index to the new tip, then the files
    /// from the open change to the change carried onto it. Returns how many
    /// files were written or deleted.
    fn move_worktree(&self, repo: &gix::Repository) -> Result<usize> {
        let Some((open, new_worktree)) = self.worktree_move() else {
            return Ok(0);
        };
        crate::index::write_index_for_tree(repo, futures::tree_of(repo, self.new_tip)?)?;
        let everything = |_: &str| true;
        let transition =
            crate::worktree::apply_tree_transition(repo, open, new_worktree, &everything)?;
        Ok(transition.written.len() + transition.deleted.len())
    }

    /// Drop the futures caches of everything the fold moved. Best-effort: a
    /// cache costs recomputation and nothing else.
    fn drop_caches(&self, repo: &gix::Repository) {
        let _ = futures::cache::remove(repo, &self.source);
        let _ = futures::cache::remove(repo, &self.onto.name);
        for t in &self.cascade.carried {
            if let Some(name) = t.name.strip_prefix("refs/heads/") {
                let _ = futures::cache::remove(repo, name);
            }
        }
    }

    /// The report, once the plan has landed.
    fn report(
        &self,
        repo: &gix::Repository,
        pre_tree: gix::ObjectId,
        files: usize,
        trash_ref: Option<String>,
        moved_tree: Option<MovedTree>,
    ) -> Result<FoldReport> {
        let new_tip_tree = futures::tree_of(repo, self.new_tip)?;
        // The target's parked change: disclosed, never applied over the
        // change that rode the fold.
        let parked = match stash::parked_entry(repo, &self.onto.name)? {
            Some(id) => {
                let sc = stash::read_stash_commit(repo, id)?;
                let applies =
                    futures::conflict_paths(repo, sc.base_tree, new_tip_tree, sc.wip_tree)?
                        .is_empty();
                Some(Parked {
                    stash: id.to_string(),
                    applies,
                })
            }
            None => None,
        };
        // What is still open is what the worktree holds once this lands,
        // against the commit it then sits on. Unmoved, the tree the preamble
        // captured stands on the same commit it stood on.
        let still_open = match self.new_worktree {
            Some(worktree) => worktree != new_tip_tree,
            None => pre_tree != new_tip_tree,
        };
        Ok(FoldReport {
            source: self.source.clone(),
            target: self.onto.name.clone(),
            anonymous: branch::is_anonymous(&self.source),
            stay: self.stay,
            replayed: self.replayed,
            advanced: self.advanced,
            old_tip: self.source_tip.to_string(),
            new_tip: self.new_tip.to_string(),
            dropped: self.dropped.clone(),
            diverged: self.diverged.clone(),
            published: self.published,
            published_on: rewrite::tracking_name(repo, &self.source)?,
            files,
            still_open,
            trash_ref,
            parked_demoted: if self.stay {
                None
            } else {
                self.parked_source.map(|id| id.to_string())
            },
            parked,
            reaimed: self.reaims.iter().map(|r| r.branch.clone()).collect(),
            cascade: self.cascade.report.clone(),
            moved_tree,
        })
    }
}

/// The committing half of a plain fold: the operation write-ahead, HEAD
/// onto the target, the refs in one transaction, the metadata, the
/// worktree, and the report.
fn commit_fold(
    repo: &gix::Repository,
    ctx: &verb::VerbContext,
    prov: &Provenance,
    argv: Vec<String>,
    plan: FoldPlan,
) -> Result<FoldReport> {
    let now = ctx.now;
    let source = plan.source.clone();
    let target = plan.onto.name.clone();
    let source_ref = format!("refs/heads/{source}");
    let target_ref = format!("refs/heads/{target}");
    let parked_ref = stash::parked_ref(&source);

    // 11. Write-ahead: the planned table is the post-fold world. HEAD on
    // the target, the target at the new tip, the source and its parked
    // change gone.
    let mut planned = observe_refs(repo)?;
    planned.head = format!("ref:{target_ref}");
    planned
        .refs
        .insert(target_ref.clone(), plan.new_tip.to_string());
    planned.refs.remove(&source_ref);
    if plan.parked_source.is_some() {
        planned.refs.remove(&parked_ref);
    }

    let mut record = OpRecord::new(
        "fold",
        format!("fold {source} into {target}{}", plan.summary_suffix()),
        now,
    );
    record.argv = argv;
    record.head = Some((format!("ref:{source_ref}"), format!("ref:{target_ref}")));
    record.refs.extend(plan.target_transition());
    record.refs.push(RefTransition {
        name: source_ref.clone(),
        old: Some(plan.source_tip.to_string()),
        new: None,
    });
    if let Some(parked) = plan.parked_source {
        record.refs.push(RefTransition {
            name: parked_ref.clone(),
            old: Some(parked.to_string()),
            new: None,
        });
    }
    record.rewrites = plan.rewrites.clone();
    record.dropped = plan.dropped.clone();
    record.inferred_parents = plan.reaims.clone();
    // The source's recorded base goes with its metadata; recorded so an undo
    // puts it back the way it stood.
    if plan.source_meta.parent.is_some() {
        record.parent = Some(ParentTransition {
            branch: source.clone(),
            old: plan.source_meta.parent.clone(),
            new: None,
        });
    }
    // The open change rides to the target, and its pending description and
    // id ride with it, the way a close moves them to the branch it lands on.
    if plan.source_meta.pending_description != plan.target_meta.pending_description {
        record.description = Some(DescriptionTransition {
            branch: target.clone(),
            old: plan.target_meta.pending_description.clone(),
            new: plan.source_meta.pending_description.clone(),
        });
    }
    if plan.source_meta.change_id != plan.target_meta.change_id {
        record.change_id = Some(ChangeIdTransition {
            branch: target.clone(),
            old: plan.target_meta.change_id.clone(),
            new: plan.source_meta.change_id.clone(),
        });
    }

    // The source's pointer into the log goes to trash below, and the record
    // says so, so an undo that brings the branch back brings its timeline
    // back with it.
    let snap_ref = format!("{}{source}", crate::ops::BRANCH_PREFIX);
    let trash = format!("refs/fufu/trash/{source}");
    let snap_tip = refs::ref_target(repo, &snap_ref)?;
    if let Some(tip) = snap_tip {
        record.pointers.push(PointerTransition {
            branch: source.clone(),
            from: snap_ref.clone(),
            to: trash.clone(),
            tip: tip.to_string(),
        });
    }

    let mut pins = plan.own_pins()?;
    plan.cascade.fold_into(&mut record, &mut planned, &mut pins);

    verb::append_op(
        repo,
        OpKind::Op,
        verb::VerbOp {
            record,
            planned,
            tree: plan.new_worktree.unwrap_or(ctx.pre_tree),
            index_tree: futures::tree_of(repo, plan.new_tip)?,
            branch: target.clone(),
            base: Some(plan.source_tip),
            session: prov.session.clone(),
            pins: &pins,
        },
        now,
    )?;

    // 12. Mutate.
    // 12.1 The source's pointer into the log goes to trash first: never lose
    // the way back.
    let mut trash_ref: Option<String> = None;
    if let Some(snap_tip) = snap_tip {
        refs::write_ref(
            repo,
            &trash,
            snap_tip,
            gix::refs::transaction::PreviousValue::Any,
            now,
            &format!("fold: pre-fold pointer of {source}"),
        )?;
        refs::delete_ref(repo, &snap_ref, snap_tip, now)?;
        trash_ref = Some(trash.clone());
    }

    // 12.2 HEAD onto the target, before the transaction that deletes the
    // branch HEAD stands on.
    branch::retarget_head(repo, &target_ref, now)?;

    // 12.3 The refs, in one transaction: the target's move, the source's
    // deletion, its parked change, and every branch above.
    let reflog_msg = format!("fold: {source} into {target}");
    let mut edits = Vec::new();
    if let Some(t) = plan.target_transition() {
        edits.push(FoldPlan::edit_for(&t, &reflog_msg)?);
    }
    edits.push(refs::delete_edit(&source_ref, plan.source_tip)?);
    if let Some(parked) = plan.parked_source {
        edits.push(refs::delete_edit(&parked_ref, parked)?);
    }
    edits.extend(plan.cascade.edits(&reflog_msg)?);
    match refs::commit_edits(repo, edits, now)? {
        refs::EditOutcome::Applied => {}
        refs::EditOutcome::Contended => {
            return Err(Error::coded(
                "ref/contended",
                "refs moved while folding; nothing was folded (re-run to fold onto the new tips)",
                vec![],
            ));
        }
    }

    // 12.4 Metadata: the target takes the open change's description and
    // id, each branch above records the target, the source's file goes,
    // and the cascade's holds land.
    let mut target_meta = plan.target_meta.clone();
    target_meta.pending_description = plan.source_meta.pending_description.clone();
    target_meta.change_id = plan.source_meta.change_id.clone();
    branchmeta::write(repo, &target, &target_meta)?;
    for reaim in &plan.reaims {
        let mut meta = branchmeta::read(repo, &reaim.branch)?;
        meta.parent = reaim.new.clone();
        branchmeta::write(repo, &reaim.branch, &meta)?;
    }
    branchmeta::write(repo, &source, &BranchMeta::default())?;
    plan.cascade.land(repo)?;

    // 12.5 The worktree, only when the replay moved HEAD's commit.
    let files = plan.move_worktree(repo)?;

    // 12.6 Caches.
    plan.drop_caches(repo);

    plan.report(repo, ctx.pre_tree, files, trash_ref, None)
}

/// The other worktree, opened, locked as a probe, reconciled and captured,
/// and checked to still stand where the plan found it, with its open change
/// merged over the new tip in memory.
fn prepare_other(
    repo: &gix::Repository,
    ctx: &verb::VerbContext,
    prov: &Provenance,
    holder: &Holder,
    plan: &FoldPlan,
) -> Result<OtherTree> {
    let path = holder.path.display().to_string();
    let target = &plan.onto.name;
    let other = if holder.id == linked::MAIN_ID {
        repo.main_repo().map_err(Error::repo)?
    } else {
        repo.worktrees()
            .map_err(Error::repo)?
            .into_iter()
            .find(|proxy| proxy.id() == holder.id)
            .ok_or_else(|| Error::msg(format!("internal: the worktree {} is gone", holder.id)))?
            .into_repo()
            .map_err(Error::repo)?
    };

    // The chain lock is taken inside every append, so this is a probe only:
    // a tree mid-command is refused before either chain is written.
    let contended = |what: String| Error::coded("ref/contended", what, vec![]);
    match crate::ops::lock::acquire_chain(repo, &holder.id, crate::ops::lock::Wait::Briefly)? {
        Some(guard) => drop(guard),
        None => {
            return Err(contended(format!(
                "something is running in {path}: its operation log is locked; nothing was written"
            )));
        }
    }
    if let Some(op) = crate::head::operation(&other) {
        return Err(Error::coded(
            "repo/mid-operation",
            format!("a {op:?} is in progress in {path}: finish it with git there first"),
            vec![],
        ));
    }
    let octx = verb::begin_verb(&other, prov, Some(ctx.now)).map_err(|err| {
        if err.id() == "ref/contended" {
            contended(format!(
                "something is running in {path}: its operation log is contended; nothing was \
                 written"
            ))
        } else {
            err
        }
    })?;
    match crate::head::head_state(&other)? {
        HeadState::Branch { name, commit, .. } if name == *target && commit == plan.onto.tip => {}
        _ => {
            return Err(contended(format!(
                "{path} moved off {target} while fold was planning; nothing was written"
            )));
        }
    }

    // Its open change over the new tip, in memory. The exact tree rather
    // than the capture's: the capture floor may have size-capped a file out.
    let old_tree = futures::tree_of(&other, plan.onto.tip)?;
    let new_tree = futures::tree_of(&other, plan.new_tip)?;
    let open = Overlay::default().open_tree(&other, old_tree)?;
    let merged = if open == old_tree {
        new_tree
    } else {
        let paths = futures::conflict_paths(repo, old_tree, new_tree, open)?;
        if !paths.is_empty() {
            return Err(Error::coded(
                "fold/other-tree-conflict",
                format!(
                    "{target}'s open change in {path} would conflict with the fold: {}; nothing \
                     was changed",
                    paths.join(", ")
                ),
                vec![
                    format!("ff fold {target}"),
                    "ff worktree".into(),
                    "ff status".into(),
                ],
            ));
        }
        let mut outcome = restack::merge_into(repo, old_tree, new_tree, open)?;
        outcome.tree.write().map_err(Error::repo)?.detach()
    };

    Ok(OtherTree {
        holder: holder.clone(),
        repo: other,
        ctx: octx,
        open,
        new_tree,
        merged,
    })
}

/// The committing half of `--stay`: the source survives at the new tip
/// with the target as its base, the target advances — in the other
/// worktree when one holds it, on its ref alone otherwise — and this
/// worktree stays where it is. With a holder, two records: this chain's
/// first, then the holder's naming it, then the mutations.
fn commit_fold_stay(
    repo: &gix::Repository,
    ctx: &verb::VerbContext,
    prov: &Provenance,
    argv: Vec<String>,
    plan: FoldPlan,
    other: Option<OtherTree>,
) -> Result<(FoldReport, Option<ReconcileReport>)> {
    let now = ctx.now;
    let source = plan.source.clone();
    let target = plan.onto.name.clone();
    let target_ref = format!("refs/heads/{target}");
    let here = linked::path::real(repo.workdir().expect("guarded above"))
        .display()
        .to_string();
    let here_chain = crate::ops::chain_id(repo);

    // 11. Write-ahead, this chain. The planned table already excludes a
    // target held elsewhere; without a holder the target is this tree's to
    // record.
    let mut planned = observe_refs(repo)?;
    if let Some(t) = plan.source_transition() {
        planned
            .refs
            .insert(t.name.clone(), plan.new_tip.to_string());
    }
    if other.is_none() {
        planned
            .refs
            .insert(target_ref.clone(), plan.new_tip.to_string());
    }
    let summary = match &other {
        Some(o) => format!(
            "fold {source} into {target} (in {}){}",
            o.holder.path.display(),
            plan.summary_suffix()
        ),
        None => format!("fold {source} into {target}{}", plan.summary_suffix()),
    };
    let mut record = OpRecord::new("fold", summary, now);
    record.argv = argv;
    record.refs.extend(plan.source_transition());
    if other.is_none() {
        record.refs.extend(plan.target_transition());
    }
    record.rewrites = plan.rewrites.clone();
    record.dropped = plan.dropped.clone();
    record.parent = Some(ParentTransition {
        branch: source.clone(),
        old: plan.source_meta.parent.clone(),
        new: Some(target.clone()),
    });
    record.inferred_parents = plan.reaims.clone();
    if let Some(o) = &other {
        record.companion = Some(Companion {
            chain: o.holder.id.clone(),
            path: o.holder.path.display().to_string(),
            op: None,
            text: format!(
                "the fold's other half is on {}'s chain ({}): {target} keeps the commits; ff \
                 undo there takes them back",
                o.holder.id,
                o.holder.path.display()
            ),
        });
    }
    let mut pins = plan.own_pins()?;
    plan.cascade.fold_into(&mut record, &mut planned, &mut pins);
    let here_op = verb::append_op(
        repo,
        OpKind::Op,
        verb::VerbOp {
            record,
            planned,
            tree: plan.new_worktree.unwrap_or(ctx.pre_tree),
            index_tree: futures::tree_of(repo, plan.new_tip)?,
            branch: source.clone(),
            base: Some(plan.source_tip),
            session: prov.session.clone(),
            pins: &pins,
        },
        now,
    )?;

    // 11b. Write-ahead, the holder's chain. Its table carries the target at
    // the new tip and every branch above at its new tip too: a branch above
    // is owned by nobody and sits in both tables, and a table that left it
    // behind would make that tree's next verb absorb the cascade as foreign.
    if let Some(o) = &other {
        let mut planned = observe_refs(&o.repo)?;
        planned
            .refs
            .insert(target_ref.clone(), plan.new_tip.to_string());
        for t in &plan.cascade.carried {
            if let Some(new) = &t.new {
                planned.refs.insert(t.name.clone(), new.clone());
            }
        }
        let mut record = OpRecord::new(
            "fold",
            format!(
                "fold: {target} advanced by {} commit(s) from {source} in {here}",
                plan.advanced
            ),
            now,
        );
        record.refs.extend(plan.target_transition());
        record.companion = Some(Companion {
            chain: here_chain.clone(),
            path: here.clone(),
            op: Some(here_op.to_string()),
            text: format!(
                "the fold's other half is on {here_chain}'s chain ({here}): {source} keeps \
                 sitting on the new tip; ff undo there puts it back at {}",
                crate::sha::short_oid(plan.source_tip)
            ),
        });
        verb::append_op(
            &o.repo,
            OpKind::Op,
            verb::VerbOp {
                record,
                planned,
                tree: o.merged,
                index_tree: o.new_tree,
                branch: target.clone(),
                base: Some(plan.onto.tip),
                session: prov.session.clone(),
                pins: &[plan.new_tip, plan.onto.tip],
            },
            now,
        )?;
    }

    // 12. Mutate.
    // 12.1 The refs, shared by every worktree, in one transaction.
    let reflog_msg = format!("fold: {source} into {target}");
    let mut edits = Vec::new();
    for t in plan
        .source_transition()
        .iter()
        .chain(plan.target_transition().iter())
    {
        edits.push(FoldPlan::edit_for(t, &reflog_msg)?);
    }
    edits.extend(plan.cascade.edits(&reflog_msg)?);
    match refs::commit_edits(repo, edits, now)? {
        refs::EditOutcome::Applied => {}
        refs::EditOutcome::Contended => {
            return Err(Error::coded(
                "ref/contended",
                "refs moved while folding; nothing was folded (re-run to fold onto the new tips)",
                vec![],
            ));
        }
    }

    // 12.2 Metadata: the source records the target, and so does every
    // branch that sat on it.
    let mut source_meta = plan.source_meta.clone();
    source_meta.parent = Some(target.clone());
    branchmeta::write(repo, &source, &source_meta)?;
    for reaim in &plan.reaims {
        let mut meta = branchmeta::read(repo, &reaim.branch)?;
        meta.parent = reaim.new.clone();
        branchmeta::write(repo, &reaim.branch, &meta)?;
    }
    plan.cascade.land(repo)?;

    // 12.3 The other worktree: index to the new tip, then its files from
    // its open change to that change carried over the tip.
    let mut moved_tree = None;
    let mut other_reconcile = None;
    if let Some(o) = other {
        crate::index::write_index_for_tree(&o.repo, o.new_tree)?;
        let everything = |_: &str| true;
        let transition =
            crate::worktree::apply_tree_transition(&o.repo, o.open, o.merged, &everything)?;
        moved_tree = Some(MovedTree {
            id: o.holder.id.clone(),
            path: o.holder.path.display().to_string(),
            files: transition.written.len() + transition.deleted.len(),
            still_open: o.merged != o.new_tree,
        });
        other_reconcile = Some(o.ctx.reconcile);
    }

    // 12.4 This worktree: HEAD stays on the source, and the files follow the
    // source to the new tip.
    let files = plan.move_worktree(repo)?;

    // 12.5 Caches.
    plan.drop_caches(repo);

    Ok((
        plan.report(repo, ctx.pre_tree, files, None, moved_tree)?,
        other_reconcile,
    ))
}
