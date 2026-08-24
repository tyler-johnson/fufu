//! Creating a linked worktree: fufu writes git's on-disk layout itself,
//! because gix has no worktree-creation API. The layout is byte-for-byte
//! what `git worktree add` would have written, and the checkout is the same
//! two calls every other fufu checkout uses.

use std::path::{Path, PathBuf};

use crate::error::{Error, Result};
use crate::ops::record::observe_refs;
use crate::ops::{OpKind, OpRecord, RefTransition, verb};

/// A linked worktree fufu created.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Created {
    /// The id git files it under, and the id its operation chain is keyed by.
    pub id: String,
    /// Where it was checked out, absolute.
    pub path: PathBuf,
    /// The branch it stands on, short form.
    pub branch: String,
    /// The commit that branch pointed at.
    pub head: gix::ObjectId,
}

/// Create a linked worktree at `path` standing on `branch`, which must
/// already exist — creating the branch is a later brief's job.
///
/// `now` is the unix-seconds timestamp of the reflog line.
pub fn create(repo: &gix::Repository, path: &Path, branch: &str, now: i64) -> Result<Created> {
    // Validate first, write second: nothing on disk changes until every check
    // has passed.
    let head = match crate::refs::ref_target(repo, &format!("refs/heads/{branch}"))? {
        Some(head) => head,
        None => {
            return Err(Error::coded(
                "branch/not-found",
                format!("no branch named {branch}"),
                vec!["ff branch list".into()],
            ));
        }
    };

    if let Some(holder) = crate::linked::holder_of(repo, branch)? {
        return Err(Error::coded(
            "branch/checked-out-elsewhere",
            format!(
                "'{branch}' is already used by worktree at '{}'",
                holder.path.display()
            ),
            vec!["git worktree list".into()],
        ));
    }

    destination_free(path)?;

    let common = repo.common_dir();
    let id = pick_id(path, common)?;
    let admin = common.join("worktrees").join(&id);
    let preexisted = path.is_dir();

    match write(repo, path, &id, &admin, branch, head, now) {
        Ok(created) => Ok(created),
        Err(err) => {
            // A failure partway must not leave a half layout, and a cleanup
            // failure must never mask the real error.
            let _ = std::fs::remove_dir_all(&admin);
            if !preexisted {
                let _ = std::fs::remove_dir_all(path);
            }
            Err(err)
        }
    }
}

/// Make a linked worktree as a recorded operation.
///
/// The earn over `git worktree add`, in two parts: the new worktree gets its
/// chain floor immediately, so `ff undo` works in it from its first command;
/// and the operation is appended to the chain of the worktree the command
/// ran in, where the undo for it belongs.
pub fn add_worktree(
    repo: &gix::Repository,
    path: &Path,
    branch: Option<&str>,
    prov: &crate::snapshot::Provenance,
    now: Option<i64>,
    argv: Vec<String>,
) -> Result<(
    crate::model::WorktreeAddReport,
    crate::ops::verb::VerbContext,
)> {
    let ctx = verb::begin_verb(repo, prov, now)?;
    let now = ctx.now;
    let head = crate::head::head_state(repo)?;
    let unborn = || {
        Error::coded(
            "worktree/unborn",
            "this repository has no commits yet, so there is nothing to check out",
            vec!["ff commit -m <message>".into()],
        )
    };

    // The branch is decided before the layout is written. A name that
    // resolves is used as-is; one that does not is validated and created at
    // HEAD; an unnamed destination takes the directory's name, or a minted
    // petname when that name would not stand as a branch or is already one.
    let named = branch.map(str::to_string);
    let (branch_name, created_branch) = if named.as_deref().is_some_and(|name| {
        crate::refs::ref_target(repo, &format!("refs/heads/{name}"))
            .is_ok_and(|target| target.is_some())
    }) {
        (named.expect("a name was offered"), false)
    } else {
        let name = match named {
            Some(name) => {
                crate::branch::validate_name(&name)?;
                name
            }
            None => match path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
            {
                Some(candidate)
                    if crate::branch::validate_name(&candidate).is_ok()
                        && crate::refs::ref_target(repo, &format!("refs/heads/{candidate}"))?
                            .is_none() =>
                {
                    candidate
                }
                _ => crate::petname::mint(repo)?,
            },
        };
        let at = crate::snapshot::chain::base_commit(&head)?.ok_or_else(unborn)?;
        crate::branch::create_at(repo, &name, at, now, "branch: created for a new worktree")?;
        (name, true)
    };

    // The primitive refuses a non-empty destination and a branch another
    // worktree holds, so neither check is repeated here.
    let created = create(repo, path, &branch_name, now)?;

    // Lay the new worktree's chain floor. Reconciliation is the existing
    // bootstrap: it writes the chain's first operation when there is no log.
    // A failure here does not unwind the checkout — the worktree exists and
    // works, and its first command would lay the floor anyway — so the error
    // is carried as a warning rather than discarding a checkout that already
    // succeeded.
    let mut warnings = Vec::new();
    let floor = repo
        .worktrees()
        .ok()
        .and_then(|proxies| proxies.into_iter().find(|proxy| proxy.id() == created.id))
        .and_then(|proxy| proxy.into_repo().ok())
        .map(|wt| verb::reconcile(&wt, now));
    if let Some(Err(err)) = floor {
        warnings.push(format!(
            "the new worktree's chain floor could not be laid: {err}"
        ));
    }

    // Write-ahead is inverted here on purpose: every other verb appends the
    // op before the work it names, but the effect records an id that `create`
    // picks, so the checkout has to happen first. A crash between the two
    // leaves a worktree git knows about and fufu did not record; the next
    // `ff worktree list` shows it as an ordinary row, because the layout is
    // git's own.
    let chain = crate::snapshot::chain::chain_name(&head);
    let mut planned = observe_refs(repo)?;
    let mut transitions = Vec::new();
    if created_branch {
        let full = format!("refs/heads/{branch_name}");
        planned.refs.insert(full.clone(), created.head.to_string());
        transitions.push(RefTransition {
            name: full,
            old: None,
            new: Some(created.head.to_string()),
        });
    }
    let mut record = OpRecord::new(
        "worktree",
        format!("add worktree {} on {}", created.id, branch_name),
        now,
    );
    record.argv = argv;
    record.refs = transitions;
    record.worktree = vec![crate::ops::record::WorktreeEffect::Add {
        id: created.id.clone(),
        path: created.path.display().to_string(),
        branch: branch_name.clone(),
    }];
    let pins = vec![created.head];
    verb::append_op(
        repo,
        OpKind::Op,
        verb::VerbOp {
            record,
            planned,
            // Adding a worktree leaves this worktree untouched.
            tree: ctx.pre_tree,
            index_tree: crate::index::tree_from_index(repo)?,
            branch: chain,
            base: crate::snapshot::chain::base_commit(&head)?,
            session: prov.session.clone(),
            pins: &pins,
        },
        now,
    )?;

    Ok((
        crate::model::WorktreeAddReport {
            id: created.id.clone(),
            path: created.path,
            branch: branch_name,
            created_branch,
            head: created.head.to_string(),
            chain: crate::ops::ops_ref(&created.id),
            warnings,
        },
        ctx,
    ))
}

