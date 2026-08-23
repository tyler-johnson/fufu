//! The operation log — one chain at `refs/fufu/ops` that every capture and
//! every mutating verb appends to. **A snapshot is what an operation
//! carries.**
//!
//! Two logs was one too many. `refs/fufu/snap/<branch>` held snapshot
//! commits and `refs/fufu/journal` held op entries: two id schemes, two
//! prefix resolvers, two trim mechanisms, two trash refs, and a user asking
//! "how do I go back" meeting three concepts where two would do. They are
//! one log here, and the per-branch ref survives only as a *pointer* into
//! it, moved in the same transaction as the tip.
//!
//! The commit shape:
//!
//! ```text
//! op N        tree = the worktree at the END of this op
//!  ├─1─ op N-1              previous op        → first-parent walk = the op log
//!  ├─2─ base                HEAD's commit then → real history stays in the ancestry
//!  ├─3─ record N            tree = { op.json, refs, index/ }   (non-capture ops only)
//!  └─4… pins                shas the op's ref transitions touch
//! ```
//!
//! Slot 1 is reserved for the chain and is never anything else. The journal
//! used it as a pin on its first entry, which is why `git log --first-parent
//! refs/fufu/journal` ran off the root into the user's own history and kept
//! going. Base sits at a fixed slot 2, *behind* slot 1, so a capture's parents
//! are `[prev, base]` — byte-identical in shape to a snapshot commit, which is
//! what lets the existing decode rule and the existing differential assertion
//! carry over unchanged.
//!
//! The log's root therefore has no base parent at all: with no chain behind
//! it there is no slot 1 to sit behind, and putting the base there would
//! reproduce the journal's bug precisely. The root is the `init` note that
//! reconciliation lays down before the first capture, and it is parentless, so
//! `git log --first-parent refs/fufu/ops` ends on something fufu wrote.
//!
//! A capture carries no record commit. It changes no ref by invariant, so it
//! has no ref table to store and no index transition to pin, and everything
//! else it needs already lives in the message. That invariant is what makes
//! the merge storage-neutral instead of storage-doubling — 1584 captures
//! against 64 verb ops in this repository alone — and it is why the decoder
//! learns the kind from the message before deciding whether to fetch
//! anything. It also exempts a capture from the write-ahead protocol: a
//! capture records a fact that already happened.

pub mod append;
pub mod id;
pub mod index;
pub(crate) mod lock;
pub mod message;
pub mod record;
pub mod verb;
pub mod walk;

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

pub use append::{CaptureOutcome, capture, capture_with};
pub use id::{CommitId, OpId};
pub use message::SegmentLink;
pub use record::{
    DescriptionTransition, HeldTransition, OpRecord, ParentTransition, Published, RefTransition,
    RefsTable, ResolveTransition, SessionTransition, StashEffect,
};
pub use verb::{VerbContext, begin_verb, read_ops, read_ops_from, read_ops_of, reconcile};
pub use walk::{Operation, Run, is_op_commit, run_at};

use append::{Append, OpDraft};

/// Where every worktree's chain lives. One namespace, one chain per
/// worktree, all of them in the *shared* ref store rather than under
/// `refs/worktree/` — verified on git 2.50.1: `git gc` from the main
/// worktree collects objects pinned only by a linked worktree's
/// `refs/worktree/*`, and reachability is fufu's entire gc pin. The same
/// fact is what makes a chain outlive the worktree it belongs to, which is
/// the whole point of keeping it after `git worktree remove`.
pub const WT_PREFIX: &str = "refs/fufu/wt/";

/// The per-branch pointer into the log: the newest op on that branch.
///
/// Branch-keyed and therefore worktree-exclusive already, because git allows
/// a branch in at most one worktree — and [`crate::linked`] is what makes
/// fufu enforce that rather than assume it.
///
/// The namespace is deliberately the one the capture chain used, because it
/// means the same thing — the newest thing fufu wrote on this branch. A
/// repository that still holds pre-cutover chains there has them parked
/// under `refs/fufu/legacy/` before the log is created, so the first
/// invocation cannot CAS one of them away; see
/// [`verb::park_legacy`](crate::ops::verb::park_legacy).
pub const BRANCH_PREFIX: &str = "refs/fufu/snap/";

/// The pre-worktree names, kept only so [`verb::migrate`] can find them.
pub(crate) const LEGACY_OPS_REF: &str = "refs/fufu/ops";
pub(crate) const LEGACY_OPS_TRASH_REF: &str = "refs/fufu/trash/@ops";

/// The id of the chain this repository handle writes to.
///
/// The log was always worktree-scoped; with one worktree the repository and
/// the worktree were the same object, and the ref name said so. Two
/// worktrees sharing one chain share one undo pointer, which is a state git
/// itself refuses to create.
pub fn chain_id(repo: &gix::Repository) -> String {
    crate::linked::id(repo)
}

