//! Spelunker: search a file's contents across every branch of a local git repo.

pub mod cli;
pub mod error;
pub mod git;
pub mod output;
pub mod scan;
pub mod search;

pub use error::{Result, SpelunkerError};
pub use search::Hit;

/// Outcome of searching a single branch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BranchResult {
    pub branch: String,
    pub status: BranchStatus,
}

/// What happened when we tried to search a particular branch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BranchStatus {
    /// File existed and contained at least one matching line.
    Matched(Vec<Hit>),
    /// File existed but contained no matching lines.
    NoMatch,
    /// File did not exist on this branch.
    FileMissing,
    /// `git show` failed for this branch for some other reason.
    Error(String),
}