/// The destination is free: no file, and no directory holding anything. An
/// existing empty directory is fine — git allows it, and so does fufu.
fn destination_free(path: &Path) -> Result<()> {
    let occupied = Error::coded(
        "worktree/exists",
        format!("{} already exists and is not empty", path.display()),
        vec![],
    );
    match std::fs::symlink_metadata(path) {
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(Error::repo(err)),
        Ok(md) if md.is_dir() => match std::fs::read_dir(path).map_err(Error::repo)?.next() {
            None => Ok(()),
            Some(Ok(_)) => Err(occupied),
            Some(Err(err)) => Err(Error::repo(err)),
        },
        Ok(_) => Err(occupied),
    }
}

/// The id is the destination's file name, with the smallest numeric suffix
/// from 1 when that name is `main` or already filed under.
fn pick_id(path: &Path, common: &Path) -> Result<String> {
    let base = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .ok_or_else(|| {
            Error::coded(
                "worktree/exists",
                format!("{} is not a directory fufu can name", path.display()),
                vec![],
            )
        })?;

    let worktrees = common.join("worktrees");
    if base != crate::linked::MAIN_ID && !worktrees.join(&base).exists() {
        return Ok(base);
    }
    let mut n = 1u64;
    loop {
        let candidate = format!("{base}{n}");
        if !worktrees.join(&candidate).exists() {
            return Ok(candidate);
        }
        n += 1;
    }
}

/// Write the layout, open the worktree through gix, and check the branch
/// out. The checkout goes through the new handle, never `repo` — passing
/// `repo` would overwrite the calling worktree with the branch's tree.
fn write(
    repo: &gix::Repository,
    path: &Path,
    id: &str,
    admin: &Path,
    branch: &str,
    head: gix::ObjectId,
    now: i64,
) -> Result<Created> {
    std::fs::create_dir_all(path).map_err(Error::repo)?;
    let dest = path.canonicalize().map_err(Error::repo)?;

    std::fs::create_dir_all(admin.join("logs")).map_err(Error::repo)?;
    std::fs::create_dir_all(admin.join("refs")).map_err(Error::repo)?;

    std::fs::write(admin.join("commondir"), "../..\n").map_err(Error::repo)?;
    std::fs::write(
        admin.join("gitdir"),
        format!("{}\n", dest.join(".git").display()),
    )
    .map_err(Error::repo)?;
    std::fs::write(admin.join("HEAD"), format!("ref: refs/heads/{branch}\n"))
        .map_err(Error::repo)?;
    std::fs::write(admin.join("ORIG_HEAD"), format!("{head}\n")).map_err(Error::repo)?;
    std::fs::write(
        admin.join("logs").join("HEAD"),
        format!(
            "0000000000000000000000000000000000000000 {head} {name} <{email}> {now} +0000\tworktree: created from {branch}\n",
            name = crate::ops::FUFU_NAME,
            email = crate::ops::FUFU_EMAIL,
        ),
    )
    .map_err(Error::repo)?;
    // The index is written by `write_index_for_tree` below, at the new
    // handle's `index_path()`, which resolves to this path.
    std::fs::write(dest.join(".git"), format!("gitdir: {}\n", admin.display()))
        .map_err(Error::repo)?;

    // Open the new worktree through gix. A successful open is itself proof
    // the layout is well-formed, and it inherits no GIT_DIR from the
    // environment the way discovering from the path would.
    let wt = repo
        .worktrees()
        .map_err(Error::repo)?
        .into_iter()
        .find(|proxy| proxy.id() == id)
        .ok_or_else(|| Error::msg(format!("the layout fufu just wrote does not open as {id}")))?
        .into_repo()
        .map_err(Error::repo)?;

    let tree = repo
        .find_commit(head)
        .map_err(Error::repo)?
        .tree_id()
        .map_err(Error::repo)?
        .detach();
    crate::index::write_index_for_tree(&wt, tree)?;
    let everything = |_: &str| true;
    crate::worktree::apply_tree_transition(
        &wt,
        gix::ObjectId::empty_tree(repo.object_hash()),
        tree,
        &everything,
    )?;

    Ok(Created {
        id: id.to_string(),
        path: dest,
        branch: branch.to_string(),
        head,
    })
}
