use super::{Level, Row};

/// Pad a string to `width` characters (escape-safe: pad is added after the
/// visible text so ANSI bytes never inflate the column width).
fn pad(text: &str, width: usize) -> String {
    let len = text.chars().count();
    let extra = width.saturating_sub(len);
    format!("{text}{}", " ".repeat(extra))
}

fn format_row(row: &Row, colored: bool) -> String {
    let level_text = match row.level {
        Level::Ok => "ok",
        Level::Info => "info",
        Level::Warn => "WARN",
    };

    match row.level {
        Level::Ok => {
            let painted = crate::render::paint_ok(level_text, colored);
            let level_pad = " ".repeat(6usize.saturating_sub(level_text.chars().count()));
            format!(
                "  {}{}{}{}",
                painted,
                level_pad,
                pad(row.name, 15),
                row.detail
            )
        }
        Level::Info => {
            let painted_level = crate::render::paint_dim(level_text, colored);
            let painted_name = crate::render::paint_dim(row.name, colored);
            let painted_detail = crate::render::paint_dim(&row.detail, colored);
            let level_pad = " ".repeat(6usize.saturating_sub(level_text.chars().count()));
            let name_pad = " ".repeat(15usize.saturating_sub(row.name.chars().count()));
            format!(
                "  {}{}{}{}{}",
                painted_level, level_pad, painted_name, name_pad, painted_detail
            )
        }
        Level::Warn => {
            let painted = crate::render::paint_warn(level_text, colored);
            let level_pad = " ".repeat(6usize.saturating_sub(level_text.chars().count()));
            format!(
                "  {}{}{}{}",
                painted,
                level_pad,
                pad(row.name, 15),
                row.detail
            )
        }
    }
}

fn summary_text(findings: usize, fixable: usize, fix: bool) -> String {
    if findings == 0 {
        "no findings — the net is under you".into()
    } else {
        let mut s = format!("{findings} finding(s)");
        if fixable > 0 && !fix {
            s.push_str(&format!(" — `ff doctor --fix` repairs {fixable} of them"));
        }
        s
    }
}

fn json_body(rows: &[Row]) -> serde_json::Value {
    let findings = rows
        .iter()
        .filter(|r| matches!(r.level, Level::Warn))
        .count();
    let fixable = rows
        .iter()
        .filter(|r| matches!(r.level, Level::Warn) && r.fixable)
        .count();
    let checks: Vec<serde_json::Value> = rows
        .iter()
        .map(|r| {
            let level_str = match r.level {
                Level::Ok => "ok",
                Level::Info => "info",
                Level::Warn => "warn",
            };
            serde_json::json!({
                "level": level_str,
                "name": r.name,
                "detail": r.detail,
            })
        })
        .collect();
    serde_json::json!({
        "findings": findings,
        "fixable": fixable,
        "checks": checks,
    })
}

pub(super) fn render(rows: &[Row], fix: bool, json: bool, colored: bool) {
    let findings = rows
        .iter()
        .filter(|r| matches!(r.level, Level::Warn))
        .count();
    let fixable = rows
        .iter()
        .filter(|r| matches!(r.level, Level::Warn) && r.fixable)
        .count();

    if json {
        let payload = json_body(rows);
        if let Err(e) = crate::machine::emit("doctor", &payload) {
            eprintln!("ff: {e}");
            std::process::exit(1);
        }
    } else {
        for row in rows {
            println!("{}", format_row(row, colored));
        }

        println!();

        let sum = summary_text(findings, fixable, fix);
        println!(
            "{}",
            crate::render::paint_ok(&sum, colored && findings == 0)
        );
    }

    if findings > 0 {
        use std::io::Write;
        let _ = std::io::stdout().flush();
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_row_plain_alignment() {
        // ok row
        let row = Row::ok("repository", "/home/x/repo/.git".into());
        let formatted = format_row(&row, false);
        assert_eq!(formatted, "  ok    repository     /home/x/repo/.git");

        // info row
        let row = Row::info("journal", "last op \"commit: close\" 2m ago".into());
        let formatted = format_row(&row, false);
        assert_eq!(
            formatted,
            "  info  journal        last op \"commit: close\" 2m ago"
        );

        // warn row
        let row = Row::warn("gc config", "something wrong".into());
        let formatted = format_row(&row, false);
        assert_eq!(formatted, "  WARN  gc config      something wrong");
    }

    #[test]
    fn format_row_colored_pads_before_ansi() {
        // ok: detail text is plain, no trailing escape garbage
        let row = Row::ok("repository", "/path".into());
        let formatted = format_row(&row, true);
        assert!(formatted.contains("/path"), "detail present: {formatted:?}");
        assert!(
            formatted.ends_with(".git") || !formatted.ends_with('\u{1b}'),
            "no trailing escape: {formatted:?}"
        );

        // info: styled level/name/detail, pad after reset
        let row = Row::info("journal", "some detail".into());
        let formatted = format_row(&row, true);
        assert!(
            formatted.contains("some detail"),
            "detail intact: {formatted:?}"
        );

        // warn: level painted, detail plain
        let row = Row::warn("log", "no refs".into());
        let formatted = format_row(&row, true);
        assert!(
            formatted.contains("no refs"),
            "detail present: {formatted:?}"
        );
    }

    #[test]
    fn summary_lines() {
        // 0 findings
        let s = summary_text(0, 0, false);
        assert_eq!(s, "no findings — the net is under you");

        // 2 findings, 1 fixable, without --fix
        let s = summary_text(2, 1, false);
        assert!(s.contains("2 finding(s)"), "count present: {s}");
        assert!(s.contains("--fix"), "hint present: {s}");

        // 2 findings, 1 fixable, with --fix → no hint
        let s = summary_text(2, 1, true);
        assert!(s.contains("2 finding(s)"), "count present: {s}");
        assert!(!s.contains("--fix"), "no hint when fixing: {s}");

        // findings with 0 fixable → no hint
        let s = summary_text(3, 0, false);
        assert!(!s.contains("--fix"), "no hint when not fixable: {s}");
    }

    #[test]
    fn json_body_shape() {
        let rows = vec![
            Row::ok("repository", "/path/.git".into()),
            Row::info("journal", "last op \"x\" 1m ago".into()),
            Row::warn_fixable("gc config", "not never".into()),
        ];
        let body = json_body(&rows);
        assert_eq!(body["findings"], 1);
        assert_eq!(body["fixable"], 1);
        assert_eq!(body["checks"].as_array().unwrap().len(), 3);
        assert_eq!(body["checks"][0]["level"], "ok");
        assert_eq!(body["checks"][1]["level"], "info");
        assert_eq!(body["checks"][2]["level"], "warn");
    }
}
