//! `ff describe` — edit the pending description: what the open change will
//! say when it closes. Two-phase descriptions: `ff new -m` writes it at the
//! open, bare `ff describe` edits it mid-flight, the close consumes it.
//! `ff describe -b <name>` renames the current branch instead (the one
//! rename that allows proper names). Reword-of-closed-revs is Phase 4.

use crate::branchmeta;
use crate::error::{Error, Result};
use crate::journal::{self, DescriptionTransition, OpKind, OpRecord};
use crate::model::DescribeReport;
use crate::snapshot::Provenance;

/// Set (or clear) the pending description of the current branch, journaled
/// as a slim entry so the change is undoable.
pub fn set_pending(
    repo: &gix::Repository,
    text: Option<String>,
    prov: &Provenance,
    now: Option<i64>,
    argv: Vec<String>,
) -> Result<(DescribeReport, journal::VerbContext)> {
    let ctx = journal::begin_verb(repo, prov, now)?;
    let now = ctx.now;
    let head = crate::head::head_state(repo)?;
    let branch = match &head {
        crate::model::HeadState::Branch { name, .. } => name.clone(),
        crate::model::HeadState::Unborn { r#ref } => r#ref
            .strip_prefix("refs/heads/")
            .unwrap_or(r#ref)
            .to_string(),
        crate::model::HeadState::Detached { .. } => {
            return Err(Error::coded(
                "repo/detached",
                "detached HEAD: there is no change to describe",
                vec!["ff switch <branch>".into()],
            ));
        }
    };
    let text = text
        .map(|t| t.trim_end().to_string())
        .filter(|t| !t.is_empty());

    let mut meta = branchmeta::read(repo, &branch)?;
    let old = meta.pending_description.clone();
    if old == text {
        return Ok((
            DescribeReport {
                branch,
                old,
                new: text,
            },
            ctx,
        ));
    }

    // Slim journal entry: no ref motion, just the description transition.
    let table = journal::observe_refs(repo)?;
    let mut record = OpRecord::new(
        OpKind::Op,
        "describe",
        match &text {
            Some(_) => format!("describe pending change on {branch}"),
            None => format!("clear pending description on {branch}"),
        },
        now,
    );
    record.argv = argv;
    record.branch = Some(branch.clone());
    record.pre_snapshot = ctx.pre_snapshot.clone();
    record.description = Some(DescriptionTransition {
        branch: branch.clone(),
        old: old.clone(),
        new: text.clone(),
    });
    let index_tree = crate::index::tree_from_index(repo)?;
    record.index_tree = Some(index_tree.to_string());
    journal::append(repo, &record, &table, index_tree, &[], now)?;

    meta.pending_description = text.clone();
    branchmeta::write(repo, &branch, &meta)?;

    Ok((
        DescribeReport {
            branch,
            old,
            new: text,
        },
        ctx,
    ))
}
