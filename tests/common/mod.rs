//! Shared fixture helpers for integration tests.

use std::path::{Path, PathBuf};
use std::process::Command;

/// A temporary directory initialised as a git repository.
///
/// Provides convenience helpers for writing files, creating commits,
/// branches, and running arbitrary git sub-commands.
pub struct Fixture {
    pub dir: tempfile::TempDir,
}

impl Fixture {
    /// Create a new temporary directory and initialise it as a git repo
    /// with a `main` branch and a minimal git identity.
    #[allow(dead_code)]
    pub fn new() -> Self {
        let dir = tempfile::tempdir().expect("tempdir");
        let f = Self { dir };
        f.git(&["init", "-q", "-b", "main"]);
        f.git(&["config", "user.email", "test@example.invalid"]);
        f.git(&["config", "user.name", "Spelunker IT"]);
        f
    }

    /// Return the path to the root of the temporary repository.
    #[allow(dead_code)]
    pub fn path(&self) -> &Path {
        self.dir.path()
    }

    /// Return the path as an owned `PathBuf`.
    #[allow(dead_code)]
    pub fn path_buf(&self) -> PathBuf {
        self.dir.path().to_path_buf()
    }

    /// Run a git sub-command inside the repository, asserting success.
    #[allow(dead_code)]
    pub fn git(&self, args: &[&str]) {
        let repo = self.path().display().to_string();
        let out = Command::new("git")
            .args(["-C", &repo])
            .args(args)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    /// Write `contents` to the file at `path` (relative to repo root).
    #[allow(dead_code)]
    pub fn write(&self, path: &str, contents: &str) {
        std::fs::write(self.path().join(path), contents).unwrap();
    }

    /// Write `contents` to `path`, stage it, and create a commit with
    /// message `msg` on the currently checked-out branch.
    #[allow(dead_code)]
    pub fn commit(&self, path: &str, contents: &str, msg: &str) {
        self.write(path, contents);
        self.git(&["add", path]);
        self.git(&["commit", "-q", "-m", msg]);
    }

    /// Create a new branch `name` pointing at `from`.
    #[allow(dead_code)]
    pub fn branch(&self, name: &str, from: &str) {
        self.git(&["branch", name, from]);
    }

    /// Check out the branch `name`.
    #[allow(dead_code)]
    pub fn checkout(&self, name: &str) {
        self.git(&["checkout", "-q", name]);
    }
}
