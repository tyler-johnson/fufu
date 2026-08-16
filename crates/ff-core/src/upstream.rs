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

/// Count commits reachable from `tip` without crossing any of `bases`.
pub(crate) fn count_exclusive(
    repo: &gix::Repository,
    tip: gix::ObjectId,
    bases: &[gix::ObjectId],
) -> Result<usize> {
    let walk = repo
        .rev_walk(Some(tip))
        .sorting(Sorting::ByCommitTime(Default::default()))
        .with_boundary(bases.iter().copied())
        .all()
        .map_err(Error::repo)?;
    let mut count = 0;
    for info in walk {
        info.map_err(Error::repo)?;
        count += 1;
    }
    Ok(count)
}
