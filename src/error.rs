//! Spelunker error types.

use std::path::PathBuf;

/// Errors that can abort the entire scan.
#[derive(thiserror::Error, Debug)]
pub enum SpelunkerError {
    /// The supplied path is not a git repository.
    #[error("not a git repository: {0}")]
    NotARepo(PathBuf),

    /// Launching or communicating with the git process failed.
    #[error("git invocation failed: {context}: {source}")]
    GitInvoke {
        /// Human-readable description of what we were trying to do.
        context: String,
        /// The underlying OS error.
        #[source]
        source: std::io::Error,
    },

    /// The git process exited with a non-zero status.
    #[error("git command exited {code}: {stderr}")]
    GitExit {
        /// The process exit code.
        code: i32,
        /// Captured stderr from the git process.
        stderr: String,
    },

    /// The supplied regex pattern could not be compiled.
    #[error("invalid regex pattern: {0}")]
    BadRegex(#[from] regex::Error),

    /// An I/O error occurred while writing output.
    #[error("I/O error writing output: {0}")]
    Output(#[from] std::io::Error),

    /// A JSON serialization error occurred while rendering output.
    #[error("JSON serialization error: {0}")]
    Json(#[from] serde_json::Error),
}

/// Convenience alias so callers can write `spelunker::Result<T>`.
pub type Result<T> = std::result::Result<T, SpelunkerError>;
