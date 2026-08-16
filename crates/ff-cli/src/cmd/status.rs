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
    // operation's ref table threaded through `head_state` and `ref_target`;
    // refuse before capturing, so a refused command writes nothing.
    ctx.refuse_past("ff status")?;
    crate::capture::pre_best_effort(&crate::provenance::pre_ff(ctx));
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
        },
    )?
    .entries
    .next()
    .transpose()?;

    // Lens map: open change id + parent id for display
    let mut ids: Vec<String> = Vec::new();
    if let Some(id) = &open.id {
        ids.push(id.clone());
    }
    if let Some(ref p) = parent {
        ids.push(p.id.clone());
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
