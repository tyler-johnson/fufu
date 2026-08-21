//! `ff history` — where you can go back to.
//!
//! `ff op log` reports what happened; this reports what you can do about it,
//! and the two are different questions. Captures outnumber verb operations by
//! more than an order of magnitude, so an operation log read as a list of
//! places to return to is mostly noise — the granularity a person moves by is
//! `ff undo`'s, one run at a time, and this is that granularity rendered.
//!
//! The rows come from `ff_core::history`, which walks with the same two
//! functions `ff undo` and `ff redo` walk with. That is the whole reason this
//! file is thin: a view that re-derived the stepping rule would be a second
//! place for it to be wrong.

use std::io::Write as _;

use ff_core::{Error, Result};

use crate::ctx::Ctx;

pub fn run(ctx: &Ctx, count: usize) -> Result<()> {
    let repo = ff_core::discover(".")?;
    let steps = ff_core::history::history(&repo, count)?;

    // The floor is a fact about the listing rather than about any row: the
    // walk either ran out of operations or ran out of budget, and only the
    // first is somewhere to stop.
    let back = steps.iter().filter(|s| s.distance > 0).count();
    let at_floor = !steps.is_empty() && (count == 0 || back < count);

    crate::render::init_palette(&repo);
    let mut out = crate::pager::LogOut::new(&repo, ctx.json);
    let colored = out.colored();
    let result = (|| -> std::io::Result<()> {
        if ctx.json {
            let payload = serde_json::json!({ "steps": steps, "floor": at_floor });
            crate::machine::write(&mut out, "history", &payload).map_err(std::io::Error::other)?;
            return Ok(());
        }
        if steps.is_empty() {
            writeln!(out, "no operations recorded yet")?;
            return Ok(());
        }
        let now = now_secs();
        for step in &steps {
            writeln!(out, "{}", crate::render::history_row(step, now, colored))?;
        }
        if at_floor {
            writeln!(
                out,
                "    {}",
                crate::render::paint_dim("(the floor)", colored)
            )?;
        }
        Ok(())
    })();
    out.finish();
    result.map_err(Error::repo)
}

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}
