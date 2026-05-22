# Spelunker CLI — Design

**Status:** Approved
**Date:** 2026-05-22
**Author:** Peter Grace (via brainstorming session)

## Purpose

Spelunker is a CLI tool that searches for a string (literal or regex) inside a
single file path across every known branch of a local git repository. It exists
to answer questions of the form: *"Which of our 300 branches has `FOO_BAR`
mentioned in `src/config.rs`?"* without forcing the user to manually check out
each branch.

## Goals

- Iterate every local branch (`refs/heads/*`) and every remote-tracking branch
  (`refs/remotes/*/*`), deduplicated so a branch present in both is searched once.
- Read the target file at each ref **without disturbing the working tree** (via
  `git show <ref>:<file>`).
- Match the file contents against a user-supplied pattern (literal substring by
  default, regex with `--regex`).
- Report results in grep-style human output by default, with `--json` for
  machine consumption.
- Scale comfortably to ~300 branches by using bounded parallelism (rayon).

## Non-goals (v1)

- Searching across multiple files in one invocation.
- Auto-fetching from remotes (we only read what is already in the local
  object database).
- Following submodules.
- A `--strict` mode that escalates per-branch errors to a fatal exit code
  (punted).
- TUI, daemon mode, or watch mode.

## CLI surface

```
spelunker <PATTERN> <FILE> [OPTIONS]

ARGS:
  <PATTERN>   The string (or regex with --regex) to search for
  <FILE>      Repository-relative path to the file to read from each branch

OPTIONS:
  -r, --regex                 Treat PATTERN as a regex instead of a literal
  -i, --ignore-case           Case-insensitive matching
  -C, --repo <PATH>           Path to the git repository (default: ".")
      --include <GLOB>        Limit branch scope (e.g. "release/*"); passed
                              straight to `git for-each-ref` as the refspec
      --json                  Emit JSON output instead of grep-style text
  -j, --jobs <N>              Parallel worker count (default: num CPUs)
  -h, --help                  Print help
  -V, --version               Print version
```

`--ignore-case` applies to both literal and regex modes (regex uses the `(?i)`
prefix internally).

## Exit codes

Mirrors `grep`:

- `0` — at least one branch matched the pattern.
- `1` — ran successfully, zero matches.
- `2` — usage error or whole-run fatal error (not a git repo, invalid regex,
  unable to enumerate refs, unable to spawn `git`).

Per-branch errors do **not** affect the exit code in v1. They are surfaced
in the output but do not fail the run.

## Module layout

```
src/
  main.rs        Thin: dotenv, tracing init, clap parse, call lib, set exit code
  lib.rs         Re-exports the public surface, nothing else
  cli.rs         Args struct (clap derive)
  git.rs         Branch enumeration; read a path from a ref via `git show`
  search.rs      Matcher enum (Literal | Regex), with optional case folding
  scan.rs        Glue: build matcher, fan branches across rayon, collect results
  output.rs      Render results — human (grep-style) and JSON
  error.rs       thiserror error types
tests/
  cli.rs         Integration tests against fixture repos built in tempdirs
  common/mod.rs  Fixture-building helpers
```

## Data model

```rust
pub struct BranchResult {
    pub branch: String,
    pub status: BranchStatus,
}

pub enum BranchStatus {
    Matched(Vec<Hit>),
    NoMatch,
    FileMissing,
    Error(String),
}

pub struct Hit {
    pub line_number: usize,  // 1-based, matches grep
    pub line: String,
}
```

## Data flow

1. **`main.rs`** initializes tracing and parses `Args` via clap, then calls
   `spelunker::run(args)`.
2. **`scan::run`** builds a `Matcher` (failing the run with a clear error if
   regex compilation fails).
3. **`git::list_branches`** invokes
   `git for-each-ref --format='%(refname:short)' refs/heads refs/remotes [include-glob]`,
   filters out `*/HEAD` symbolic refs, dedupes so a branch present both locally
   and as a remote-tracking ref is searched once (local wins). Returns a
   stably-sorted `Vec<String>`.
4. **`rayon::par_iter`** maps each branch to a `BranchResult`:
   - `git::read_blob(repo, branch, file)` runs `git show <branch>:<file>`.
   - Exit code 128 with stderr containing "does not exist" →
     `BranchStatus::FileMissing`.
   - Other failures → `BranchStatus::Error(stderr)`.
   - Success → `search::scan(&matcher, &bytes)` produces `Matched(...)` or
     `NoMatch`.
5. Results are collected into a `Vec<BranchResult>` and re-sorted by branch
   name so output ordering is deterministic across runs.
6. **`output::render`** writes to stdout in either human or JSON format.
7. **`main.rs`** translates the result count into the appropriate exit code.

## Behavioral details

- **Branch enumeration:** `git for-each-ref` with both `refs/heads` and
  `refs/remotes` namespaces. The optional `--include <glob>` flag is appended
  to the refspec list, so the glob is interpreted by git itself rather than
  filtered in-process.
- **Dedup rule:** if both `feature/x` (local) and `origin/feature/x`
  (remote-tracking) exist, the local form is kept. Different remotes
  pointing at the same short name are each kept because their refnames differ.
- **Blob read:** `std::process::Command::new("git").args(["-C", repo, "show",
  &format!("{branch}:{file}")])` with stdout captured. UTF-8 invalid bytes are
  decoded with `String::from_utf8_lossy` so binary blobs don't panic the matcher.
