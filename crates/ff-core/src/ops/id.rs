//! The two address spaces, as two types.
//!
//! An operation *is* a commit, and both spell their ids in hex, so on the
//! page a `9dfd5e5d` could be either. What keeps them apart is the slot, the
//! way jj's does: `--at-op` and `ff op show` read a hex prefix as an
//! operation, `-r` and `--from` read one as a commit, and each redirects the
//! other's kind by name. In the code the newtypes do that work: the one place
//! a plain `gix::ObjectId` can go, an op id can go too, and it resolves.
//! Wrapping each space in its own type moves the refusal from runtime to the
//! compiler, and leaves the runtime check needed only where text becomes a
//! type.
//!
//! Letters are a change id and nothing else. A change id typed where an
//! operation belongs, or an operation where a revision belongs, is refused
//! by name rather than looked up in the wrong space.

use crate::error::{Error, Result};

/// The width an operation id prints at in every column: jj's.
pub const SHORT: usize = 12;

/// An operation's address. Displays in hex, always.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct OpId(gix::ObjectId);

/// A revision's address. Displays in hex, always.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CommitId(gix::ObjectId);

impl OpId {
    pub fn new(id: gix::ObjectId) -> Self {
        OpId(id)
    }

    /// The underlying object. Named the long way on purpose — reaching for
    /// it is leaving the type system's protection, and should read like it.
    pub fn object_id(&self) -> gix::ObjectId {
        self.0
    }

    /// The raw hex, for the id index and anywhere git itself is addressed.
    pub fn hex(&self) -> String {
        self.0.to_string()
    }

    /// The hex truncated to `len`, for the highlighted prefix.
    pub fn short(&self, len: usize) -> String {
        self.to_string().chars().take(len).collect()
    }

    /// Parse a full hex id. Prefixes are a resolution question, not a
    /// parsing one, so they go through [`super::OpLog::resolve`].
    pub fn parse(hex: &str) -> Result<Self> {
        if hex.len() != 40 {
            return Err(not_an_op(hex));
        }
        let id = gix::ObjectId::from_hex(hex.as_bytes()).map_err(|_| not_an_op(hex))?;
        Ok(OpId(id))
    }
}

impl CommitId {
    pub fn new(id: gix::ObjectId) -> Self {
        CommitId(id)
    }

    pub fn object_id(&self) -> gix::ObjectId {
        self.0
    }
}

impl From<gix::ObjectId> for CommitId {
    fn from(id: gix::ObjectId) -> Self {
        CommitId(id)
    }
}

impl std::fmt::Display for OpId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Display::fmt(&self.0, f)
    }
}

impl std::fmt::Display for CommitId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Display::fmt(&self.0, f)
    }
}

/// Both ids serialize the way they display, so the machine surface and the
/// human surface name an operation identically — a script that reads
/// `ff op log --json` can paste an id straight back into `--at-op`.
impl serde::Serialize for OpId {
    fn serialize<S: serde::Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        s.collect_str(self)
    }
}

impl serde::Serialize for CommitId {
    fn serialize<S: serde::Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        s.collect_str(self)
    }
}

fn not_an_op(spec: &str) -> Error {
    Error::coded(
        "op/not-found",
        format!("`{spec}` is not an operation id: operation ids are hex, `ff op log` prints them"),
        vec!["ff op log".into()],
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn oid(byte: u8) -> gix::ObjectId {
        gix::ObjectId::from_bytes_or_panic(&[byte; 20])
    }

    #[test]
    fn op_ids_display_in_hex_and_round_trip() {
        let id = OpId::new(oid(0x3f));
        let spelled = id.to_string();
        assert_eq!(spelled, "3f".repeat(20));
        assert_eq!(OpId::parse(&spelled).unwrap(), id);
    }

    #[test]
    fn commit_ids_display_in_hex() {
        let id = CommitId::new(oid(0xab));
        assert_eq!(id.to_string(), "ab".repeat(20));
    }

    #[test]
    fn letters_are_refused_where_an_op_belongs() {
        let err = OpId::parse(&"kl".repeat(20)).unwrap_err();
        assert_eq!(err.id(), "op/not-found");
        assert!(OpId::parse("").is_err(), "empty is not an id");
        assert!(
            OpId::parse("3f3f").is_err(),
            "a prefix is a resolution question, not a parse"
        );
    }

    #[test]
    fn short_takes_the_hex_prefix() {
        let id = OpId::new(oid(0x00));
        assert_eq!(id.short(4), "0000");
        assert_eq!(id.short(SHORT).len(), 12);
    }
}