/// One chain's tip ref.
pub fn ops_ref(chain: &str) -> String {
    format!("{WT_PREFIX}{chain}/ops")
}

/// Where trim parks that chain's pre-trim tip — the last trim's own undo.
pub fn ops_trash_ref(chain: &str) -> String {
    format!("{WT_PREFIX}{chain}/trash/@ops")
}

/// This worktree's chain tip ref.
pub fn ops_ref_of(repo: &gix::Repository) -> String {
    ops_ref(&chain_id(repo))
}

/// This worktree's chain trash ref.
pub fn ops_trash_ref_of(repo: &gix::Repository) -> String {
    ops_trash_ref(&chain_id(repo))
}

/// Every chain id the repository holds refs for, sorted.
///
/// Wider than the set of live worktrees on purpose: a chain outlives the
/// worktree it belonged to, and the operations on it stay addressable —
/// `ff op show` and `ff restore --at-op` reach a dead worktree's captures
/// through the one global id space, with no new grammar to learn.
pub fn chain_ids(repo: &gix::Repository) -> Result<Vec<String>> {
    let platform = repo.references().map_err(Error::repo)?;
    let iter = platform.prefixed(WT_PREFIX).map_err(Error::repo)?;
    let mut out: Vec<String> = Vec::new();
    for reference in iter {
        let reference = reference.map_err(|err| {
            Error::coded(
                "op/unreadable",
                format!("ref iteration failed: {err}"),
                vec![],
            )
        })?;
        let name = reference.name().as_bstr().to_string();
        let Some(rest) = name.strip_prefix(WT_PREFIX) else {
            continue;
        };
        let Some((id, _)) = rest.split_once('/') else {
            continue;
        };
        if !out.iter().any(|seen| seen == id) {
            out.push(id.to_string());
        }
    }
    out.sort();
    Ok(out)
}

/// The fixed identity every op commit bears as both author and committer.
/// Unchanged from what snapshots carry today, so the cutover moves no bytes
/// on this axis.
pub const FUFU_NAME: &str = "fufu";
pub const FUFU_EMAIL: &str = "fufu@local";

/// Whether a commit was written by fufu. Necessary, never sufficient — a
/// record commit bears the identity too, and only [`is_op_commit`] tells the
/// two apart.
pub(crate) fn is_fufu_commit(commit: &gix::objs::CommitRef<'_>) -> bool {
    commit.author.name == FUFU_NAME
        && commit.author.email == FUFU_EMAIL
        && commit.committer.name == FUFU_NAME
        && commit.committer.email == FUFU_EMAIL
}

/// What an operation is. Kinds sort the log; they do not fork the model —
/// every operation has a tree, and that is what makes restore uniform.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OpKind {
    /// The tree alone, taken before every action at machine rate. Changes no
    /// ref, ever — the invariant the whole storage argument rests on.
    Capture,
    /// A fufu mutation, recorded write-ahead.
    Op,
    /// External motion absorbed by reconciliation.
    Foreign,
    /// A non-pinning marker (init, trim).
    Note,
}

impl OpKind {
    pub fn as_str(self) -> &'static str {
        match self {
            OpKind::Capture => "capture",
            OpKind::Op => "op",
            OpKind::Foreign => "foreign",
            OpKind::Note => "note",
        }
    }

    pub(crate) fn from_str(s: &str) -> Option<Self> {
        Some(match s {
            "capture" => OpKind::Capture,
            "op" => OpKind::Op,
            "foreign" => OpKind::Foreign,
            "note" => OpKind::Note,
            _ => return None,
        })
    }
}

/// The log, opened against one repository.
///
/// Read-rich and write-poor, and the split is an access modifier rather than
/// a convention: [`OpLog::append`] and [`OpDraft`] are `pub(crate)`, so a
/// third party gets a complete reader and no way to become a second author
/// of the cache. That is DESIGN's extension rule — extensions read fufu
/// state and call fufu verbs; only fufu writes fufu state — with the author
/// named by the compiler.
pub struct OpLog<'r> {
    repo: &'r gix::Repository,
    /// The worktree whose chain this handle reads and writes. Resolved once
    /// at open, so the ~55 call sites that hand over a repository and ask
    /// for the log keep saying exactly that.
    chain: String,
}

impl<'r> OpLog<'r> {
    pub fn open(repo: &'r gix::Repository) -> Result<Self> {
        Ok(OpLog {
            repo,
            chain: chain_id(repo),
        })
    }

    /// The chain this handle is open on.
    pub fn chain(&self) -> &str {
        &self.chain
    }

