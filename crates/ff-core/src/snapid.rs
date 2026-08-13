//! The snapshot id spelling: jj's "reverse hex" alphabet. Hex digit value
//! `i` maps to `ALPHABET[i]`, so `0` → `z` down to `f` → `k`. The letter
//! range k–z is disjoint from hex digits (0–9, a–f), which makes a spelled
//! snapshot id visually and grammatically distinct from a commit sha: a
//! parser can accept both without ambiguity. Ids in the read model stay
//! lowercase hex; encoding happens only at presentation and input edges.

/// Hex digit value → letter, in order: `z` for 0 through `k` for f.
pub const ALPHABET: &[u8; 16] = b"zyxwvutsrqponmlk";

/// Spell a lowercase-hex id in the letters alphabet.
///
/// # Panics
/// On non-hex input — callers encode ids that came out of the read model,
/// which are hex by construction.
pub fn encode(hex: &str) -> String {
    hex.chars()
        .map(|c| {
            let v = c.to_digit(16).expect("snapid::encode takes hex input");
            ALPHABET[v as usize] as char
        })
        .collect()
}

/// Decode a letters spelling back to lowercase hex. `None` when any
/// character falls outside the alphabet (case-insensitive).
pub fn decode(letters: &str) -> Option<String> {
    letters
        .chars()
        .map(|c| {
            let b = (c as u32 <= 0x7f).then_some(c.to_ascii_lowercase() as u8)?;
            let v = ALPHABET.iter().position(|&a| a == b)?;
            char::from_digit(v as u32, 16)
        })
        .collect()
}

/// Whether a string is entirely alphabet letters (case-insensitive) — i.e.
/// decodable. Empty strings are not encoded ids.
pub fn is_encoded(s: &str) -> bool {
    !s.is_empty() && decode(s).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_vectors() {
        assert_eq!(encode("0123456789abcdef"), "zyxwvutsrqponmlk");
        assert_eq!(
            decode("zyxwvutsrqponmlk").as_deref(),
            Some("0123456789abcdef")
        );
        assert_eq!(encode("3f8d62c"), "wkrmtxn");
        assert_eq!(
            decode("WKRMTXN").as_deref(),
            Some("3f8d62c"),
            "case-insensitive"
        );
    }

    #[test]
    fn roundtrip() {
        let hex = "d3adbeef0123456789abcdef";
        assert_eq!(decode(&encode(hex)).as_deref(), Some(hex));
    }

    #[test]
    fn rejects_non_alphabet() {
        assert_eq!(decode("abc"), None, "hex letters are not in the alphabet");
        assert_eq!(decode("kl0"), None);
        assert_eq!(decode("kl-"), None);
        assert_eq!(decode("klé"), None, "non-ascii rejected");
    }

    #[test]
    fn is_encoded_gate() {
        assert!(is_encoded("noon"), "date words shadowed by design");
        assert!(is_encoded("KLMZ"));
        assert!(!is_encoded(""));
        assert!(!is_encoded("abcd"));
        assert!(!is_encoded("12kl"));
    }
}
