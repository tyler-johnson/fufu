//! The chain: a held rewrite replayed all the way through, with its conflicts
//! carried forward as literal marker content rather than refused. Nothing is
//! committed — this walks trees only — but the trees and blobs it writes are
//! real, so `ff resolve` can check the last one out.

use std::collections::HashSet;

use gix::bstr::ByteSlice;

use super::markers::{CHAIN_OURS, OPENER, blocks};
use super::replay::{Change, Range, range_of, subject, tree_of};
use crate::error::{Error, Result};

/// One unresolved region standing in a tree, and the step that wrote it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Region {
    /// Index into the chain's `steps`.
    pub step: usize,
    pub path: String,
    /// The block's exact text — opening marker line through closing marker
    /// line, trailing newline included — so it can be found again verbatim in
    /// the tree the step produced and replaced there.
    pub block: String,
}

/// One step of a chain: a commit replayed onto everything before it, and
/// what it left behind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Step {
    /// The commit being replayed, full sha.
    pub old: String,
    pub subject: String,
    /// The tree this step produced — carrying markers, when it conflicted.
    pub tree: gix::ObjectId,
    /// Paths this step left unresolved regions in, sorted and deduped.
    pub paths: Vec<String>,
}

/// The commit a chain stopped before, and the marks it would have written
/// over. Two conflicts on one region do not nest — they interleave, and the
/// earlier block stops being findable — so the chain stops rather than write
/// a tangle nobody can unpick.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tangle {
    pub old: String,
    pub subject: String,
    pub path: String,
}

/// What a chain run produced: the replay simulated all the way through, with
/// unresolved regions carried forward as ordinary marker content rather than
/// stopping at the first one.
#[derive(Debug, Clone)]
pub struct Chain {
    /// The steps that ran, oldest-first.
    pub steps: Vec<Step>,
    /// The tree the last step produced, or the base's tree if none ran.
    pub tree: gix::ObjectId,
    /// Set when the chain stopped early.
    pub tangled: Option<Tangle>,
}

/// One resolved region, folded back into the step that wrote it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolution {
    /// Index into `Chain::steps`.
    pub step: usize,
    pub path: String,
    /// The block's exact text as the step wrote it.
    pub block: String,
    /// What replaces it.
    pub with: String,
}

/// Replay `target..tip` under `change` without stopping at a conflict: each
/// step merges against the previous step's *result*, unresolved regions
/// carried forward as literal marker content, and a conflict a later commit
/// resolves anyway vanishes along the way. `resolutions` are applied to the
/// step that owns them, after that step merges and before the next one
/// replays over it, which is what makes the whole stack land clean from edits
/// made once at the end.
pub fn chain(
    repo: &gix::Repository,
    target: gix::ObjectId,
    tip: gix::ObjectId,
    change: &Change,
    resolutions: &[Resolution],
) -> Result<Chain> {
    let Range { ordered, affected } = range_of(repo, target, tip)?;
    // The full size of the stack, even when the chain stops early: the label
    // tells the reader the size of the stack, not the size of this attempt.
    let n = ordered.iter().filter(|&&id| affected.contains(&id)).count();

    let start_cursor = match change {
        Change::Onto(onto) => tree_of(repo, *onto)?,
        Change::Tree { tree, .. } => *tree,
        // A reword moves no tree, so the cursor never stands in for a tree;
        // the empty tree only matters if no step runs at all.
        Change::Message(_) => gix::ObjectId::empty_tree(repo.object_hash()),
    };

    let mut cursor = start_cursor;
    let mut steps: Vec<Step> = Vec::new();
    let mut reported: HashSet<String> = HashSet::new();
    let mut tangled: Option<Tangle> = None;

    for &id in &ordered {
        if !affected.contains(&id) {
            continue;
        }
        let k = steps.len() + 1;
        let subject = subject(repo, id)?;

        // This step's tree and the regions it left unresolved. The target
        // under a tree change takes its new tree directly (no merge); a reword
        // carries every commit's own tree; everything else replays the commit
        // onto the cursor — the previous step's result.
        let (merged_tree, paths) = if id == target {
            match change {
                // The caller folded something into the target and handed
                // the result over. It can already carry marks — an absorb
                // whose fold conflicted hands over exactly that — so the
                // paths it changed are scanned like any other step's, or the
                // region would stand in every later tree with no step
                // claiming it.
                Change::Tree { tree, .. } => {
                    (*tree, marked_paths(repo, tree_of(repo, id)?, *tree)?)
                }
                Change::Message(_) => (tree_of(repo, id)?, Vec::new()),
                Change::Onto(onto) => {
                    let base = old_first_parent_tree(repo, id)?;
                    merged(repo, id, base, tree_of(repo, *onto)?, k, n, &subject)?
                }
            }
        } else {
            match change {
                Change::Message(_) => (tree_of(repo, id)?, Vec::new()),
                Change::Tree { .. } | Change::Onto(_) => {
                    let base = old_first_parent_tree(repo, id)?;
                    merged(repo, id, base, cursor, k, n, &subject)?
                }
            }
        };

        // Tangle check: every path any step so far reported unresolved — this
        // step's, plus every earlier one's — that still stands in this step's
        // tree must parse as clean fufu blocks. A fresh single conflict is
        // clean; a second one on the same region interleaves and is not.
        let mut to_check: Vec<String> = reported.iter().cloned().collect();
        to_check.extend(paths.iter().cloned());
        to_check.sort();
        to_check.dedup();
        let mut first_tangled: Option<&String> = None;
        for path in &to_check {
            if let Some(blob) = blob_of(repo, merged_tree, path)?
                && blocks(&blob).1
                && first_tangled.is_none()
            {
                first_tangled = Some(path);
            }
        }
        if let Some(path) = first_tangled {
            tangled = Some(Tangle {
                old: id.to_string(),
                subject,
                path: path.clone(),
            });
            break; // discard this step; the chain stops before it
        }

        // Fold this step's resolutions in, threading the tree, so the step's
        // stored tree is the one a landing pass can take straight.
        let idx = steps.len();
        let final_tree = apply_resolutions(repo, merged_tree, idx, resolutions)?;
        reported.extend(paths.iter().cloned());
        steps.push(Step {
            old: id.to_string(),
            subject,
            tree: final_tree,
            paths,
        });
        cursor = final_tree;
    }

    let tree = steps.last().map(|s| s.tree).unwrap_or(start_cursor);
    Ok(Chain {
        steps,
        tree,
        tangled,
    })
}

