//! Doctor verifies the net — read-only by design (it must never absorb the
//! foreign drift it reports), one consented write behind `--fix`.

mod render;
mod repo;
mod wiring;

use ff_core::Result;

use crate::ctx::Ctx;

pub(super) enum Level {
    Ok,
    Info,
    Warn,
}

pub(super) struct Row {
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
        rows.extend(repo::log_checks(repo, now, fix)?);
        rows.extend(repo::trash_row(repo, now)?);
        rows.push(repo::objects_row(repo));
        rows.push(repo::id_index_row(repo)?);
        rows.extend(repo::last_op_rows(repo, now)?);
        rows.extend(repo::legacy_row(repo)?);
        rows.extend(repo::parked_row(repo)?);
        rows.extend(repo::settings_checks(repo, now)?);
        rows.push(repo::signing_row(repo));
        rows.extend(repo::branch_checks(repo, fix)?);
    }

    // raw git — what the gitPolicy lane has seen, whichever tier it was on
    if let Some(repo) = &repo
        && let Some(row) = repo::raw_git_row(repo, now)
    {
        rows.push(row);
    }

    // wiring + update checks — always run
    rows.extend(wiring::wiring_rows(&crate::integ::statuses(), fix));
    rows.push(wiring::update_row());

    render::render(&rows, fix, ctx.json, colored);

    Ok(())
}
