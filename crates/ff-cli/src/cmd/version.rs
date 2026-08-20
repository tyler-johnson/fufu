//! `ff version` — which fufu this is, and whether it is the current one.
//!
//! The earn over `git version` is the second half of that sentence, and the
//! envelope under it. The passive update lane already keeps a cache of the
//! latest release (`selfupdate::notify`), so the answer to "am I behind?" is
//! sitting on disk and costs nothing to read — no network, no repository. And
//! `--json` reports the version, the commit, and the build date as fields
//! rather than as one line a caller has to take apart.
//!
//! `ff -v` prints the same two lines and stops. What the verb adds is the
//! update lane: the flag answers "what am I running", and the verb answers
//! "and should I still be".

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

    // The same text `ff -v` prints, name and all, so the two spellings are
    // diffable against each other rather than merely similar. clap prepends
    // the name to its half; this does it by hand.
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
