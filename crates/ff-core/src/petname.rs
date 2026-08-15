//! Petnames for anonymous branches: `ff/<adjective>-<noun>`. The prefix is
//! the namespace claim; the words are embedded so minting never needs the
//! network or a config. Collisions retry with fresh entropy.

use crate::branch::ANON_PREFIX;
use crate::error::{Error, Result};

const ADJECTIVES: &[&str] = &[
    "amber",
    "bold",
    "brisk",
    "calm",
    "clever",
    "cosmic",
    "crisp",
    "daring",
    "deft",
    "dusky",
    "eager",
    "early",
    "fabled",
    "fleet",
    "fond",
    "gentle",
    "gilded",
    "glad",
    "hardy",
    "hidden",
    "humble",
    "keen",
    "kind",
    "lively",
    "lucid",
    "mellow",
    "merry",
    "misty",
    "nimble",
    "noble",
    "pale",
    "plucky",
    "proud",
    "quiet",
    "rapid",
    "rosy",
    "rustic",
    "silent",
    "snug",
    "spry",
    "stout",
    "sunny",
    "swift",
    "tidy",
    "vivid",
    "wandering",
    "warm",
    "wise",
    "witty",
    "young",
];

const NOUNS: &[&str] = &[
    "badger", "bear", "beacon", "brook", "cedar", "cliff", "cloud", "comet", "coral", "crane",
    "creek", "dawn", "delta", "drake", "ember", "falcon", "fern", "finch", "fox", "garnet",
    "glade", "grove", "harbor", "hawk", "heron", "hollow", "lark", "lynx", "maple", "meadow",
    "mole", "moth", "otter", "owl", "pine", "quail", "raven", "reef", "ridge", "river", "sparrow",
    "spruce", "stone", "swan", "thicket", "tide", "trail", "vale", "willow", "wren",
];

/// Weak-but-sufficient entropy for name picking: the clock and the pid.
/// Not security-relevant — a collision just retries.
fn entropy(round: u64) -> u64 {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos() as u64 ^ (d.as_secs() << 20))
        .unwrap_or(0);
    let pid = std::process::id() as u64;
    // SplitMix64 scrambling.
    let mut z = nanos ^ (pid << 32) ^ round.wrapping_mul(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// Mint an unused anonymous branch name.
pub fn mint(repo: &gix::Repository) -> Result<String> {
    for round in 0..64 {
        let bits = entropy(round);
        let adjective = ADJECTIVES[(bits % ADJECTIVES.len() as u64) as usize];
        let noun = NOUNS[((bits >> 16) % NOUNS.len() as u64) as usize];
        let name = format!("{ANON_PREFIX}{adjective}-{noun}");
        let full = format!("refs/heads/{name}");
        if crate::refs::ref_target(repo, &full)?.is_none() {
            return Ok(name);
        }
    }
    Err(Error::msg(
        "could not find a free anonymous branch name after 64 tries",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn words_make_valid_ref_names() {
        for adjective in ADJECTIVES {
            for noun in NOUNS {
                let full = format!("refs/heads/ff/{adjective}-{noun}");
                let name: std::result::Result<gix::refs::FullName, _> = full.as_str().try_into();
                assert!(name.is_ok(), "{full}");
            }
        }
    }
}
