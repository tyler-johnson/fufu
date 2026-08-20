//! `ff describe` — edit the pending description: what the open change will
//! say when it closes. Two-phase descriptions: `ff new -m` writes it at the
//! open, bare `ff describe` edits it mid-flight, the close consumes it.
//! `ff describe -b <name>` renames the current branch instead (the one
//! rename that allows proper names). `ff describe <rev>` rewords a closed
//! commit and restacks what sits on top; the rewriting itself lives in
//! [`crate::rewrite`].

use crate::branchmeta;
use crate::close;
use crate::error::{Error, Result};
use crate::model::{DescribeReport, RewordReport};
use crate::ops::record::observe_refs;
use crate::ops::{DescriptionTransition, OpKind, OpRecord, verb};
use crate::refs;
use crate::rewrite;
use crate::snapshot::Provenance;

/// Set (or clear) the pending description of the current branch, recorded as
/// a slim operation so the change is undoable.
pub fn set_pending(
    repo: &gix::Repository,
    text: Option<String>,
    prov: &Provenance,
    now: Option<i64>,
    argv: Vec<String>,
) -> Result<(DescribeReport, verb::VerbContext)> {
    let ctx = verb::begin_verb(repo, prov, now)?;
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

    // A slim operation: no ref motion, just the description transition. Its
    // planned end state is the present on every axis but that one — describe
    // touches neither the working tree nor the index.
    let table = observe_refs(repo)?;
    let mut record = OpRecord::new(
        "describe",
        match &text {
            Some(_) => format!("describe pending change on {branch}"),
            None => format!("clear pending description on {branch}"),
        },
        now,
    );
    record.argv = argv;
    record.description = Some(DescriptionTransition {
        branch: branch.clone(),
        old: old.clone(),
        new: text.clone(),
    });
    verb::append_op(
        repo,
        OpKind::Op,
        verb::VerbOp {
            record,
            planned: table,
            tree: ctx.pre_tree,
            index_tree: crate::index::tree_from_index(repo)?,
            branch: branch.clone(),
            base: crate::snapshot::chain::base_commit(&head)?,
            session: prov.session.clone(),
            pins: &[],
        },
        now,
    )?;

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

/// Reword a closed commit and restack whatever sits on top of it, write-ahead
/// like every other verb: reconcile, capture, plan, append, then move refs.
pub fn reword(
    repo: &gix::Repository,
    target: gix::ObjectId,
    message: String,
    prov: &Provenance,
    now: Option<i64>,
    argv: Vec<String>,
) -> Result<(RewordReport, verb::VerbContext)> {
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

    let normalized = close::normalize_message(&message);
    if normalized.is_empty() {
        return Err(Error::coded(
            "usage/needs-message",
            "a reworded commit needs a description",
            vec!["ff describe <rev> -m <msg>".into()],
        ));
    }
    let subject = normalized
        .lines()
        .next()
        .unwrap_or("(no description)")
        .to_string();

    let ctx = verb::begin_verb(repo, prov, now)?;
    let now = ctx.now;

    let head = crate::head::head_state(repo)?;
    let (branch, tip) = match &head {
        crate::model::HeadState::Branch { name, commit, .. } => (
            name.clone(),
            gix::ObjectId::from_hex(commit.as_bytes()).map_err(Error::repo)?,
        ),
        crate::model::HeadState::Unborn { .. } => {
            return Err(Error::coded(
                "target/unresolvable",
                "nothing is committed yet: there is no revision to reword",
                vec!["ff commit -m <msg>".into()],
            ));
        }
        crate::model::HeadState::Detached { .. } => {
            return Err(Error::coded(
                "repo/detached",
                "detached HEAD: there is no branch to carry the rewrite",
                vec!["ff switch <branch>".into()],
            ));
        }
    };

    let plan = rewrite::plan(
        repo,
        target,
        tip,
        &rewrite::Change::Message(normalized),
        now,
    )?;

    // No-op short-circuit: the reword changed nothing (message already
    // normalized to what it was), so nothing is written and no operation is
    // appended.
    if plan.new_tip == tip {
        return Ok((
            RewordReport {
                branch,
                old: target.to_string(),
                new: target.to_string(),
                subject,
                restacked: 0,
                moved: Vec::new(),
                published: 0,
            },
            ctx,
        ));
    }

    let published = rewrite::published_count(repo, &branch, &plan)?;

    // Write-ahead: the planned table is the post-reword world. HEAD does not
    // move — it stays symbolic on the same branch.
    let mut planned = observe_refs(repo)?;
    for t in &plan.carried {
        if let Some(new) = &t.new {
            planned.refs.insert(t.name.clone(), new.clone());
        }
    }

    let target_short = crate::sha::short_oid(target);
    let mut record = OpRecord::new(
        "describe",
        format!("reword {target_short} on {branch}: {subject}"),
        now,
    );
    record.argv = argv;
    record.refs = plan.carried.clone();
    record.rewrites = plan.rewrites.clone();
    record.dropped = plan.dropped.clone();

    let mut pins: Vec<gix::ObjectId> = plan
        .rewrites
        .iter()
        .map(|r| gix::ObjectId::from_hex(r.new.as_bytes()).map_err(Error::repo))
        .collect::<Result<_>>()?;
    pins.push(tip);

    verb::append_op(
        repo,
        OpKind::Op,
        verb::VerbOp {
            record,
            planned,
            // The target's tree is unchanged by construction, so a reword
            // writes neither the worktree nor the index.
            tree: ctx.pre_tree,
            index_tree: crate::index::tree_from_index(repo)?,
            branch: branch.clone(),
            base: Some(tip),
            session: prov.session.clone(),
            pins: &pins,
        },
        now,
    )?;

    // Move the refs: one atomic transaction over every carried head.
    let reflog_msg = format!("describe: reword {target_short}");
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
                "refs moved while rewording; nothing was rewritten (re-run to reword on the new \
                 tips)",
                vec![],
            ));
        }
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

    Ok((
        RewordReport {
            branch,
            old: target.to_string(),
            // The target is first by construction (oldest-first, target
            // first).
            new: plan.rewrites[0].new.clone(),
            subject,
            restacked: plan.rewrites.len() - 1,
            moved,
            published,
        },
        ctx,
    ))
}
