use ff_core::{Error, LogOptions, Result};

pub fn run(json: bool, count: usize) -> Result<()> {
    let mut repo = ff_core::discover(".")?;
    let limit = if count == 0 { None } else { Some(count) };
    let entries = ff_core::log(&mut repo, &LogOptions { limit })?;
    if json {
        let commits: Vec<_> = entries.collect::<Result<_>>()?;
        // Envelope object so future fields can be added without breaking consumers.
        let body = serde_json::to_string(&serde_json::json!({ "commits": commits }))
            .map_err(Error::repo)?;
        println!("{body}");
    } else {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        for entry in entries {
            let entry = entry?;
            println!("{}", crate::render::log_row(&entry, now));
        }
    }
    Ok(())
}
