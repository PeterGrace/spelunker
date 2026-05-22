//! Git subprocess helpers used by spelunker.

use crate::error::{Result, SpelunkerError};
use std::collections::BTreeSet;
use std::path::Path;
use std::process::Command;

/// Return `Ok(())` if `repo` is a git working tree or bare repo, else `NotARepo`.
///
/// # Arguments
///
/// * `repo` - Path to the directory to check.
///
/// # Errors
///
/// Returns `SpelunkerError::NotARepo` if the path is not inside a git repo.
/// Returns `SpelunkerError::GitInvoke` if the git binary cannot be launched.
pub fn ensure_repo(repo: &Path) -> Result<()> {
    let repo_str = repo.display().to_string();
    let output = Command::new("git")
        .args(["-C", &repo_str, "rev-parse", "--git-dir"])
        .output()
        .map_err(|e| SpelunkerError::GitInvoke {
            context: "git rev-parse --git-dir".into(),
            source: e,
        })?;
    if output.status.success() {
        Ok(())
    } else {
        Err(SpelunkerError::NotARepo(repo.to_path_buf()))
    }
}

/// Enumerate branches to search. Optional `include` glob is passed straight
/// through to `git for-each-ref` as the refspec.
///
/// Local branches are preferred over remote-tracking refs with the same short
/// name (e.g. `main` is kept and `origin/main` is dropped). Remote branches
/// that have no local counterpart are included.
///
/// # Arguments
///
/// * `repo` - Path to the git repository root.
/// * `include` - Optional glob pattern (e.g. `"release/*"`) that limits which
///   branches are returned.
///
/// # Errors
///
/// Returns `SpelunkerError::GitInvoke` if the git binary cannot be launched.
/// Returns `SpelunkerError::GitExit` if `git for-each-ref` exits non-zero.
pub fn list_branches(repo: &Path, include: Option<&str>) -> Result<Vec<String>> {
    let locals = run_for_each_ref(repo, "refs/heads", include)?;
    let remotes = run_for_each_ref(repo, "refs/remotes", include)?;
    let local_set: BTreeSet<String> = locals.iter().cloned().collect();
    let mut out: Vec<String> = local_set.iter().cloned().collect();
    // Dedup: prefer a local branch over a remote-tracking ref with the same
    // short name. Remote-tracking refs are formatted "<remote>/<short>";
    // strip the first path component to compare.
    for r in remotes {
        let short = r.split_once('/').map(|x| x.1).unwrap_or(&r);
        if !local_set.contains(short) {
            out.push(r);
        }
    }
    out.sort();
    Ok(out)
}