/// The first commit a rewrite cannot replay, and what stopped it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Conflict {
    /// The commit the replay stopped on. Always `At::Commit` from here — the
    /// open change is not a commit and no caller finds it by replaying one.
    pub at: crate::futures::At,
    pub paths: Vec<String>,
    /// How many commits the rewrite would have replayed in all, so a report
    /// can say "1 of 5" rather than leave the size of the stack unsaid.
    pub of: usize,
}

/// The verdict half of `plan`: the same replay, run for what it would say
/// rather than for the objects it would write. `None` means `plan` will not
/// conflict either, so a verb that asks this first never has to catch the
/// engine's refusal.
pub fn conflict(
    repo: &gix::Repository,
    target: gix::ObjectId,
    tip: gix::ObjectId,
    change: &Change,
) -> Result<Option<Conflict>> {
    // A question costs no loose objects: the replay's trees and blobs are
    // written to a memory-backed store that is dropped with the answer.
    let memory = repo.clone().with_object_memory();
    let chain = chain(&memory, target, tip, change, &[])?;
    // The stopped chain still knows the commit it stopped before, and that
    // commit is part of the stack the report sizes.
    let of = chain.steps.len() + usize::from(chain.tangled.is_some());
    Ok(chain
        .steps
        .into_iter()
        .find(|step| !step.paths.is_empty())
        .map(|step| Conflict {
            at: crate::futures::At::Commit {
                id: step.old,
                subject: step.subject,
            },
            paths: step.paths,
            of,
        }))
}

/// The paths that differ between two trees and carry a fufu conflict block in
/// the second. Used where a tree arrives from outside the chain — a fold the
/// caller performed — so its marks are attributed rather than left ownerless.
fn marked_paths(
    repo: &gix::Repository,
    before: gix::ObjectId,
    after: gix::ObjectId,
) -> Result<Vec<String>> {
    if before == after {
        return Ok(Vec::new());
    }
    let mut changed: Vec<String> = Vec::new();
    let from = repo.find_tree(before).map_err(Error::repo)?;
    let to = repo.find_tree(after).map_err(Error::repo)?;
    from.changes()
        .map_err(Error::repo)?
        .for_each_to_obtain_tree(
            &to,
            |change| -> std::result::Result<_, std::convert::Infallible> {
                changed.push(change.location().to_string());
                Ok(gix::object::tree::diff::Action::Continue)
            },
        )
        .map_err(Error::repo)?;

    let mut marked: Vec<String> = Vec::new();
    for path in changed {
        if let Some(text) = blob_of(repo, after, &path)?
            && !blocks(&text).0.is_empty()
        {
            marked.push(path);
        }
    }
    marked.sort();
    marked.dedup();
    Ok(marked)
}

