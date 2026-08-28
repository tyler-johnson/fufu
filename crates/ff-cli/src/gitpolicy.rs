//! `fufu.gitPolicy` — the tier, and the counter behind it.
//!
//! Three graduated tiers govern both places raw git reaches fufu: `ff git`
//! through the recommended alias, and a `git …` shell string inside an
//! agent's tool call. **observe** records and says nothing. **coach**, the
//! default, names the fufu alternative the first time a word comes up.
//! **strict** refuses the words fufu has verbs for and says what to run
//! instead. One setting, one meaning, two places it lands.
//!
//! What no tier does is rewrite a command line. Shell parsing and compound
//! commands make that unsafe, so fufu *names* an alternative and never
//! composes one — the write that runs is always the one that was typed, or
//! none at all.
//!
//! The tally is `<common-dir>/fufu/gitpolicy-<chain>.json`, on the same
//! template as the auto-trim stamp: serde struct, atomic temp-and-rename,
//! and every read failure yielding [`Tally::default`]. A counter must never
//! be able to fail a capture. Under concurrency its read-modify-write can
//! undercount, and that is acceptable: this is a counter behind a nudge,
//! not an audit. The operation log is the record.

use serde::{Deserialize, Serialize};

/// What to do about a git word fufu has a verb for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Policy {
    /// Record it and say nothing.
    Observe,
    /// Name the fufu verb, once per word per session.
    Coach,
    /// Refuse it, naming the fufu verb.
    Strict,
}

/// The session id `ff git` records under. A person at a shell has no
/// session, so the throttle becomes once per word per repository — one rule
/// (*coach names each word once per session*) with the alias as one long
/// session called local.
pub const LOCAL: &str = "local";

/// Read the tier. A total match with a default arm, so an absent value, an
/// unreadable one, and a misspelled one are all `coach`.
pub fn read(repo: &ff_core::gix::Repository) -> Policy {
    let raw = repo
        .config_snapshot()
        .string("fufu.gitPolicy")
        .map(|value| value.to_string());
    match raw.as_deref() {
        Some(value) => match value.to_lowercase().as_str() {
            "observe" => Policy::Observe,
            "strict" => Policy::Strict,
            _ => Policy::Coach,
        },
        None => Policy::Coach,
    }
}

/// gitpolicy-<chain>.json — how much raw git this chain has seen.
///
/// `coached` is the words already named this session; an incoming session
/// id that differs from `session` resets it. Per word rather than
/// once-per-session on purpose: an agent that runs `git commit` and later
/// `git rebase -i` should hear about the second, which is the one that
/// fights the model hardest. It is bounded by the table's size, so it
/// cannot grow.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Tally {
    pub writes: u64,
    pub denied: u64,
    pub last_at: i64,
    pub session: String,
    pub coached: Vec<String>,
}

impl Tally {
    pub fn is_empty(&self) -> bool {
        self.writes == 0
    }
}

fn state_path(repo: &ff_core::gix::Repository) -> std::path::PathBuf {
    let chain = ff_core::ops::chain_id(repo);
    repo.common_dir()
        .join("fufu")
        .join(format!("gitpolicy-{chain}.json"))
}

/// Load the tally. Any error — missing, unreadable, corrupt JSON — yields
/// [`Tally::default`].
pub fn load(repo: &ff_core::gix::Repository) -> Tally {
    std::fs::read(state_path(repo))
        .ok()
        .and_then(|data| serde_json::from_slice(&data).ok())
        .unwrap_or_default()
}

/// Save with atomic temp-file + rename. One temp name per destination, so
/// two worktrees sharing the `fufu` directory cannot steal each other's
/// half-written file.
fn save(repo: &ff_core::gix::Repository, tally: &Tally) -> std::io::Result<()> {
    let path = state_path(repo);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let name = path.file_name().unwrap().to_string_lossy().into_owned();
    let tmp = path.with_file_name(format!("{name}.ff-tmp"));
    let body = serde_json::to_string(tally)?;
    std::fs::write(&tmp, &body)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// The decision a fresh session makes about a word, applied to a tally.
/// Split out from the disk so it is testable without one.
pub(crate) fn mark(tally: &mut Tally, session: &str, word: &str, denied: bool, now: i64) -> bool {
    if tally.session != session {
        tally.session = session.to_string();
        tally.coached.clear();
    }
    tally.writes += 1;
    tally.last_at = now;
    if denied {
        tally.denied += 1;
        // A refusal prints every time — it is the answer, not a nudge — so
        // it never spends the word's one coaching slot.
        return false;
    }
    let fresh = !tally.coached.iter().any(|seen| seen == word);
    if fresh {
        tally.coached.push(word.to_string());
    }
    fresh
}

/// Record one raw-git write against this chain. Answers whether this word
/// is the first of its kind this session, which is what coach throttles on.
/// Never errors: a tally that could not be written is a nudge lost, and
/// losing a nudge is cheaper than failing the command underneath it.
pub fn record(repo: &ff_core::gix::Repository, session: &str, word: &str, denied: bool) -> bool {
    let mut tally = load(repo);
    let fresh = mark(&mut tally, session, word, denied, now_secs());
    let _ = save(repo, &tally);
    fresh
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_word_is_coached_once_per_session() {
        let mut tally = Tally::default();
        assert!(mark(&mut tally, "s1", "commit", false, 10));
        assert!(!mark(&mut tally, "s1", "commit", false, 11));
        // A different word in the same session still gets named.
        assert!(mark(&mut tally, "s1", "rebase", false, 12));
        assert_eq!(tally.writes, 3);
        assert_eq!(tally.last_at, 12);
    }

    #[test]
    fn a_new_session_starts_over() {
        let mut tally = Tally::default();
        assert!(mark(&mut tally, "s1", "commit", false, 10));
        assert!(mark(&mut tally, "s2", "commit", false, 11));
        assert_eq!(tally.coached, vec!["commit".to_string()]);
    }

    #[test]
    fn a_refusal_counts_and_never_spends_the_coaching_slot() {
        let mut tally = Tally::default();
        assert!(!mark(&mut tally, "s1", "commit", true, 10));
        assert_eq!(tally.denied, 1);
        assert_eq!(tally.writes, 1);
        assert!(tally.coached.is_empty());
        // Dropping back to coach in the same session still names it.
        assert!(mark(&mut tally, "s1", "commit", false, 11));
    }
}
