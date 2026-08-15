#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("not a git repository (or any parent): {0}")]
    Discover(#[from] Box<gix::discover::Error>),
    #[error("{0}")]
    Repo(String),
    #[error("{message}")]
    Coded {
        id: &'static str,
        message: String,
        exits: Vec<String>,
    },
}

impl From<gix::discover::Error> for Error {
    fn from(err: gix::discover::Error) -> Self {
        Error::Discover(Box::new(err))
    }
}

impl Error {
    /// Flatten a gix error and its source chain into one message.
    pub fn repo(err: impl std::error::Error) -> Self {
        let mut msg = err.to_string();
        let mut src = err.source();
        while let Some(s) = src {
            msg.push_str(": ");
            msg.push_str(&s.to_string());
            src = s.source();
        }
        Error::Repo(msg)
    }

    pub fn msg(m: impl Into<String>) -> Self {
        Error::Repo(m.into())
    }

    pub fn coded(id: &'static str, message: impl Into<String>, exits: Vec<String>) -> Self {
        Error::Coded {
            id,
            message: message.into(),
            exits,
        }
    }

    /// Stable identifier for this error. `"internal"` for uncoded variants.
    pub fn id(&self) -> &str {
        match self {
            Error::Discover(_) => "repo/not-found",
            Error::Repo(_) => "internal",
            Error::Coded { id, .. } => id,
        }
    }

    /// Suggested exit commands. Empty for uncoded variants.
    pub fn exits(&self) -> &[String] {
        match self {
            Error::Coded { exits, .. } => exits,
            _ => &[],
        }
    }

    /// Exit code derived from the id's namespace prefix.
    /// `usage/` → 2, `held/` → 3, anything else → 1.
    pub fn exit_code(&self) -> i32 {
        match self {
            Error::Coded { id, .. } => {
                if id.starts_with("usage/") {
                    2
                } else if id.starts_with("held/") {
                    3
                } else {
                    1
                }
            }
            _ => 1,
        }
    }
}

pub type Result<T> = std::result::Result<T, Error>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exit_code_from_namespace() {
        let e = Error::coded("usage/bad-value", "oops", vec![]);
        assert_eq!(e.exit_code(), 2);

        let e = Error::coded("held/wait", "paused", vec![]);
        assert_eq!(e.exit_code(), 3);

        let e = Error::coded("internal/something", "err", vec![]);
        assert_eq!(e.exit_code(), 1);

        let e = Error::msg("plain");
        assert_eq!(e.exit_code(), 1);
    }
}
