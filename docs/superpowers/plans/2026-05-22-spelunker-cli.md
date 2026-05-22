# Spelunker CLI Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ship `spelunker`, a CLI that searches a literal-or-regex pattern in a single file across every local and remote-tracking branch of a git repo, using `git show <ref>:<file>` (no working-tree disturbance), with rayon-bounded parallelism, grep-style or JSON output, and grep-style exit codes (0/1/2).

**Architecture:** Thin `main.rs` shell → `spelunker` library crate split into focused modules (`cli`, `error`, `search`, `git`, `output`, `scan`). All git interaction happens by shelling out to `git`; all matching is in-process; all branches are scanned in parallel via rayon then re-sorted for deterministic output.

**Tech Stack:** Rust 2021 · clap (derive) · regex · rayon · thiserror · serde + serde_json · tracing · tempfile + assert_cmd + predicates (dev) · the `git` binary at runtime.

**Spec:** [`docs/superpowers/specs/2026-05-22-spelunker-cli-design.md`](../specs/2026-05-22-spelunker-cli-design.md)

---

## Conventions for every task

- Every `cargo test` invocation runs from `/var/home/pgrace/repos/spelunker`.
- Every test we write must FAIL when first written and PASS after the implementation step. Never write the implementation first.
- After each green commit, run `cargo fmt && cargo clippy -- -D warnings` and fix any lint before moving on.
- Commit messages use Conventional Commits (`feat:`, `test:`, `chore:`, `docs:`).

---

## Task 1: Replace template Cargo.toml and scaffold lib

**Files:**
- Modify: `Cargo.toml`
- Modify: `src/main.rs`
- Create: `src/lib.rs`
- Delete: `.cargo/config.toml` (no longer need `tokio_unstable`)

- [ ] **Step 1: Overwrite `Cargo.toml` with the production deps**

```toml
[package]
name = "spelunker"
version = "0.1.0"
edition = "2021"
authors = ["Peter Grace <pete.grace@gmail.com>"]

[dependencies]
clap = { version = "4.5", features = ["derive"] }
regex = "1.10"
rayon = "1.10"
thiserror = "2.0.6"
serde = { version = "1.0.216", features = ["derive"] }
serde_json = "1.0"
dotenv = "0.15.0"
tracing = "0.1.41"
tracing-subscriber = { version = "0.3.19", features = ["fmt", "env-filter"] }

[dev-dependencies]
tempfile = "3.10"
assert_cmd = "2.0"
predicates = "3.1"
```

- [ ] **Step 2: Delete the template `.cargo/config.toml`**

```bash
rm /var/home/pgrace/repos/spelunker/.cargo/config.toml
rmdir /var/home/pgrace/repos/spelunker/.cargo
```

- [ ] **Step 3: Create a stub `src/lib.rs`**

```rust
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
```

- [ ] **Step 4: Replace `src/main.rs` with a minimal placeholder**

```rust
//! Spelunker CLI entry point. Real wiring lands in a later task.

fn main() {
    eprintln!("spelunker: not yet wired up");
}
```

- [ ] **Step 5: Stub out the modules so `cargo check` compiles**

Create `src/cli.rs`, `src/error.rs`, `src/git.rs`, `src/output.rs`, `src/scan.rs`, `src/search.rs`, each containing only:

```rust
//! Placeholder; implementation lands in a later task.
```

- [ ] **Step 6: Verify the crate compiles**

