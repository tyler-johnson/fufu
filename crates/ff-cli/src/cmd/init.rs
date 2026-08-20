//! `ff init` — start a repository with the net already on.
//!
//! Every other verb opens with `ff_core::discover(".")`, which is the honest
//! shape for everything that acts on a repository that exists. The two
//! moments before one does are the two moments fufu had nothing to say, and
//! a new user's first command lands in exactly one of them.
//!
//! The earn: `git init` leaves a repository whose operation log begins
//! whenever some later ff verb happens to take a floor, and whose gc guard is
//! written at that same unpredictable moment. This one leaves a repository
//! where `ff undo` already has a floor to land on and `gc` cannot expire the
//! namespace, from minute zero.
//!
//! Run inside a repository that already exists it means *turn fufu on here* —
//! the same work, reported honestly as already done when it was.
//!
//! It spawns nothing: `gix::init` needs no installation config, so the
//! zero-spawn proof covers this verb unchanged. `ff clone` is the one that
//! reaches outside the process, and says so.

use ff_core::{CaptureOutcome, Error, Provenance, Result};

use crate::ctx::Ctx;

/// What both verbs have when their work is done, and the only thing either
/// of them reports from — so the sentence about the net being on is written
/// once.
pub(crate) struct Armed {
    /// The branch the repository is on, born or not.
    pub branch: String,
    /// The floor `ff undo` can land on: the first operation the log holds.
    /// `None` only if another fufu process held the log at this instant,
    /// which is a fact about that race and not about this repository.
    pub floor: Option<String>,
}

/// Write the gc guard, lay the operation log's floor, and say what branch we
/// are on.
///
/// Order matters twice over. The guard goes first because it is what stops
/// `git gc` expiring the refs the floor is about to create — writing it
/// afterwards would leave a window, however short, where the thing being
/// built could be collected. And the floor goes last because there is nothing
/// to capture until the repository exists: every other verb captures *first*,
/// and these two structurally cannot.
///
/// The floor itself is `reconcile`'s parentless `init` note. A bare capture
/// would not do: on a clean tree with no previous operation it deliberately
/// writes nothing, which is right for a read command and is exactly the gap
/// this verb exists to close. The capture that follows is the ordinary one,
/// and it records something only when the tree has content the note does not
/// — which is the adopt case, a repository somebody has already been working
/// in.
pub(crate) fn arm(repo: &gix::Repository, prov: &Provenance) -> Result<Armed> {
    ff_core::snapshot::config::ensure_gc_config(repo)?;

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or_default();
    let mut reconciled = ff_core::ops::reconcile(repo, now)?;
    let entry = reconciled.entry.clone();
    // Bootstrapping the log is what these two verbs are *for*, so the notice
    // every other verb prints on finding none would say twice what the tail
    // says once. The rest of the report still goes out: warnings, and a log
    // that had to be parked and re-initialized, are news either way.
    reconciled.bootstrapped = false;
    crate::render::reconcile_notice(&reconciled);

    let captured = ff_core::capture(repo, prov)?;
    for warning in warnings(&captured) {
        eprintln!("ff: {warning}");
    }
    let floor = match &captured {
        CaptureOutcome::Created { id, .. } => Some(id.to_string()),
        CaptureOutcome::NoOp { tip, .. } => tip.as_ref().map(ToString::to_string),
        CaptureOutcome::Contended => None,
    }
    .or(entry);

    let head = ff_core::head_state(repo)?;
    Ok(Armed {
        branch: ff_core::snapshot::chain::chain_name(&head),
        floor,
    })
}

fn warnings(outcome: &CaptureOutcome) -> &[String] {
    match outcome {
        CaptureOutcome::Created { warnings, .. } | CaptureOutcome::NoOp { warnings, .. } => {
            warnings
        }
        CaptureOutcome::Contended => &[],
    }
}

/// The tail both verbs print: what was actually gained by starting here.
pub(crate) fn tail(colored: bool) {
    println!(
        "{}",
        crate::render::paint_dim(
            "the net is on: ff undo has a floor to land on, and every verb takes one first",
            colored
        )
    );
}

/// The working tree's path, resolved rather than echoed: `ff init` in the
/// current directory would otherwise report `"."`, which tells a script
/// nothing it did not already know.
pub(crate) fn workdir(repo: &gix::Repository) -> Option<String> {
    let dir = repo.workdir()?;
    Some(
        std::fs::canonicalize(dir)
            .unwrap_or_else(|_| dir.to_path_buf())
            .display()
            .to_string(),
    )
}

pub fn run(ctx: &Ctx, dir: Option<String>, bare: bool) -> Result<()> {
    // A bare repository has no working tree, so there is nothing for the
    // capture floor to hold and nothing `ff undo` could restore. Refused
    // rather than half-done, and answered with the spelling that works.
    if bare {
        return Err(Error::coded(
            "init/bare",
            "a bare repository has no working tree, so there is no floor for ff undo to \
             land on and nothing for a capture to hold",
            vec!["ff git init --bare".into()],
        ));
    }

    let dir = dir.unwrap_or_else(|| ".".to_string());
    let path = std::path::Path::new(&dir);

    // Two shapes, and which one this is decides the first line. `discover`
    // walks upward, so a subdirectory of a repository is an adopt too — which
    // is the right answer: fufu is turned on for the repository, not for the
    // directory the command was typed in.
    let (repo, created) = match ff_core::discover(path) {
        Ok(repo) => (repo, false),
        Err(Error::Discover(_)) => (
            // Honors `init.defaultBranch`, and defaults to `main` — which is
            // git's own behavior on any recent version.
            gix::init(path).map_err(|err| {
                Error::coded(
                    "init/failed",
                    format!("could not create a repository at {dir}: {err}"),
                    vec![],
                )
            })?,
            true,
        ),
        Err(err) => return Err(err),
    };

    if repo.workdir().is_none() {
        return Err(Error::coded(
            "repo/bare",
            "this is a bare repository, and there is no working tree for the capture \
             floor to hold",
            vec![],
        ));
    }

    crate::render::init_palette(&repo);
    let armed = arm(&repo, &crate::provenance::pre_ff(ctx))?;

    if ctx.json {
        let payload = serde_json::json!({
            "path": workdir(&repo),
            "branch": armed.branch,
            "created": created,
            "floor": armed.floor,
        });
        crate::machine::emit("init", &payload)?;
        return Ok(());
    }

    let colored = crate::pager::color_enabled();
    let line = if created {
        format!("initialized an empty repository on {}", armed.branch)
    } else {
        format!("already a git repository on {}", armed.branch)
    };
    println!("{}", crate::render::paint_ok(&line, colored));
    tail(colored);
    Ok(())
}
