//! `ff session` — inspect capture sessions.

use ff_core::Result;
use ff_core::gix;

use crate::ctx::Ctx;

/// Run the session command. `action` is `None` for bare `ff session`,
/// `Some("list")` for `ff session list`, `Some("diff")` for `ff session diff`.
pub fn run(ctx: &Ctx, action: Option<&str>, name: Option<String>) -> Result<()> {
    let repo = ff_core::discover(".")?;

    match action {
        None => status(ctx),
        Some("list") => list(&repo, ctx.json),
        Some("diff") => diff(ctx, &repo, name),
        Some(other) => Err(ff_core::Error::msg(format!(
            "unknown session action: {other}"
        ))),
    }
}

/// Cap on the bounded chain reads `list` and `diff` perform — the same
/// value `ff evolog`'s `-n/--max-count` defaults to (see the `Evolog` arm in
/// `crates/ff-cli/src/cli.rs`), so a session query costs no more than an
/// evolog listing already does.
const SESSION_QUERY_LIMIT: usize = 25;

fn list(repo: &gix::Repository, json: bool) -> Result<()> {
    let spans = ff_core::spans(repo, Some(SESSION_QUERY_LIMIT))?;

    if json {
        let payload = serde_json::json!({ "spans": spans });
        crate::machine::emit("session", &payload)?;
        return Ok(());
    }

    if spans.is_empty() {
        println!("no sessions on this branch");
        return Ok(());
    }

    crate::render::init_palette(repo);
    let colored = crate::pager::color_enabled();
    let now = now_secs();
    for span in &spans {
        println!("{}", crate::render::session_span_row(span, now, colored));
    }
    Ok(())
}

fn diff(ctx: &Ctx, repo: &gix::Repository, name: Option<String>) -> Result<()> {
    let name = match name {
        Some(n) => n,
        None => match ctx.session.clone() {
            Some(n) => n,
            None => {
                return Err(ff_core::Error::coded(
                    "usage/needs-session",
                    "no session open and none named",
                    vec!["ff session list".into(), "ff session diff <name>".into()],
                ));
            }
        },
    };

    let spans = ff_core::spans(repo, Some(SESSION_QUERY_LIMIT))?;
    let span = spans.into_iter().find(|s| s.name == name).ok_or_else(|| {
        ff_core::Error::coded(
            "usage/needs-session",
            format!("no session named {name} found on this branch"),
            vec!["ff session list".into()],
        )
    })?;

    let newest_oid =
        gix::ObjectId::from_hex(span.newest.as_bytes()).map_err(ff_core::Error::repo)?;
    let newest_tree = repo
        .find_commit(newest_oid)
        .map_err(ff_core::Error::repo)?
        .tree_id()
        .map_err(ff_core::Error::repo)?
        .detach();
    let start_tree = ff_core::span_start_tree(repo, &span)?;
    let change_stat = ff_core::tree_diff_stat(repo, start_tree, newest_tree)?;

    if ctx.json {
        let payload = serde_json::json!({
            "name": span.name,
            "span": span,
            "changes": change_stat.files,
            "insertions": change_stat.insertions,
            "deletions": change_stat.deletions,
        });
        crate::machine::emit("session", &payload)?;
        return Ok(());
    }

    // Spans of the same name never merge across a gap (see
    // `ff_core::session::spans`), so a repeat name is always reported as its
    // newest span here — say so, rather than let a silent pick look like the
    // only one that ever existed.
    println!(
        "session {} — newest span, {} snapshot{}",
        span.name,
        span.snapshots,
        if span.snapshots == 1 { "" } else { "s" }
    );
    if change_stat.files.is_empty() {
        println!("no changes");
    } else {
        crate::render::init_palette(repo);
        let colored = crate::pager::color_enabled();
        println!("{}", crate::render::render_diffstat(&change_stat, colored));
    }
    Ok(())
}

fn status(ctx: &Ctx) -> Result<()> {
    match &ctx.session {
        Some(name) => {
            if ctx.json {
                let payload = serde_json::json!({ "name": name });
                crate::machine::emit("session", &payload)?;
            } else {
                println!("session {name}");
            }
        }
        None => {
            if ctx.json {
                let payload = serde_json::json!({ "name": serde_json::Value::Null });
                crate::machine::emit("session", &payload)?;
            } else {
                println!("no session set");
            }
        }
    }
    Ok(())
}

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}