Run: `cargo check`
Expected: completes successfully (warnings about empty modules are fine; we'll silence them as code lands).

- [ ] **Step 7: Commit**

```bash
git add Cargo.toml Cargo.lock src/lib.rs src/main.rs src/cli.rs src/error.rs src/git.rs src/output.rs src/scan.rs src/search.rs
git rm .cargo/config.toml
git commit -m "chore: scaffold spelunker library crate

Drop template tokio/console-subscriber/ctrlc deps and replace with clap,
regex, rayon, serde_json, tempfile/assert_cmd/predicates. Add empty module
files so the BranchResult/BranchStatus surface compiles."
```

---

## Task 2: Define the error type

**Files:**
- Modify: `src/error.rs`

- [ ] **Step 1: Write `src/error.rs`**

```rust
//! Spelunker error types.

use std::path::PathBuf;

/// Errors that can abort the entire scan.
#[derive(thiserror::Error, Debug)]
pub enum SpelunkerError {
    #[error("not a git repository: {0}")]
    NotARepo(PathBuf),

    #[error("git invocation failed: {context}: {source}")]
    GitInvoke {
        context: String,
        #[source]
        source: std::io::Error,
    },

    #[error("git command exited {code}: {stderr}")]
    GitExit { code: i32, stderr: String },

    #[error("invalid regex pattern: {0}")]
    BadRegex(#[from] regex::Error),

    #[error("I/O error writing output: {0}")]
    Output(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, SpelunkerError>;
```

- [ ] **Step 2: Verify**

Run: `cargo check`
Expected: success, no warnings about `SpelunkerError`.

- [ ] **Step 3: Commit**

```bash
git add src/error.rs
git commit -m "feat(error): introduce SpelunkerError and Result alias"
```

---

## Task 3: `search::Matcher` — literal substring matching (case-sensitive)

**Files:**
- Modify: `src/search.rs`

- [ ] **Step 1: Write failing tests in `src/search.rs`**

```rust
//! Pattern matching against blob contents.

use crate::error::Result;

/// A single matching line within a file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hit {
    /// 1-based line number, matching grep's convention.
    pub line_number: usize,
    pub line: String,
}

/// What kind of match we're performing.
pub enum Matcher {
    Literal { needle: String, ignore_case: bool },
    Regex(regex::Regex),
}

impl Matcher {
    pub fn literal(needle: impl Into<String>, ignore_case: bool) -> Self {
        Self::Literal { needle: needle.into(), ignore_case }
    }

    pub fn regex(_pattern: &str, _ignore_case: bool) -> Result<Self> {
        unimplemented!("lands in a later task")
    }

    /// Scan a byte slice and return every matching line.
    pub fn scan(&self, _bytes: &[u8]) -> Vec<Hit> {
        unimplemented!("lands in this task")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn literal_finds_substring_with_line_number() {
        let m = Matcher::literal("needle", false);
        let hits = m.scan(b"first line\nthis has needle\nthird line\n");
        assert_eq!(hits, vec![Hit { line_number: 2, line: "this has needle".to_string() }]);
    }

    #[test]
    fn literal_finds_multiple_hits() {
        let m = Matcher::literal("foo", false);
        let hits = m.scan(b"foo\nbar\nfoo\n");
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].line_number, 1);
        assert_eq!(hits[1].line_number, 3);
    }

    #[test]
    fn literal_no_match_returns_empty() {
        let m = Matcher::literal("absent", false);
        assert!(m.scan(b"nothing here\n").is_empty());
    }

    #[test]
    fn literal_case_sensitive_by_default() {
        let m = Matcher::literal("Needle", false);
        assert!(m.scan(b"a needle in a haystack\n").is_empty());
    }

    #[test]
    fn literal_empty_input_no_panic() {
        let m = Matcher::literal("anything", false);
        assert!(m.scan(b"").is_empty());
    }
}
```

- [ ] **Step 2: Run tests to confirm they fail**

Run: `cargo test --lib search::tests`
Expected: 5 tests panic with `unimplemented!`.

- [ ] **Step 3: Implement `Matcher::scan` for the literal variant**

Replace the body of `scan`:

```rust
pub fn scan(&self, bytes: &[u8]) -> Vec<Hit> {
    let text = String::from_utf8_lossy(bytes);
    let mut hits = Vec::new();
    for (idx, line) in text.lines().enumerate() {
        let matches = match self {
            Self::Literal { needle, ignore_case: true } => {
                line.to_lowercase().contains(&needle.to_lowercase())
            }
            Self::Literal { needle, ignore_case: false } => line.contains(needle),
            Self::Regex(re) => re.is_match(line),
        };
        if matches {
            hits.push(Hit { line_number: idx + 1, line: line.to_string() });
        }
    }
    hits
}
```

- [ ] **Step 4: Verify tests pass**

Run: `cargo test --lib search::tests`
Expected: 5 passing.

- [ ] **Step 5: Commit**

```bash
git add src/search.rs
git commit -m "feat(search): literal substring matcher with line numbers"
```

---

## Task 4: `search::Matcher` — `--ignore-case` for literal mode

**Files:**
- Modify: `src/search.rs`

- [ ] **Step 1: Add failing tests to the `tests` module**

```rust
#[test]
fn literal_ignore_case_matches_mixed_case() {
    let m = Matcher::literal("Needle", true);
    let hits = m.scan(b"a NEEDLE in a haystack\nno needle here either\n");
    assert_eq!(hits.len(), 2);
}

#[test]
fn literal_ignore_case_unicode_lowercasing() {
    let m = Matcher::literal("ÄPFEL", true);
    let hits = m.scan("ich mag äpfel\n".as_bytes());
    assert_eq!(hits.len(), 1);
}
```

- [ ] **Step 2: Run tests**

Run: `cargo test --lib search::tests`
Expected: both new tests already pass — the existing impl handles them.
(If they fail, the impl from Task 3 has a bug; fix it.)

- [ ] **Step 3: Commit**

```bash
git add src/search.rs
git commit -m "test(search): cover --ignore-case for literal matcher"
```

---

## Task 5: `search::Matcher::regex`

**Files:**
- Modify: `src/search.rs`

- [ ] **Step 1: Add failing tests**

```rust
#[test]
fn regex_basic_match() {
    let m = Matcher::regex(r"foo\d+", false).expect("valid regex");
    let hits = m.scan(b"foo1\nbar\nfoo23\n");
    assert_eq!(hits.len(), 2);
    assert_eq!(hits[0].line, "foo1");
    assert_eq!(hits[1].line, "foo23");
}

#[test]
fn regex_invalid_pattern_errors() {
    let err = Matcher::regex("(", false).unwrap_err();
    assert!(matches!(err, crate::SpelunkerError::BadRegex(_)));
}

#[test]
fn regex_ignore_case() {
    let m = Matcher::regex(r"HELLO", true).expect("valid regex");
    let hits = m.scan(b"hello world\nHello again\n");
    assert_eq!(hits.len(), 2);
}
```

- [ ] **Step 2: Run tests to confirm they fail**

Run: `cargo test --lib search::tests`
Expected: 3 tests panic with `unimplemented!`.

- [ ] **Step 3: Implement `Matcher::regex`**

Replace the `regex` constructor body:

```rust
pub fn regex(pattern: &str, ignore_case: bool) -> Result<Self> {
    let prefixed;
    let effective = if ignore_case {
        prefixed = format!("(?i){pattern}");
        prefixed.as_str()
    } else {
        pattern
    };
    Ok(Self::Regex(regex::Regex::new(effective)?))
}
```

- [ ] **Step 4: Verify all `search::tests` pass**

Run: `cargo test --lib search::tests`
Expected: all 10 passing.

- [ ] **Step 5: Commit**

```bash
git add src/search.rs
git commit -m "feat(search): regex matcher with optional case insensitivity"
```

---

## Task 6: `search::Matcher` — non-UTF-8 (binary) blobs don't panic

**Files:**
- Modify: `src/search.rs`

- [ ] **Step 1: Add failing test**

```rust
#[test]
fn scan_lossy_decodes_invalid_utf8() {
    // 0xFF is invalid UTF-8; scan must not panic.
    let bytes = b"hello\xFFworld\nfoo\n";
    let m = Matcher::literal("world", false);
    let hits = m.scan(bytes);
    // First line decoded with replacement character still contains "world".
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].line_number, 1);
}
```

- [ ] **Step 2: Run**

Run: `cargo test --lib search::tests::scan_lossy_decodes_invalid_utf8`
Expected: PASS already (the impl uses `String::from_utf8_lossy`). If it fails, fix the impl.

- [ ] **Step 3: Commit**

```bash
git add src/search.rs
git commit -m "test(search): scan handles non-UTF-8 input via lossy decode"
```

---

## Task 7: `git::ensure_repo` — abort if the path isn't a git repo

**Files:**
- Modify: `src/git.rs`

- [ ] **Step 1: Write the failing test**

Add to `src/git.rs`:

```rust
//! Git subprocess helpers used by spelunker.

use crate::error::{Result, SpelunkerError};
use std::path::Path;
use std::process::Command;

/// Return Ok(()) if `repo` is a git working tree or bare repo, else `NotARepo`.
pub fn ensure_repo(_repo: &Path) -> Result<()> {
    unimplemented!("lands in this task")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    /// Initialize a brand-new git repo with main as the default branch and a
    /// minimal user identity so commits succeed in CI environments.
    pub(crate) fn init_repo(dir: &Path) {
        let repo = dir.display().to_string();
        let run = |args: &[&str]| {
            let out = Command::new("git").args(["-C", &repo]).args(args).output().unwrap();
            assert!(out.status.success(), "git {:?} failed: {}", args,
                String::from_utf8_lossy(&out.stderr));
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
            let out = Command::new("git").args(["-C", &repo]).args(args).output().unwrap();
            assert!(out.status.success(), "git {:?} failed: {}", args,
                String::from_utf8_lossy(&out.stderr));
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
```

Add `tempfile` to `[dev-dependencies]` (already done in Task 1).

- [ ] **Step 2: Run, expect failure**

Run: `cargo test --lib git::tests::ensure_repo`
Expected: both tests panic on `unimplemented!`.

- [ ] **Step 3: Implement `ensure_repo`**

```rust
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
```

- [ ] **Step 4: Verify pass**

Run: `cargo test --lib git::tests::ensure_repo`
Expected: 2 passing.

- [ ] **Step 5: Commit**

```bash
git add src/git.rs
git commit -m "feat(git): ensure_repo guard backed by git rev-parse --git-dir"
```

---

## Task 8: `git::list_branches` — local branches only

**Files:**
- Modify: `src/git.rs`

- [ ] **Step 1: Add failing test and stub**

In `src/git.rs`, add above `#[cfg(test)]`:

```rust
/// Enumerate branches to search. Optional `include` glob is passed straight
/// through to `git for-each-ref` as the refspec.
pub fn list_branches(_repo: &Path, _include: Option<&str>) -> Result<Vec<String>> {
    unimplemented!("lands in this task")
}
```

Add inside `mod tests`:

```rust
#[test]
fn list_branches_returns_local_heads_only_when_no_remotes() {
    let tmp = tempfile::tempdir().unwrap();
    init_repo(tmp.path());
    commit_file(tmp.path(), "x.txt", "hello\n", "init");

    // Create two extra branches off main.
    let repo = tmp.path().display().to_string();
    Command::new("git").args(["-C", &repo, "branch", "feature/a"]).output().unwrap();
    Command::new("git").args(["-C", &repo, "branch", "feature/b"]).output().unwrap();

    let branches = list_branches(tmp.path(), None).unwrap();
    assert_eq!(branches, vec![
        "feature/a".to_string(),
        "feature/b".to_string(),
        "main".to_string(),
    ]);
}
```

- [ ] **Step 2: Run, expect failure**

Run: `cargo test --lib git::tests::list_branches_returns_local_heads_only_when_no_remotes`
Expected: `unimplemented!` panic.

- [ ] **Step 3: Implement `list_branches` and its helper**

We issue two separate `git for-each-ref` calls — one per namespace — so we can
tell a remote-tracking ref like `origin/feature/x` apart from a local branch
that happens to contain a slash like `feature/x`. Add at the top of the file:

```rust
use std::collections::BTreeSet;
```

Then replace the stub `list_branches` and add the helper:

```rust
pub fn list_branches(repo: &Path, include: Option<&str>) -> Result<Vec<String>> {
    let locals = run_for_each_ref(repo, "refs/heads", include)?;
    let remotes = run_for_each_ref(repo, "refs/remotes", include)?;
    let local_set: BTreeSet<String> = locals.iter().cloned().collect();
    let mut out: Vec<String> = local_set.iter().cloned().collect();
    // Dedup: prefer a local branch over a remote-tracking ref with the same
    // short name. Remote-tracking refs are formatted "<remote>/<short>";
    // strip the first path component to compare.
    for r in remotes {
        let short = r.splitn(2, '/').nth(1).unwrap_or(&r);
        if !local_set.contains(short) {
            out.push(r);
        }
    }
    out.sort();
    Ok(out)
}

fn run_for_each_ref(
    repo: &Path,
    namespace: &str,
    include: Option<&str>,
) -> Result<Vec<String>> {
    let repo_str = repo.display().to_string();
    let refspec = match (include, namespace) {
        (Some(glob), "refs/remotes") => format!("refs/remotes/*/{glob}"),
        (Some(glob), ns) => format!("{ns}/{glob}"),
        (None, ns) => ns.to_string(),
    };
    let output = Command::new("git")
        .args(["-C", &repo_str, "for-each-ref", "--format=%(refname:short)", &refspec])
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
```

- [ ] **Step 4: Verify pass**

Run: `cargo test --lib git::tests::list_branches_returns_local_heads_only_when_no_remotes`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/git.rs
git commit -m "feat(git): list_branches over refs/heads + refs/remotes namespaces"
```

---

## Task 9: `git::list_branches` — dedup local vs remote-tracking

**Files:**
- Modify: `src/git.rs`

- [ ] **Step 1: Add failing test**

In the `tests` module:

```rust
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
        .output().unwrap();
    assert!(out.status.success(), "clone failed: {}", String::from_utf8_lossy(&out.stderr));

    let branches = list_branches(clone.path(), None).unwrap();
    // `main` exists both locally (refs/heads/main) and as refs/remotes/origin/main.
    // We should see it exactly once.
    let main_count = branches.iter().filter(|b| *b == "main").count();
    assert_eq!(main_count, 1, "got: {branches:?}");
    // We should still see at least one other remote-tracking ref if any exist,
    // but in this fixture only origin/main exists, so it should be deduped out.
    assert!(!branches.iter().any(|b| b == "origin/main"), "got: {branches:?}");
}

