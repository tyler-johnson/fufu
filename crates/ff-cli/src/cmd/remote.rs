//! `ff remote` — what the remotes here are called, and where each one points.
//!
//! The answer is read through the ordinary `ff_core::discover` handle, which
//! is the whole of the earn over `git remote -v`: a listing that must never
//! be the reason a reader reached outside the process. The wire in `net.rs`
//! opens its own handle with `git_binary` on because a fetch needs git's
//! credential and proxy config, and that costs a `git config -l` spawn —
//! `find_remote` runs on that handle down there, and `tests/zero_spawn.rs`
//! pins that this verb is not it.

use ff_core::Result;

use crate::ctx::Ctx;

pub fn run(ctx: &Ctx) -> Result<()> {
    // Reading the remotes as of a past operation would need a past-state view
    // of the config that does not exist — the same refusal `ff branch list`
    // makes for its rows.
    ctx.refuse_past("ff remote")?;

    let repo = ff_core::discover(".")?;
    let remotes = ff_core::remote::list(&repo)?;

    if ctx.json {
        // An object with a named array rather than a bare one: the envelope
        // has somewhere to grow.
        return crate::machine::emit("remote", &serde_json::json!({ "remotes": remotes }));
    }

    crate::render::init_palette(&repo);
    let colored = crate::pager::color_enabled();

    if remotes.is_empty() {
        // Not having a remote is a state, not a failure: the exit is 0.
        println!(
            "{}",
            crate::render::paint_dim("no remotes configured", colored)
        );
        return Ok(());
    }

    // Neither the name nor the URL is a palette role — the palette colors
    // roles, and a remote name is one of the things fufu names, not a role
    // it paints. The dim lines are the only paint in this output.
    let width = remotes
        .iter()
        .map(|remote| remote.name.chars().count())
        .max()
        .unwrap_or_default()
        .max(8);
    for remote in &remotes {
        let url = remote
            .fetch_url
            .clone()
            .unwrap_or_else(|| crate::render::paint_dim("no fetch url", colored));
        println!("{:<width$}  {}", remote.name, url);
    }
    Ok(())
}
