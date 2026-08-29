use super::Row;

/// Every wiring finding, folded out of the one `integ::statuses()` vector
/// that `ff hook -l` also renders — so the two commands cannot disagree
/// about what is wired, which is exactly what two hand-written enumerations
/// used to allow.
///
/// `fix` is consented repair: a wiring that still captures but is written
/// in a spelling fufu no longer writes gets rewritten here. That matters
/// because a stored string is only rewritten when somebody runs the
/// installer again, and doctor is the command people run when they are
/// already suspicious.
pub(super) fn wiring_rows(statuses: &[crate::integ::Status], fix: bool) -> Vec<Row> {
    let mut rows: Vec<Row> = Vec::new();
    for status in statuses {
        // The shells answer through the alias and ambient rows below,
        // which report their two pieces separately — except when the
        // wiring is written in a spelling fufu no longer writes, which is
        // about the slug rather than about either piece.
        if !status.parts.is_empty() {
            if status.stale {
                rows.push(fixed_or_fixable(
                    status,
                    "wired, in a retired spelling".into(),
                    &format!("`ff hook {}`", status.slug),
                    fix,
                ));
            }
            continue;
        }
        if let Some(row) = client_row(status, fix) {
            rows.push(row);
        }
    }
    rows.push(alias_row(statuses));
    rows.push(ambient_row(statuses));
    if let Some(row) = skill_row(statuses, fix) {
        rows.push(row);
    }
    if let Some(row) = triggers_row(statuses) {
        rows.push(row);
    }
    rows
}

/// One agent client's row. A client that is neither on this machine nor
/// wired earns no row: doctor reports the net, and a client you do not have
/// is not a hole in it.
fn client_row(status: &crate::integ::Status, fix: bool) -> Option<Row> {
    use crate::integ::Wiring;

    let slug = status.slug;
    let repair = format!("`ff hook {slug}`");
    let with_note = |detail: String| match &status.note {
        Some(note) => format!("{detail} — {note}"),
        None => detail,
    };

    Some(match &status.wiring {
        Wiring::Unavailable(complaint) => Row::info(slug, complaint.clone()),
        Wiring::NotWired if !status.presence.is_present() => return None,
        Wiring::NotWired => Row::info(slug, format!("not wired (optional — {repair})")),
        Wiring::Partial { missing, at } => {
            let detail = format!(
                "wired in {} but {missing} missing — capture is partial",
                at.display()
            );
            fixed_or_fixable(status, detail, &repair, fix)
        }
        Wiring::HandWritten => Row::info(slug, with_note("hand-written — not fufu-managed".into())),
        Wiring::Wired { mechanism, at } => {
            let detail = format!("{} wired in {}", mechanism.word(), at.display());
            if status.stale {
                fixed_or_fixable(
                    status,
                    format!("{detail}, in a retired spelling"),
                    &repair,
                    fix,
                )
            } else {
                Row::ok(slug, with_note(detail))
            }
        }
    })
}

/// The shipped skill, across the clients that read one. Aggregated the way
/// the alias and ambient rows are, because it is one question — is fufu's
/// own manual on this machine — and answering it per client would say the
/// same thing twice.
///
/// A skill is never a finding by its absence. Without it an agent is down
/// to the once-per-session briefing, which costs it spelling and not file
/// state, and the whole point of the split is that the briefing alone is
/// enough to work safely. Drift is the one thing worth a warning, because
/// a manual describing a fufu that has moved teaches commands that fail.
fn skill_row(statuses: &[crate::integ::Status], fix: bool) -> Option<Row> {
    use crate::integ::Wiring;

    let skills: Vec<(&crate::integ::Status, &Wiring)> = statuses
        .iter()
        .filter_map(|status| status.skill.as_ref().map(|wiring| (status, wiring)))
        .filter(|(status, wiring)| status.presence.is_present() || wiring.at().is_some())
        .collect();
    if skills.is_empty() {
        return None;
    }
    // Drift first: it is the only state here anybody has to act on.
    if let Some((status, wiring)) = skills
        .iter()
        .find(|(_, wiring)| matches!(wiring, Wiring::Partial { .. }))
    {
        let at = wiring
            .at()
            .map(|at| at.display().to_string())
            .unwrap_or_default();
        return Some(fixed_or_fixable(
            status,
            format!("an older fufu wrote the skill in {at}"),
            &format!("`ff hook {}`", status.slug),
            fix,
        ));
    }
    let wired: Vec<&str> = skills
        .iter()
        .filter(|(_, wiring)| matches!(wiring, Wiring::Wired { .. }))
        .map(|(status, _)| status.slug)
        .collect();
    if wired.is_empty() {
        let slugs: Vec<&str> = skills.iter().map(|(status, _)| status.slug).collect();
        return Some(Row::info(
            "skill",
            format!(
                "not installed (optional — `ff hook {}` writes fufu's manual for the agent)",
                slugs.join(" ")
            ),
        ));
    }
    Some(Row::ok(
        "skill",
        format!("fufu's manual, for {}", wired.join(", ")),
    ))
}

