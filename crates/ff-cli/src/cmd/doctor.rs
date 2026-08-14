//! Doctor verifies the net — read-only by design (it must never absorb the
//! foreign drift it reports), one consented write behind `--fix`.

use ff_core::{Error, Result};

enum Level {
    Ok,
    Info,
    Warn,
}

struct Row {
    level: Level,
    name: &'static str,
    detail: String,
    fixable: bool,
}

impl Row {
    fn ok(name: &'static str, detail: String) -> Self {
        Self {
            level: Level::Ok,
            name,
            detail,
            fixable: false,
        }
    }

    fn info(name: &'static str, detail: String) -> Self {
        Self {
            level: Level::Info,
            name,
            detail,
            fixable: false,
        }
    }

    fn warn(name: &'static str, detail: String) -> Self {
        Self {
            level: Level::Warn,
            name,
            detail,
            fixable: false,
        }
    }

    fn warn_fixable(name: &'static str, detail: String) -> Self {
        Self {
            level: Level::Warn,
            name,
            detail,
            fixable: true,
        }
    }
}

/// Pad a string to `width` characters (escape-safe: pad is added after the
/// visible text so ANSI bytes never inflate the column width).
fn pad(text: &str, width: usize) -> String {
    let len = text.chars().count();
    let extra = width.saturating_sub(len);
    format!("{text}{}", " ".repeat(extra))
}

