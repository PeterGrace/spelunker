//! Git subprocess helpers used by spelunker.

use crate::error::{Result, SpelunkerError};
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
    #[allow(dead_code)]
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
}