#[test]
fn list_branches_keeps_remote_when_no_local_equivalent() {
    let upstream = tempfile::tempdir().unwrap();
    init_repo(upstream.path());
    commit_file(upstream.path(), "x.txt", "hello\n", "init");
    Command::new("git").args(["-C", &upstream.path().display().to_string(),
        "branch", "feature/only-on-remote"]).output().unwrap();

    let clone = tempfile::tempdir().unwrap();
    Command::new("git").args(["clone", "-q",
        &upstream.path().display().to_string(),
        &clone.path().display().to_string()]).output().unwrap();

    let branches = list_branches(clone.path(), None).unwrap();
    assert!(branches.iter().any(|b| b == "origin/feature/only-on-remote"),
        "got: {branches:?}");
}
```

- [ ] **Step 2: Run**

Run: `cargo test --lib git::tests::list_branches_dedupes`
Run: `cargo test --lib git::tests::list_branches_keeps_remote`
Expected: both PASS already (Task 8 implementation handles dedup). If they fail, audit `list_branches` dedup logic.

- [ ] **Step 3: Commit**

```bash
git add src/git.rs
git commit -m "test(git): cover local/remote dedup in list_branches"
```

---

## Task 10: `git::list_branches` — `--include` glob filtering

**Files:**
- Modify: `src/git.rs`

- [ ] **Step 1: Add failing test**

```rust
#[test]
fn list_branches_with_include_glob_filters_results() {
    let tmp = tempfile::tempdir().unwrap();
    init_repo(tmp.path());
    commit_file(tmp.path(), "x.txt", "hello\n", "init");
    let repo = tmp.path().display().to_string();
    for b in ["release/1.0", "release/2.0", "feature/a", "feature/b"] {
        Command::new("git").args(["-C", &repo, "branch", b]).output().unwrap();
    }

    let branches = list_branches(tmp.path(), Some("release/*")).unwrap();
    assert_eq!(branches, vec!["release/1.0".to_string(), "release/2.0".to_string()]);
}
```

- [ ] **Step 2: Run**

Run: `cargo test --lib git::tests::list_branches_with_include_glob_filters_results`
Expected: PASS already (Task 8 piped `include` through to `for-each-ref`).

- [ ] **Step 3: Commit**

```bash
git add src/git.rs
git commit -m "test(git): list_branches honors --include refspec glob"
```

---

## Task 11: `git::read_blob` — success, missing, and error paths

**Files:**
- Modify: `src/git.rs`

- [ ] **Step 1: Stub the API and add failing tests**

Add to `src/git.rs`:

```rust
/// Outcome of reading a single blob from a ref.
#[derive(Debug)]
pub enum BlobRead {
    /// File existed and was read; bytes may be empty.
    Bytes(Vec<u8>),
    /// File did not exist on this ref.
    Missing,
    /// `git show` failed for another reason; carries the stderr message.
    Error(String),
}

