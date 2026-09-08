use gix::traverse::commit::simple::{CommitTimeOrder, Sorting};

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
///
/// The walk reads commits through [`Grafted`] rather than the store itself,
/// so a shallow clone's boundary holds on both sides of the range: gix's
/// own shallow handling is a filter on the commits a walk may return, and
/// a hidden base's parents are queued without asking it, so painting the
/// base's ancestry would go on past the boundary to a parent the clone
/// never fetched and fail there.
pub(crate) fn range<'repo>(
    repo: &'repo gix::Repository,
    tip: gix::ObjectId,
    bases: impl IntoIterator<Item = gix::ObjectId>,
) -> Result<
    impl Iterator<
        Item = std::result::Result<
            gix::traverse::commit::Info,
            gix::traverse::commit::simple::Error,
        >,
    > + 'repo,
> {
    let objects = Grafted {
        objects: &repo.objects,
        shallow: repo.shallow_commits().map_err(Error::repo)?,
    };
    gix::traverse::commit::Simple::new(Some(tip), objects)
        .sorting(Sorting::ByCommitTime(CommitTimeOrder::NewestFirst))
        .map_err(Error::repo)?
        .hide(bases)
        .map_err(Error::repo)
}

/// The object store with every shallow commit shown as parentless, which is
/// what git itself does at a shallow boundary: the commit still names its
/// parents, and the clone has none of them.
struct Grafted<'repo> {
    objects: &'repo gix::OdbHandle,
    /// The `shallow` file, sorted; `None` in a clone with full history.
    shallow: Option<gix::shallow::Commits>,
}

impl gix::objs::Find for Grafted<'_> {
    fn try_find<'a>(
        &self,
        id: &gix::oid,
        buffer: &'a mut Vec<u8>,
    ) -> std::result::Result<Option<gix::objs::Data<'a>>, gix::objs::find::Error> {
        let kind = match self.objects.try_find(id, buffer)? {
            Some(data) => data.kind,
            None => return Ok(None),
        };
        let grafted = kind == gix::objs::Kind::Commit
            && self
                .shallow
                .as_ref()
                .is_some_and(|shallow| shallow.binary_search(&id.to_owned()).is_ok());
        if grafted {
            strip_parents(buffer);
        }
        Ok(Some(gix::objs::Data { kind, data: buffer }))
    }
}

/// Drop the `parent` lines from a commit's headers, which end at the first
/// blank line. A continuation line of a multi-line header starts with a
/// space, so it is never taken for one.
fn strip_parents(buffer: &mut Vec<u8>) {
    let headers_end = buffer
        .windows(2)
        .position(|pair| pair == b"\n\n")
        .map_or(buffer.len(), |at| at + 1);
    let mut kept = Vec::with_capacity(buffer.len());
    for line in buffer[..headers_end].split_inclusive(|&byte| byte == b'\n') {
        if !line.starts_with(b"parent ") {
            kept.extend_from_slice(line);
        }
    }
    kept.extend_from_slice(&buffer[headers_end..]);
    *buffer = kept;
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
