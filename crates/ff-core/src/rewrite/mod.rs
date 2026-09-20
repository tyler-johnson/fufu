//! The writing half of the engine [`crate::futures::probe`] simulates: it
//! re-parents a range of commits — replaying them by three-way merge when the
//! change moves the target's tree or the range's floor onto a different base —
//! and writes the rewritten objects, moving no refs of its own. Every rewrite
//! verb aims here rather than forking its own commit-writing logic.
//!
//! A merge in the range is an ordinary commit to the engine: its parents are
//! mapped through the rewrite, re-merged, its own change — what
//! [`crate::measure`] diffs it against — is laid over the result, and a
//! parent left beneath another is dropped. One left with a single parent is
//! written as an ordinary commit, or dropped as empty like any other.

mod chain;
mod markers;
mod replay;

pub use chain::{
    Attribution, Chain, Conflict, Region, Resolution, Step, Tangle, attribute, chain, conflict,
    regions,
};
pub(crate) use chain::{carries_markers, chain_labels, stack_size};
pub use replay::{
    Change, Clearing, Decided, DropReason, Dropped, Flattened, MoveInto, Rewrite, RewritePlan,
    plan, plan_with, published_count, tracking_name,
};
pub(crate) use replay::{changed_paths, filtered, join_paths, superseded_in};