    /// This chain's tip ref.
    pub fn ops_ref(&self) -> String {
        ops_ref(&self.chain)
    }

    /// This chain's trash ref.
    pub fn ops_trash_ref(&self) -> String {
        ops_trash_ref(&self.chain)
    }

    /// The newest operation, if the log exists.
    pub fn tip(&self) -> Result<Option<OpId>> {
        Ok(crate::refs::ref_target(self.repo, &self.ops_ref())?.map(OpId::new))
    }

    /// The newest operation trim parked, if it has parked any. Trimmed
    /// operations are still objects and still resolve, so this is a real
    /// reading surface rather than bookkeeping.
    pub fn trash_tip(&self) -> Result<Option<OpId>> {
        Ok(crate::refs::ref_target(self.repo, &self.ops_trash_ref())?.map(OpId::new))
    }

    /// The newest operation on one branch — the per-branch pointer.
    pub fn branch_tip(&self, branch: &str) -> Result<Option<OpId>> {
        let name = format!("{BRANCH_PREFIX}{branch}");
        Ok(crate::refs::ref_target(self.repo, name.as_str())?.map(OpId::new))
    }

    /// Decode one operation. Trimmed operations still decode: they are
    /// objects, and `ff op show` on one is a legitimate question.
    pub fn get(&self, id: OpId) -> Result<Operation<'r>> {
        walk::decode(self.repo, id.object_id())
    }

    /// Decode one operation that must still be on the live log — the check
    /// every verb that *moves* to an operation owes, since restoring to
    /// something trim already dropped is a different answer from restoring
    /// to something that is simply old.
    pub fn live(&self, id: OpId) -> Result<Operation<'r>> {
        if !index::contains(self.repo, index::Kind::Live, id.object_id())? {
            return Err(Error::coded(
                "op/trimmed",
                format!("operation {id} has been trimmed off the log"),
                vec!["ff op log".into(), "ff config keep <duration>".into()],
            ));
        }
        self.get(id)
    }

    /// Every operation, newest first. Lazy: `.take(25)` costs 25 decodes,
    /// and filtering the captures out costs nothing extra. An eager
    /// `read_ops(repo, limit)` made laziness a discipline instead.
    pub fn iter(&self) -> impl Iterator<Item = Result<Operation<'r>>> + '_ {
        walk::OpWalk::new(self.repo, self.tip(), walk::Follow::Log)
    }

    /// Every operation from `start` backwards, newest first — the same walk
    /// bounded at a past operation instead of at the tip, which is all
    /// `--at-op` needs here: the log as it read then is the log now with its
    /// head cut off, since operations behind a point never change.
    pub fn iter_from(&self, start: OpId) -> impl Iterator<Item = Result<Operation<'r>>> + '_ {
        walk::OpWalk::new(self.repo, Ok(Some(start)), walk::Follow::Log)
    }

    /// The operations that are not captures, newest first — the same walk
    /// with the runs of captures hopped whole rather than decoded and
    /// discarded. That decoding is what made `ff op log` grow with the log
    /// while its answer stayed twenty-five rows.
    ///
    /// The start is yielded whatever it is: the tip is nearly always a
    /// capture, and refusing to begin there would leave nowhere to begin.
    pub fn iter_verbs(&self) -> impl Iterator<Item = Result<Operation<'r>>> + '_ {
        walk::OpWalk::new(self.repo, self.tip(), walk::Follow::Verbs)
    }

    /// [`Self::iter_verbs`] bounded at a past operation, the way
    /// [`Self::iter_from`] bounds [`Self::iter`].
    pub fn iter_verbs_from(&self, start: OpId) -> impl Iterator<Item = Result<Operation<'r>>> + '_ {
        walk::OpWalk::new(self.repo, Ok(Some(start)), walk::Follow::Verbs)
    }

    /// One branch's operations, newest first.
    ///
    /// This link and `prev` skip different things, which is why both ship:
    /// `prev_on_branch` skips other branches' ops, while the segment link
    /// (`fufu-prev-segment`) skips runs of same-base ops — the thing that
    /// grows without bound while an open change stays open.
    pub fn iter_branch(&self, branch: &str) -> impl Iterator<Item = Result<Operation<'r>>> + '_ {
        walk::OpWalk::new(self.repo, self.branch_tip(branch), walk::Follow::Branch)
    }

    /// Resolve an operation address: `@` for the newest, a letters-spelled id
    /// or prefix, either of them wearing git's own first-parent suffixes.
    ///
    /// The suffixes are git's because an operation's first parent *is* the
    /// operation before it, so `@^` and `@~3` already say what a bespoke
    /// `@-` and `@-3` would have said — which is why those are gone from
    /// this space as well as from revisions. Hex is refused rather than
    /// accepted quietly: `ff log` prints op ids beside commit shas, and a hex
    /// spelling that worked would teach the wrong model on the first try.
    pub fn resolve(&self, spec: &str) -> Result<OpId> {
        let spec = spec.trim();
        let (base, suffixes) = split_suffixes(spec);
        let mut cur = self.resolve_base(base)?;
        for step in 0..steps_back(spec, suffixes)? {
            cur = self.get(cur)?.prev().ok_or_else(|| {
                Error::coded(
                    "op/floor",
                    format!(
                        "the log goes back {step} operation(s), and `{spec}` asks for more: \
                         everything earlier is past the undo floor"
                    ),
                    vec!["ff op log".into()],
                )
            })?;
        }
        Ok(cur)
    }

    /// The base of an address, before any suffix walks back from it.
    fn resolve_base(&self, base: &str) -> Result<OpId> {
        if base == "@" {
            return self.tip()?.ok_or_else(|| {
                Error::coded(
                    "op/not-found",
                    "no operations have been recorded in this repository",
                    vec![],
                )
            });
        }
        let hex = crate::snapid::decode(base).ok_or_else(|| no_such_op(base))?;
        let mut matches = Vec::new();
        for candidate in index::prefix_matches(self.repo, &hex)? {
            // The index is a cache, so every candidate it offers is checked
            // against the object store before it counts. A stale entry can
            // only ever produce one that fails here.
            if is_op_commit(self.repo, candidate)? {
                matches.push(OpId::new(candidate));
            }
        }
        matches.sort_unstable();
        matches.dedup();
        match matches.as_slice() {
            [] => Err(no_such_op(base)),
            [one] => Ok(*one),
            many => {
                let list: Vec<String> = many.iter().map(|id| id.short(12)).collect();
                Err(Error::coded(
                    "op/ambiguous",
                    format!(
                        "{base} matches {} operations: {}",
                        many.len(),
                        list.join(", ")
                    ),
                    vec!["ff op log".into()],
                ))
            }
        }
    }

    /// Write one operation. `pub(crate)` on purpose: see the type docs.
    pub(crate) fn append(&self, draft: &OpDraft, now: i64) -> Result<Append> {
        append::commit_op(self.repo, draft, now)
    }
}

