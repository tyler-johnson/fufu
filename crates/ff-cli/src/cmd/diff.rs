//! `ff diff` — the patch layer's set half.
//!
//! One verb, three questions, one tree diff: bare, what the open change
//! will land; `-r`, a connected set's total patch, from what its root is
//! measured against to its head, where a merge at the root is measured the
//! way `ff show` measures it; `--from`/`--to`, the difference between two
//! points.
//! `ff show` is the other half — one revision, with its identity and
//! message as furniture above the patch. Every route here ends in
//! `tree_diff` with the same `DiffOptions`, so paths filter and hunks keep
//! their shape whichever two trees are in question.
//!
//! The depth is the view set's, shared with `ff show` and `ff log`: the
//! patch by default, `--stat` and `--name-only` the shorter forms, and `-U`
//! the context width around each change.

use std::io::Write as _;

use ff_core::gix::ObjectId;
use ff_core::revset::{Rev, Revset};
use ff_core::{Against, Error, Fallback, Result};

use crate::cmd::fileview::{self, Depth, FileView, Flags};
use crate::ctx::Ctx;

pub fn run(
    ctx: &Ctx,
    revisions: Option<String>,
    from: Option<String>,
    to: Option<String>,
    flags: Flags,
    paths: Vec<String>,
) -> Result<()> {
    // Load-bearing, not ceremonial: the open change is HEAD's tree against
    // the branch's *newest operation's* tree, so an edit made since the last
    // operation is invisible until something captures it. Without this line
    // `ff diff` would report a clean tree on a file you just wrote — the
    // same bug `ff op diff` carried until 3b7a7fca.
    let repo = ff_core::discover(".")?;
    // The positional is paths only: `main..HEAD` here is a revision that
    // wanted `-r`, and answering it with an empty patch reads as "no
    // changes". Refused before the tree walk.
    crate::cmd::paths::require(
        &repo,
        "diff",
        "takes paths in its positional, and revisions behind -r, --from, and --to",
        &paths,
        |_| {
            vec![
                "ff diff -r <revset>".into(),
                "ff diff --from <rev> --to <rev>".into(),
                "ff status".into(),
            ]
        },
    )?;
    // Coded rather than a clap conflict so the refusal carries its id and
    // exits under `--json` like every other one.
    if revisions.is_some() && (from.is_some() || to.is_some()) {
        return Err(Error::coded(
            "usage/bad-flags",
            "-r names a set and --from/--to name two points: a patch has one pair of ends, and which pair is not something fufu will rank for you",
            vec![
                "ff diff -r <revset>".into(),
                "ff diff --from <rev> --to <rev>".into(),
            ],
        ));
    }

    let view = FileView::resolve(flags, Depth::Patch)?;
    let opts = view.diff_options(paths);
    // The `-r` route measures its root through a memory handle: a merge at
    // the root is diffed from the auto-merge of its parents, and that tree
    // exists only in memory. The diff runs through the same handle so the
    // tree resolves; blobs still come from the real store.
    let memory = repo.clone().with_object_memory();
    // Only `-r` answers what the root was measured against: the bare form
    // and two points name their own ends.
    let mut against: Option<(Rev, Against)> = None;
    let (stat, from_id, to_rev) = match (revisions, from, to) {
        // The default is `change_diff` verbatim rather than the two-tree
        // route with `@^` and `@` filled in: `ff show`'s bare patch is
        // measured the same way, and its tests hold the two byte for byte.
        (None, None, None) => (
            ff_core::change_diff(&repo, &opts)?,
            ff_core::revset::resolve::open_commit(&repo)?,
            Rev::Open(None),
        ),
        (Some(src), _, _) => {
            let range = Revset::parse(&src)?.range(&repo)?;
            let (from_tree, from_id, measured) = parent_of(&memory, range.root)?;
            let to_tree = tree_of(&repo, range.head)?;
            against = Some((range.root, measured));
            (
                ff_core::tree_diff(&memory, from_tree, to_tree, &opts)?,
                from_id,
                range.head,
            )
        }
        (None, from, to) => {
            let point = |src: &str| -> Result<Rev> { Ok(Revset::parse(src)?.point(&repo)?.rev) };
            let from_rev = match from {
                Some(src) => point(&src)?,
                None => point("@^")?,
            };
            let to_rev = match to {
                Some(src) => point(&src)?,
                None => point("@")?,
            };
            let from_id = match from_rev {
                Rev::Open(_) => ff_core::revset::resolve::open_commit(&repo)?,
                Rev::Commit(id) => Some(id.object_id()),
            };
            let from_tree = tree_of(&repo, from_rev)?;
            let to_tree = tree_of(&repo, to_rev)?;
            (
                ff_core::tree_diff(&repo, from_tree, to_tree, &opts)?,
                from_id,
                to_rev,
            )
        }
    };

    if ctx.json {
        let end = |rev: Rev| match rev {
            Rev::Open(_) => serde_json::Value::String("@".into()),
            Rev::Commit(id) => serde_json::Value::String(id.object_id().to_string()),
        };
        let mut payload = serde_json::Map::new();
        payload.insert(
            "from".into(),
            serde_json::json!(from_id.map(|id| id.to_string())),
        );
        payload.insert("to".into(), end(to_rev));
        if let Some((_, against)) = &against {
            payload.insert("against".into(), serde_json::json!(against.word()));
        }
        for (key, value) in fileview::json_keys(&stat, view.depth) {
            payload.insert(key.into(), value);
        }
        return crate::machine::emit("diff", &serde_json::Value::Object(payload));
    }

    // A root measured against its first parent because the auto-merge could
    // not be made: one line on stderr, so the patch stays a patch and the
    // reader still learns what it was measured from. `ff show` says the same
    // in its header.
    if let (Some((Rev::Commit(root), Against::FirstParent(why))), Some(parent)) =
        (&against, from_id)
    {
        eprintln!(
            "ff: {} is a merge {}; measured against its first parent {}",
            ff_core::sha::short_oid(root.object_id()),
            fallback_words(why),
            ff_core::sha::short_oid(parent)
        );
    }

    crate::render::init_palette(&repo);
    let mut out = crate::pager::LogOut::new(&repo, ctx.json);
    let colored = out.colored();
    // A clean tree prints nothing under every depth, git's convention, and
    // so does an identical pair of trees: this verb's output is meant to be
    // piped into `git apply`, and prose in that stream is a bug for whatever
    // reads it. A path that exists but has no changes is the same empty
    // patch, exit 0: only a path that names nothing is refused.
    let result = write!(out, "{}", fileview::text(&stat, view.depth, colored));
    out.finish();
    result.map_err(Error::repo)
}

