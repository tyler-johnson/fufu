//! `ff diff` — the open change as a patch.
//!
//! The one question no other tool here answers: *what will `ff commit`
//! actually land, and what does it say?* Every other fufu surface is
//! stat-level, and `git diff` — the only patch tool a user had — is blind to
//! the untracked sweep, which is exactly where a wrong commit comes from.
//! This reads the same tree diff `ff status` counts, all the way down.
//!
//! Output is the patch and nothing else. The diffstat already has a home in
//! `ff status`, and two verbs printing the same block is a second dialect
//! for one fact.

use std::io::Write as _;

use ff_core::{DiffOptions, Error, Result};

use crate::ctx::Ctx;

pub fn run(ctx: &Ctx, paths: Vec<String>) -> Result<()> {
    // Load-bearing, not ceremonial: the open change is HEAD's tree against
    // the branch's *newest operation's* tree, so an edit made since the last
    // operation is invisible until something captures it. Without this line
    // `ff diff` would report a clean tree on a file you just wrote — the
    // same bug `ff op diff` carried until 3b7a7fca.
    crate::capture::pre_best_effort(&crate::provenance::pre_ff(ctx));
    let repo = ff_core::discover(".")?;
    let stat = ff_core::change_diff(&repo, &DiffOptions { hunks: true, paths })?;

    if ctx.json {
        let payload = serde_json::json!({
            "changes": stat.files,
            "insertions": stat.insertions,
            "deletions": stat.deletions,
        });
        return crate::machine::emit("diff", &payload);
    }

    crate::render::init_palette(&repo);
    let mut out = crate::pager::LogOut::new(&repo, ctx.json);
    let colored = out.colored();
    // A clean tree prints nothing, git's convention: this verb's output is
    // meant to be piped into `git apply`, and prose in that stream is a bug
    // for whatever reads it.
    let result = write!(out, "{}", crate::render::patch_block(&stat.files, colored));
    out.finish();
    result.map_err(Error::repo)
}