/// Every unresolved region standing in a chain's final tree, in path order
/// then in-file order, each tagged with the step that wrote it. This is the
/// list `ff resolve` shows and `ff done` attributes edits against.
pub fn regions(repo: &gix::Repository, chain: &Chain) -> Result<Vec<Region>> {
    let mut paths: Vec<String> = chain
        .steps
        .iter()
        .flat_map(|s| s.paths.iter().cloned())
        .collect();
    paths.sort();
    paths.dedup();

    let mut regions = Vec::new();
    for path in &paths {
        // A path some step conflicted on may have been resolved by a later
        // one; it simply yields no regions.
        let Some(blob) = blob_of(repo, chain.tree, path)? else {
            continue;
        };
        let (found, _tangled) = blocks(&blob);
        for block in found {
            regions.push(Region {
                step: block.step,
                path: path.clone(),
                block: block.text,
            });
        }
    }
    Ok(regions)
}

/// What a reader made of a resolution session, worked out per step.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attribution {
    /// Resolutions to hand straight back to `chain`. Never carries the last
    /// step: its tree is the resolved tree itself, so nothing about it needs
    /// attributing.
    pub resolutions: Vec<Resolution>,
    /// Regions the reader left alone, still carrying their markers.
    pub unresolved: Vec<Region>,
}

/// Work out which step each edit belongs to.
///
/// `chain.tree` is what `ff resolve` laid down; `resolved` is what the reader
/// made of it. Every marked region carries its owning step in its closing
/// label, so an edit landing inside a region belongs to that step, and its
/// resolution is whatever text now stands where the region stood.
///
/// An edit touching no region belongs to the last step and is therefore not
/// returned at all — the marker tree *is* the post-rewrite tip's tree, so an
/// unmarked edit is an edit to the final state, and attributing it earlier
/// would put content into a commit the reader never looked at.
pub fn attribute(
    repo: &gix::Repository,
    chain: &Chain,
    resolved: gix::ObjectId,
) -> Result<Attribution> {
    let mut paths: Vec<String> = chain
        .steps
        .iter()
        .flat_map(|s| s.paths.iter().cloned())
        .collect();
    paths.sort();
    paths.dedup();

    // The last step's tree is the resolved tree itself, so a resolution aimed
    // at it is a no-op and is dropped on the floor rather than returned.
    // A chain that stopped at a tangle has no such step: its tree is the
    // prefix's, not the post-rewrite tip's, so nothing the reader wrote is a
    // final state and every region it carries has to be attributed for real.
    let last: Option<usize> = chain
        .tangled
        .is_none()
        .then(|| chain.steps.len().saturating_sub(1));

    let mut resolutions: Vec<Resolution> = Vec::new();
    let mut unresolved: Vec<Region> = Vec::new();

    for path in &paths {
        // A path some step marked but the marker tree holds no blob at has
        // nothing to attribute.
        let Some(before) = blob_of(repo, chain.tree, path)? else {
            continue;
        };
        // The path absent from `resolved` means the reader deleted the file:
        // the last step's tree carries the deletion, and no earlier step can
        // express it as a text replacement, so nothing is attributed.
        let Some(after) = blob_of(repo, resolved, path)? else {
            continue;
        };
        let (found, _tangled) = blocks(&before);
        if found.is_empty() {
            continue;
        }
        let hunks = line_hunks(&before, &after);

        for block in &found {
            // Expand the block's range to absorb any hunk it overlaps, until
            // it stops growing: an edit that spilled past a marker line is
            // still that region's edit. A hunk already inside the range
            // widens it not at all, so the loop only runs while the range
            // actually grows — that is what makes it terminate.
            let mut range = block.lines.clone();
            loop {
                let mut next = range.clone();
                for (b, _) in &hunks {
                    if b.start < next.end && next.start < b.end {
                        next.start = next.start.min(b.start);
                        next.end = next.end.max(b.end);
                    }
                }
                if next == range {
                    break;
                }
                range = next;
            }

            // Map the expanded range into the resolved blob. A hunk lying
            // entirely before the range shifts it by its net delta; a hunk
            // the expansion absorbed shifts the range's far end by its net
            // delta. Together they bound the range's image, so the image is a
            // valid half-open line span within `after`.
            let mut delta_before = 0i64;
            let mut delta_inside = 0i64;
            for (b, a) in &hunks {
                let delta = (a.end as i64) - (a.start as i64) - (b.end as i64) + (b.start as i64);
                if b.end <= range.start {
                    delta_before += delta;
                } else if b.start < range.end {
                    delta_inside += delta;
                }
            }
            let start = (range.start as i64) + delta_before;
            let end = (range.end as i64) + delta_before + delta_inside;

            // The resolution text is the resolved blob's lines over the mapped
            // range, taken byte-for-byte (newline terminators included), so it
            // can stand in for the region exactly as the reader left it.
            let after_lines: Vec<&str> = after.split_inclusive('\n').collect();
            let with: String = after_lines[start as usize..end as usize].concat();

            // A surviving opener means the region was not finished, and text
            // byte-identical to the block means nobody touched it: both are
            // still unresolved. Otherwise it is a resolution — unless it
            // belongs to the last step, in which case it is dropped.
            if with.contains(OPENER) || with == block.text {
                unresolved.push(Region {
                    step: block.step,
                    path: path.clone(),
                    block: block.text.clone(),
                });
            } else if Some(block.step) != last {
                resolutions.push(Resolution {
                    step: block.step,
                    path: path.clone(),
                    block: block.text.clone(),
                    with,
                });
            }
        }
    }

    resolutions.sort_by(|a, b| a.path.cmp(&b.path).then(a.step.cmp(&b.step)));
    unresolved.sort_by(|a, b| a.path.cmp(&b.path).then(a.step.cmp(&b.step)));
    Ok(Attribution {
        resolutions,
        unresolved,
    })
}