- **Output ordering:** parallel collection followed by `sort_by(|a, b|
  a.branch.cmp(&b.branch))` before rendering. Human and JSON outputs both use
  this stable order.
- **File-missing in human mode:** silent (no line printed). In JSON mode the
  record carries `status: "file_missing"` so scripts can distinguish.
- **Per-branch error in human mode:** one line to **stderr** of the form
  `branch: <error message>`. In JSON mode: `status: "error"` with the message.
  Stdout remains clean for piping.
- **Final summary** (human mode only, to stderr): `N/M branches matched`
  where M counts only branches that were actually searched (i.e., after the
  `--include` filter).

## Error handling

```rust
#[derive(thiserror::Error, Debug)]
pub enum SpelunkerError {
    #[error("not a git repository: {0}")]
    NotARepo(PathBuf),

    #[error("git invocation failed: {context}: {source}")]
    GitInvoke { context: String, #[source] source: std::io::Error },

    #[error("git command exited {code}: {stderr}")]
    GitExit { code: i32, stderr: String },

    #[error("invalid regex pattern: {0}")]
    BadRegex(#[from] regex::Error),

    #[error("I/O error writing output: {0}")]
    Output(#[from] std::io::Error),
}
```

Two-tier split:

- **Whole-run fatal:** Not a git repo, regex won't compile, can't spawn `git`,
  can't list refs at all → `SpelunkerError` is returned from `lib::run`,
  logged via `tracing::error!`, and `main` exits `2`.
- **Per-branch recoverable:** A single branch's `git show` fails for any
  non-`FileMissing` reason → recorded as `BranchStatus::Error(String)`,
  surfaced in output, scan continues.

No `.unwrap()` in lib code (per project rules). `.expect()` allowed only for
documented invariants — e.g., a regex constructed from a known-good literal.

## Dependencies

Add to `Cargo.toml`:

- `clap` (with `derive` feature) — CLI parsing.
- `regex` — regex matching mode.
- `rayon` — bounded parallel branch fan-out.
- `anyhow` — error context where appropriate (lib uses `thiserror`).
- `serde_json` — JSON output.
- `tempfile` (dev-dep) — fixture repos for tests.
- `assert_cmd` + `predicates` (dev-deps) — drive the binary in integration tests.

Open question for implementation: the current template pulls in `tokio` and
`console-subscriber`. Spelunker is synchronous (subprocess + CPU-bound
matching, parallelized via rayon), so neither is needed. We will remove both
during implementation unless the user prefers to keep them as scaffolding for
future features. **Recommend: remove.**

## Testing strategy

**Unit tests (`#[cfg(test)]` in each `src/` module):**

| Module | Coverage |
|---|---|
| `search` | Literal match (case-sensitive + `--ignore-case`); regex match (anchored, unicode classes, no-match); line-number accuracy; multiple hits per file; empty file; binary blob → lossy decode doesn't panic. |
| `output` | Human renderer formats `branch:line:text`; JSON renderer round-trips through `serde_json` and covers every `BranchStatus` variant; summary line counts add up. |
| `cli` | clap parses representative invocations; mutually-exclusive flags rejected; `--jobs 0` rejected; missing positional args produce a useful error. |

**Integration tests (`tests/cli.rs`)** against real fixture repos built fresh
in a `tempfile::TempDir`:

1. Happy path — 3 branches, only one contains the needle. Exit `0`, exactly
   one `branch:line:text` line on stdout.
2. File missing on some branches — present on A, deleted on B, never existed
   on C. B and C produce no human-mode output; JSON marks them `file_missing`.
3. Regex mode — `--regex 'foo[0-9]+'` matches across branches; bad regex
   returns exit `2`.
4. `--ignore-case` — honored by both literal and regex modes.
5. `--include 'release/*'` — non-matching branches don't appear in JSON
   output at all.
6. Local + remote dedupe — `origin/main` set to a different commit than
   `main`; each is searched once and labeled distinctly.
7. JSON shape — `--json` output is parseable; each record has
   `{branch, status, hits?, error?}`.
8. Per-branch error doesn't abort — corrupt one ref / point at a missing
   object; scan completes and reports `status: "error"` on that ref only.
9. No matches anywhere — exit `1`. Human mode: empty stdout. JSON mode:
   one record per searched branch with `status: "no_match"` or
   `status: "file_missing"` (the array is only empty if `--include` matched
   zero refs).
10. Not a git repo — run against a plain `TempDir`; exit `2` with a clear
    error message.

**Test infrastructure:**

- `tempfile` for scratch repos; `assert_cmd` + `predicates` to drive the
  binary.
- Fixture builders live in `tests/common/mod.rs` so test bodies stay short.
- **No mocking of `git`** — integration tests shell out to the real `git`
  binary against real temp repos. Mocking the subprocess would invalidate
  the entire approach.

## TDD discipline

The implementation plan handed to `writing-plans` will be structured as
RED → GREEN → REFACTOR per slice, per the superpowers TDD skill and project
rules. Each test lands before the code that makes it pass.

## Out of scope (future work)

- `--strict` flag to escalate per-branch errors to exit `2`.
- Multiple file paths in one invocation.
- Auto-`git fetch` before scanning to ensure remotes are up to date.
- Submodule traversal.
- Search across tags (`refs/tags/*`) in addition to branches.
