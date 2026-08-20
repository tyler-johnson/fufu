//! How a commit hash is spelled on screen: a plain eight characters.
//!
//! The commit-space sibling of [`crate::snapid`]. Eight is fixed rather than
//! probed — no odb lookup, no `core.abbrev` — because a column that changes
//! width when the repository grows is a column that stops lining up, and
//! eight hex characters are effectively always unique at any scale fufu
//! meets. git resolves the rare collision when one is pasted back.
//!
//! No shortest-unique-prefix highlighting rides on these: the op id column is
//! where that pays, and two highlighted columns side by side read as one.

/// The display width of a short commit hash.
pub const SHORT: usize = 8;

/// The display spelling of a full hex id, borrowed. Short of `SHORT`
/// characters the whole thing comes back — an id is never padded here, the
/// column does that.
pub fn short(hex: &str) -> &str {
    &hex[..hex.len().min(SHORT)]
}

/// [`short`] for an object id that has not been spelled out yet.
pub fn short_oid(id: gix::ObjectId) -> String {
    short(&id.to_string()).to_string()
}
