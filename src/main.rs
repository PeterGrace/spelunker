//! Spelunker CLI entry point.

use clap::Parser;
use std::process::ExitCode;

use spelunker::cli::Args;
use spelunker::output::{render, Format};

fn main() -> ExitCode {
    // Load a `.env` file if present; ignore the error if none exists.
    let _ = dotenv::dotenv();

    // Configure tracing to stderr, respecting the `RUST_LOG` env var.
    // Falls back to "warn" level when the variable is absent or invalid.
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn")),
        )
        .with_writer(std::io::stderr)
        .init();

    let args = Args::parse();
    let format = if args.json {
        Format::Json
    } else {
        Format::Human
    };

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

    // Exit 0 when at least one branch matched; 1 when the search succeeded
    // but nothing was found (grep-style); 2 for hard errors above.
    if matched > 0 {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    }
}
