use crate::error::{Error, Result};
use crate::model::{HeadState, Operation};

pub fn head_state(repo: &gix::Repository) -> Result<HeadState> {
    let head = repo.head().map_err(Error::repo)?;
    Ok(match head.kind {
        gix::head::Kind::Unborn(name) => HeadState::Unborn {
            r#ref: name.as_bstr().to_string(),
        },
        gix::head::Kind::Detached { target, peeled } => HeadState::Detached {
            commit: peeled.unwrap_or(target).to_string(),
        },
        gix::head::Kind::Symbolic(reference) => {
            let name = reference.name.as_ref().shorten().to_string();
            let full = reference.name.as_bstr().to_string();
            let commit = match reference.target.try_id() {
                Some(id) => id.to_string(),
                // A branch pointing at another symbolic ref: follow the chain.
                None => repo
                    .find_reference(reference.name.as_ref())
                    .map_err(Error::repo)?
                    .peel_to_id_in_place()
                    .map_err(Error::repo)?
                    .to_string(),
            };
            HeadState::Branch {
                name,
                r#ref: full,
                commit,
            }
        }
    })
}

pub fn operation(repo: &gix::Repository) -> Option<Operation> {
    repo.state().map(Operation::from)
}
