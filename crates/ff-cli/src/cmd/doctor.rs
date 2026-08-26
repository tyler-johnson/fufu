//! Doctor verifies the net — read-only by design (it must never absorb the
//! foreign drift it reports), one consented write behind `--fix`.

use ff_core::{Error, Result};

use crate::ctx::Ctx;

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

fn tip_time(repo: &ff_core::gix::Repository, id: ff_core::gix::ObjectId) -> Result<i64> {
    let obj = repo.find_object(id).map_err(Error::repo)?;
    let commit = ff_core::gix::objs::CommitRef::from_bytes(&obj.data).map_err(Error::repo)?;
    Ok(commit.committer.time().map_err(Error::repo)?.seconds)
}

/// Whether the shared copy of `branch` still exists on its remote: `None`
/// when no tracking ref can even be named (no upstream, or a section that
/// names one without a `merge`), `Some(true)`/`Some(false)` for present
/// once it can. The two remote-floor rows both ask this question, so it is
/// derived once — the same way `ff_core::upstream` does — rather than twice.
fn tracking_state(repo: &ff_core::gix::Repository, branch: &str) -> Result<Option<bool>> {
    let full: ff_core::gix::refs::FullName = format!("refs/heads/{branch}")
        .as_str()
        .try_into()
        .map_err(Error::repo)?;
    let Some(tracking) =
        repo.branch_remote_tracking_ref_name(full.as_ref(), ff_core::gix::remote::Direction::Fetch)
    else {
        return Ok(None);
    };
    let tracking = tracking.map_err(Error::repo)?;
    match repo.try_find_reference(tracking.as_ref()) {
        Ok(Some(_)) => Ok(Some(true)),
        Ok(None) => Ok(Some(false)),
        Err(err) => Err(Error::repo(err)),
    }
}

/// Loose objects and packs, counted where `git count-objects` counts them:
/// the 256 fanout directories, then the pack directory. Best effort — an
/// unreadable objects directory reports zeroes rather than failing a
/// read-only check.
fn count_objects(objects_dir: &std::path::Path) -> (usize, usize) {
    let dir = |path: std::path::PathBuf| std::fs::read_dir(path).into_iter().flatten().flatten();
    let loose = dir(objects_dir.to_path_buf())
        .filter(|entry| {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            name.len() == 2 && name.bytes().all(|b| b.is_ascii_hexdigit())
        })
        .map(|fanout| dir(fanout.path()).count())
        .sum();
    let packs = dir(objects_dir.join("pack"))
        .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "pack"))
        .count();
    (loose, packs)
}

