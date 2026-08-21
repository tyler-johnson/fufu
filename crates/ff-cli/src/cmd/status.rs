use ff_core::{LogEntry, Result};

use crate::ctx::Ctx;

/// Raw foreign ref change tuple before conversion to the model's `ForeignEntry`.
type ForeignRefTuple = (String, Option<String>, Option<String>);

/// Everything `ff status` computed. Both renderings consume this and only
/// this, so neither can carry a fact the other lacks.
#[derive(serde::Serialize)]
pub struct StatusModel {
    pub head: ff_core::HeadState,
    pub operation: Option<ff_core::InProgress>,
    pub upstream: Option<ff_core::Upstream>,
    pub changes: Vec<ff_core::FileStat>,
    pub insertions: u32,
    pub deletions: u32,
    pub open: OpenStatus,
    pub parent: Option<ParentStatus>,
    pub conflicts: Vec<String>,
    pub foreign: Option<Vec<ForeignEntry>>,
    /// What syncing would cost, one entry per axis fufu can name: the base
    /// beneath this branch, and the remote copy of it.
    pub futures: ff_core::futures::Futures,
    /// An editing session running on the branch underfoot, if one is.
    pub session: Option<SessionStatus>,
    /// A rewrite held on the branch underfoot — a conflict that stopped a
    /// replay and is waiting for `ff resolve`.
    pub held: Option<HeldStatus>,
    /// A resolution open on the branch underfoot: the hold's conflicts are
    /// standing in the working tree right now, waiting for `ff done`.
    pub resolving: Option<ResolvingStatus>,
}

/// A rewrite held on the branch underfoot, as `ff status --json` spells it.
/// Carried, not recomputed: the count and the wording both come from what
/// the hold recorded, so a status render never replays a chain.
#[derive(serde::Serialize)]
pub struct HeldStatus {
    /// The verb that recorded the hold: restack, done, absorb or lift.
    pub verb: String,
    /// Where the replay stopped — the commit it could not reapply, or the
    /// open change — spelled the way a report spells it.
    pub at: ff_core::futures::At,
    /// The files the conflict stands in.
    pub paths: Vec<String>,
    /// Unix seconds when it was held.
    pub time: i64,
}

/// A resolution open on the branch underfoot, as `ff status --json` spells
/// it. The conflicts are counted off what the session recorded, not off the
/// working tree: a status render never runs a chain.
#[derive(serde::Serialize)]
pub struct ResolvingStatus {
    /// The verb whose rewrite the session is resolving.
    pub verb: String,
    /// The conflicts standing in the working tree.
    pub conflicts: usize,
    /// Each step's subject, oldest-first — what the session will land.
    pub steps: Vec<String>,
}

/// An editing session running on the branch underfoot.
#[derive(serde::Serialize)]
pub struct SessionStatus {
    /// The session branch — the one HEAD is on.
    pub branch: String,
    /// The commit being edited, full sha. It is that branch's own tip.
    pub editing: String,
    pub subject: String,
    /// The branch its commits will replay onto when the session ends.
    pub onto: String,
}

/// The open change as serialized by `ff status --json`. `base` and `time`
/// are carried, not skipped: the human view renders `time` as a relative age
/// and `base` as the born/unborn distinction, and one model means whatever a
/// person can read a script reads as data.
#[derive(serde::Serialize)]
pub struct OpenStatus {
    pub id: Option<String>,
    pub id_letters: Option<String>,
    pub pending: Option<String>,
    pub subject: Option<String>,
    pub clean: bool,
    pub base: Option<String>,
    /// Unix seconds of the newest snapshot; the age the human view prints.
    pub time: Option<i64>,
}

/// The parent commit as serialized by `ff status --json`.
#[derive(serde::Serialize)]
pub struct ParentStatus {
    pub id: String,
    pub subject: String,
    /// Unix seconds; `ff log --json` spells the same field the same way.
    pub time: i64,
    /// The commit's chain-segment anchor: the capture whose base and tree
    /// this commit repeats, which is the id `ff log` prints in that row's
    /// first column. `None` when no capture answers to it — an unborn
    /// repository, or history that arrived from outside fufu.
    pub segment: Option<String>,
}

