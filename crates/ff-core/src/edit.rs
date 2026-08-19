//! `ff edit <rev>` opens an editing session on a commit: it mints an
//! anonymous branch at the commit, switches to it, and records in that
//! branch's metadata which branch must replay onto it when the session ends.
//! The commit's content is then edited against its own tree, with the whole
//! toolchain pointed at it, and `ff done` ends the session.
//!
//! Travel happens in ref-space: HEAD and the working tree agree at every
//! moment, so plain git sees an ordinary branch with ordinary edits, and no
//! other verb has to ask whether a session is running.
//!
//! The branch you came from stays exactly where it stands, its commits
//! waiting ahead until `ff done`.

use gix::prelude::ObjectIdExt;

use crate::branch;
use crate::branchmeta;
use crate::error::{Error, Result};
use crate::model::{EditOutcome, EditReport, HeadState};
use crate::ops::record::{SessionTransition, observe_refs};
use crate::ops::{OpKind, OpRecord, RefTransition, verb};
use crate::revset::{Rev, Revset};
use crate::snapshot::Provenance;

/// A 7-hex-character-ish abbreviation, git's own minimal-unique-prefix
/// shortening with a fixed fallback.
fn short(repo: &gix::Repository, id: gix::ObjectId) -> String {
    id.attach(repo)
        .shorten()
        .map(|p| p.to_string())
        .unwrap_or_else(|_| id.to_string()[..7].to_string())
}

/// The subject of a commit, through the object handle — the raw `CommitRef`
/// message has no summary.
fn subject(repo: &gix::Repository, commit: gix::ObjectId) -> Result<String> {
    let commit = repo.find_object(commit).map_err(Error::repo)?.into_commit();
    Ok(commit.message().map_err(Error::repo)?.summary().to_string())
}

fn resolve_now(now: Option<i64>) -> i64 {
    now.unwrap_or_else(|| {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0)
    })
}

