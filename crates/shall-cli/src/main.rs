//! Command line interface of `shall`.

mod new;

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
    ///
    /// Prints the ID. With `--file`, appends an item skeleton to the file
    /// and prints its location instead.
    New {
        /// Item type, a level from `shall.toml`, e.g. `swr`.
        typ: String,
        /// Title of the new item; the readable part of the ID is made from it.
        #[arg(allow_hyphen_values = true)]
        title: String,
        /// Markdown file to append the item to; created if missing, but its
        /// directory must exist.
        #[arg(long)]
        file: Option<PathBuf>,
        /// Component put in front of the name, e.g. `motor` for
        /// `swr~motor.overtemp-k4f2~1`.
        #[arg(long)]
        component: Option<String>,
        /// Do not search the Git history for IDs deleted earlier, e.g. in a
        /// shallow clone. Deleted IDs may then be handed out again.
        #[arg(long)]
        no_history: bool,
    },
}

fn main() -> anyhow::Result<()> {
    match Cli::parse().command {
        Command::Check { .. } => bail!("`shall check` is not implemented yet (plan: M2)"),
        Command::Report { .. } => bail!("`shall report` is not implemented yet (plan: M5)"),
        Command::New {
            typ,
            title,
            file,
            component,
            no_history,
        } => new::run(&new::NewArgs {
            typ,
            title,
            file,
            component,
            no_history,
        }),
    }
}
