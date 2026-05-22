//! CLI argument parsing.

use clap::Parser;
use std::path::PathBuf;

/// Parsed command-line arguments for spelunker.
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
    #[arg(short = 'j', long, value_parser = parse_jobs)]
    pub jobs: Option<usize>,
}

/// Parse a `--jobs` value, rejecting zero.
///
/// clap's `value_parser!(usize)` returns an `_AnonymousValueParser` that does
/// not expose `.range()`, so we use a plain function as the value parser
/// instead, which clap calls with the raw string and expects a `Result`.
fn parse_jobs(s: &str) -> Result<usize, String> {
    let n: usize = s
        .parse()
        .map_err(|_| format!("'{s}' is not a valid integer"))?;
    if n == 0 {
        Err("jobs must be at least 1".to_string())
    } else {
        Ok(n)
    }
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
            "spelunker",
            "-r",
            "-i",
            "-C",
            "/tmp/repo",
            "--include",
            "release/*",
            "--json",
            "-j",
            "4",
            "foo.*",
            "src/foo.rs",
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