/// The consented write, and the row that reports it either way.
fn fixed_or_fixable(status: &crate::integ::Status, detail: String, repair: &str, fix: bool) -> Row {
    if !fix {
        return Row::warn_fixable(status.slug, format!("{detail} ({repair} repairs)"));
    }
    let Some(integration) = crate::integ::by_slug(status.slug) else {
        return Row::warn_fixable(status.slug, format!("{detail} ({repair} repairs)"));
    };
    match integration.repair() {
        Ok(_) => Row::ok(status.slug, format!("{detail} (rewired)")),
        // A repair that could not be written is still a finding, and the
        // complaint says why rather than the generic hint.
        Err(err) => Row::warn(status.slug, format!("{detail}; repair failed: {err}")),
    }
}

/// The alias, folded across the three shells. Installed beats hand-written
/// beats absent: one shell wired is the question answered.
fn alias_row(statuses: &[crate::integ::Status]) -> Row {
    piece_row(
        statuses,
        "alias",
        "alias",
        "git='ff git' wired in",
        "no `ff git` alias found in shell rc files (heuristic)",
        "found in the {} rc file (heuristic — check `type git` in your shell)",
    )
}

fn ambient_row(statuses: &[crate::integ::Status]) -> Row {
    piece_row(
        statuses,
        "ambient",
        "ambient",
        "prompt hook snapshots at every prompt, wired in",
        "no prompt hook found in shell rc files (heuristic)",
        "found in the {} rc file (heuristic — check your prompt)",
    )
}

/// One row for a piece the shells wire independently.
fn piece_row(
    statuses: &[crate::integ::Status],
    part_name: &str,
    row_name: &'static str,
    wired_prefix: &str,
    absent: &str,
    hand_written: &str,
) -> Row {
    use crate::integ::Wiring;

    let pieces = statuses
        .iter()
        .flat_map(|status| status.parts.iter().map(move |part| (status.slug, part)))
        .filter(|(_, part)| part.name == part_name);
    let mut hand: Option<&'static str> = None;
    for (slug, part) in pieces {
        match &part.wiring {
            Wiring::Wired { at, .. } => {
                return Row::ok(
                    row_name,
                    format!(
                        "{wired_prefix} {} (`ff hook {slug}` manages it)",
                        at.display(),
                    ),
                );
            }
            Wiring::HandWritten => hand = hand.or(Some(slug)),
            _ => {}
        }
    }
    match hand {
        Some(slug) => Row::info(row_name, hand_written.replace("{}", slug)),
        None => Row::info(row_name, absent.into()),
    }
}

/// The one finding that is about the whole net rather than any one piece
/// of it: nothing at all feeds capture, so snapshots only happen when you
/// type an ff command. A silent engine feels safe while capturing nothing.
fn triggers_row(statuses: &[crate::integ::Status]) -> Option<Row> {
    let fed = statuses.iter().any(|status| {
        status.wiring.feeds_capture() || status.parts.iter().any(|p| p.wiring.feeds_capture())
    });
    if fed {
        return None;
    }
    Some(Row::warn(
        "triggers",
        "nothing is wired — snapshots only happen when you run `ff` by hand (`ff hook` \
         reports what is on this machine and offers to wire it)"
            .into(),
    ))
}