fn no_such_op(spec: &str) -> Error {
    Error::coded(
        "op/not-found",
        format!("no operation matches {spec}"),
        vec!["ff op log".into()],
    )
}

/// Split an address at the first `^` or `~`. An op id is letters and an
/// address has no braces, so there is nothing else a seam could hide behind.
fn split_suffixes(spec: &str) -> (&str, &str) {
    match spec.find(['^', '~']) {
        Some(i) => spec.split_at(i),
        None => (spec, ""),
    }
}

/// How many operations back a suffix chain walks.
///
/// `^` and `~n` mean the same thing here and that is not a redundancy in the
/// grammar but a fact about the shape: an operation's first parent is the
/// operation before it, so first-parent ancestry and log order are one
/// sequence. `^2` is the only spelling that means something else, and what it
/// means is the op's *base* — a commit, in the other address space — so it is
/// refused by name rather than followed across.
fn steps_back(spec: &str, suffixes: &str) -> Result<usize> {
    let mut steps = 0usize;
    let mut rest = suffixes;
    while let Some(first) = rest.chars().next() {
        rest = &rest[1..];
        let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
        rest = &rest[digits.len()..];
        let n: usize = if digits.is_empty() {
            1
        } else {
            digits.parse().map_err(|_| bad_address(spec))?
        };
        match first {
            '~' => steps += n,
            '^' if n == 1 => steps += 1,
            '^' => return Err(cross_space_parent(spec, n)),
            _ => return Err(bad_address(spec)),
        }
    }
    Ok(steps)
}

fn bad_address(spec: &str) -> Error {
    Error::coded(
        "op/not-found",
        format!("`{spec}` is not an operation address"),
        vec!["ff op log".into(), "ff op show @".into()],
    )
}

/// An operation's parents past the first leave the log: slot 2 is the base
/// commit, and the rest are pins. Following one would hand a commit to a verb
/// that takes an operation, so the refusal names the crossing that is spelled
/// on purpose.
fn cross_space_parent(spec: &str, slot: usize) -> Error {
    Error::coded(
        "usage/rev-in-op-position",
        format!(
            "`{spec}` asks for parent {slot} of an operation, which is not an operation: \
             slot 2 is the commit the op ran on and the rest are pins. The crossing has a \
             name, and it is `base()`"
        ),
        vec![
            format!(
                "ff op show {}",
                spec.split(['^', '~']).next().unwrap_or("@")
            ),
            "ff log -r 'base(@)'".into(),
        ],
    )
}

#[cfg(test)]
mod tests;
