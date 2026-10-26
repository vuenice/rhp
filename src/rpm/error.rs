use std::path::PathBuf;
use thiserror::Error;

pub type Result<T> = std::result::Result<T, RpmError>;

#[derive(Debug, Error)]
pub enum RpmError {
    #[error("Manifest file '{0}' not found. Run 'rpm init' to create one.")]
    ManifestNotFound(PathBuf),

    #[error("Failed to parse manifest '{path}': {message}")]
    ManifestParseError {
        path: PathBuf,
        message: String,
    },

    #[error("Failed to write manifest '{path}': {message}")]
    ManifestWriteError {
        path: PathBuf,
        message: String,
    },

    #[error("Lockfile error at '{path}': {message}")]
    LockfileError {
        path: PathBuf,
        message: String,
    },

    #[error("Git error: {0}")]
    Git(String),

    #[error("Package '{0}' not found in dependencies")]
    PackageNotFound(String),

    #[error("Invalid package specifier '{spec}': {reason}")]
    InvalidPackageSpec {
        spec: String,
        reason: String,
    },

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Autoload error: {0}")]
    Autoload(String),

    #[error("{0}")]
    General(String),
}
