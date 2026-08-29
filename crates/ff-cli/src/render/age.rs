const AGE_STEPS: &[(i64, &str)] = &[
    (60, "s"),
    (60 * 60, "m"),
    (60 * 60 * 24, "h"),
    (60 * 60 * 24 * 7, "d"),
    (60 * 60 * 24 * 30, "w"),
    (60 * 60 * 24 * 365, "mo"),
];

/// A non-negative span of seconds, bucketed to its coarsest unit — the
/// shared core of `relative_age` (which appends " ago") and
/// `duration_human` (a plain elapsed span, no "ago").
fn bucketed(delta: i64) -> String {
    let mut prev = 1;
    for &(limit, unit) in AGE_STEPS {
        if delta < limit {
            return format!("{}{unit}", delta / prev);
        }
        prev = limit;
    }
    format!("{}y", delta / (60 * 60 * 24 * 365))
}

pub fn relative_age(now: i64, then: i64) -> String {
    let delta = now - then;
    if delta < 0 {
        return "future".into();
    }
    format!("{} ago", bucketed(delta))
}

#[cfg(test)]
mod tests {
    use super::relative_age;

    #[test]
    fn ages() {
        assert_eq!(relative_age(1000, 990), "10s ago");
        assert_eq!(relative_age(10_000, 100), "2h ago");
        assert_eq!(relative_age(1_000_000, 100), "1w ago");
        assert_eq!(relative_age(100_000_000, 100), "3y ago");
    }
}
