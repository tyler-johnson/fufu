//! The change id: the identity a commit keeps through rewrites, jj's model.
//!
//! Sixteen random bytes, minted when a change first gets a description or a
//! capture and written into the commit as a `change-id` header spelled in the
//! letters alphabet (`snapid`), which is exactly the header jj writes and
//! reads, so a colocated jj sees fufu's ids and fufu sees jj's. Replay copies
//! every header but the signature, so restack, absorb, describe, and pull
//! carry the id for free. A commit without the header — one made by git, or
//! cloned from a repository fufu never touched — gets a derived id, jj's
//! derivation, so every clone agrees on it without a walk.
//!
//! Op ids share the alphabet. The slot decides which kind a token names: a
//! revision slot reads change ids and shas, an op slot reads op ids.

use std::collections::HashMap;
use std::fmt;

use gix::bstr::BString;

use crate::error::{Error, Result};
use crate::snapid;

/// Bytes in a change id.
pub const LEN: usize = 16;
/// Letters in a spelled change id.
pub const LETTERS: usize = 32;
/// The commit header carrying the id.
pub const HEADER: &str = "change-id";

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct ChangeId([u8; LEN]);

impl ChangeId {
    /// A fresh id from the system's entropy.
    pub fn mint() -> Result<Self> {
        let mut bytes = [0u8; LEN];
        getrandom::fill(&mut bytes)
            .map_err(|err| Error::msg(format!("could not mint a change id: {err}")))?;
        Ok(ChangeId(bytes))
    }

    /// The id of a commit that carries no header: the sha's bytes reversed in
    /// order, each byte's bits reversed, the first sixteen kept. jj's
    /// `change_id_from_git_commit_id`, byte for byte.
    pub fn derive(sha: &gix::oid) -> Self {
        let mut bytes = [0u8; LEN];
        for (slot, byte) in bytes.iter_mut().zip(sha.as_bytes().iter().rev()) {
            *slot = byte.reverse_bits();
        }
        ChangeId(bytes)
    }

    /// Exactly thirty-two alphabet letters, case-insensitive. `snapid::decode`
    /// is length-agnostic, so the length check lives here.
    pub fn parse(letters: &str) -> Option<Self> {
        if letters.chars().count() != LETTERS {
            return None;
        }
        let hex = snapid::decode(letters)?;
        let mut bytes = [0u8; LEN];
        for (i, slot) in bytes.iter_mut().enumerate() {
            *slot = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).ok()?;
        }
        Some(ChangeId(bytes))
    }

    /// Thirty-two lowercase hex digits.
    pub fn hex(&self) -> String {
        self.0.iter().map(|b| format!("{b:02x}")).collect()
    }

    /// The letters spelling, the header's and the column's.
    pub fn letters(&self) -> String {
        snapid::encode(&self.hex())
    }

    /// Whether a letters token (case-insensitive) is a prefix of this id.
    pub fn has_prefix(&self, letters: &str) -> bool {
        let Some(hex) = snapid::decode(letters) else {
            return false;
        };
        self.hex().starts_with(&hex)
    }
}

impl fmt::Display for ChangeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.letters())
    }
}

/// The commit's id: its header when present and well formed, else derived.
/// A malformed header is not an error, because a raw object is not fufu's to
/// refuse; it reads as no header at all.
pub fn of_commit(raw: &[u8], sha: &gix::oid) -> ChangeId {
    header_of(raw).unwrap_or_else(|| ChangeId::derive(sha))
}

/// The commit's `change-id` header, when it carries a well-formed one. The
/// distinction `of_commit` erases: a commit with a header was closed or
/// rewritten by fufu or jj, and the operation log has its history; one
/// without gets a derived id and has none.
pub fn header_of(raw: &[u8]) -> Option<ChangeId> {
    gix::objs::CommitRef::from_bytes(raw)
        .ok()
        .and_then(|commit| {
            commit
                .extra_headers()
                .find(HEADER)
                .and_then(|value| std::str::from_utf8(value).ok())
                .and_then(|value| ChangeId::parse(value.trim()))
        })
}

/// The header pair a commit object carries: `("change-id", letters)`.
pub fn header(id: &ChangeId) -> (BString, BString) {
    (BString::from(HEADER), BString::from(id.letters()))
}

