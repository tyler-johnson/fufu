//! The cadence grammar behind fufu's "how often" settings — `fufu.updateCheck`,
//! `fufu.autoTrim` and `fufu.autoFetch` — which share one value language: a bool (`true` = the
//! default cadence, `false`/`never` = off), a compact duration (`12h`, `2w`),
//! or a bare number of days. State files cache the *parsed* answer so hot
//! paths decide staleness from one file read and never load config.

/// Parse a cadence string (the shared value language).
///
/// Returns `Some(-1)` for disabled, `Some(0)` for default, `Some(secs)` for
/// explicit durations, or `None` for unparseable input.
pub fn parse(raw: &str) -> Option<i64> {
    let raw = raw.trim();
    match raw.to_ascii_lowercase().as_str() {
        "false" | "no" | "off" | "never" | "0" => return Some(-1),
        "true" | "yes" | "on" => return Some(0),
        _ => {}
    }
    ff_core::snapshot::config::parse_keep(raw).map(|secs| secs.max(60))
}

/// One day, the default cadence of every setting that does not name its own.
const DAILY: i64 = 86_400;

/// Decode an encoded interval value into an effective interval in seconds.
///
/// `-1` → disabled (`None`), `0` → daily default, `n` → `n` floored at 60.
pub fn effective(encoded: i64) -> Option<i64> {
    effective_with(encoded, DAILY)
}

/// [`effective`] for a setting whose default cadence is not the day —
/// `fufu.autoFetch`'s ten minutes.
pub fn effective_with(encoded: i64, default_secs: i64) -> Option<i64> {
    match encoded {
        -1 => None,
        0 => Some(default_secs),
        n => Some(n.max(60)),
    }
}

/// Read a cadence key from a gix config file and encode its value.
///
/// Absent or invalid values behave like every other fufu reader: fall back to
/// `0` (default).
pub fn read_encoded(file: &ff_core::gix::config::File, key: &str) -> i64 {
    match file.string(key) {
        Some(val) => parse(&val.to_string()).unwrap_or(0),
        None => 0,
    }
}

/// How many seconds before a cached stamp is considered stale.
///
/// `n >= 1` → `n` floored at 60, everything else → daily default.
pub fn stale_after(cached: i64) -> i64 {
    stale_after_with(cached, DAILY)
}

/// [`stale_after`] with the setting's own default in place of the day. A
/// disabled stamp (`-1`) re-reads config on the default cadence, so turning
/// a setting back on is noticed without a stamp being deleted by hand.
pub fn stale_after_with(cached: i64, default_secs: i64) -> i64 {
    match cached {
        n if n >= 1 => n.max(60),
        _ => default_secs,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_values() {
        assert_eq!(parse("false"), Some(-1));
        assert_eq!(parse("NO"), Some(-1));
        assert_eq!(parse("off"), Some(-1));
        assert_eq!(parse("never"), Some(-1));
        assert_eq!(parse("0"), Some(-1));
        assert_eq!(parse("true"), Some(0));
        assert_eq!(parse("YES"), Some(0));
        assert_eq!(parse("on"), Some(0));
        assert_eq!(parse("12h"), Some(43_200));
        assert_eq!(parse("7"), Some(604_800));
        assert_eq!(parse("45s"), Some(60)); // floor
        assert_eq!(parse("2w"), Some(1_209_600));
        assert!(parse("bogus").is_none());
        assert_eq!(parse("  true  "), Some(0));
    }

    #[test]
    fn effective_values() {
        assert_eq!(effective(-1), None);
        assert_eq!(effective(0), Some(86_400));
        assert_eq!(effective(30), Some(60));
        assert_eq!(effective(7_200), Some(7_200));
    }

    #[test]
    fn effective_with_takes_the_settings_own_default() {
        assert_eq!(effective_with(-1, 600), None);
        assert_eq!(effective_with(0, 600), Some(600));
        assert_eq!(effective_with(30, 600), Some(60));
        assert_eq!(effective_with(7_200, 600), Some(7_200));
    }

    #[test]
    fn stale_after_with_takes_the_settings_own_default() {
        assert_eq!(stale_after(0), 86_400);
        assert_eq!(stale_after(-1), 86_400);
        assert_eq!(stale_after_with(0, 600), 600);
        assert_eq!(stale_after_with(-1, 600), 600);
        assert_eq!(stale_after_with(10, 600), 60);
        assert_eq!(stale_after_with(7_200, 600), 7_200);
    }
}