fn format_row(row: &Row, colored: bool) -> String {
    let style = match row.level {
        Level::Ok => anstyle::AnsiColor::Green.on_default(),
        Level::Info => anstyle::Style::new().dimmed(),
        Level::Warn => anstyle::AnsiColor::Yellow.on_default(),
    };

    let level_text = match row.level {
        Level::Ok => "ok",
        Level::Info => "info",
        Level::Warn => "WARN",
    };

    if colored {
        match row.level {
            Level::Ok => {
                let painted = format!("{}{}{}", style.render(), level_text, style.render_reset());
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
                let painted_level =
                    format!("{}{}{}", style.render(), level_text, style.render_reset());
                let painted_name =
                    format!("{}{}{}", style.render(), row.name, style.render_reset());
                let painted_detail =
                    format!("{}{}{}", style.render(), row.detail, style.render_reset());
                let level_pad = " ".repeat(6usize.saturating_sub(level_text.chars().count()));
                let name_pad = " ".repeat(15usize.saturating_sub(row.name.chars().count()));
                format!(
                    "  {}{}{}{}{}",
                    painted_level, level_pad, painted_name, name_pad, painted_detail
                )
            }
            Level::Warn => {
                let painted = format!("{}{}{}", style.render(), level_text, style.render_reset());
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
    } else {
        format!(
            "  {}{}{}",
            pad(level_text, 6),
            pad(row.name, 15),
            row.detail
        )
    }
}

fn summary_text(findings: usize, fixable: usize, fix: bool) -> String {
    if findings == 0 {
        "no findings — the net is under you".into()
    } else {
        let mut s = format!("{findings} finding(s)");
        if fixable > 0 && !fix {
            s.push_str(" — `ff doctor --fix` repairs the gc config");
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

fn tip_time(repo: &ff_core::gix::Repository, id: ff_core::gix::ObjectId) -> Result<i64> {
    let obj = repo.find_object(id).map_err(Error::repo)?;
    let commit = ff_core::gix::objs::CommitRef::from_bytes(&obj.data).map_err(Error::repo)?;
    Ok(commit.committer.time().map_err(Error::repo)?.seconds)
}

// ── Pure row builders (unit-testable, no IO) ─────────────────────────────

fn hooks_row(wiring: &crate::cmd::hook::AgentWiring) -> Row {
    match wiring {
        crate::cmd::hook::AgentWiring::Events {
            path,
            pre_tool,
            prompt,
        } if *pre_tool && *prompt => Row::ok(
            "claude hooks",
            format!("`ff hook agent trigger claude` wired in {}", path.display()),
        ),
        crate::cmd::hook::AgentWiring::Events {
            pre_tool, prompt, ..
        } if !pre_tool && !prompt => Row::info(
            "claude hooks",
            "not wired (optional — `ff hook agent install`)".into(),
        ),
        crate::cmd::hook::AgentWiring::Events {
            pre_tool,
            prompt: _,
            ..
        } => {
            let (wired, missing) = if *pre_tool {
                ("PreToolUse", "UserPromptSubmit")
            } else {
                ("UserPromptSubmit", "PreToolUse")
            };
            Row::warn(
                "claude hooks",
                format!(
                    "{wired} wired but {missing} missing — capture is partial (`ff hook agent install` repairs)"
                ),
            )
        }
        crate::cmd::hook::AgentWiring::Unavailable(complaint) => {
            Row::info("claude hooks", complaint.clone())
        }
    }
}

fn alias_row(states: &[crate::cmd::shell::ShellAlias]) -> Row {
    if let Some(entry) = states
        .iter()
        .find(|e| e.state == crate::cmd::shell::AliasState::Installed)
    {
        Row::ok(
            "alias",
            format!(
                "git='ff git' installed in {} (`ff hook shell` manages it)",
                entry.rc.as_ref().unwrap().display()
            ),
        )
    } else if let Some(entry) = states
        .iter()
        .find(|e| e.state == crate::cmd::shell::AliasState::HandWritten)
    {
        Row::info(
            "alias",
            format!(
                "found in {} (heuristic — check `type git` in your shell)",
                entry.rc.as_ref().unwrap().display()
            ),
        )
    } else {
        Row::info(
            "alias",
            "no `ff git` alias found in shell rc files (heuristic)".into(),
        )
    }
}

fn triggers_row(
    wiring: &crate::cmd::hook::AgentWiring,
    states: &[crate::cmd::shell::ShellAlias],
) -> Option<Row> {
    let agent_wired = match wiring {
        crate::cmd::hook::AgentWiring::Events {
            pre_tool, prompt, ..
        } => *pre_tool || *prompt,
        crate::cmd::hook::AgentWiring::Unavailable(_) => false,
    };
    let shell_wired = states
        .iter()
        .any(|e| e.state != crate::cmd::shell::AliasState::Absent);
    if !agent_wired && !shell_wired {
        Some(Row::warn(
            "triggers",
            "neither the alias nor the claude hooks are wired — snapshots only happen when you run `ff` by hand (`ff hook agent install`, `ff hook shell install`)".into(),
        ))
    } else {
        None
    }
}

fn update_row() -> Row {
    use crate::selfupdate::notify::CheckStatus;
    match crate::selfupdate::notify::check_status(env!("CARGO_PKG_VERSION")) {
        CheckStatus::Unofficial => {
            Row::info("update", "source build — updates via cargo install".into())
        }
        CheckStatus::NoCheckYet => Row::info("update", "no check yet".into()),
        CheckStatus::Available(tag) => {
            Row::info("update", format!("{tag} available — `ff update`"))
        }
        CheckStatus::UpToDate => Row::info(
            "update",
            format!("up to date (v{})", env!("CARGO_PKG_VERSION")),
        ),
    }
}

pub fn run(fix: bool, json: bool) -> Result<()> {
    // No capture call — doctor observes; capturing would absorb the very drift
    // the journal check reports.

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);

    let colored = !matches!(
        anstream::AutoStream::choice(&std::io::stdout()),
        anstream::ColorChoice::Never
    );

    let mut rows: Vec<Row> = Vec::new();

    // repository
    let repo: Option<ff_core::gix::Repository> = match ff_core::discover(".") {
        Ok(r) => {
            if r.workdir().is_none() {
                rows.push(Row::info(
                    "repository",
                    "bare — nothing to snapshot, repo checks skipped".into(),
                ));
                None
            } else {
                let repo_path = r
                    .git_dir()
                    .canonicalize()
                    .map(|p| p.display().to_string())
                    .unwrap_or_else(|_| r.git_dir().display().to_string());
                rows.push(Row::ok("repository", repo_path));
                Some(r)
            }
        }
        Err(_) => {
            rows.push(Row::info(
                "repository",
                "not inside a git repository — repo checks skipped".into(),
            ));
            None
        }
    };

    if let Some(repo) = &repo {
        // chains
        let mut chain_data: Vec<(String, ff_core::gix::ObjectId)> = Vec::new();
        let mut chain_full_names: Vec<String> = Vec::new();
        {
            let platform = repo.references().map_err(Error::repo)?;
            let iter = platform
                .prefixed(ff_core::snapshot::chain::SNAP_PREFIX)
                .map_err(Error::repo)?;
            for reference in iter {
                let reference =
                    reference.map_err(|err| Error::msg(format!("ref iteration failed: {err}")))?;
                let name = reference.name().as_bstr().to_string();
                let Some(tip) = reference.target().try_id().map(|id| id.to_owned()) else {
                    continue;
                };
                let short = name
                    .strip_prefix(ff_core::snapshot::chain::SNAP_PREFIX)
                    .unwrap_or(&name)
                    .to_string();
                chain_full_names.push(name);
                chain_data.push((short, tip));
            }
        }

        let has_chains = !chain_data.is_empty();

        if has_chains {
            let mut parts: Vec<String> = Vec::new();
            for (short, tip) in &chain_data {
                let age = crate::render::relative_age(now, tip_time(&repo, *tip)?);
                parts.push(format!("{short} {age}"));
            }
            rows.push(Row::ok(
                "chains",
                format!("{} chain(s): {}", chain_data.len(), parts.join(", ")),
            ));

            // identity
            let mut offenders: Vec<String> = Vec::new();
            for (short, tip) in &chain_data {
                if !ff_core::snapshot::chain::id_is_snapshot(&repo, *tip)? {
                    offenders.push(short.clone());
                }
            }
            if offenders.is_empty() {
                rows.push(Row::ok(
                    "identity",
                    "chain tips carry fufu <fufu@local>".into(),
                ));
            } else {
                for short in &offenders {
                    rows.push(Row::warn(
                    "identity",
                    format!(
                        "{short} tip is not a fufu snapshot commit — the chain was moved by something other than fufu"
                    ),
                ));
                }
            }

            // reflogs
            let mut missing_reflogs: Vec<String> = Vec::new();
            for full_name in &chain_full_names {
                let Some(reference) = repo.try_find_reference(full_name).map_err(Error::repo)?
                else {
                    let short = full_name
                        .strip_prefix(ff_core::snapshot::chain::SNAP_PREFIX)
                        .unwrap_or(full_name);
                    missing_reflogs.push(short.to_string());
                    continue;
                };
                let mut platform = reference.log_iter();
                let has_log = platform.all().map_err(Error::repo)?.is_some();
                if !has_log {
                    let short = full_name
                        .strip_prefix(ff_core::snapshot::chain::SNAP_PREFIX)
                        .unwrap_or(full_name);
                    missing_reflogs.push(short.to_string());
                }
            }
            if missing_reflogs.is_empty() {
                rows.push(Row::ok("reflogs", "every chain ref has a reflog".into()));
            } else {
                for name in &missing_reflogs {
                    rows.push(Row::warn(
                        "reflogs",
                        format!("{name} has no reflog — @{{time}} queries will not work on it"),
                    ));
                }
            }

            // gc config
            let config_path = repo.common_dir().join("config");
            let config_file = ff_core::snapshot::config::load_config_file(
                &config_path,
                ff_core::gix::config::Source::Local,
            )?;

            let all_never = ff_core::snapshot::config::GC_KEYS.iter().all(|key| {
                config_file
                    .string(
                        format!("gc.{}.{}", ff_core::snapshot::config::GC_SUBSECTION, key).as_str(),
                    )
                    .is_some_and(|v| *v == "never")
            });

            if all_never {
                rows.push(Row::ok(
                    "gc config",
                    "reflog expiry disabled for refs/fufu/*".into(),
                ));
            } else if fix {
                ff_core::snapshot::config::force_gc_config(&repo)?;
                rows.push(Row::ok(
                    "gc config",
                    "reflog expiry disabled for refs/fufu/* (fixed)".into(),
                ));
            } else {
                rows.push(Row::warn_fixable(
                "gc config",
                "gc.refs/fufu/*.reflogExpire{,Unreachable} not `never` — a manual `git gc` could expire fufu reflog entries (--fix writes them)".into(),
            ));
            }
        } else {
            rows.push(Row::warn(
            "chains",
            "no refs/fufu/snap/* refs — the engine has never run here (run `ff`, or any git command via the alias)".into(),
        ));
        }

        // trash
        {
            let mut trash_parts: Vec<String> = Vec::new();
            let platform = repo.references().map_err(Error::repo)?;
            let iter = platform
                .prefixed(ff_core::snapshot::chain::TRASH_PREFIX)
                .map_err(Error::repo)?;
            for reference in iter {
                let reference =
                    reference.map_err(|err| Error::msg(format!("ref iteration failed: {err}")))?;
                let name = reference.name().as_bstr().to_string();
                let Some(tip) = reference.target().try_id().map(|id| id.to_owned()) else {
                    continue;
                };
                let short = name
                    .strip_prefix(ff_core::snapshot::chain::TRASH_PREFIX)
                    .unwrap_or(&name);
                let age = crate::render::relative_age(now, tip_time(&repo, tip)?);
                trash_parts.push(format!("{short} {age}"));
            }
            if !trash_parts.is_empty() {
                rows.push(Row::info(
                    "trash",
                    format!(
                        "{} — pre-trim tips held until the next trim",
                        trash_parts.join(", ")
                    ),
                ));
            }
        }

        // journal
        match ff_core::journal::tip(&repo)? {
            None => {
                rows.push(Row::info(
                    "journal",
                    "no journal yet — initializes on the first fufu operation".into(),
                ));
            }
            Some(tip) => match ff_core::journal::read_entry(&repo, tip) {
                Err(err) => {
                    rows.push(Row::warn(
                        "journal",
                        format!("journal tip does not parse — {err}"),
                    ));
                }
                Ok(entry) => {
                    let age = crate::render::relative_age(now, entry.record.time);
                    let summary_trunc = crate::provenance::truncate(&entry.record.summary, 60);
                    rows.push(Row::info(
                        "journal",
                        format!("last op \"{}\" {age}", summary_trunc),
                    ));

                    // drift
                    let current = ff_core::journal::observe_refs(&repo)?;
                    let drift = entry.refs.diff(&current);
                    if !drift.is_empty() {
                        rows.push(Row::info(
                        "drift",
                        format!(
                            "{} ref(s) moved outside fufu — absorbed on the next fufu operation",
                            drift.len()
                        ),
                    ));
                    }
                }
            },
        }

        // parked
        {
            let mut parked_parts: Vec<String> = Vec::new();
            let platform = repo.references().map_err(Error::repo)?;
            let iter = platform
                .prefixed(ff_core::stash::PARKED_PREFIX)
                .map_err(Error::repo)?;
            for reference in iter {
                let reference =
                    reference.map_err(|err| Error::msg(format!("ref iteration failed: {err}")))?;
                let name = reference.name().as_bstr().to_string();
                let short = name
                    .strip_prefix(ff_core::stash::PARKED_PREFIX)
                    .unwrap_or(&name);
                parked_parts.push(short.to_string());
            }
            if !parked_parts.is_empty() {
                rows.push(Row::info(
                    "parked",
                    format!("tree memory held for: {}", parked_parts.join(", ")),
                ));
            }
        }

        // settings
        let snap = repo.config_snapshot();
        let file = snap.plumbing();

        let mut keep_valid = true;
        let mut keep_display = "90d".to_string();
        let mut non_default: Vec<String> = Vec::new();

        for setting in crate::cmd::config::registry() {
            let raw = file.string(setting.key);
            if let Some(raw_val) = raw {
                let raw_str = raw_val.to_string();
                let is_default = raw_str == setting.def;
                let valid = crate::cmd::config::value_is_valid(setting, &raw_str);

                if setting.name == "keep" {
                    keep_valid = valid;
                    keep_display = raw_str.clone();
                }

                if !valid {
                    rows.push(Row::warn(
                        "settings",
                        format!(
                            "fufu.{} is \"{raw_str}\" — invalid (`ff config {}` explains)",
                            setting.name, setting.name
                        ),
                    ));
                } else if !is_default {
                    non_default.push(format!("{0} {1}", setting.name, raw_str));
                }
            }
        }

        if non_default.is_empty() {
            rows.push(Row::info("settings", "all defaults".into()));
        } else {
            rows.push(Row::info("settings", non_default.join(", ")));
        }

        // trim preview — skip if fufu.keep was present-and-invalid
        if keep_valid {
            let keep_secs = ff_core::snapshot::config::keep_secs(&repo)?;
            let report = ff_core::trim(
                &repo,
                &ff_core::TrimOptions {
                    now: Some(now),
                    dry_run: true,
                    gone: false,
                    keep_secs: Some(keep_secs),
                },
            )?;
            let total_dropped: usize = report.chains.iter().map(|c| c.dropped).sum();

            if total_dropped > 0 {
                rows.push(Row::info(
                    "trim",
                    format!(
                        "{} snapshot(s) older than {} — `ff trim` drops them (--dry-run previews)",
                        total_dropped, keep_display
                    ),
                ));
            } else {
                rows.push(Row::info(
                    "trim",
                    "nothing to drop — every snapshot is inside the keep window".into(),
                ));
            }
        }
    }

    // wiring + update checks — always run
    let wiring = crate::cmd::hook::agent_wiring();
    let aliases = crate::cmd::shell::alias_states();

    rows.push(hooks_row(&wiring));
    rows.push(alias_row(&aliases));
    if let Some(row) = triggers_row(&wiring, &aliases) {
        rows.push(row);
    }
    rows.push(update_row());

    if let Some(repo) = &repo {
        crate::selfupdate::notify::maybe_spawn_check(repo);
    }

    render(&rows, fix, json, colored);

    Ok(())
}

fn render(rows: &[Row], fix: bool, json: bool, colored: bool) {
    let findings = rows
        .iter()
        .filter(|r| matches!(r.level, Level::Warn))
        .count();
    let fixable = rows
        .iter()
        .filter(|r| matches!(r.level, Level::Warn) && r.fixable)
        .count();

    if json {
        let body = serde_json::to_string(&json_body(rows)).map_err(|e| {
            eprintln!("ff: {e}");
            std::process::exit(1);
        });
        println!("{}", body.unwrap());
    } else {
        for row in rows {
            println!("{}", format_row(row, colored));
        }

        println!();

        let sum = summary_text(findings, fixable, fix);
        if colored && findings == 0 {
            println!(
                "{}{}{}",
                anstyle::AnsiColor::Green.on_default().render(),
                sum,
                anstyle::AnsiColor::Green.on_default().render_reset()
            );
        } else {
            println!("{}", sum);
        }
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
        let row = Row::warn("chains", "no refs".into());
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

    // ------------------------------------------------------------------
    // hooks_row
    // ------------------------------------------------------------------

    #[test]
    fn hooks_row_both_wired() {
        let wiring = crate::cmd::hook::AgentWiring::Events {
            path: std::path::PathBuf::from("/home/u/.claude/settings.json"),
            pre_tool: true,
            prompt: true,
        };
        let row = hooks_row(&wiring);
        assert!(matches!(row.level, Level::Ok));
        assert!(row.detail.contains("wired in"));
    }

    #[test]
    fn hooks_row_none_wired() {
        let wiring = crate::cmd::hook::AgentWiring::Events {
            path: std::path::PathBuf::from("/home/u/.claude/settings.json"),
            pre_tool: false,
            prompt: false,
        };
        let row = hooks_row(&wiring);
        assert!(matches!(row.level, Level::Info));
        assert!(row.detail.contains("not wired"));
    }

    #[test]
    fn hooks_row_partial_pre_tool_only() {
        let wiring = crate::cmd::hook::AgentWiring::Events {
            path: std::path::PathBuf::from("/home/u/.claude/settings.json"),
            pre_tool: true,
            prompt: false,
        };
        let row = hooks_row(&wiring);
        assert!(matches!(row.level, Level::Warn));
        assert!(
            row.detail
                .contains("PreToolUse wired but UserPromptSubmit missing")
        );
    }

    #[test]
    fn hooks_row_partial_prompt_only() {
        let wiring = crate::cmd::hook::AgentWiring::Events {
            path: std::path::PathBuf::from("/home/u/.claude/settings.json"),
            pre_tool: false,
            prompt: true,
        };
        let row = hooks_row(&wiring);
        assert!(matches!(row.level, Level::Warn));
        assert!(
            row.detail
                .contains("UserPromptSubmit wired but PreToolUse missing")
        );
    }

    #[test]
    fn hooks_row_unavailable() {
        let wiring = crate::cmd::hook::AgentWiring::Unavailable("HOME is not set".into());
        let row = hooks_row(&wiring);
        assert!(matches!(row.level, Level::Info));
        assert!(row.detail.contains("HOME is not set"));
    }

    // ------------------------------------------------------------------
    // alias_row
    // ------------------------------------------------------------------

    #[test]
    fn alias_row_installed_beats_hand_written() {
        let states = vec![
            crate::cmd::shell::ShellAlias {
                shell: "bash",
                state: crate::cmd::shell::AliasState::HandWritten,
                rc: Some(std::path::PathBuf::from("/home/u/.bashrc")),
            },
            crate::cmd::shell::ShellAlias {
                shell: "zsh",
                state: crate::cmd::shell::AliasState::Installed,
                rc: Some(std::path::PathBuf::from("/home/u/.zshrc")),
            },
        ];
        let row = alias_row(&states);
        assert!(matches!(row.level, Level::Ok));
        assert!(row.detail.contains("installed"));
    }

    #[test]
    fn alias_row_hand_written() {
        let states = vec![
            crate::cmd::shell::ShellAlias {
                shell: "bash",
                state: crate::cmd::shell::AliasState::Absent,
                rc: Some(std::path::PathBuf::from("/home/u/.bashrc")),
            },
            crate::cmd::shell::ShellAlias {
                shell: "zsh",
                state: crate::cmd::shell::AliasState::HandWritten,
                rc: Some(std::path::PathBuf::from("/home/u/.zshrc")),
            },
        ];
        let row = alias_row(&states);
        assert!(matches!(row.level, Level::Info));
        assert!(row.detail.contains("found in"));
    }

    #[test]
    fn alias_row_all_absent() {
        let states = vec![crate::cmd::shell::ShellAlias {
            shell: "bash",
            state: crate::cmd::shell::AliasState::Absent,
            rc: Some(std::path::PathBuf::from("/home/u/.bashrc")),
        }];
        let row = alias_row(&states);
        assert!(matches!(row.level, Level::Info));
        assert!(row.detail.contains("no `ff git` alias"));
    }

    // ------------------------------------------------------------------
    // triggers_row
    // ------------------------------------------------------------------

    #[test]
    fn triggers_row_nothing_wired() {
        let wiring = crate::cmd::hook::AgentWiring::Events {
            path: std::path::PathBuf::from("/home/u/.claude/settings.json"),
            pre_tool: false,
            prompt: false,
        };
        let states = vec![crate::cmd::shell::ShellAlias {
            shell: "bash",
            state: crate::cmd::shell::AliasState::Absent,
            rc: Some(std::path::PathBuf::from("/home/u/.bashrc")),
        }];
        let row = triggers_row(&wiring, &states);
        assert!(row.is_some());
        let row = row.unwrap();
        assert!(matches!(row.level, Level::Warn));
        assert!(
            row.detail
                .contains("neither the alias nor the claude hooks")
        );
    }

    #[test]
    fn triggers_row_hooks_partial_only() {
        let wiring = crate::cmd::hook::AgentWiring::Events {
            path: std::path::PathBuf::from("/home/u/.claude/settings.json"),
            pre_tool: true,
            prompt: false,
        };
        let states = vec![crate::cmd::shell::ShellAlias {
            shell: "bash",
            state: crate::cmd::shell::AliasState::Absent,
            rc: Some(std::path::PathBuf::from("/home/u/.bashrc")),
        }];
        assert!(triggers_row(&wiring, &states).is_none());
    }

    #[test]
    fn triggers_row_hand_written_alias_only() {
        let wiring = crate::cmd::hook::AgentWiring::Events {
            path: std::path::PathBuf::from("/home/u/.claude/settings.json"),
            pre_tool: false,
            prompt: false,
        };
        let states = vec![crate::cmd::shell::ShellAlias {
            shell: "bash",
            state: crate::cmd::shell::AliasState::HandWritten,
            rc: Some(std::path::PathBuf::from("/home/u/.bashrc")),
        }];
        assert!(triggers_row(&wiring, &states).is_none());
    }

    #[test]
    fn triggers_row_all_wired() {
        let wiring = crate::cmd::hook::AgentWiring::Events {
            path: std::path::PathBuf::from("/home/u/.claude/settings.json"),
            pre_tool: true,
            prompt: true,
        };
        let states = vec![crate::cmd::shell::ShellAlias {
            shell: "bash",
            state: crate::cmd::shell::AliasState::Installed,
            rc: Some(std::path::PathBuf::from("/home/u/.bashrc")),
        }];
        assert!(triggers_row(&wiring, &states).is_none());
    }

    // ------------------------------------------------------------------
    // update_row
    // ------------------------------------------------------------------

    #[test]
    fn update_row_unofficial() {
        let row = update_row();
        // In dev builds (non-official), this should be the unofficial message.
        // The exact variant depends on compile-time FF_OFFICIAL_BUILD.
        assert_eq!(row.name, "update");
        assert!(matches!(row.level, Level::Info));
    }
}
