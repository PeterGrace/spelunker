# spelunker — Final Review Fixes

**Date:** 2026-05-22  
**Commit scope:** `src/main.rs`, `src/output.rs`, `tests/cli.rs`, `Cargo.toml`

## Summary

Three issues caught during the final end-to-end review, all addressed in a single commit.

---

## Fix 1 (Critical): BrokenPipe exits 0, not 2

**Problem:** When spelunker's stdout was closed mid-stream (e.g. `spelunker NEEDLE FILE | head`), the
`writeln!` macro returned `ErrorKind::BrokenPipe`. This was propagated as
`SpelunkerError::Output(io::Error)`, emitted an `eprintln!("error: ...")` message to stderr, and
caused an exit code of 2 — the same as a hard fatal error.

**Impact:** Every user who pipes spelunker into `head`, `grep`, `less`, or any pager would see a
spurious error message and non-zero exit. CI pipelines checking exit code would also break.

**Fix:** In `src/main.rs`, the render-error match arm now pattern-matches
`SpelunkerError::Output(io_err)` with a guard on `io_err.kind() == ErrorKind::BrokenPipe` and
returns `ExitCode::SUCCESS`. This matches the behavior of standard POSIX tools including `grep`.

**New test:** `broken_pipe_on_stdout_exits_zero_not_fatal` in `tests/cli.rs` generates 10,000
matching lines, pipes through `head -n 5`, and asserts the exit code is not 2.

---

## Fix 2 (Important): stdout flushed before summary line

**Problem:** `output::render` wrote matches to block-buffered stdout, then wrote the
`N/M branches matched` summary to line-buffered stderr. Because stdout is block-buffered when
redirected or when the terminal is a TTY with large output, the stderr write could flush first,
printing the summary *before* the match lines.

**Fix:** In `src/output.rs`, `stdout.flush()?` is called between the match-writing loop and the
`writeln!(stderr, ...)` summary call in the `Format::Human` arm. `Write::flush` was already in
scope via `use std::io::Write;`.

**Test impact:** The existing unit tests use `Vec<u8>` writers (unbuffered), so they correctly
continue to pass without modification.

---

## Fix 3 (Compliance): `dotenv` replaced with `dotenvy`

**Problem:** `CLAUDE.md` explicitly mandates `dotenvy` as the maintained successor to the deprecated
`dotenv` crate. The initial implementation used `dotenv = "0.15.0"`.

**Fix:**
- `Cargo.toml`: `dotenv = "0.15.0"` → `dotenvy = "0.15"`
- `src/main.rs`: `dotenv::dotenv()` → `dotenvy::dotenv()`

Drop-in compatible; no behavioral change.

---

## Test Results

```
test result: ok. 32 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out  (lib)
test result: ok. 11 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out  (integration)
```

Total: **43 tests, all passing.**

`cargo fmt --check` and `cargo clippy --all-targets -- -D warnings` both clean.

---

## Manual Broken-Pipe Verification

```
$ ./target/release/spelunker "BranchStatus" src/lib.rs | head -1; echo "PIPESTATUS=${PIPESTATUS[@]}"
main:19:    pub status: BranchStatus,
PIPESTATUS=0 0
```

spelunker's exit code is **0** — not 2.
