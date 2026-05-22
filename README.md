# spelunker

Search a file's contents across every branch of a local git repository.

`spelunker` is a small Rust CLI that answers the question *"which branches
contain this string in this file?"* It walks every ref in a repo, reads the
target file out of each one with `git show`, and prints grep-style matches —
without ever checking branches out or touching your working tree.

## Why

Sometimes you want to know which branches still carry a stale config value, a
deprecated import, or a TODO you wrote six months ago. `git grep` only sees
one ref at a time, and checking out 40 branches by hand is miserable. This
tool reads each branch's copy of a single file in parallel and tells you
where the pattern hits.

## Install

From source:

```sh
cargo install --path .
```

Or build a release binary:

```sh
cargo build --release
# binary at ./target/release/spelunker
```

## Usage

```
spelunker <PATTERN> <FILE> [OPTIONS]
```

- `PATTERN` — literal substring by default, or a regex with `--regex`.
- `FILE` — repository-relative path to read from each branch.

### Options

| Flag | Description |
| --- | --- |
| `-r`, `--regex` | Treat `PATTERN` as a regular expression. |
| `-i`, `--ignore-case` | Case-insensitive matching. |
| `-C`, `--repo <PATH>` | Path to the git repository (default: `.`). |
| `--include <REFSPEC>` | Limit branch scope, e.g. `release/*`. Passed to `git for-each-ref`. |
| `--json` | Emit JSON instead of grep-style text. |
| `-j`, `--jobs <N>` | Parallel worker count (default: number of CPUs). |

### Examples

Find every branch where `config/app.toml` still references the old endpoint:

```sh
spelunker "api.old.example.com" config/app.toml
```

Same thing, but only across release branches and case-insensitive:

```sh
spelunker -i --include "release/*" "api.old.example.com" config/app.toml
```

Regex search with JSON output, against a repo somewhere else:

```sh
spelunker -r --json -C /srv/repos/widget "TODO\(.*\):" src/lib.rs
```

### Output

**Human format** (default) writes matches to stdout as `branch:line_number:line`
and a summary `N/M branches matched` to stderr:

```
main:42:    endpoint = "api.old.example.com"
release/1.4:42:    endpoint = "api.old.example.com"
2/17 branches matched
```

**JSON format** (`--json`) writes a single array to stdout, one object per
branch, with each branch's status and any matching hits. Nothing goes to
stderr in JSON mode, which makes it safe to pipe.

### Exit codes

`spelunker` follows grep conventions:

- `0` — at least one branch matched.
- `1` — search ran cleanly but no branch matched.
- `2` — hard error (bad repo, git failure, I/O error, etc.).

### Environment

- `RUST_LOG` — controls log verbosity (default `warn`). Logs go to stderr.
- A `.env` file in the working directory, if present, is loaded automatically.

## Development

```sh
cargo test          # unit + integration tests
cargo clippy -- -D warnings
cargo fmt --check
```

Integration tests in `tests/cli.rs` spin up real temporary git repos via
`tempfile` and drive the binary with `assert_cmd`.

## License

[MIT](LICENSE) © Peter Grace
