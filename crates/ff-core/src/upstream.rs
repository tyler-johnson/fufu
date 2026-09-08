use gix::revision::walk::Sorting;

use crate::error::{Error, Result};
use crate::model::Upstream;

/// The upstream of the current branch, with ahead/behind counts, or `None` when
/// HEAD is detached or no upstream is configured.
pub fn upstream(repo: &gix::Repository) -> Result<Option<Upstream>> {
    let head = repo.head().map_err(Error::repo)?;
    let (ref_name, local_id) = match head.kind {
        gix::head::Kind::Symbolic(reference) => {
            let id = match reference.target.try_id() {
                Some(id) => id.to_owned(),
                None => repo
                    .find_reference(reference.name.as_ref())
                    .map_err(Error::repo)?
                    .peel_to_id_in_place()
                    .map_err(Error::repo)?
                    .detach(),
            };
            (reference.name, Some(id))
        }
        gix::head::Kind::Unborn(name) => (name, None),
        gix::head::Kind::Detached { .. } => return Ok(None),
    };
    upstream_for(repo, ref_name, local_id)
}

/// The upstream of an arbitrary branch ref.
pub(crate) fn upstream_for(
    repo: &gix::Repository,
    ref_name: gix::refs::FullName,
    local_id: Option<gix::ObjectId>,
) -> Result<Option<Upstream>> {
    let Some(tracking) =
        repo.branch_remote_tracking_ref_name(ref_name.as_ref(), gix::remote::Direction::Fetch)
    else {
        return Ok(None);
    };
    let tracking = tracking.map_err(Error::repo)?;
    let short = tracking.as_ref().shorten().to_string();

    let mut tracking_ref = match repo.find_reference(tracking.as_ref()) {
        Ok(r) => r,
        Err(gix::reference::find::existing::Error::NotFound { .. }) => {
            return Ok(Some(Upstream {
                r#ref: short,
                gone: true,
                ahead: 0,
                behind: 0,
            }));
        }
        Err(err) => return Err(Error::repo(err)),
    };
    let upstream_id = tracking_ref
        .peel_to_id_in_place()
        .map_err(Error::repo)?
        .detach();

    let Some(local_id) = local_id else {
        // Unborn branch with a live upstream: no commits to compare.
        return Ok(Some(Upstream {
            r#ref: short,
            gone: false,
            ahead: 0,
            behind: 0,
        }));
    };

    // All merge bases, not just the best one: with criss-cross histories a
    // single base over-counts both sides. No base (unrelated histories) leaves
    // the boundary empty, giving unbounded counts on each side — like git.
    let bases: Vec<gix::ObjectId> = repo
        .merge_bases_many(local_id, &[upstream_id])
        .map_err(Error::repo)?
        .into_iter()
        .map(|id| id.detach())
        .collect();

    let ahead = count_exclusive(repo, local_id, &bases)?;
    let behind = count_exclusive(repo, upstream_id, &bases)?;

    Ok(Some(Upstream {
        r#ref: short,
        gone: false,
        ahead,
        behind,
    }))
}

/// The range `tip ^bases`, newest first: every commit reachable from the tip
/// that no base reaches, and never a base itself — git's own meaning of a
/// range. Every range walk in the crate comes through here. The bases are
/// hidden rather than set as a boundary: gix's `with_boundary` stops where
/// a base is reached instead of painting what the base reaches, so a merge
/// that reached around the base let the base's own history through, and it
/// also installs a commit-time cutoff at the oldest base, which dropped any
/// commit dated older than the base — a teammate's morning commit pushed
/// after lunch, a cherry-pick of old work — as if it were not in the range
/// at all. Hiding answers by ancestry alone.
pub(crate) fn range<'repo>(
    repo: &'repo gix::Repository,
    tip: gix::ObjectId,
    bases: impl IntoIterator<Item = gix::ObjectId>,
) -> Result<gix::revision::Walk<'repo>> {
    repo.rev_walk(Some(tip))
        .with_hidden(bases)
        .sorting(Sorting::ByCommitTime(Default::default()))
        .all()
        .map_err(Error::repo)
}

/// The commits reachable from `tip` without crossing any of `bases`.
/// One walk, each excluded commit exactly once; callers do not depend
/// on the order they come out in.
pub(crate) fn exclusive(
    repo: &gix::Repository,
    tip: gix::ObjectId,
    bases: &[gix::ObjectId],
) -> Result<Vec<gix::ObjectId>> {
    let mut ids = Vec::new();
    for info in range(repo, tip, bases.iter().copied())? {
        ids.push(info.map_err(Error::repo)?.id);
    }
    Ok(ids)
}

/// Count commits reachable from `tip` without crossing any of `bases`.
pub(crate) fn count_exclusive(
    repo: &gix::Repository,
    tip: gix::ObjectId,
    bases: &[gix::ObjectId],
) -> Result<usize> {
    Ok(exclusive(repo, tip, bases)?.len())
}