/// The line-diff hunks between two texts: each change as the half-open line
/// range removed from `before` and the half-open line range inserted into
/// `after`, in strictly increasing order. `gix::diff::blob` re-exports
/// imara-diff, and its `InternedInput` tokenizes a `&str` into lines, so
/// these are line ranges, not byte or word ranges.
fn line_hunks(before: &str, after: &str) -> Vec<(std::ops::Range<usize>, std::ops::Range<usize>)> {
    use gix::diff::blob::{Algorithm, intern::InternedInput};
    let input = InternedInput::new(before, after);
    let mut hunks: Vec<(std::ops::Range<usize>, std::ops::Range<usize>)> = Vec::new();
    gix::diff::blob::diff(
        Algorithm::Histogram,
        &input,
        |b: std::ops::Range<u32>, a: std::ops::Range<u32>| {
            hunks.push((
                b.start as usize..b.end as usize,
                a.start as usize..a.end as usize,
            ));
        },
    );
    hunks
}

/// One non-trivial step: the commit's tree replayed onto `ours`. When the two
/// agree, no merge runs and the commit's own tree is carried — the same
/// short-circuit `replayed_tree` takes. Otherwise the three-way merge runs
/// with the chain's attribution labels, and the unresolved regions are the
/// step's paths.
fn merged(
    repo: &gix::Repository,
    id: gix::ObjectId,
    base: gix::ObjectId,
    ours: gix::ObjectId,
    k: usize,
    n: usize,
    subject: &str,
) -> Result<(gix::ObjectId, Vec<String>)> {
    if base == ours {
        return Ok((tree_of(repo, id)?, Vec::new()));
    }
    let their = tree_of(repo, id)?;
    let options = repo.tree_merge_options().map_err(Error::repo)?;
    let (ours_label, theirs) = chain_labels(subject, k, n);
    let labels = gix::merge::blob::builtin_driver::text::Labels {
        ancestor: None,
        current: Some(ours_label.as_bytes().as_bstr()),
        other: Some(theirs.as_bytes().as_bstr()),
    };
    let mut outcome = repo
        .merge_trees(base, ours, their, labels, options)
        .map_err(Error::repo)?;
    let paths = crate::futures::unresolved(&outcome);
    let tree = outcome.tree.write().map_err(Error::repo)?.detach();
    Ok((tree, paths))
}

/// The pair of labels one chain step's merge writes, `(ours, theirs)`.
///
/// The theirs label is the whole of the attribution: it is the only thing that
/// survives into the tree saying which commit a marker block belongs to. A
/// subject containing a quote is written verbatim (not escaped) and is parsed
/// back by its outermost quotes — the first and the last on the line.
///
/// Shared, rather than private to `merged`, because a caller that folds a tree
/// of its own and hands the result to `chain` — an absorb, whose fold IS step
/// one — has to write the same labels: `blocks` only sees a block whose closer
/// carries a step, and a block nobody can attribute is a block that lands in a
/// commit.
pub(crate) fn chain_labels(subject: &str, k: usize, n: usize) -> (String, String) {
    (
        format!("{CHAIN_OURS} ({k}/{n})"),
        format!("rebasing \"{subject}\" ({k}/{n})"),
    )
}

