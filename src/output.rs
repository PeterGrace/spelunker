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
    // Count matched branches once, immutably; both arms below share this value.
    let matched = results
        .iter()
        .filter(|r| matches!(r.status, BranchStatus::Matched(_)))
        .count();

    match format {
        Format::Human => {
            for r in results {
                match &r.status {
                    BranchStatus::Matched(hits) => {
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
            // Flush stdout before writing the summary to stderr. Without this,
            // block-buffered stdout may not drain until after the stderr write,
            // causing the summary line to appear BEFORE the match output in
            // interactive terminals.
            stdout.flush()?;
            writeln!(stderr, "{}/{} branches matched", matched, results.len())?;
        }
        Format::Json => {
            // Pre-size to avoid geometric reallocations during extend.
            let mut json: Vec<serde_json::Value> = Vec::with_capacity(results.len());
            json.extend(results.iter().map(to_json));
            // Reborrow: to_writer takes the writer by value; we need stdout again
            // on the next line for the trailing newline, so we hand it a fresh &mut.
            serde_json::to_writer(&mut *stdout, &serde_json::Value::Array(json))?;
            writeln!(stdout)?;
        }
    }
    Ok(matched)
}

/// Serialize a single `BranchResult` into a `serde_json::Value` object.
///
/// The shape is:
/// - `branch`: string
/// - `status`: one of `"matched"`, `"no_match"`, `"file_missing"`, `"error"`
/// - `hits`: array of `{ line_number, line }` objects (present only when `status == "matched"`)
/// - `error`: string (present only when `status == "error"`)
fn to_json(r: &BranchResult) -> serde_json::Value {
    let mut obj = serde_json::Map::new();
    obj.insert("branch".into(), serde_json::Value::String(r.branch.clone()));
    match &r.status {
        BranchStatus::Matched(hits) => {
            obj.insert("status".into(), "matched".into());
            obj.insert(
                "hits".into(),
                serde_json::Value::Array(
                    hits.iter()
                        .map(|h| {
                            serde_json::json!({
                                "line_number": h.line_number,
                                "line": h.line,
                            })
                        })
                        .collect(),
                ),
            );
        }
        BranchStatus::NoMatch => {
            obj.insert("status".into(), "no_match".into());
        }
        BranchStatus::FileMissing => {
            obj.insert("status".into(), "file_missing".into());
        }
        BranchStatus::Error(msg) => {
            obj.insert("status".into(), "error".into());
            obj.insert("error".into(), serde_json::Value::String(msg.clone()));
        }
    }
    serde_json::Value::Object(obj)
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

    #[test]
    fn json_emits_one_record_per_branch_with_correct_status() {
        let results = vec![
            matched("main", vec![(2, "hi")]),
            BranchResult {
                branch: "stale".into(),
                status: BranchStatus::NoMatch,
            },
            BranchResult {
                branch: "old".into(),
                status: BranchStatus::FileMissing,
            },
            BranchResult {
                branch: "broken".into(),
                status: BranchStatus::Error("boom".into()),
            },
        ];
        let mut out = Vec::new();
        let mut err = Vec::new();
        let n = render(&results, Format::Json, &mut out, &mut err).unwrap();
        let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
        let arr = v.as_array().expect("top-level array");
        assert_eq!(arr.len(), 4);
        assert_eq!(arr[0]["branch"], "main");
        assert_eq!(arr[0]["status"], "matched");
        assert_eq!(arr[0]["hits"][0]["line_number"], 2);
        assert_eq!(arr[0]["hits"][0]["line"], "hi");
        assert_eq!(arr[1]["status"], "no_match");
        assert_eq!(arr[2]["status"], "file_missing");
        assert_eq!(arr[3]["status"], "error");
        assert_eq!(arr[3]["error"], "boom");
        assert_eq!(n, 1);
    }

    #[test]
    fn json_with_empty_results_emits_empty_array() {
        let mut out = Vec::new();
        let mut err = Vec::new();
        let n = render(&[], Format::Json, &mut out, &mut err).unwrap();
        let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
        assert_eq!(v, serde_json::json!([]));
        assert_eq!(n, 0);
    }
}
