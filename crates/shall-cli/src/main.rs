//! Command line interface of `shall`.

mod check;
mod new;
mod project;
mod report;

use std::path::PathBuf;
use std::process::ExitCode;

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
    /// Trace all requirements, code and tests and report the defects.
    ///
    /// Prints one line `path:line: message` per defect, paths relative to
    /// the project root, and a summary on stderr. Exit code 0 unless
    /// `--mode strict` finds defects (1); 2 if the check cannot run.
    Check {
        /// A directory in the project; the project root is the closest
        /// directory at or above it with a `shall.toml`.
        #[arg(default_value = ".")]
        path: PathBuf,
        /// `report` never fails because of defects, `strict` fails on
        /// every error or warning.
        #[arg(long, value_enum, default_value_t = check::Mode::Report)]
        mode: check::Mode,
        /// Check only the items of this variant from `shall.toml`, with
        /// its test results.
        #[arg(long, conflicts_with = "workspace")]
        variant: Option<String>,
        /// Check every member of the workspace that the project belongs
        /// to; the default in the workspace root.
        #[arg(long)]
        workspace: bool,
    },
    /// Write the traced project as static files into the output directory.
    ///
    /// Writes `index.html`, the report as one self-contained file, and
    /// `trace.json`, the whole graph as JSON. Prints the summary of `shall
    /// check` to stderr; defects do not change the exit code.
    Report {
        /// A directory in the project; the project root is the closest
        /// directory at or above it with a `shall.toml`.
        #[arg(default_value = ".")]
        path: PathBuf,
        /// Output directory, created if missing; relative to the current
        /// directory.
        #[arg(long, default_value = "site")]
        out: PathBuf,
        /// Report only the items of this variant from `shall.toml`, with
        /// its test results.
        #[arg(long, conflicts_with = "all_variants")]
        variant: Option<String>,
        /// Write a report for every variant into `<out>/<variant>/` and an
        /// overview into `<out>/index.html`.
        #[arg(long)]
        all_variants: bool,
    },
    /// Create a new item with a generated ID.
    ///
    /// Prints the ID. With `--file`, appends an item skeleton to the file
    /// and prints its location instead.
    New {
        /// Item type, a level from `shall.toml`, e.g. `swr`.
        #[arg(value_name = "TYPE")]
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

/// Exit code when a command cannot run, e.g. without `shall.toml`.
const FAILURE: u8 = 2;

fn main() -> ExitCode {
    match run(Cli::parse().command) {
        Ok(code) => code,
        Err(error) => {
            eprintln!("Error: {error:#}");
            ExitCode::from(FAILURE)
        }
    }
}

fn run(command: Command) -> anyhow::Result<ExitCode> {
    match command {
        Command::Check {
            path,
            mode,
            variant,
            workspace,
        } => check::run(&check::CheckArgs {
            path,
            mode,
            variant,
            workspace,
        }),
        Command::Report {
            path,
            out,
            variant,
            all_variants,
        } => report::run(&report::ReportArgs {
            path,
            out,
            variant,
            all_variants,
        }),
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
        })
        .map(|()| ExitCode::SUCCESS),
    }
}