pub(super) fn update_row() -> Row {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cmd::doctor::Level;

    // ------------------------------------------------------------------
    // the wiring rows, all folded out of one statuses() vector
    // ------------------------------------------------------------------

    use crate::integ::{Mechanism, Part, Presence, Status, Wiring};

    fn status(slug: &'static str, wiring: Wiring) -> Status {
        Status {
            slug,
            presence: Presence::Present {
                evidence: std::path::PathBuf::from("/home/u/.claude"),
            },
            wiring,
            note: None,
            parts: Vec::new(),
            skill: None,
            stale: false,
        }
    }

    fn wired() -> Wiring {
        Wiring::Wired {
            mechanism: Mechanism::Settings,
            at: std::path::PathBuf::from("/home/u/.claude/settings.json"),
        }
    }

    fn shell_status(alias: Wiring, ambient: Wiring) -> Status {
        Status {
            slug: "bash",
            presence: Presence::Absent,
            wiring: Wiring::NotWired,
            note: None,
            parts: vec![
                Part {
                    name: "alias",
                    wiring: alias,
                },
                Part {
                    name: "ambient",
                    wiring: ambient,
                },
            ],
            skill: None,
            stale: false,
        }
    }

    fn rc_wired() -> Wiring {
        Wiring::Wired {
            mechanism: Mechanism::Rc,
            at: std::path::PathBuf::from("/home/u/.bashrc"),
        }
    }

    #[test]
    fn a_wired_client_is_ok_and_names_where() {
        let row = client_row(&status("claude", wired()), false).unwrap();
        assert!(matches!(row.level, Level::Ok));
        assert!(row.detail.contains("settings.json"), "{}", row.detail);
    }

    /// Half-wired is a finding, and a fixable one: capture is partial and
    /// the repair is one command.
    #[test]
    fn a_partial_client_is_a_fixable_warning() {
        let row = client_row(
            &status(
                "claude",
                Wiring::Partial {
                    missing: "UserPromptSubmit".into(),
                    at: std::path::PathBuf::from("/home/u/.claude/settings.json"),
                },
            ),
            false,
        )
        .unwrap();
        assert!(matches!(row.level, Level::Warn));
        assert!(row.fixable);
        assert!(row.detail.contains("UserPromptSubmit"), "{}", row.detail);
    }

    /// A retired spelling still captures, so it is never an outage — but it
    /// is what `--fix` exists to rewrite.
    #[test]
    fn stale_wiring_is_a_fixable_warning_rather_than_an_outage() {
        let mut stale = status("claude", wired());
        stale.stale = true;
        let row = client_row(&stale, false).unwrap();
        assert!(matches!(row.level, Level::Warn));
        assert!(row.fixable);
        assert!(row.detail.contains("retired spelling"), "{}", row.detail);
    }

    /// Doctor reports the net; a client you do not have is not a hole in it.
    #[test]
    fn a_client_that_is_neither_present_nor_wired_earns_no_row() {
        let mut absent = status("codex", Wiring::NotWired);
        absent.presence = Presence::Absent;
        assert!(client_row(&absent, false).is_none());
        // Present but unwired is news, not a finding.
        let row = client_row(&status("codex", Wiring::NotWired), false).unwrap();
        assert!(matches!(row.level, Level::Info));
        assert!(row.detail.contains("ff hook codex"), "{}", row.detail);
    }

    #[test]
    fn a_note_reaches_the_row() {
        let mut noted = status("codex", wired());
        noted.note = Some("run /hooks in Codex".into());
        let row = client_row(&noted, false).unwrap();
        assert!(row.detail.contains("run /hooks in Codex"), "{}", row.detail);
    }

    #[test]
    fn the_alias_row_prefers_wired_over_hand_written() {
        let statuses = vec![
            shell_status(Wiring::HandWritten, Wiring::NotWired),
            shell_status(rc_wired(), Wiring::NotWired),
        ];
        let row = alias_row(&statuses);
        assert!(matches!(row.level, Level::Ok));
        assert!(row.detail.contains(".bashrc"), "{}", row.detail);
    }

    #[test]
    fn a_hand_written_alias_is_news_and_not_a_finding() {
        let statuses = vec![shell_status(Wiring::HandWritten, Wiring::NotWired)];
        let row = alias_row(&statuses);
        assert!(matches!(row.level, Level::Info));
        assert!(row.detail.contains("heuristic"), "{}", row.detail);
    }

    /// The bug the alias/ambient split exists to fix, at the doctor level:
    /// a file whose only wiring is the prompt hook must not report the
    /// alias wired.
    #[test]
    fn the_two_shell_pieces_report_independently() {
        let statuses = vec![shell_status(Wiring::NotWired, rc_wired())];
        assert!(matches!(alias_row(&statuses).level, Level::Info));
        assert!(matches!(ambient_row(&statuses).level, Level::Ok));
    }

    #[test]
    fn the_triggers_row_warns_only_when_nothing_at_all_feeds_capture() {
        let nothing = vec![
            status("claude", Wiring::NotWired),
            shell_status(Wiring::NotWired, Wiring::NotWired),
        ];
        let row = triggers_row(&nothing).expect("a warning");
        assert!(matches!(row.level, Level::Warn));
        assert!(row.detail.contains("ff hook"), "{}", row.detail);

        // Anything at all feeding capture silences it — including a
        // hand-written line and a half-finished install.
        assert!(triggers_row(&[status("claude", wired())]).is_none());
        assert!(triggers_row(&[shell_status(Wiring::HandWritten, Wiring::NotWired)]).is_none());
        assert!(
            triggers_row(&[status(
                "claude",
                Wiring::Partial {
                    missing: "UserPromptSubmit".into(),
                    at: std::path::PathBuf::new(),
                }
            )])
            .is_none()
        );
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