/// Run `git for-each-ref --format='%(refname:short)'` against a given namespace
/// and optional glob, returning all non-empty non-HEAD lines.
///
/// # Arguments
///
/// * `repo` - Path to the git repository root.
/// * `namespace` - The ref namespace to query (e.g. `"refs/heads"` or `"refs/remotes"`).
/// * `include` - Optional glob pattern to restrict results.
///
/// # Errors
///
/// Returns `SpelunkerError::GitInvoke` if the git binary cannot be launched.
/// Returns `SpelunkerError::GitExit` if git exits non-zero.
fn run_for_each_ref(repo: &Path, namespace: &str, include: Option<&str>) -> Result<Vec<String>> {
    let repo_str = repo.display().to_string();
    let refspec = match (include, namespace) {
        (Some(glob), "refs/remotes") => format!("refs/remotes/*/{glob}"),
        (Some(glob), ns) => format!("{ns}/{glob}"),
        (None, ns) => ns.to_string(),
    };
    let output = Command::new("git")
        .args([
            "-C",
            &repo_str,
            "for-each-ref",
            "--format=%(refname:short)",
            &refspec,
        ])
        .output()
        .map_err(|e| SpelunkerError::GitInvoke {
            context: format!("git for-each-ref {refspec}"),
            source: e,
        })?;
    if !output.status.success() {
        return Err(SpelunkerError::GitExit {
            code: output.status.code().unwrap_or(-1),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        });
    }
    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter(|s| !s.is_empty() && !s.ends_with("/HEAD"))
        .map(|s| s.to_string())
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    use std::process::Command;

    /// Initialize a brand-new git repo with main as the default branch and a
    /// minimal user identity so commits succeed in CI environments.
    pub(crate) fn init_repo(dir: &Path) {
        let repo = dir.display().to_string();
        let run = |args: &[&str]| {
            let out = Command::new("git")
                .args(["-C", &repo])
                .args(args)
                .output()
                .unwrap();
            assert!(
                out.status.success(),
                "git {:?} failed: {}",
                args,
                String::from_utf8_lossy(&out.stderr)
            );
        };
        run(&["init", "-q", "-b", "main"]);
        run(&["config", "user.email", "test@example.invalid"]);
        run(&["config", "user.name", "Spelunker Tests"]);
    }

    /// Write `path` with `contents` and commit it on the current branch.
    pub(crate) fn commit_file(dir: &Path, path: &str, contents: &str, msg: &str) {
        std::fs::write(dir.join(path), contents).unwrap();
        let repo = dir.display().to_string();
        let run = |args: &[&str]| {
            let out = Command::new("git")
                .args(["-C", &repo])
                .args(args)
                .output()
                .unwrap();
            assert!(
                out.status.success(),
                "git {:?} failed: {}",
                args,
                String::from_utf8_lossy(&out.stderr)
            );
        };
        run(&["add", path]);
        run(&["commit", "-q", "-m", msg]);
    }

    #[test]
    fn ensure_repo_ok_on_real_repo() {
        let tmp = tempfile::tempdir().unwrap();
        init_repo(tmp.path());
        ensure_repo(tmp.path()).expect("should accept a real repo");
    }

    #[test]
    fn ensure_repo_rejects_plain_directory() {
        let tmp = tempfile::tempdir().unwrap();
        let err = ensure_repo(tmp.path()).unwrap_err();
        assert!(matches!(err, SpelunkerError::NotARepo(_)));
    }

    #[test]
    fn list_branches_returns_local_heads_only_when_no_remotes() {
        let tmp = tempfile::tempdir().unwrap();
        init_repo(tmp.path());
        commit_file(tmp.path(), "x.txt", "hello\n", "init");

        // Create two extra branches off main.
        let repo = tmp.path().display().to_string();
        Command::new("git")
            .args(["-C", &repo, "branch", "feature/a"])
            .output()
            .unwrap();
        Command::new("git")
            .args(["-C", &repo, "branch", "feature/b"])
            .output()
            .unwrap();

        let branches = list_branches(tmp.path(), None).unwrap();
        assert_eq!(
            branches,
            vec![
                "feature/a".to_string(),
                "feature/b".to_string(),
                "main".to_string(),
            ]
        );
    }

    #[test]
    fn list_branches_dedupes_local_vs_remote_same_short_name() {
        let upstream = tempfile::tempdir().unwrap();
        init_repo(upstream.path());
        commit_file(upstream.path(), "x.txt", "hello\n", "init");

        let clone = tempfile::tempdir().unwrap();
        let upstream_url = upstream.path().display().to_string();
        let clone_path = clone.path().display().to_string();
        let out = Command::new("git")
            .args(["clone", "-q", &upstream_url, &clone_path])
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "clone failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );

        let branches = list_branches(clone.path(), None).unwrap();
        let main_count = branches.iter().filter(|b| *b == "main").count();
        assert_eq!(main_count, 1, "got: {branches:?}");
        assert!(
            !branches.iter().any(|b| b == "origin/main"),
            "got: {branches:?}"
        );
    }

    #[test]
    fn list_branches_keeps_remote_when_no_local_equivalent() {
        let upstream = tempfile::tempdir().unwrap();
        init_repo(upstream.path());
        commit_file(upstream.path(), "x.txt", "hello\n", "init");
        Command::new("git")
            .args([
                "-C",
                &upstream.path().display().to_string(),
                "branch",
                "feature/only-on-remote",
            ])
            .output()
            .unwrap();

        let clone = tempfile::tempdir().unwrap();
        Command::new("git")
            .args([
                "clone",
                "-q",
                &upstream.path().display().to_string(),
                &clone.path().display().to_string(),
            ])
            .output()
            .unwrap();

        let branches = list_branches(clone.path(), None).unwrap();
        assert!(
            branches
                .iter()
                .any(|b| b == "origin/feature/only-on-remote"),
            "got: {branches:?}"
        );
    }
}
