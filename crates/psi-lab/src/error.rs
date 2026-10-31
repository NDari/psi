#[derive(Debug, thiserror::Error)]
pub enum LabError {
    #[error("{context}: {err}")]
    Io {
        context: String,
        err: std::io::Error,
    },
    #[error("{0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, LabError>;

pub(crate) trait IoContext<T> {
    fn ctx(self, context: impl FnOnce() -> String) -> Result<T>;
}

impl<T> IoContext<T> for std::io::Result<T> {
    fn ctx(self, context: impl FnOnce() -> String) -> Result<T> {
        self.map_err(|err| LabError::Io {
            context: context(),
            err,
        })
    }
}

pub(crate) fn other(msg: impl Into<String>) -> LabError {
    LabError::Other(msg.into())
}