/// One foreign ref change (reconciled motion outside fufu).
#[derive(serde::Serialize)]
pub struct ForeignEntry {
    pub r#ref: String,
    pub old: Option<String>,
    pub new: Option<String>,
}

pub fn run(ctx: &Ctx) -> Result<()> {
    // Rendering the two-row picture as of a past operation needs that
    // operation's ref table threaded through `head_state` and `ref_target` —
    // a past-state view that does not exist yet.
    ctx.refuse_past("ff status")?;
    run_inner(ctx)
}

/// The verb itself, capture already handled (or deliberately skipped).
pub fn run_inner(ctx: &Ctx) -> Result<()> {
    let mut repo = ff_core::discover(".")?;

    let status = ff_core::status(&repo)?;
    let open = ff_core::open_change(&repo)?;
    let change_stat = ff_core::change_stat(&repo)?;

    // Parent commit: fetch at most 1 log entry
    let parent: Option<LogEntry> = ff_core::log(
        &mut repo,
        &ff_core::LogOptions {
            limit: Some(1),
            revs: None,
            paths: Vec::new(),
        },
    )?
    .entries
    .next()
    .transpose()?;

    // The parent row's chain-segment anchor. `ff status` is `ff log` cropped
    // to two rows, so the column that names a commit's capture belongs on
    // this row too; a blank here was the crop dropping a column rather than
    // a considered silence. One anchor for one commit, against a walk the
    // full `ff log` already pays for a whole page.
    let parent_segment: Option<String> = match &parent {
        Some(p) => ff_core::segment_anchors(&repo, std::slice::from_ref(&p.id))?.remove(&p.id),
        None => None,
    };

    // Lens map: the ids actually on screen, and only those. Prefixes are
    // priced over the operation log, so the parent's commit sha never
    // resolved against it -- its anchor is the id that does, and the id the
    // row prints.
    let mut ids: Vec<String> = Vec::new();
    if let Some(id) = &open.id {
        ids.push(id.clone());
    }
    if let Some(anchor) = &parent_segment {
        ids.push(anchor.clone());
    }
    let lens = crate::cmd::evolog::displayed_prefix_lens(&repo, &ids)?;

    // Reconcile pinned (foreign changes)
    let foreign = reconcile_foreign(&repo);

    // Futures: what syncing this branch would cost, on both axes it answers
    // to. Detached HEAD and unborn branches have no branch tip to simulate
    // from, so they short-circuit without calling into futures at all.
    let no_futures = ff_core::futures::Futures {
        base: None,
        remote: None,
    };
    let futures = match &status.head {
        ff_core::HeadState::Branch { commit, .. } => {
            let compute = || -> ff_core::Result<ff_core::futures::Futures> {
                let branch = ff_core::snapshot::chain::chain_name(&status.head);
                let tip = ff_core::gix::ObjectId::from_hex(commit.as_bytes()).ok();
                let open = ff_core::futures::open_tree(&repo, &branch)?;
                ff_core::futures::futures_for(&repo, &branch, tip, open)
            };
            // Futures never fail a command: a simulation that cannot run is a
            // missing line, never a failed `ff status`.
            compute().unwrap_or(no_futures)
        }
        _ => no_futures,
    };

    // An editing session running on the branch underfoot. A lookup that
    // cannot run is a missing line, never a failed `ff status` — the same
    // rule the futures block a few lines above runs.
    let session = match &status.head {
        ff_core::HeadState::Branch { name, commit, .. } => {
            let compute = || -> ff_core::Result<Option<SessionStatus>> {
                let onto = ff_core::branchmeta::read(&repo, name)?
                    .session
                    .map(|s| s.onto)
                    .ok_or_else(|| ff_core::Error::msg("no editing session on this branch"))?;
                let tip = ff_core::gix::ObjectId::from_hex(commit.as_bytes())
                    .map_err(ff_core::Error::repo)?;
                let subject = repo
                    .find_object(tip)
                    .map_err(ff_core::Error::repo)?
                    .into_commit()
                    .message()
                    .map_err(ff_core::Error::repo)?
                    .summary()
                    .to_string();
                Ok(Some(SessionStatus {
                    branch: name.clone(),
                    editing: commit.clone(),
                    subject,
                    onto,
                }))
            };
            compute().unwrap_or(None)
        }
        _ => None,
    };

    // A rewrite held on the branch underfoot, and the resolution, if one is
    // open on it. Both read the branch's metadata and nothing else — the
    // counts are carried, so a status render never replays a chain — and
    // they follow the session block's rule exactly: a lookup that cannot
    // run is a missing line, never a failed `ff status`.
    let (held, resolving) = match &status.head {
        ff_core::HeadState::Branch { name, .. } => {
            let compute = || -> ff_core::Result<(Option<HeldStatus>, Option<ResolvingStatus>)> {
                let meta = ff_core::branchmeta::read(&repo, name)?;
                Ok((
                    meta.held.as_ref().map(|h| HeldStatus {
                        verb: verb_of(&h.intent).to_string(),
                        at: h.at.clone(),
                        paths: h.paths.clone(),
                        time: h.time,
                    }),
                    meta.resolving.as_ref().map(|r| ResolvingStatus {
                        verb: verb_of(&r.hold.intent).to_string(),
                        conflicts: r.hold.paths.len(),
                        steps: r.steps.clone(),
                    }),
                ))
            };
            compute().unwrap_or((None, None))
        }
        _ => (None, None),
    };

    // Build the single data model both renderers consume
    let id_letters = open.id.as_deref().map(ff_core::snapid::encode);
    let model = StatusModel {
        head: status.head.clone(),
        operation: status.operation,
        upstream: status.upstream.clone(),
        changes: change_stat.files.clone(),
        insertions: change_stat.insertions,
        deletions: change_stat.deletions,
        open: OpenStatus {
            id: open.id.clone(),
            id_letters,
            pending: open.pending.clone(),
            subject: open.subject.clone(),
            clean: open.clean,
            base: open.base.clone(),
            time: open.time,
        },
        parent: parent.as_ref().map(|p| ParentStatus {
            id: p.id.clone(),
            subject: p.subject.clone(),
            time: p.time,
            segment: parent_segment.clone(),
        }),
        conflicts: status.conflicts.clone(),
        foreign: foreign.map(|entries| {
            entries
                .into_iter()
                .map(|(name, old, new)| ForeignEntry {
                    r#ref: name,
                    old,
                    new,
                })
                .collect()
        }),
        futures,
        session,
        held,
        resolving,
    };

    let now = now_secs();
    let colored = crate::pager::color_enabled();

    if ctx.json {
        render_json(&model)?;
    } else {
        crate::render::init_palette(&repo);
        let view = crate::render::StatusView {
            model: &model,
            lens: &lens,
            now,
            colored,
        };
        print!("{}", crate::render::status_human(&view));
    }

    Ok(())
}

