//! The writing half of the engine [`crate::futures::probe`] simulates: it
//! re-parents a range of commits — replaying them by three-way merge when the
//! change moves the target's tree or the range's floor onto a different base —
//! and writes the rewritten objects, moving no refs of its own. Every rewrite
//! verb, reword today and absorb and lift later, aims here rather than
//! forking its own commit-writing logic.

mod chain;
mod markers;
mod replay;

pub use chain::{
    Attribution, Chain, Conflict, Region, Resolution, Step, Tangle, attribute, chain, conflict,
    regions,
};
pub(crate) use chain::{carries_markers, chain_labels, stack_size};
pub(crate) use replay::join_paths;
pub use replay::{
    Change, Clearing, Decided, Dropped, Rewrite, RewritePlan, plan, plan_with, published_count,
    tracking_name,
};