/// The shortest prefix that tells each id apart from every other id on one
/// page, keyed by the full letters, at least one letter and at most the
/// whole id. Page-local by design: a prefix unique here may still be
/// ambiguous across the repository, which is what the resolver says when it
/// is. Never `ops::index::prefix_lens`, which prices op ids.
pub fn prefix_lens<'a>(ids: impl Iterator<Item = &'a str>) -> HashMap<String, usize> {
    let ids: Vec<&str> = ids.collect();
    let mut lens = HashMap::new();
    for id in &ids {
        let mut len = 1;
        for other in &ids {
            if other == id {
                continue;
            }
            let shared = id
                .chars()
                .zip(other.chars())
                .take_while(|(a, b)| a == b)
                .count();
            len = len.max((shared + 1).min(LETTERS));
        }
        lens.insert((*id).to_string(), len.min(id.chars().count().max(1)));
    }
    lens
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mint_round_trips_and_differs() {
        let a = ChangeId::mint().unwrap();
        let b = ChangeId::mint().unwrap();
        assert_ne!(a, b);
        assert_eq!(ChangeId::parse(&a.letters()), Some(a));
        assert_eq!(a.letters().len(), LETTERS);
        assert_eq!(a.hex().len(), 32);
    }

    #[test]
    fn derivation_matches_jj() {
        let sha = gix::ObjectId::from_hex(b"6e50901ae01a2c2071423876bb8fccecfe175c69").unwrap();
        assert_eq!(
            ChangeId::derive(&sha).letters(),
            "qtwplrskwswwkymmtlynvxrlzvwvurzs"
        );
    }

    #[test]
    fn parse_rejects_hex_and_wrong_lengths() {
        let ok = "qtwplrskwswwkymmtlynvxrlzvwvurzs";
        assert!(ChangeId::parse(ok).is_some());
        assert!(ChangeId::parse(&ok.to_uppercase()).is_some());
        assert!(ChangeId::parse("6e50901ae01a2c2071423876bb8fccec").is_none());
        assert!(ChangeId::parse(&ok[..31]).is_none());
        assert!(ChangeId::parse(&format!("{ok}k")).is_none());
    }

    #[test]
    fn has_prefix_is_case_insensitive_and_alphabet_only() {
        let id = ChangeId::parse("qtwplrskwswwkymmtlynvxrlzvwvurzs").unwrap();
        assert!(id.has_prefix("qtwp"));
        assert!(id.has_prefix("QTWP"));
        assert!(!id.has_prefix("qtwq"));
        assert!(!id.has_prefix("6e50"));
    }

    fn raw_commit(extra: &[(&str, &str)]) -> Vec<u8> {
        use gix::objs::WriteTo as _;
        let sig = gix::actor::Signature {
            name: "t".into(),
            email: "t@x".into(),
            time: gix::date::Time::new(0, 0),
        };
        let commit = gix::objs::Commit {
            tree: gix::ObjectId::empty_tree(gix::hash::Kind::Sha1),
            parents: Default::default(),
            author: sig.clone(),
            committer: sig,
            encoding: None,
            message: "m\n".into(),
            extra_headers: extra
                .iter()
                .map(|(k, v)| (BString::from(*k), BString::from(*v)))
                .collect(),
        };
        let mut buf = Vec::new();
        commit.write_to(&mut buf).unwrap();
        buf
    }

    #[test]
    fn of_commit_prefers_a_well_formed_header() {
        let sha = gix::ObjectId::from_hex(b"6e50901ae01a2c2071423876bb8fccecfe175c69").unwrap();
        let want = "kkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkk";
        let raw = raw_commit(&[(HEADER, want)]);
        assert_eq!(of_commit(&raw, &sha).letters(), want);
        let raw = raw_commit(&[(HEADER, "not an id")]);
        assert_eq!(
            of_commit(&raw, &sha).letters(),
            "qtwplrskwswwkymmtlynvxrlzvwvurzs",
            "a malformed header derives"
        );
        let raw = raw_commit(&[]);
        assert_eq!(
            of_commit(&raw, &sha).letters(),
            "qtwplrskwswwkymmtlynvxrlzvwvurzs"
        );
    }

    #[test]
    fn header_pair() {
        let id = ChangeId::parse("qtwplrskwswwkymmtlynvxrlzvwvurzs").unwrap();
        let (name, value) = header(&id);
        assert_eq!(name, "change-id");
        assert_eq!(value, "qtwplrskwswwkymmtlynvxrlzvwvurzs");
    }

    #[test]
    fn prefix_lens_on_a_page() {
        let a = "qtwplrskwswwkymmtlynvxrlzvwvurzs";
        let b = "qtwzlrskwswwkymmtlynvxrlzvwvurzs";
        let c = "kkkklrskwswwkymmtlynvxrlzvwvurzs";
        let lens = prefix_lens([a, b, c].into_iter());
        assert_eq!(lens[a], 4);
        assert_eq!(lens[b], 4);
        assert_eq!(lens[c], 1);
        let alone = prefix_lens([a].into_iter());
        assert_eq!(alone[a], 1);
        let twins = prefix_lens([a, a].into_iter());
        assert_eq!(twins[a], 1, "the same id twice is one id");
    }
}