/// The tree a revision names: the open change's for `@`, a commit's own
/// otherwise.
fn tree_of(repo: &ff_core::gix::Repository, rev: Rev) -> Result<ObjectId> {
    match rev {
        Rev::Open(_) => ff_core::open_tree_id(repo),
        Rev::Commit(id) => Ok(repo
            .find_commit(id.object_id())
            .map_err(Error::repo)?
            .tree_id()
            .map_err(Error::repo)?
            .detach()),
    }
}

/// Why a merge was measured against its first parent, as the notice and
/// `ff show`'s header say it.
pub(crate) fn fallback_words(why: &Fallback) -> String {
    match why {
        Fallback::NoBase => "with no merge base between its parents".into(),
        Fallback::Conflicts(paths) => format!(
            "whose auto-merge conflicts in {} file{}",
            paths.len(),
            if paths.len() == 1 { "" } else { "s" }
        ),
    }
}

/// The tree below a range's root, the commit that tree belongs to, and what
/// the root was measured against: HEAD under the open change; under a
/// commit, `measure`'s rule, the one parent, the auto-merge of several, or
/// the first parent when the auto-merge could not be made, and the empty
/// tree with no commit under a root commit. An auto-merge tree is written
/// through `repo`, so a memory handle keeps it out of the store.
fn parent_of(
    repo: &ff_core::gix::Repository,
    root: Rev,
) -> Result<(ObjectId, Option<ObjectId>, Against)> {
    match root {
        Rev::Open(_) => Ok((
            repo.head_tree_id_or_empty().map_err(Error::repo)?.detach(),
            ff_core::revset::resolve::open_commit(repo)?,
            Against::Parent,
        )),
        Rev::Commit(id) => {
            let m = ff_core::measure(repo, id.object_id())?;
            Ok((m.tree, m.commit, m.against))
        }
    }
}
