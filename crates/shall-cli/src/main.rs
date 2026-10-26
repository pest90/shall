//! Command line interface of `shall`.

use std::path::PathBuf;

use anyhow::bail;
use clap::{Parser, Subcommand};

/// Specifications Hooked to Actual Lines of Logic: requirements-as-code
/// traceability from Markdown requirements down to code and tests.
#[derive(Debug, Parser)]
#[command(name = "shall", version, about)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Trace all requirements and report defects.
    Check {
        /// Project directory containing `shall.toml`.
        #[arg(default_value = ".")]
        path: PathBuf,
    },
    /// Write the HTML/PDF report as static files.
    Report {
        /// Project directory containing `shall.toml`.
        #[arg(default_value = ".")]
        path: PathBuf,
        /// Output directory.
        #[arg(long, default_value = "site")]
        out: PathBuf,
    },
    /// Create a new item with a generated ID.
    New {
        /// Item type, e.g. `swr`.
        typ: String,
        /// Title of the new item.
        title: String,
    },
}

fn main() -> anyhow::Result<()> {
    match Cli::parse().command {
        Command::Check { .. } => bail!("`shall check` is not implemented yet (plan: M2)"),
        Command::Report { .. } => bail!("`shall report` is not implemented yet (plan: M5)"),
        Command::New { .. } => bail!("`shall new` is not implemented yet (plan: M1.4)"),
    }
}