// ── Pure row builders (unit-testable, no IO) ─────────────────────────────

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
fn wiring_rows(
    statuses: &[crate::integ::Status],
    repo: Option<&ff_core::gix::Repository>,
    fix: bool,
) -> Vec<Row> {
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
    rows.push(ambient_row(statuses, repo));
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

fn ambient_row(statuses: &[crate::integ::Status], repo: Option<&ff_core::gix::Repository>) -> Row {
    if let Some(repo) = repo
        && !repo
            .config_snapshot()
            .boolean("fufu.ambient")
            .unwrap_or(true)
    {
        return Row::info(
            "ambient",
            "off (the ambient setting) — the prompt channel is silent".into(),
        );
    }
    piece_row(
        statuses,
        "ambient",
        "ambient",
        "prompt hook wired in",
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

pub fn run(ctx: &Ctx, fix: bool) -> Result<()> {
    // No capture call — doctor observes; capturing would absorb the very drift
    // the journal check reports.

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);

    let colored = crate::pager::color_enabled();

    let mut rows: Vec<Row> = Vec::new();

    // repository
    let repo: Option<ff_core::gix::Repository> = match ff_core::discover(".") {
        Ok(r) => {
            crate::render::init_palette(&r);
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
        // The log. One ref, so one row — the per-branch refs under
        // BRANCH_PREFIX are pointers *into* it and are reported as such
        // below, rather than as a second population of chains.
        let log_tip = ff_core::ops::OpLog::open(repo)?.tip()?;

        if let Some(tip) = log_tip.map(|id| id.object_id()) {
            let age = crate::render::relative_age(now, tip_time(repo, tip)?);
            rows.push(Row::ok(
                "log",
                format!("{}, newest operation {age}", ff_core::ops::ops_ref_of(repo)),
            ));

            // identity — `is_op_commit` and not "does it bear the fufu
            // identity": a record commit bears the identity too.
            if ff_core::ops::is_op_commit(repo, tip)? {
                rows.push(Row::ok(
                    "identity",
                    "the log tip is a fufu operation".into(),
                ));
            } else {
                rows.push(Row::warn(
                    "identity",
                    "the log tip is not a fufu operation — the ref was moved by something \
                     other than fufu"
                        .into(),
                ));
            }

            // Pointers into the log: one per branch, the newest operation on
            // each. A pointer naming an operation the log has not got is the
            // one thing the two-ref append exists to prevent.
            let mut pointers: Vec<String> = Vec::new();
            {
                let platform = repo.references().map_err(Error::repo)?;
                let iter = platform
                    .prefixed(ff_core::ops::BRANCH_PREFIX)
                    .map_err(Error::repo)?;
                for reference in iter {
                    let reference = reference.map_err(|err| {
                        Error::coded(
                            "op/unreadable",
                            format!("ref iteration failed: {err}"),
                            vec![],
                        )
                    })?;
                    let name = reference.name().as_bstr().to_string();
                    let Some(at) = reference.target().try_id().map(|id| id.to_owned()) else {
                        continue;
                    };
                    let short = name
                        .strip_prefix(ff_core::ops::BRANCH_PREFIX)
                        .unwrap_or(&name)
                        .to_string();
                    let age = crate::render::relative_age(now, tip_time(repo, at)?);
                    pointers.push(format!("{short} {age}"));
                }
            }
            if !pointers.is_empty() {
                rows.push(Row::ok(
                    "pointers",
                    format!(
                        "{} branch pointer(s) into the log: {}",
                        pointers.len(),
                        pointers.join(", ")
                    ),
                ));
            }

            // The log's reflog is load-bearing now, not merely nice to have:
            // `ff undo` steps the ref back rather than appending, so where
            // the pointer has stood is recorded only here — and that is what
            // `ff redo` walks forward along and what keeps an abandoned
            // branch of the log addressable.
            let has_reflog = match repo
                .try_find_reference(ff_core::ops::ops_ref_of(repo).as_str())
                .map_err(Error::repo)?
            {
                None => false,
                Some(reference) => {
                    let mut platform = reference.log_iter();
                    platform.all().map_err(Error::repo)?.is_some()
                }
            };
            if has_reflog {
                rows.push(Row::ok(
                    "reflogs",
                    "the log ref has a reflog — undo and redo have somewhere to record where \
                     the pointer has stood"
                        .into(),
                ));
            } else {
                rows.push(Row::warn(
                    "reflogs",
                    "the log ref has no reflog — ff redo cannot walk forward, and --at cannot \
                     answer questions about where the log has been"
                        .into(),
                ));
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
                ff_core::snapshot::config::force_gc_config(repo)?;
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
            "log",
            format!("no {} — the engine has never run here (run `ff`, or any git command via the alias)", ff_core::ops::ops_ref_of(repo)),
        ));
        }

        // trash
        {
            let mut trash_parts: Vec<String> = Vec::new();
            let platform = repo.references().map_err(Error::repo)?;
            let iter = platform
                .prefixed(ff_core::ops::WT_PREFIX)
                .map_err(Error::repo)?;
            for reference in iter {
                let reference = reference.map_err(|err| {
                    Error::coded(
                        "op/unreadable",
                        format!("ref iteration failed: {err}"),
                        vec![],
                    )
                })?;
                let name = reference.name().as_bstr().to_string();
                let Some(chain) = name
                    .strip_prefix(ff_core::ops::WT_PREFIX)
                    .and_then(|rest| rest.strip_suffix("/trash/@ops"))
                else {
                    continue;
                };
                let Some(tip) = reference.target().try_id().map(|id| id.to_owned()) else {
                    continue;
                };
                let age = crate::render::relative_age(now, tip_time(repo, tip)?);
                trash_parts.push(format!("{chain} {age}"));
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

        // objects
        {
            let (loose, packs) = count_objects(&repo.common_dir().join("objects"));
            // fufu writes objects natively, so nothing here ever triggers
            // git's auto-gc on its own; `ff trim` is what nudges it.
            let auto = repo
                .config_snapshot()
                .integer("gc.auto")
                .unwrap_or(6700)
                .max(0) as usize;
            let summary = format!(
                "{loose} loose, {packs} pack{}",
                if packs == 1 { "" } else { "s" }
            );
            if auto > 0 && loose >= auto {
                rows.push(Row::info(
                    "objects",
                    format!("{summary} — past gc.auto ({auto}); `ff trim` nudges git to pack them"),
                ));
            } else {
                rows.push(Row::ok("objects", summary));
            }
        }

        // id index — read-only: reports what's there, never rebuilds. An
        // absent or stale index is a fact, not a finding: both self-heal on
        // the next `ff log`/`ff evolog`. One log means one index, so this is
        // one row rather than one per chain.
        {
            let detail = match ff_core::ops::index::status(repo)? {
                ff_core::ops::index::Status::InSync { ids } => {
                    Some(Row::ok("id index", format!("{ids} ids, in sync")))
                }
                ff_core::ops::index::Status::Stale => Some(Row::info(
                    "id index",
                    "stale — rebuilds on the next read".into(),
                )),
                ff_core::ops::index::Status::Absent => Some(Row::info(
                    "id index",
                    "absent — builds on the next log".into(),
                )),
            };
            rows.extend(detail);
        }

        // the operation log
        {
            let log = ff_core::ops::OpLog::open(repo)?;
            match log.tip()? {
                None => {
                    rows.push(Row::info(
                        "last op",
                        "no operations yet — the log opens on the first fufu command".into(),
                    ));
                }
                Some(tip) => match log.get(tip) {
                    Err(err) => {
                        rows.push(Row::warn(
                            "last op",
                            format!("the log tip does not parse — {err}"),
                        ));
                    }
                    Ok(op) => {
                        let age = crate::render::relative_age(now, op.time());
                        let summary_trunc = crate::provenance::truncate(op.summary(), 60);
                        rows.push(Row::info("last op", format!("\"{summary_trunc}\" {age}")));

                        // drift
                        let drift = ff_core::ops::verb::pending_foreign(repo)?;
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
        }

        // the pre-cutover receipt
        {
            let mut parked: Vec<String> = Vec::new();
            let platform = repo.references().map_err(Error::repo)?;
            let iter = platform
                .prefixed("refs/fufu/legacy/")
                .map_err(Error::repo)?;
            for reference in iter {
                let reference = reference.map_err(|err| {
                    Error::coded(
                        "op/unreadable",
                        format!("ref iteration failed: {err}"),
                        vec![],
                    )
                })?;
                parked.push(reference.name().as_bstr().to_string());
            }
            if !parked.is_empty() {
                rows.push(Row::info(
                    "legacy",
                    format!(
                        "{} ref(s) under refs/fufu/legacy/ hold snapshots and operations from \
                         before the one-log cutover; this fufu cannot read them, and they are \
                         kept only so nothing was destroyed silently. Delete them with git when \
                         you no longer want them.",
                        parked.len()
                    ),
                ));
            }
        }

        // parked
        {
            let mut parked_parts: Vec<String> = Vec::new();
            let platform = repo.references().map_err(Error::repo)?;
            let iter = platform
                .prefixed(ff_core::stash::PARKED_PREFIX)
                .map_err(Error::repo)?;
            for reference in iter {
                let reference = reference.map_err(|err| {
                    Error::coded(
                        "op/unreadable",
                        format!("ref iteration failed: {err}"),
                        vec![],
                    )
                })?;
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
            let keep_secs = ff_core::snapshot::config::keep_secs(repo)?;
            let report = ff_core::trim(
                repo,
                &ff_core::TrimOptions {
                    now: Some(now),
                    dry_run: true,
                    gone: false,
                    keep_secs: Some(keep_secs),
                },
            )?;
            let total_dropped: usize = report.pointers.iter().map(|c| c.dropped).sum();

            if total_dropped > 0 {
                rows.push(Row::info(
                    "trim",
                    format!(
                        "{} operation(s) older than {} — `ff trim` drops them (--dry-run previews)",
                        total_dropped, keep_display
                    ),
                ));
            } else {
                rows.push(Row::info(
                    "trim",
                    "nothing to drop — every operation is inside the keep window".into(),
                ));
            }
        }

        // auto-trim lane — reported even when fufu.keep is unreadable
        let encoded = crate::cadence::read_encoded(file, "fufu.autoTrim");
        let at_state = crate::autotrim::load(repo);
        let at_display = file
            .string("fufu.autoTrim")
            .and_then(|v| crate::cadence::parse(&v.to_string()).map(|_| v.to_string()))
            .unwrap_or_else(|| "1d".to_string());
        match crate::cadence::effective(encoded) {
            None => {
                rows.push(Row::info(
                    "auto-trim",
                    "off (the autoTrim setting) — trim runs only by hand".into(),
                ));
            }
            Some(_) if at_state.trimmed_at == 0 => {
                rows.push(Row::info(
                    "auto-trim",
                    format!("on — a trim rides an ff command at most every {at_display}"),
                ));
            }
            Some(_) => {
                rows.push(Row::info(
                    "auto-trim",
                    format!(
                        "last ran {} (at most every {at_display})",
                        crate::render::relative_age(now, at_state.trimmed_at)
                    ),
                ));
            }
        }

        // The remote floor: which local branches can name a remote, what their
        // `[branch "<n>"]` sections point at, and whether the shared copy those
        // sections promise still exists. All three need `repo`, which only
        // exists in this block.
        let local_branches: Vec<String> = {
            let platform = repo.references().map_err(Error::repo)?;
            let iter = platform.prefixed("refs/heads/").map_err(Error::repo)?;
            let mut out = Vec::new();
            for reference in iter {
                let reference = reference.map_err(|err| {
                    Error::coded(
                        "op/unreadable",
                        format!("ref iteration failed: {err}"),
                        vec![],
                    )
                })?;
                out.push(reference.name().shorten().to_string());
            }
            out.sort();
            out
        };

        // remotes — a local-only repository has no remote floor and no
        // finding; otherwise every branch must be able to name a remote.
        {
            let remotes = repo.remote_names();
            if !remotes.is_empty() {
                let ambiguous: Vec<String> = local_branches
                    .iter()
                    .filter(|name| {
                        matches!(
                            ff_core::remote::for_branch(repo, name),
                            ff_core::remote::RemoteChoice::Ambiguous { .. }
                        )
                    })
                    .cloned()
                    .collect();
                if !ambiguous.is_empty() {
                    let list = ambiguous.join(", ");
                    rows.push(Row::warn(
                        "remotes",
                        format!(
                            "no nameable remote for {list} — `ff sync` and `ff publish` both refuse; `ff publish --to <remote>` chooses one"
                        ),
                    ));
                } else {
                    let mut names: Vec<String> = remotes.iter().map(|n| n.to_string()).collect();
                    names.sort();
                    rows.push(Row::ok(
                        "remotes",
                        format!(
                            "{} configured ({}) — every branch names one",
                            names.len(),
                            names.join(", ")
                        ),
                    ));
                }
            }
        }

        // upstreams — `[branch "<n>"]` sections naming branches that are not
        // here. A plain `ff branch delete` of a published branch deliberately
        // keeps both the section and its tracking ref, and says so, so that
        // undo stays exact — that residue is `info`, not a warning. Only a
        // section whose shared copy is *also* gone is repairable, and only
        // under `--fix`, via `remove_branch_section` and nothing else.
        {
            let config_path = repo.common_dir().join("config");
            let file = ff_core::snapshot::config::load_config_file(
                &config_path,
                ff_core::gix::config::Source::Local,
            )?;
            let mut surviving = Vec::new();
            let mut gone = Vec::new();
            for section in file.sections_by_name("branch").into_iter().flatten() {
                let Some(subsection) = section.header().subsection_name() else {
                    continue;
                };
                let name = subsection.to_string();
                if local_branches.contains(&name) {
                    continue;
                }
                match tracking_state(repo, &name)? {
                    Some(true) => surviving.push(name),
                    // `None` (no ref can even be named) and `Some(false)`
                    // (the ref is gone) both mean the section points at nothing.
                    _ => gone.push(name),
                }
            }
            surviving.sort();
            gone.sort();
            if !surviving.is_empty() {
                let list = surviving.join(", ");
                rows.push(Row::info(
                    "upstreams",
                    format!(
                        "config for {list} names no branch here — the shared copy is still on the remote, which is what `ff branch delete` leaves behind"
                    ),
                ));
            }
            if !gone.is_empty() {
                let list = gone.join(", ");
                if fix {
                    for name in &gone {
                        ff_core::snapshot::config::remove_branch_section(repo, name)?;
                    }
                    rows.push(Row::ok(
                        "upstreams",
                        format!(
                            "removed {} config section(s) that named nothing: {list}",
                            gone.len()
                        ),
                    ));
                } else {
                    rows.push(Row::warn_fixable(
                        "upstreams",
                        format!(
                            "config for {list} names no branch here and no tracking ref either — `ff doctor --fix` removes the section"
                        ),
                    ));
                }
            }
        }

        // tracking — branches that exist but whose upstream's shared copy is
        // gone. `info`, not `warn`: `ff status` already reports it as `remote
        // is gone` for the branch underfoot, so telling the person repo-wide
        // is news, not a problem. Disjoint from `upstreams` by construction —
        // here the branch is here, there it is not.
        {
            let mut missing = Vec::new();
            for name in &local_branches {
                if matches!(tracking_state(repo, name)?, Some(false)) {
                    missing.push(name.clone());
                }
            }
            if !missing.is_empty() {
                let list = missing.join(", ");
                rows.push(Row::info(
                    "tracking",
                    format!(
                        "{list}: upstream configured, tracking ref absent — the shared copy is gone"
                    ),
                ));
            }
        }
    }

    // wiring + update checks — always run
    rows.extend(wiring_rows(&crate::integ::statuses(), repo.as_ref(), fix));
    rows.push(update_row());

    render(&rows, fix, ctx.json, colored);

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
        assert!(matches!(ambient_row(&statuses, None).level, Level::Ok));
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
