#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("not a git repository (or any parent): {0}")]
    Discover(#[from] Box<gix::discover::Error>),
    #[error("{0}")]
    Repo(String),
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
}

pub type Result<T> = std::result::Result<T, Error>;
