//! `ff version` — which fufu this is, and whether it is the current one.
//!
//! The earn over `git version` is the second half of that sentence, and the
//! envelope under it. The passive update lane already keeps a cache of the
//! latest release (`selfupdate::notify`), so the answer to "am I behind?" is
//! sitting on disk and costs nothing to read — no network, no repository. And
//! `--json` reports the version, the commit, and the build date as fields
//! rather than as one line a caller has to take apart.
//!
//! There is one answer now, and two ways to type it: `-v` is the verb,
//! spelled as a flag. So the flag reads the update cache, prints the
//! "available" line when there is one, rides the passive lane's auto-install,
//! and takes `--json` for the same fields — the answer never changes with
//! the spelling.

use ff_core::Result;

use crate::ctx::Ctx;
use crate::selfupdate::notify::{self, CheckStatus};

pub fn run(ctx: &Ctx) -> Result<()> {
    let status = notify::check_status(env!("CARGO_PKG_VERSION"));

    if ctx.json {
        let payload = serde_json::json!({
            "version": env!("CARGO_PKG_VERSION"),
            // Null rather than empty when git was not available at build time:
            // "this binary does not know its commit" is a different fact from
            // "its commit is the empty string".
            "commit": some(env!("FF_BUILD_SHA")),
            "date": some(env!("FF_BUILD_DATE")),
            "update": {
                "status": match &status {
                    CheckStatus::Unofficial => "unofficial",
                    CheckStatus::NoCheckYet => "unchecked",
                    CheckStatus::Available(_) => "available",
                    CheckStatus::UpToDate => "current",
                },
                "latest": match &status {
                    CheckStatus::Available(tag) => Some(tag.as_str()),
                    _ => None,
                },
            },
        });
        return crate::machine::emit("version", &payload);
    }

    // The name goes on by hand. Nothing else prepends it any more — `-v`
    // reaches this same line rather than clap's, which is what makes the two
    // spellings one answer instead of two that have to be kept in step.
    println!("{} {}", crate::cli::NAME, crate::cli::VERSION);

    // Only news gets a second line. "Up to date" is the expected state, and a
    // verb that says it every time trains people to stop reading the output.
    if let CheckStatus::Available(tag) = &status {
        println!(
            "{}",
            crate::render::paint_dim(
                &format!("{tag} available — ff update"),
                crate::pager::color_enabled()
            )
        );
    }
    Ok(())
}

/// An empty build variable means "not recorded", which JSON spells as null.
fn some(value: &'static str) -> Option<&'static str> {
    (!value.is_empty()).then_some(value)
}