fn render_json(model: &StatusModel) -> Result<()> {
    crate::machine::emit("status", model)
}

/// The verb a hold was recorded by, the way a report names it.
fn verb_of(intent: &ff_core::held::Intent) -> &'static str {
    match intent {
        ff_core::held::Intent::Restack { .. } => "restack",
        ff_core::held::Intent::Done { .. } => "done",
        ff_core::held::Intent::Absorb { .. } => "absorb",
        ff_core::held::Intent::Lift { .. } => "lift",
    }
}

/// Reconcile (best-effort — status must never fail because the operation log
/// can't be written) and return foreign ref changes while the tip is foreign.
fn reconcile_foreign(repo: &ff_core::gix::Repository) -> Option<Vec<ForeignRefTuple>> {
    repo.workdir()?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    match ff_core::ops::reconcile(repo, now) {
        Ok(report) => {
            for warning in &report.warnings {
                eprintln!("ff: {warning}");
            }
        }
        Err(err) => {
            if std::env::var_os("FF_DEBUG").is_some() {
                eprintln!("ff[debug]: reconcile failed: {err}");
            }
            return None;
        }
    }
    let log = ff_core::ops::OpLog::open(repo).ok()?;
    let op = log.get(log.tip().ok().flatten()?).ok()?;
    if op.kind() != ff_core::ops::OpKind::Foreign {
        return None;
    }
    Some(
        op.record()
            .ok()
            .flatten()?
            .refs
            .iter()
            .map(|t| (t.name.clone(), t.old.clone(), t.new.clone()))
            .collect(),
    )
}

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}
