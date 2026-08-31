use ff_core::{Error, Result};

use super::Row;

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

/// The log. One ref, so one row — the per-branch refs under `BRANCH_PREFIX`
/// are pointers *into* it and are reported as such here, rather than as a
/// second population of chains. Covers the log tip, its identity, the
/// pointers, the reflog, and the gc config — they all hang off the one tip —
/// and the gc config row holds one of the two `--fix` writes.
pub(super) fn log_checks(repo: &ff_core::gix::Repository, now: i64, fix: bool) -> Result<Vec<Row>> {
    let mut rows: Vec<Row> = Vec::new();
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
                .string(format!("gc.{}.{}", ff_core::snapshot::config::GC_SUBSECTION, key).as_str())
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

    Ok(rows)
}

/// The trash refs: pre-trim tips held until the next trim.
pub(super) fn trash_row(repo: &ff_core::gix::Repository, now: i64) -> Result<Option<Row>> {
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
    Ok((!trash_parts.is_empty()).then(|| {
        Row::info(
            "trash",
            format!(
                "{} — pre-trim tips held until the next trim",
                trash_parts.join(", ")
            ),
        )
    }))
}

/// The object store: loose count and packs.
pub(super) fn objects_row(repo: &ff_core::gix::Repository) -> Row {
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
        Row::info(
            "objects",
            format!("{summary} — past gc.auto ({auto}); `ff trim` nudges git to pack them"),
        )
    } else {
        Row::ok("objects", summary)
    }
}

/// The id index — read-only: reports what's there, never rebuilds. An
/// absent or stale index is a fact, not a finding: both self-heal on
/// the next `ff log`/`ff evolog`. One log means one index, so this is
/// one row rather than one per chain.
pub(super) fn id_index_row(repo: &ff_core::gix::Repository) -> Result<Row> {
    Ok(match ff_core::ops::index::status(repo)? {
        ff_core::ops::index::Status::InSync { ids } => {
            Row::ok("id index", format!("{ids} ids, in sync"))
        }
        ff_core::ops::index::Status::Stale => {
            Row::info("id index", "stale — rebuilds on the next read".into())
        }
        ff_core::ops::index::Status::Absent => {
            Row::info("id index", "absent — builds on the next log".into())
        }
    })
}

/// The operation log's newest entry, and the drift row that hangs off it.
pub(super) fn last_op_rows(repo: &ff_core::gix::Repository, now: i64) -> Result<Vec<Row>> {
    let mut rows: Vec<Row> = Vec::new();
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
    Ok(rows)
}

/// The pre-cutover receipt: refs an older fufu parked under
/// `refs/fufu/legacy/`.
pub(super) fn legacy_row(repo: &ff_core::gix::Repository) -> Result<Option<Row>> {
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
    Ok((!parked.is_empty()).then(|| {
        Row::info(
            "legacy",
            format!(
                "{} ref(s) under refs/fufu/legacy/ hold snapshots and operations from \
                 before the one-log cutover; this fufu cannot read them, and they are \
                 kept only so nothing was destroyed silently. Delete them with git when \
                 you no longer want them.",
                parked.len()
            ),
        )
    }))
}

/// Parked tree memory, per branch.
pub(super) fn parked_row(repo: &ff_core::gix::Repository) -> Result<Option<Row>> {
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
    Ok((!parked_parts.is_empty()).then(|| {
        Row::info(
            "parked",
            format!("tree memory held for: {}", parked_parts.join(", ")),
        )
    }))
}

/// Settings, the trim preview, and the auto-trim lane. One function on
/// purpose: the settings pass creates the `snap`/`file` config borrows the
/// auto-trim row reuses, and the trim preview consumes the `keep`
/// validity/display the settings pass worked out — splitting them would mean
/// re-reading config.
pub(super) fn settings_checks(repo: &ff_core::gix::Repository, now: i64) -> Result<Vec<Row>> {
    let mut rows: Vec<Row> = Vec::new();

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

    Ok(rows)
}

/// Commit signing: whether it is on, and whether the setup it names will
/// actually work. Read-only and spawn-free — the program is looked for on
/// PATH rather than run, because a doctor that ran gpg could sit on a
/// pinentry prompt nobody asked for.
pub(super) fn signing_row(repo: &ff_core::gix::Repository) -> Row {
    let setup = ff_core::sign::setup(repo);
    if !setup.on {
        return Row::info("signing", "off (commit.gpgsign)".into());
    }
    let Some(format) = setup.format else {
        return Row::warn(
            "signing",
            format!(
                "commit.gpgsign is on, but gpg.format is \"{}\" — fufu signs with openpgp, x509 or ssh",
                setup.raw_format
            ),
        );
    };
    let mut missing: Vec<String> = Vec::new();
    if !ff_core::sign::program_available(&setup.program) {
        missing.push(format!("{} is not on PATH", setup.program));
    }
    // openpgp and x509 fall back to gpg's own default key; ssh has no such
    // fallback, so a key is part of the setup rather than a nicety.
    if format == ff_core::sign::Format::Ssh
        && setup.key.is_none()
        && setup.default_key_command.is_none()
    {
        missing.push("no user.signingkey, which ssh signing requires".into());
    }
    if format == ff_core::sign::Format::Ssh && setup.allowed_signers.is_none() {
        missing.push(
            "no gpg.ssh.allowedSignersFile — signing will work, verification will not".into(),
        );
    }
    let named = match &setup.key {
        Some(key) => format!("{} via {}, key {key}", format.as_str(), setup.program),
        None => format!("{} via {}", format.as_str(), setup.program),
    };
    if missing.is_empty() {
        Row::ok("signing", format!("on — {named}"))
    } else {
        Row::warn("signing", format!("on — {named}; {}", missing.join("; ")))
    }
}

/// The remote floor: which local branches can name a remote, what their
/// `[branch "<n>"]` sections point at, and whether the shared copy those
/// sections promise still exists. The upstreams row holds the second of the
/// two `--fix` writes, `remove_branch_section`.
pub(super) fn branch_checks(repo: &ff_core::gix::Repository, fix: bool) -> Result<Vec<Row>> {
    let mut rows: Vec<Row> = Vec::new();

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

    Ok(rows)
}

/// How much raw git this chain has seen, and — under `observe` — the nudge
/// that tier exists to earn. Silent when nothing has been counted: a row
/// saying zero is a row about a thing that never happened.
pub(super) fn raw_git_row(repo: &ff_core::gix::Repository, now: i64) -> Option<Row> {
    let tally = crate::gitpolicy::load(repo);
    if tally.is_empty() {
        return None;
    }
    let mut detail = format!("{} write(s)", tally.writes);
    if tally.denied > 0 {
        detail.push_str(&format!(", {} refused", tally.denied));
    }
    if tally.last_at > 0 {
        detail.push_str(&format!(
            ", last {}",
            crate::render::relative_age(now, tally.last_at)
        ));
    }
    if crate::gitpolicy::read(repo) == crate::gitpolicy::Policy::Observe {
        detail.push_str(" — `ff config gitPolicy coach` names the alternative");
    }
    Some(Row::info("raw git", detail))
}
