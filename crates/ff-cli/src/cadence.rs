//! The cadence grammar behind fufu's "how often" settings — `fufu.updateCheck`
//! and `fufu.autoTrim` — which share one value language: a bool (`true` = the
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

/// Decode an encoded interval value into an effective interval in seconds.
///
/// `-1` → disabled (`None`), `0` → daily default, `n` → `n` floored at 60.
pub fn effective(encoded: i64) -> Option<i64> {
    match encoded {
        -1 => None,
        0 => Some(86_400),
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
    match cached {
        n if n >= 1 => n.max(60),
        _ => 86_400,
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
}
