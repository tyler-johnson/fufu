//! `fufu.onConflict` — what `ff restack`, `ff pull`, and `ff merge` do when
//! the replay on the branch underfoot conflicts. **hold**, the default,
//! records the hold and stops, exit 3, for `ff resolve`. **resolve**
//! records it and opens the resolution session in the same run. `--resolve`
//! and `--no-resolve` on the verb override it for one run; clap refuses the
//! pair.

use ff_core::OnConflict;

/// Read the setting. A total match with a default arm, so an absent value,
/// an unreadable one, and a misspelled one are all `hold`.
pub fn read(repo: &ff_core::gix::Repository) -> OnConflict {
    let raw = repo
        .config_snapshot()
        .string("fufu.onConflict")
        .map(|value| value.to_string());
    match raw.as_deref() {
        Some(value) if value.eq_ignore_ascii_case("resolve") => OnConflict::Resolve,
        _ => OnConflict::Hold,
    }
}

/// The answer for one run: the flag that was typed, or the setting.
pub fn settle(repo: &ff_core::gix::Repository, resolve: bool, no_resolve: bool) -> OnConflict {
    if resolve {
        OnConflict::Resolve
    } else if no_resolve {
        OnConflict::Hold
    } else {
        read(repo)
    }
}
