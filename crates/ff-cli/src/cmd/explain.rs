//! `ff explain` — look up an error id and see what it means, plus the exits.
//! Runs outside a repository: discover is never called.

use std::io::Write;

use ff_core::{Error, Result};

use crate::ctx::Ctx;

pub fn run(ctx: &Ctx, id: Option<String>, list: bool) -> Result<()> {
    if list {
        if ctx.json {
            return crate::explain::emit_json_list();
        }
        return crate::explain::render_list().map_err(Error::repo);
    }

    let Some(id) = id else {
        return Err(Error::coded(
            "usage/bad-flags",
            "explain requires an id, or --list",
            vec!["ff explain --list".into()],
        ));
    };

    // The prefix a declared extension's own ids carry — `tower/flight/not-
    // found` — is exactly the name `ff extension add` recorded it under, so
    // the split that routes an id back to its raiser is the same split that
    // finds it here. A prefix nothing is declared under falls through to
    // the lookup below and gets today's usual `usage/unknown-error-id`.
    if let Some((name, rest)) = id.split_once('/')
        && let Some(declared) = crate::registry::read().get(name)
    {
        let mut args: Vec<&str> = vec![rest];
        if ctx.json {
            args.push("--json");
        }
        let said = crate::ext::delegate(declared, "explain", &args)?;
        std::io::stdout().write_all(&said).map_err(Error::repo)?;
        return Ok(());
    }

    let entry = crate::explain::find(&id).ok_or_else(|| crate::explain::unknown_id(&id))?;

    if ctx.json {
        crate::explain::emit_json(entry)
    } else {
        crate::explain::render(entry).map_err(Error::repo)
    }
}
