use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum EditorError {
    #[error("the document has no file path; save it with a path first")]
    NoPath,

    #[error("cannot read {path}: {source}")]
    Read {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("cannot write {path}: {source}")]
    Write {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

pub type Result<T> = std::result::Result<T, EditorError>;