/// Open an editing session on the named commit. See the module docs.
pub fn edit(
    repo: &gix::Repository,
    rev: &str,
    prov: &Provenance,
    now: Option<i64>,
    argv: Vec<String>,
) -> Result<(EditOutcome, verb::VerbContext)> {
    if repo.workdir().is_none() {
        return Err(Error::coded(
            "repo/bare",
            "bare repository: nothing to edit",
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

    // One resolver, one precedence: the revset refuses an ambiguous,
    // unknown, or multi-member target with its own id.
    let point = Revset::parse(rev)?.point(repo)?;

    // A branch name is a kind mismatch, and the mismatch redirects rather
    // than refuses: `ff edit` targets commits, `ff switch` targets branches,
    // and the one available reading is taken and announced. Before the
    // session guard on purpose — `ff edit main` while a session is running
    // is the designed way to defer it.
    if let Some(branch) = point.name {
        let (report, ctx) = crate::switch::switch(
            repo,
            &crate::switch::SwitchOptions {
                target: branch,
                now,
                argv: argv.clone(),
            },
            prov,
        )?;
        return Ok((EditOutcome::Switched(report), ctx));
    }

    // Refused on the *resolved* revision rather than on the literal "@":
    // `latest(@)` and `heads(@)` were never a different request, and a check
    // on the spelling would have let them through.
    let at = match point.rev {
        Rev::Open => {
            return Err(Error::coded(
                "target/unresolvable",
                "@ is the open change, not a commit: it is already what you are editing",
                vec!["ff edit HEAD".into(), "ff log".into()],
            ));
        }
        Rev::Commit(id) => id.object_id(),
    };
    let at_short = short(repo, at);
    let at_subject = subject(repo, at)?;

    let head = crate::head::head_state(repo)?;
    let (current, current_tip) = match head {
        HeadState::Branch { name, commit, .. } => {
            let tip = gix::ObjectId::from_hex(commit.as_bytes()).map_err(Error::repo)?;
            (name, tip)
        }
        HeadState::Unborn { .. } => {
            return Err(Error::coded(
                "target/unresolvable",
                "nothing is committed yet: there is nothing to edit",
                vec!["ff commit -m <msg>".into()],
            ));
        }
        HeadState::Detached { .. } => {
            return Err(Error::coded(
                "repo/detached",
                "detached HEAD: an editing session needs a branch to return to",
                vec!["ff switch <branch>".into()],
            ));
        }
    };

    // The branch underfoot is itself an unfinished session. Nesting sessions
    // is an open design question; refusing is the honest answer until it is
    // settled.
    if branchmeta::read(repo, &current)?.session.is_some() {
        let tip_short = short(repo, current_tip);
        let tip_subject = subject(repo, current_tip)?;
        return Err(Error::coded(
            "session/open",
            format!(
                "already editing {tip_short} \"{tip_subject}\": finish or abandon that session \
                 before opening another"
            ),
            vec![
                "ff done".into(),
                "ff done --abandon".into(),
                "ff switch <branch>".into(),
            ],
        ));
    }

    // `at` is in `current`'s history iff it is its own merge base. The
    // session must replay onto *some* branch when it ends, and the branch
    // you are standing on is the one it will be — so a commit that branch
    // does not contain has nowhere to land.
    let bases: Vec<gix::ObjectId> = repo
        .merge_bases_many(at, &[current_tip])
        .map_err(Error::repo)?
        .into_iter()
        .map(|id| id.detach())
        .collect();
    if !bases.contains(&at) {
        return Err(Error::coded(
            "edit/not-in-history",
            format!(
                "{at_short} \"{at_subject}\" is not in {current}'s history: there is nothing to \
                 edit there"
            ),
            vec!["ff log".into(), "ff switch <branch>".into()],
        ));
    }

    // Ending this session will rewrite `current`, and finding that out now
    // beats finding it out after the work.
    branch::guard_other_worktrees(repo, &current)?;

    let ahead = crate::upstream::count_exclusive(repo, current_tip, &[at])?;

    let name = crate::petname::mint(repo)?;
    let at_string = at.to_string();

    mint_session(repo, &name, at, &current, resolve_now(now), &argv, prov)?;

    // Park the open change, retarget HEAD, and materialize the commit's tree
    // — `start` makes exactly this call for exactly this reason.
    let (switch_report, ctx) = crate::switch::switch(
        repo,
        &crate::switch::SwitchOptions {
            target: name.clone(),
            now,
            argv,
        },
        prov,
    )?;

    Ok((
        EditOutcome::Opened(EditReport {
            session: name,
            editing: at_string,
            subject: at_subject,
            onto: current,
            ahead,
            parked: switch_report.parked,
        }),
        ctx,
    ))
}

/// Mint the session branch at the commit, recorded, with the session written
/// into its metadata.
///
/// This runs BEFORE the switch that follows it, and therefore before any
/// preamble — so it reconciles nothing and captures nothing itself. That is
/// safe precisely because it is write-ahead: the planned table it records
/// already contains the branch it is about to create, so the switch's own
/// reconcile finds the world exactly where this operation said it would be.
fn mint_session(
    repo: &gix::Repository,
    name: &str,
    at: gix::ObjectId,
    onto: &str,
    now: i64,
    argv: &[String],
    prov: &Provenance,
) -> Result<()> {
    let at_short = short(repo, at);
    let session = branchmeta::Session {
        onto: onto.to_string(),
        at: at.to_string(),
    };
    let head = crate::head::head_state(repo)?;
    let mut planned = observe_refs(repo)?;
    planned
        .refs
        .insert(format!("refs/heads/{name}"), at.to_string());
    let mut record = OpRecord::new(
        "edit",
        format!("edit {at_short}: session {name} on {onto}"),
        now,
    );
    record.argv = argv.to_vec();
    record.refs = vec![RefTransition {
        name: format!("refs/heads/{name}"),
        old: None,
        new: Some(at.to_string()),
    }];
    record.edit_session = Some(SessionTransition {
        branch: name.to_string(),
        old: None,
        new: Some(session.clone()),
    });
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
        &format!("branch: editing session at {at_short}"),
    )?;
    branchmeta::write(
        repo,
        name,
        &branchmeta::BranchMeta {
            pending_description: None,
            forked_from: Some(at_short.to_string()),
            parent: None,
            session: Some(session),
            held: None,
            resolving: None,
        },
    )?;
    Ok(())
}
