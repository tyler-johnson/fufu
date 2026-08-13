use ff_core::{Error, Result};

pub fn run(json: bool) -> Result<()> {
    crate::capture::pre_best_effort(&crate::provenance::pre_ff());
    run_inner(json)
}

/// The verb itself, capture already handled (or deliberately skipped).
pub fn run_inner(json: bool) -> Result<()> {
    let repo = ff_core::discover(".")?;
    let status = ff_core::status(&repo)?;
    if json {
        // No isatty check on this path: identical bytes TTY vs pipe by construction.
        let body = serde_json::to_string(&status).map_err(Error::repo)?;
        println!("{body}");
    } else {
        print!("{}", crate::render::status_human(&status));
    }
    Ok(())
}
