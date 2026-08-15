//! `ff explain` — look up an error id and see what it means, plus the exits.
//! Runs outside a repository: discover is never called.

use ff_core::{Error, Result};

pub fn run(id: Option<String>, list: bool, json: bool) -> Result<()> {
    if list {
        if json {
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

    let entry = crate::explain::find(&id).ok_or_else(|| crate::explain::unknown_id(&id))?;

    if json {
        crate::explain::emit_json(entry)
    } else {
        crate::explain::render(entry).map_err(Error::repo)
    }
}
