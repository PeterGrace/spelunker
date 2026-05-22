//! Render `BranchResult`s for human or machine consumption.

use std::io::Write;

use crate::error::Result;
use crate::{BranchResult, BranchStatus};

/// Controls how results are serialized.
#[derive(Debug, Clone, Copy)]
pub enum Format {
    /// Grep-style human-readable output: `branch:line_number:line`.
    Human,
    /// Machine-readable JSON array, one object per branch.
    Json,
}

/// Render branch search results to the given writers.
///
/// For `Format::Human`, matching lines are emitted to `stdout` in the form
/// `branch:line_number:line\n`.  Branch-level errors go to `stderr`.  A
/// summary line `N/M branches matched` is always written to `stderr`.
///
/// For `Format::Json`, a single JSON array is written to `stdout`, one
/// object per branch.  Nothing is written to `stderr`.
///
/// # Arguments
///
/// * `results` - Slice of per-branch outcomes to render.
/// * `format`  - Output format selector.
/// * `stdout`  - Writer that receives the primary output.
/// * `stderr`  - Writer that receives diagnostic/summary messages.
///
/// # Returns
///
/// The number of branches whose status is `Matched`.
///
/// # Errors
///
/// Returns `SpelunkerError::Output` if any write fails.
/// Returns `SpelunkerError::Json` (JSON format only) if serialization fails.
pub fn render<W: Write, E: Write>(
    results: &[BranchResult],
    format: Format,
    stdout: &mut W,
    stderr: &mut E,
) -> Result<usize> {
    let mut matched = 0;
    match format {
        Format::Human => {
            for r in results {
                match &r.status {
                    BranchStatus::Matched(hits) => {
                        matched += 1;
                        for h in hits {
                            writeln!(stdout, "{}:{}:{}", r.branch, h.line_number, h.line)?;
                        }
                    }
                    // NoMatch and FileMissing are silent in human mode.
                    BranchStatus::NoMatch | BranchStatus::FileMissing => {}
                    BranchStatus::Error(msg) => {
                        writeln!(stderr, "{}: {}", r.branch, msg)?;
                    }
                }
            }
            writeln!(stderr, "{}/{} branches matched", matched, results.len())?;
        }
        Format::Json => {
            unimplemented!("lands in the next task")
        }
    }
    Ok(matched)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{BranchResult, BranchStatus, Hit};

    /// Build a `BranchResult` with `Matched` status from a list of `(line_number, line)` pairs.
    fn matched(branch: &str, hits: Vec<(usize, &str)>) -> BranchResult {
        BranchResult {
            branch: branch.to_string(),
            status: BranchStatus::Matched(
                hits.into_iter()
                    .map(|(n, l)| Hit {
                        line_number: n,
                        line: l.to_string(),
                    })
                    .collect(),
            ),
        }
    }

    #[test]
    fn human_prints_branch_lineno_line_for_each_hit() {
        let results = vec![
            matched("main", vec![(2, "first hit"), (7, "second hit")]),
            BranchResult {
                branch: "no-match".into(),
                status: BranchStatus::NoMatch,
            },
            BranchResult {
                branch: "missing".into(),
                status: BranchStatus::FileMissing,
            },
        ];
        let mut out = Vec::new();
        let mut err = Vec::new();
        let n = render(&results, Format::Human, &mut out, &mut err).unwrap();
        let out = String::from_utf8(out).unwrap();
        assert_eq!(out, "main:2:first hit\nmain:7:second hit\n");
        let err = String::from_utf8(err).unwrap();
        assert!(err.contains("1/3 branches matched"));
        assert_eq!(n, 1);
    }

    #[test]
    fn human_writes_branch_errors_to_stderr_not_stdout() {
        let results = vec![
            BranchResult {
                branch: "bad".into(),
                status: BranchStatus::Error("boom".into()),
            },
            matched("good", vec![(1, "yes")]),
        ];
        let mut out = Vec::new();
        let mut err = Vec::new();
        render(&results, Format::Human, &mut out, &mut err).unwrap();
        let out = String::from_utf8(out).unwrap();
        let err = String::from_utf8(err).unwrap();
        assert!(out.contains("good:1:yes"));
        assert!(!out.contains("boom"));
        assert!(err.contains("bad: boom"));
    }
}