/// How many commits a rewrite of `target..tip` replays — the `n` a chain
/// label's `(k/n)` counts against.
pub(crate) fn stack_size(
    repo: &gix::Repository,
    target: gix::ObjectId,
    tip: gix::ObjectId,
) -> Result<usize> {
    let Range { ordered, affected } = range_of(repo, target, tip)?;
    Ok(ordered.iter().filter(|&&id| affected.contains(&id)).count())
}

/// The tree of a commit's old first parent, or the empty tree for a root.
fn old_first_parent_tree(repo: &gix::Repository, id: gix::ObjectId) -> Result<gix::ObjectId> {
    let obj = repo.find_object(id).map_err(Error::repo)?;
    let commit_ref = gix::objs::CommitRef::from_bytes(&obj.data).map_err(Error::repo)?;
    match commit_ref.parents.first() {
        Some(hex) => {
            let parent = gix::ObjectId::from_hex(hex).map_err(Error::repo)?;
            tree_of(repo, parent)
        }
        None => Ok(gix::ObjectId::empty_tree(repo.object_hash())),
    }
}

/// Fold every resolution that belongs to step `idx` into `tree`, one at a
/// time, threading the tree. A resolution whose block is not in the blob is
/// an error: the caller handed the engine a resolution the engine cannot
/// honor, and silently ignoring it would land the wrong content.
fn apply_resolutions(
    repo: &gix::Repository,
    tree: gix::ObjectId,
    idx: usize,
    resolutions: &[Resolution],
) -> Result<gix::ObjectId> {
    let mut tree = tree;
    for res in resolutions.iter().filter(|r| r.step == idx) {
        let Some(blob) = blob_of(repo, tree, &res.path)? else {
            return Err(Error::msg(format!(
                "no such path to resolve at {}: the step's tree has no file there",
                res.path
            )));
        };
        if !blob.contains(&res.block) {
            return Err(Error::msg(format!(
                "no marker block to resolve at {}: the resolution does not match the tree",
                res.path
            )));
        }
        let updated = blob.replacen(&res.block, &res.with, 1);
        let new_blob = repo
            .write_object(gix::objs::Blob {
                data: updated.into_bytes(),
            })
            .map_err(Error::repo)?
            .detach();
        let kind = entry_kind(repo, tree, &res.path)?;
        let mut editor = repo.edit_tree(tree).map_err(Error::repo)?;
        editor
            .upsert(res.path.as_str(), kind, new_blob)
            .map_err(Error::repo)?;
        tree = editor.write().map_err(Error::repo)?.detach();
    }
    Ok(tree)
}

/// The kind of the entry at `path` in `tree`, so an amended blob keeps its
/// mode — an executable file does not lose its bit.
fn entry_kind(
    repo: &gix::Repository,
    tree: gix::ObjectId,
    path: &str,
) -> Result<gix::objs::tree::EntryKind> {
    let tree = repo.find_tree(tree).map_err(Error::repo)?;
    let entry = tree
        .lookup_entry_by_path(path)
        .map_err(Error::repo)?
        .ok_or_else(|| Error::msg(format!("no entry at {path} in the tree")))?;
    Ok(entry.mode().kind())
}

/// The text of the blob at `path` in `tree`, or `None` when the path is not a
/// plain file in that tree.
fn blob_of(repo: &gix::Repository, tree: gix::ObjectId, path: &str) -> Result<Option<String>> {
    let tree = repo.find_tree(tree).map_err(Error::repo)?;
    let Some(entry) = tree.lookup_entry_by_path(path).map_err(Error::repo)? else {
        return Ok(None);
    };
    let kind = entry.mode().kind();
    if kind != gix::objs::tree::EntryKind::Blob
        && kind != gix::objs::tree::EntryKind::BlobExecutable
    {
        return Ok(None);
    }
    let blob = repo.find_blob(entry.id().detach()).map_err(Error::repo)?;
    Ok(Some(String::from_utf8_lossy(&blob.data).into_owned()))
}

/// Whether the blob at `path` in `tree` carries a fufu opener line. The
/// check `ff done` runs over a landed chain's steps to catch a fix that
/// created a conflict further up the stack.
pub(crate) fn carries_markers(
    repo: &gix::Repository,
    tree: gix::ObjectId,
    path: &str,
) -> Result<bool> {
    Ok(matches!(
        blob_of(repo, tree, path)?,
        Some(text) if text.contains(OPENER)
    ))
}
