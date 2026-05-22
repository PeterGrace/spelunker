//! Orchestrates a full spelunker run: build matcher, fan branches across
//! rayon, collect results in a deterministic order.

use rayon::prelude::*;

use crate::cli::Args;
use crate::error::Result;
use crate::git::{self, BlobRead};
use crate::search::Matcher;
use crate::{BranchResult, BranchStatus};

/// Run a full spelunker search across all branches matching `args`.
///
/// Constructs the matcher, enumerates branches via `git for-each-ref`,
/// fans work across a rayon thread pool, and returns results sorted
/// alphabetically by branch name.
///
/// # Arguments
///
/// * `args` - Parsed CLI arguments controlling pattern, file, repo, and
///   parallelism.
///
/// # Errors
///
/// Returns `SpelunkerError::NotARepo` if `args.repo` is not a git repository.
/// Returns `SpelunkerError::BadRegex` if `args.regex` is true and
/// `args.pattern` is not a valid regular expression.
/// Returns `SpelunkerError::GitInvoke` or `SpelunkerError::GitExit` if branch
/// enumeration fails.
pub fn run(args: &Args) -> Result<Vec<BranchResult>> {
    git::ensure_repo(&args.repo)?;

    // Build the matcher eagerly so a bad regex is a fatal error before any
    // branch scanning begins.
    let matcher = if args.regex {
        Matcher::regex(&args.pattern, args.ignore_case)?
    } else {
        Matcher::literal(args.pattern.clone(), args.ignore_case)
    };

    let branches = git::list_branches(&args.repo, args.include.as_deref())?;

    // Closure that scans one branch. Captures matcher and args by reference.
    // rayon's par_iter drives this concurrently; results are collected then
    // sorted so output order is deterministic regardless of scheduling.
    let scan = |branch: &String| -> BranchResult {
        let status = match git::read_blob(&args.repo, branch, &args.file) {
            Ok(BlobRead::Bytes(bytes)) => {
                let hits = matcher.scan(&bytes);
                if hits.is_empty() {
                    BranchStatus::NoMatch
                } else {
                    BranchStatus::Matched(hits)
                }
            }
            Ok(BlobRead::Missing) => BranchStatus::FileMissing,
            Ok(BlobRead::Error(msg)) => BranchStatus::Error(msg),
            Err(e) => BranchStatus::Error(e.to_string()),
        };
        BranchResult {
            branch: branch.clone(),
            status,
        }
    };

    let mut results: Vec<BranchResult> = if let Some(n) = args.jobs {
        // Build a custom pool so the caller-specified parallelism is respected
        // without mutating the global rayon pool. The pool was validated by
        // the CLI parser to be >= 1, so the `expect` here documents a
        // programmer invariant rather than a recoverable condition.
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(n)
            .build()
            .expect("rayon pool builds with validated worker count");
        pool.install(|| branches.par_iter().map(scan).collect())
    } else {
        branches.par_iter().map(scan).collect()
    };

    results.sort_by(|a, b| a.branch.cmp(&b.branch));
    Ok(results)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::tests::{commit_file, init_repo};
    use std::process::Command;

    /// Check out a branch in `repo`, optionally creating it with `-b`.
    fn checkout(repo: &std::path::Path, branch: &str, create: bool) {
        let repo_str = repo.display().to_string();
        let mut args = vec!["-C", &repo_str, "checkout", "-q"];
        if create {
            args.push("-b");
        }
        args.push(branch);
        let out = Command::new("git").args(&args).output().unwrap();
        assert!(
            out.status.success(),
            "checkout failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    /// Build a repo with 3 branches:
    ///   - `main`:  "the needle is here"
    ///   - `other`: "no haystack content"
    ///   - `empty`: file deleted
    fn three_branch_fixture() -> tempfile::TempDir {
        let tmp = tempfile::tempdir().unwrap();
        init_repo(tmp.path());
        commit_file(tmp.path(), "x.txt", "the needle is here\n", "needle");
        checkout(tmp.path(), "other", true);
        commit_file(tmp.path(), "x.txt", "no haystack content\n", "no-needle");
        checkout(tmp.path(), "empty", true);
        std::fs::remove_file(tmp.path().join("x.txt")).unwrap();
        let repo = tmp.path().display().to_string();
        Command::new("git")
            .args(["-C", &repo, "add", "-A"])
            .output()
            .unwrap();
        Command::new("git")
            .args(["-C", &repo, "commit", "-q", "-m", "delete"])
            .output()
            .unwrap();
        checkout(tmp.path(), "main", false);
        tmp
    }

    /// Build a minimal `Args` pointing at `repo` with a literal pattern.
    fn args_for(repo: &std::path::Path, pattern: &str) -> Args {
        Args {
            pattern: pattern.into(),
            file: "x.txt".into(),
            regex: false,
            ignore_case: false,
            repo: repo.to_path_buf(),
            include: None,
            json: false,
            jobs: Some(2),
        }
    }

    #[test]
    fn run_returns_results_sorted_by_branch_name() {
        let tmp = three_branch_fixture();
        let results = run(&args_for(tmp.path(), "needle")).unwrap();
        let names: Vec<_> = results.iter().map(|r| r.branch.as_str()).collect();
        assert_eq!(names, vec!["empty", "main", "other"]);
    }

    #[test]
    fn run_marks_each_branch_with_correct_status() {
        let tmp = three_branch_fixture();
        let results = run(&args_for(tmp.path(), "needle")).unwrap();
        let by_branch: std::collections::HashMap<_, _> =
            results.into_iter().map(|r| (r.branch, r.status)).collect();
        assert!(matches!(by_branch["main"], BranchStatus::Matched(_)));
        assert!(matches!(by_branch["other"], BranchStatus::NoMatch));
        assert!(matches!(by_branch["empty"], BranchStatus::FileMissing));
    }

    #[test]
    fn run_returns_bad_regex_as_fatal() {
        let tmp = three_branch_fixture();
        let mut args = args_for(tmp.path(), "(");
        args.regex = true;
        let err = run(&args).unwrap_err();
        assert!(matches!(err, crate::SpelunkerError::BadRegex(_)));
    }
}