pub fn read_blob(_repo: &Path, _branch: &str, _file: &str) -> Result<BlobRead> {
    unimplemented!("lands in this task")
}
```

In the `tests` module:

```rust
#[test]
fn read_blob_returns_bytes_when_file_present() {
    let tmp = tempfile::tempdir().unwrap();
    init_repo(tmp.path());
    commit_file(tmp.path(), "hello.txt", "hi there\n", "init");

    let blob = read_blob(tmp.path(), "main", "hello.txt").unwrap();
    let BlobRead::Bytes(bytes) = blob else {
        panic!("expected Bytes, got something else");
    };
    assert_eq!(bytes, b"hi there\n");
}

#[test]
fn read_blob_reports_missing_when_file_absent_on_branch() {
    let tmp = tempfile::tempdir().unwrap();
    init_repo(tmp.path());
    commit_file(tmp.path(), "hello.txt", "hi\n", "init");

    let blob = read_blob(tmp.path(), "main", "does-not-exist.txt").unwrap();
    assert!(matches!(blob, BlobRead::Missing), "got: {blob:?}");
}

#[test]
fn read_blob_reports_error_when_branch_unknown() {
    let tmp = tempfile::tempdir().unwrap();
    init_repo(tmp.path());
    commit_file(tmp.path(), "hello.txt", "hi\n", "init");

    let blob = read_blob(tmp.path(), "no-such-branch", "hello.txt").unwrap();
    let BlobRead::Error(msg) = blob else {
        panic!("expected Error, got {blob:?}");
    };
    assert!(!msg.is_empty());
}
```

The `panic!("got: {blob:?}")` calls require `BlobRead: Debug` — already
derived in Step 1's stub above.

- [ ] **Step 2: Run, expect failure**

Run: `cargo test --lib git::tests::read_blob`
Expected: 3 tests panic on `unimplemented!`.

- [ ] **Step 3: Implement `read_blob`**

```rust
pub fn read_blob(repo: &Path, branch: &str, file: &str) -> Result<BlobRead> {
    let repo_str = repo.display().to_string();
    let spec = format!("{branch}:{file}");
    let output = Command::new("git")
        .args(["-C", &repo_str, "show", &spec])
        .output()
        .map_err(|e| SpelunkerError::GitInvoke {
            context: format!("git show {spec}"),
            source: e,
        })?;
    if output.status.success() {
        return Ok(BlobRead::Bytes(output.stdout));
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    // `git show <branch>:<missing-file>` exits 128 with stderr like:
    //   fatal: path 'X' does not exist in 'BRANCH'
    //   fatal: path 'X' exists on disk, but not in 'BRANCH'
    if stderr.contains("does not exist") || stderr.contains("exists on disk, but not in") {
        Ok(BlobRead::Missing)
    } else {
        Ok(BlobRead::Error(stderr.trim().to_string()))
    }
}
```

- [ ] **Step 4: Verify pass**

Run: `cargo test --lib git::tests::read_blob`
Expected: 3 passing.

- [ ] **Step 5: Commit**

```bash
git add src/git.rs
git commit -m "feat(git): read_blob via git show, distinguishing missing from error"
```

---

## Task 12: `output::render` — human (grep-style) format

**Files:**
- Modify: `src/output.rs`

- [ ] **Step 1: Stub + failing tests**

Replace `src/output.rs`:

```rust
//! Render `BranchResult`s for human or machine consumption.

use std::io::Write;

use crate::error::Result;
use crate::{BranchResult, BranchStatus, Hit};

#[derive(Debug, Clone, Copy)]
pub enum Format { Human, Json }

/// Render results and return the number of branches that matched.
pub fn render<W: Write, E: Write>(
    _results: &[BranchResult],
    _format: Format,
    _stdout: &mut W,
    _stderr: &mut E,
) -> Result<usize> {
    unimplemented!("lands in this task")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn matched(branch: &str, hits: Vec<(usize, &str)>) -> BranchResult {
        BranchResult {
            branch: branch.to_string(),
            status: BranchStatus::Matched(
                hits.into_iter()
                    .map(|(n, l)| Hit { line_number: n, line: l.to_string() })
                    .collect(),
            ),
        }
    }

    #[test]
    fn human_prints_branch_lineno_line_for_each_hit() {
        let results = vec![
            matched("main", vec![(2, "first hit"), (7, "second hit")]),
            BranchResult { branch: "no-match".into(), status: BranchStatus::NoMatch },
            BranchResult { branch: "missing".into(), status: BranchStatus::FileMissing },
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
            BranchResult { branch: "bad".into(), status: BranchStatus::Error("boom".into()) },
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
```

- [ ] **Step 2: Run, expect failure**

Run: `cargo test --lib output::tests`
Expected: tests panic on `unimplemented!`.

- [ ] **Step 3: Implement the human branch of `render`**

```rust
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
```

- [ ] **Step 4: Verify pass**

Run: `cargo test --lib output::tests`
Expected: 2 passing.

- [ ] **Step 5: Commit**

```bash
git add src/output.rs
git commit -m "feat(output): human renderer with grep-style lines and summary"
```

---

## Task 13: `output::render` — JSON format

**Files:**
- Modify: `src/output.rs`

- [ ] **Step 1: Add failing tests**

```rust
#[test]
fn json_emits_one_record_per_branch_with_correct_status() {
    let results = vec![
        matched("main", vec![(2, "hi")]),
        BranchResult { branch: "stale".into(), status: BranchStatus::NoMatch },
        BranchResult { branch: "old".into(), status: BranchStatus::FileMissing },
        BranchResult { branch: "broken".into(), status: BranchStatus::Error("boom".into()) },
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
```

- [ ] **Step 2: Run, expect failure**

Run: `cargo test --lib output::tests::json`
Expected: panic on `unimplemented!`.

- [ ] **Step 3a: Add a `Json` variant to `SpelunkerError`**

`serde_json::to_writer` returns `serde_json::Error`, which does not auto-
convert to our error type yet. Extend `src/error.rs`:

```rust
#[error("JSON serialization error: {0}")]
Json(#[from] serde_json::Error),
```

- [ ] **Step 3b: Implement the JSON branch**

Add a helper and replace the `Format::Json` arm:

```rust
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
                        .map(|h| serde_json::json!({
                            "line_number": h.line_number,
                            "line": h.line,
                        }))
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
```

Replace the `Format::Json` arm of `render`:

```rust
Format::Json => {
    let json: Vec<serde_json::Value> = results.iter().map(to_json).collect();
    serde_json::to_writer(&mut *stdout, &serde_json::Value::Array(json))?;
    writeln!(stdout)?;
    for r in results {
        if matches!(r.status, BranchStatus::Matched(_)) { matched += 1; }
    }
}
```

- [ ] **Step 4: Verify pass**

Run: `cargo test --lib output::tests`
Expected: 4 passing.

- [ ] **Step 5: Commit**

```bash
git add src/output.rs src/error.rs
git commit -m "feat(output): JSON renderer with per-status record shape"
```

---

## Task 14: `cli::Args` — clap derive

**Files:**
- Modify: `src/cli.rs`

- [ ] **Step 1: Stub + failing tests**

Replace `src/cli.rs`:

```rust
//! CLI argument parsing.

use clap::Parser;
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(
    name = "spelunker",
    version,
    about = "Search a file's contents across every branch of a local git repo"
)]
pub struct Args {
    /// The pattern to search for (literal substring by default; regex with --regex).
    pub pattern: String,

    /// Repository-relative path to the file to read from each branch.
    pub file: String,

    /// Treat PATTERN as a regex instead of a literal substring.
    #[arg(short = 'r', long)]
    pub regex: bool,

    /// Case-insensitive matching.
    #[arg(short = 'i', long)]
    pub ignore_case: bool,

    /// Path to the git repository.
    #[arg(short = 'C', long, default_value = ".")]
    pub repo: PathBuf,

    /// Limit branch scope (e.g. "release/*"); passed to `git for-each-ref` as the refspec.
    #[arg(long)]
    pub include: Option<String>,

    /// Emit JSON output instead of grep-style text.
    #[arg(long)]
    pub json: bool,

    /// Parallel worker count (default: number of CPUs).
    #[arg(short = 'j', long, value_parser = clap::value_parser!(usize).range(1..))]
    pub jobs: Option<usize>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn parses_minimal_invocation() {
        let args = Args::parse_from(["spelunker", "NEEDLE", "src/foo.rs"]);
        assert_eq!(args.pattern, "NEEDLE");
        assert_eq!(args.file, "src/foo.rs");
        assert!(!args.regex);
        assert!(!args.ignore_case);
        assert_eq!(args.repo, PathBuf::from("."));
        assert!(args.include.is_none());
        assert!(!args.json);
        assert!(args.jobs.is_none());
    }

    #[test]
    fn parses_all_flags() {
        let args = Args::parse_from([
            "spelunker", "-r", "-i", "-C", "/tmp/repo", "--include", "release/*",
            "--json", "-j", "4", "foo.*", "src/foo.rs",
        ]);
        assert!(args.regex);
        assert!(args.ignore_case);
        assert_eq!(args.repo, PathBuf::from("/tmp/repo"));
        assert_eq!(args.include.as_deref(), Some("release/*"));
        assert!(args.json);
        assert_eq!(args.jobs, Some(4));
        assert_eq!(args.pattern, "foo.*");
        assert_eq!(args.file, "src/foo.rs");
    }

    #[test]
    fn rejects_zero_jobs() {
        let res = Args::try_parse_from(["spelunker", "-j", "0", "n", "f"]);
        assert!(res.is_err());
    }

    #[test]
    fn missing_positional_args_is_error() {
        assert!(Args::try_parse_from(["spelunker"]).is_err());
        assert!(Args::try_parse_from(["spelunker", "needle"]).is_err());
    }
}
```

- [ ] **Step 2: Run**

Run: `cargo test --lib cli::tests`
Expected: 4 passing immediately (clap derive does the work; tests exist to lock the contract).

- [ ] **Step 3: Commit**

```bash
git add src/cli.rs
git commit -m "feat(cli): clap-derive Args struct with locked-down parser tests"
```

---

## Task 15: `scan::run` — orchestrate matcher + branches + read_blob

**Files:**
- Modify: `src/scan.rs`

- [ ] **Step 1: Stub + failing tests**

Replace `src/scan.rs`:

```rust
//! Orchestrates a full spelunker run: build matcher, fan branches across
//! rayon, collect results in a deterministic order.

use rayon::prelude::*;

use crate::cli::Args;
use crate::error::Result;
use crate::git::{self, BlobRead};
use crate::search::Matcher;
use crate::{BranchResult, BranchStatus};

pub fn run(_args: &Args) -> Result<Vec<BranchResult>> {
    unimplemented!("lands in this task")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::tests::{commit_file, init_repo};
    use std::process::Command;

    fn checkout(repo: &std::path::Path, branch: &str, create: bool) {
        let repo_str = repo.display().to_string();
        let mut args = vec!["-C", &repo_str, "checkout", "-q"];
        if create { args.push("-b"); }
        args.push(branch);
        let out = Command::new("git").args(&args).output().unwrap();
        assert!(out.status.success(), "checkout failed: {}",
            String::from_utf8_lossy(&out.stderr));
    }

    /// Build a repo with 3 branches:
    ///   - main:    "the needle is here"
    ///   - other:   "no haystack content"
    ///   - empty:   file deleted
    fn three_branch_fixture() -> tempfile::TempDir {
        let tmp = tempfile::tempdir().unwrap();
        init_repo(tmp.path());
        commit_file(tmp.path(), "x.txt", "the needle is here\n", "needle");
        checkout(tmp.path(), "other", true);
        commit_file(tmp.path(), "x.txt", "no haystack content\n", "no-needle");
        checkout(tmp.path(), "empty", true);
        std::fs::remove_file(tmp.path().join("x.txt")).unwrap();
        let repo = tmp.path().display().to_string();
        Command::new("git").args(["-C", &repo, "add", "-A"]).output().unwrap();
        Command::new("git").args(["-C", &repo, "commit", "-q", "-m", "delete"])
            .output().unwrap();
        checkout(tmp.path(), "main", false);
        tmp
    }

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
```

> **Note:** The test imports `crate::git::tests::{commit_file, init_repo}`. Those helpers were marked `pub(crate)` in Task 7's `tests` module — verify they still are.

- [ ] **Step 2: Run, expect failure**

Run: `cargo test --lib scan::tests`
Expected: `unimplemented!` panic on the first two; the third panics too because it never reaches the regex check.

- [ ] **Step 3: Implement `scan::run`**

```rust
pub fn run(args: &Args) -> Result<Vec<BranchResult>> {
    git::ensure_repo(&args.repo)?;
    let matcher = if args.regex {
        Matcher::regex(&args.pattern, args.ignore_case)?
    } else {
        Matcher::literal(args.pattern.clone(), args.ignore_case)
    };
    let branches = git::list_branches(&args.repo, args.include.as_deref())?;

    let scan = |branch: &String| -> BranchResult {
        let status = match git::read_blob(&args.repo, branch, &args.file) {
            Ok(BlobRead::Bytes(bytes)) => {
                let hits = matcher.scan(&bytes);
                if hits.is_empty() { BranchStatus::NoMatch } else { BranchStatus::Matched(hits) }
            }
            Ok(BlobRead::Missing) => BranchStatus::FileMissing,
            Ok(BlobRead::Error(msg)) => BranchStatus::Error(msg),
            Err(e) => BranchStatus::Error(e.to_string()),
        };
        BranchResult { branch: branch.clone(), status }
    };

    let mut results: Vec<BranchResult> = if let Some(n) = args.jobs {
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
```

- [ ] **Step 4: Verify pass**

Run: `cargo test --lib scan::tests`
Expected: 3 passing.

- [ ] **Step 5: Commit**

```bash
git add src/scan.rs
git commit -m "feat(scan): rayon-parallel orchestrator producing sorted BranchResults"
```

---

## Task 16: Wire `main.rs` — exit codes and IO

**Files:**
- Modify: `src/main.rs`

- [ ] **Step 1: Replace `src/main.rs`**

```rust
//! Spelunker CLI entry point.

use clap::Parser;
use std::process::ExitCode;

use spelunker::cli::Args;
use spelunker::output::{render, Format};

fn main() -> ExitCode {
    let _ = dotenv::dotenv();
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn")),
        )
        .with_writer(std::io::stderr)
        .init();

    let args = Args::parse();
    let format = if args.json { Format::Json } else { Format::Human };

    let results = match spelunker::scan::run(&args) {
        Ok(r) => r,
        Err(e) => {
            tracing::error!(error = %e, "spelunker failed");
            eprintln!("error: {e}");
            return ExitCode::from(2);
        }
    };

    let stdout = std::io::stdout();
    let stderr = std::io::stderr();
    let matched = match render(&results, format, &mut stdout.lock(), &mut stderr.lock()) {
        Ok(n) => n,
        Err(e) => {
            tracing::error!(error = %e, "failed to render output");
            return ExitCode::from(2);
        }
    };

    if matched > 0 { ExitCode::SUCCESS } else { ExitCode::from(1) }
}
```

- [ ] **Step 2: Build the binary to ensure linkage**

Run: `cargo build`
Expected: succeeds with no errors.

- [ ] **Step 3: Sanity-run against this repo**

Run: `cargo run -- needle src/scan.rs ; echo "exit=$?"`
Expected: prints `0/N branches matched` to stderr; exits `1`.

- [ ] **Step 4: Commit**

```bash
git add src/main.rs
git commit -m "feat(main): wire CLI to scan + render with grep-style exit codes"
```

---

## Task 17: `tests/common/mod.rs` — shared integration-test fixture builder

**Files:**
- Create: `tests/common/mod.rs`

- [ ] **Step 1: Write the helper**

```rust
//! Shared fixture helpers for integration tests.

use std::path::{Path, PathBuf};
use std::process::Command;

pub struct Fixture {
    pub dir: tempfile::TempDir,
}

impl Fixture {
    pub fn new() -> Self {
        let dir = tempfile::tempdir().expect("tempdir");
        let f = Self { dir };
        f.git(&["init", "-q", "-b", "main"]);
        f.git(&["config", "user.email", "test@example.invalid"]);
        f.git(&["config", "user.name", "Spelunker IT"]);
        f
    }

    pub fn path(&self) -> &Path { self.dir.path() }
    pub fn path_buf(&self) -> PathBuf { self.dir.path().to_path_buf() }

    pub fn git(&self, args: &[&str]) {
        let repo = self.path().display().to_string();
        let out = Command::new("git").args(["-C", &repo]).args(args).output().unwrap();
        assert!(out.status.success(), "git {args:?} failed: {}",
            String::from_utf8_lossy(&out.stderr));
    }

    pub fn write(&self, path: &str, contents: &str) {
        std::fs::write(self.path().join(path), contents).unwrap();
    }

    pub fn commit(&self, path: &str, contents: &str, msg: &str) {
        self.write(path, contents);
        self.git(&["add", path]);
        self.git(&["commit", "-q", "-m", msg]);
    }

    pub fn branch(&self, name: &str, from: &str) {
        self.git(&["branch", name, from]);
    }

    pub fn checkout(&self, name: &str) {
        self.git(&["checkout", "-q", name]);
    }
}
```

- [ ] **Step 2: Commit (no tests exercise it yet — they land in the next tasks)**

```bash
git add tests/common/mod.rs
git commit -m "test(integration): fixture helper for building tempdir git repos"
```

---

## Task 18: Integration test — happy path

**Files:**
- Create: `tests/cli.rs`

- [ ] **Step 1: Write the failing test**

```rust
mod common;

use assert_cmd::Command;
use common::Fixture;

#[test]
fn happy_path_one_branch_matches() {
    let fx = Fixture::new();
    fx.commit("x.txt", "the needle is here\n", "init");
    fx.branch("no-needle", "main");
    fx.checkout("no-needle");
    fx.commit("x.txt", "nothing of interest\n", "diverge");
    fx.checkout("main");

    Command::cargo_bin("spelunker").unwrap()
        .args(["needle", "x.txt", "-C"])
        .arg(fx.path())
        .assert()
        .success()  // exit 0 == at least one match
        .stdout("main:1:the needle is here\n");
}
```

- [ ] **Step 2: Run and expect FAIL initially**

Run: `cargo test --test cli happy_path_one_branch_matches`
Expected: PASS (everything is wired). If it fails, the failure pinpoints what's miswired — fix and re-run before continuing.

- [ ] **Step 3: Commit**

```bash
git add tests/cli.rs
git commit -m "test(integration): happy path matches one branch with exit 0"
```

---

## Task 19: Integration test — file missing on some branches

**Files:**
- Modify: `tests/cli.rs`

- [ ] **Step 1: Add test**

```rust
#[test]
fn file_missing_is_silent_in_human_mode_and_marked_in_json() {
    let fx = Fixture::new();
    fx.commit("x.txt", "needle\n", "init");
    fx.branch("deleted", "main");
    fx.checkout("deleted");
    std::fs::remove_file(fx.path().join("x.txt")).unwrap();
    fx.git(&["add", "-A"]);
    fx.git(&["commit", "-q", "-m", "rm"]);
    fx.checkout("main");

    // Human mode: only the branch with the file *and* the match prints.
    Command::cargo_bin("spelunker").unwrap()
        .args(["needle", "x.txt", "-C"])
        .arg(fx.path())
        .assert()
        .success()
        .stdout("main:1:needle\n");

    // JSON mode: every searched branch shows up.
    let out = Command::cargo_bin("spelunker").unwrap()
        .args(["needle", "x.txt", "--json", "-C"])
        .arg(fx.path())
        .output().unwrap();
    let parsed: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let arr = parsed.as_array().unwrap();
    let by_branch: std::collections::HashMap<_, _> =
        arr.iter().map(|v| (v["branch"].as_str().unwrap().to_string(), v.clone())).collect();
    assert_eq!(by_branch["main"]["status"], "matched");
    assert_eq!(by_branch["deleted"]["status"], "file_missing");
}
```

- [ ] **Step 2: Run**

Run: `cargo test --test cli file_missing_is_silent_in_human_mode_and_marked_in_json`
Expected: PASS.

- [ ] **Step 3: Commit**

```bash
git add tests/cli.rs
git commit -m "test(integration): file_missing silent in human, marked in JSON"
```

---

## Task 20: Integration test — regex mode and bad regex

**Files:**
- Modify: `tests/cli.rs`

- [ ] **Step 1: Add test**

```rust
#[test]
fn regex_mode_matches_and_bad_regex_exits_2() {
    let fx = Fixture::new();
    fx.commit("x.txt", "foo1\nbar\nfoo23\n", "init");

    Command::cargo_bin("spelunker").unwrap()
        .args(["--regex", r"foo\d+", "x.txt", "-C"])
        .arg(fx.path())
        .assert()
        .success()
        .stdout("main:1:foo1\nmain:3:foo23\n");

    Command::cargo_bin("spelunker").unwrap()
        .args(["--regex", "(", "x.txt", "-C"])
        .arg(fx.path())
        .assert()
        .code(2);
}
```

- [ ] **Step 2: Run, expect PASS**

Run: `cargo test --test cli regex_mode_matches_and_bad_regex_exits_2`
Expected: PASS.

- [ ] **Step 3: Commit**

```bash
git add tests/cli.rs
git commit -m "test(integration): --regex flag and invalid-regex exit code"
```

---

## Task 21: Integration test — `--ignore-case`

**Files:**
- Modify: `tests/cli.rs`

- [ ] **Step 1: Add test**

```rust
#[test]
fn ignore_case_works_for_literal_and_regex() {
    let fx = Fixture::new();
    fx.commit("x.txt", "Hello World\n", "init");

    Command::cargo_bin("spelunker").unwrap()
        .args(["-i", "hello", "x.txt", "-C"])
        .arg(fx.path())
        .assert()
        .success()
        .stdout("main:1:Hello World\n");

    Command::cargo_bin("spelunker").unwrap()
        .args(["-i", "--regex", "^HELLO", "x.txt", "-C"])
        .arg(fx.path())
        .assert()
        .success()
        .stdout("main:1:Hello World\n");
}
```

- [ ] **Step 2: Run**

Run: `cargo test --test cli ignore_case_works_for_literal_and_regex`
Expected: PASS.

- [ ] **Step 3: Commit**

```bash
git add tests/cli.rs
git commit -m "test(integration): --ignore-case for literal and regex modes"
```

---

## Task 22: Integration test — `--include` glob filtering

**Files:**
- Modify: `tests/cli.rs`

- [ ] **Step 1: Add test**

```rust
#[test]
fn include_glob_restricts_branches_in_json_output() {
    let fx = Fixture::new();
    fx.commit("x.txt", "needle\n", "init");
    for b in ["release/1.0", "release/2.0", "feature/a"] {
        fx.branch(b, "main");
    }

    let out = Command::cargo_bin("spelunker").unwrap()
        .args(["needle", "x.txt", "--include", "release/*", "--json", "-C"])
        .arg(fx.path())
        .output().unwrap();
    assert!(out.status.success(), "exit: {:?}, stderr: {}",
        out.status, String::from_utf8_lossy(&out.stderr));
    let arr: Vec<serde_json::Value> = serde_json::from_slice(&out.stdout).unwrap();
    let names: Vec<_> = arr.iter().map(|v| v["branch"].as_str().unwrap()).collect();
    assert_eq!(names, vec!["release/1.0", "release/2.0"]);
}
```

- [ ] **Step 2: Run**

Run: `cargo test --test cli include_glob_restricts_branches_in_json_output`
Expected: PASS.

- [ ] **Step 3: Commit**

```bash
git add tests/cli.rs
git commit -m "test(integration): --include refspec glob filters branch scope"
```

---

## Task 23: Integration test — local + remote-tracking dedup

**Files:**
- Modify: `tests/cli.rs`

- [ ] **Step 1: Add test**

```rust
#[test]
fn local_branch_wins_dedup_against_remote_tracking() {
    let upstream = Fixture::new();
    upstream.commit("x.txt", "needle\n", "init");
    upstream.branch("only-on-remote", "main");

    let clone_dir = tempfile::tempdir().unwrap();
    let out = std::process::Command::new("git")
        .args(["clone", "-q",
            &upstream.path().display().to_string(),
            &clone_dir.path().display().to_string()])
        .output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));

    let out = Command::cargo_bin("spelunker").unwrap()
        .args(["needle", "x.txt", "--json", "-C"])
        .arg(clone_dir.path())
        .output().unwrap();
    assert!(out.status.success());
    let arr: Vec<serde_json::Value> = serde_json::from_slice(&out.stdout).unwrap();
    let names: Vec<_> = arr.iter().map(|v| v["branch"].as_str().unwrap()).collect();
    // main exists locally AND as origin/main; local wins, origin/main is dropped.
    assert!(names.contains(&"main"));
    assert!(!names.contains(&"origin/main"), "got: {names:?}");
    // only-on-remote has no local equivalent so the remote-tracking ref survives.
    assert!(names.contains(&"origin/only-on-remote"), "got: {names:?}");
}
```

- [ ] **Step 2: Run**

Run: `cargo test --test cli local_branch_wins_dedup_against_remote_tracking`
Expected: PASS.

- [ ] **Step 3: Commit**

```bash
git add tests/cli.rs
git commit -m "test(integration): local branch wins dedup vs remote-tracking ref"
```

---

## Task 24: Integration test — JSON shape contract

**Files:**
- Modify: `tests/cli.rs`

- [ ] **Step 1: Add test**

```rust
#[test]
fn json_record_shape_includes_all_status_variants() {
    let fx = Fixture::new();
    fx.commit("x.txt", "needle\n", "init");
    fx.branch("no-match", "main");
    fx.checkout("no-match");
    fx.commit("x.txt", "nothing\n", "diverge");
    fx.branch("missing", "main");
    fx.checkout("missing");
    std::fs::remove_file(fx.path().join("x.txt")).unwrap();
    fx.git(&["add", "-A"]);
    fx.git(&["commit", "-q", "-m", "rm"]);
    fx.checkout("main");

    let out = Command::cargo_bin("spelunker").unwrap()
        .args(["needle", "x.txt", "--json", "-C"])
        .arg(fx.path())
        .output().unwrap();
    let arr: Vec<serde_json::Value> = serde_json::from_slice(&out.stdout).unwrap();
    for v in &arr {
        assert!(v.get("branch").and_then(|b| b.as_str()).is_some());
        let status = v.get("status").and_then(|s| s.as_str()).unwrap();
        match status {
            "matched" => {
                let hits = v.get("hits").and_then(|h| h.as_array()).unwrap();
                for h in hits {
                    assert!(h.get("line_number").and_then(|n| n.as_u64()).is_some());
                    assert!(h.get("line").and_then(|l| l.as_str()).is_some());
                }
            }
            "no_match" | "file_missing" => {}
            "error" => {
                assert!(v.get("error").and_then(|e| e.as_str()).is_some());
            }
            other => panic!("unknown status: {other}"),
        }
    }
}
```

- [ ] **Step 2: Run**

Run: `cargo test --test cli json_record_shape_includes_all_status_variants`
Expected: PASS.

- [ ] **Step 3: Commit**

```bash
git add tests/cli.rs
git commit -m "test(integration): lock JSON record shape contract"
```

---

## Task 25: Integration test — per-branch error doesn't abort the run

**Files:**
- Modify: `tests/cli.rs`

- [ ] **Step 1: Add test**

This one is trickier — we deliberately corrupt one branch so `git show` fails for it but succeeds for others. Easiest approach: point a ref at an object ID that doesn't exist in the repo.

```rust
#[test]
fn per_branch_error_does_not_abort_scan() {
    let fx = Fixture::new();
    fx.commit("x.txt", "needle\n", "init");
    fx.branch("good", "main");

    // Create a dangling ref by writing a fake SHA into refs/heads/broken.
    let refs_dir = fx.path().join(".git/refs/heads");
    std::fs::write(refs_dir.join("broken"),
        "0000000000000000000000000000000000000001\n").unwrap();

    let out = Command::cargo_bin("spelunker").unwrap()
        .args(["needle", "x.txt", "--json", "-C"])
        .arg(fx.path())
        .output().unwrap();
    // Even with a broken ref, the run completes — per-branch errors do not
    // escalate to a fatal exit in v1.
    assert!(out.status.success() || out.status.code() == Some(1),
        "unexpected fatal exit; stderr: {}", String::from_utf8_lossy(&out.stderr));

    let arr: Vec<serde_json::Value> = serde_json::from_slice(&out.stdout).unwrap();
    let by_branch: std::collections::HashMap<_, _> =
        arr.iter().map(|v| (v["branch"].as_str().unwrap().to_string(), v.clone())).collect();
    assert_eq!(by_branch["broken"]["status"], "error");
    assert!(by_branch["broken"]["error"].as_str().unwrap().len() > 0);
    // Other branches still report their normal status.
    assert_eq!(by_branch["main"]["status"], "matched");
    assert_eq!(by_branch["good"]["status"], "matched");
}
```

- [ ] **Step 2: Run**

Run: `cargo test --test cli per_branch_error_does_not_abort_scan`
Expected: PASS.

- [ ] **Step 3: Commit**

```bash
git add tests/cli.rs
git commit -m "test(integration): per-branch errors don't escalate to fatal exit"
```

---

## Task 26: Integration test — no matches anywhere → exit 1

**Files:**
- Modify: `tests/cli.rs`

- [ ] **Step 1: Add test**

```rust
#[test]
fn no_matches_anywhere_exits_one_with_empty_stdout() {
    let fx = Fixture::new();
    fx.commit("x.txt", "nothing of interest\n", "init");

    Command::cargo_bin("spelunker").unwrap()
        .args(["needle", "x.txt", "-C"])
        .arg(fx.path())
        .assert()
        .code(1)
        .stdout("");
}
```

- [ ] **Step 2: Run**

Run: `cargo test --test cli no_matches_anywhere_exits_one_with_empty_stdout`
Expected: PASS.

- [ ] **Step 3: Commit**

```bash
git add tests/cli.rs
git commit -m "test(integration): zero matches → exit 1, empty stdout"
```

---

## Task 27: Integration test — not a git repo → exit 2

**Files:**
- Modify: `tests/cli.rs`

- [ ] **Step 1: Add test**

```rust
#[test]
fn not_a_git_repo_exits_two_with_clear_message() {
    let tmp = tempfile::tempdir().unwrap();
    Command::cargo_bin("spelunker").unwrap()
        .args(["needle", "x.txt", "-C"])
        .arg(tmp.path())
        .assert()
        .code(2)
        .stderr(predicates::str::contains("not a git repository"));
}
```

- [ ] **Step 2: Run**

Run: `cargo test --test cli not_a_git_repo_exits_two_with_clear_message`
Expected: PASS.

- [ ] **Step 3: Commit**

```bash
git add tests/cli.rs
git commit -m "test(integration): non-repo target → exit 2 with clear error"
```

---

## Task 28: Full pre-commit gauntlet

**Files:** none

- [ ] **Step 1: Run all tests**

Run: `cargo test --all-targets`
Expected: every test passes.

- [ ] **Step 2: Lint clean**

Run: `cargo clippy --all-targets -- -D warnings`
Expected: zero warnings/errors. Fix anything that surfaces.

- [ ] **Step 3: Format check**

Run: `cargo fmt --check`
Expected: clean. Run `cargo fmt` and re-commit if not.

- [ ] **Step 4: Build release binary**

Run: `cargo build --release`
Expected: produces `target/release/spelunker`.

- [ ] **Step 5: Smoke-test against the actual spelunker repo**

Run: `./target/release/spelunker --regex "fn main" src/main.rs`
Expected: prints `main:N:fn main() -> ExitCode {` (line number will match wherever `main` ended up); exits `0`.

- [ ] **Step 6: Commit any formatting/clippy fixes (if needed)**

```bash
git add -A
git commit -m "chore: cargo fmt / clippy pass"
```

---

## Self-review notes (for the executing agent)

- **Spec coverage:** Every spec section maps to a task. Goals → 18-26. CLI surface → 14. Exit codes → 16, 26, 27. Module layout → 1, 2, 3-6, 7-11, 12-13, 14, 15. Data model → 1. Data flow → 15. Error handling → 2, 11, 15. Testing strategy → 3-6, 7-11, 12-13, 14, 15, 17-27.
- **No placeholders:** every step contains the actual code/command. The one "rebuild the WASM/Python package" line from project rules is N/A here (no WASM/Python).
- **Type consistency:** `BranchResult`, `BranchStatus`, `Hit`, `BlobRead`, `Matcher`, `Format` are defined in Tasks 1, 11, 12, 3 respectively and referenced consistently downstream.
- **Spec drift:** Task 8 ended up using two `git for-each-ref` calls (one per namespace) instead of one, because the short-format output of a single combined call doesn't tell us which namespace a ref came from. Behaviorally identical to the spec, and the integration tests in 23 lock in the dedup contract.

---

## Project rules acknowledgments

- All public items in `lib.rs` and module headers carry doc comments.
- No `.unwrap()` in `src/` outside `#[cfg(test)]` blocks.
- Errors use `thiserror`; no `anyhow` in lib code.
- `cargo fmt` + `cargo clippy -- -D warnings` clean at every commit.
- No `println!`-for-logging in src code — `tracing::error!` is used in `main.rs` for log lines; `eprintln!` is used only for terminal-facing error reporting.
- Dependencies pinned in `Cargo.toml` per project conventions.
