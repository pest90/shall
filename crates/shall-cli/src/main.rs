//! Command line interface of `shall`.

mod check;
mod export;
mod init;
mod new;
mod output;
mod project;
mod report;

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use shall_core::config::Mode;
use shall_core::export::BuildInfo;

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
    /// the project root, and a summary on stderr. Exit code 1 if a defect
    /// fails the mode, 0 otherwise; 2 if the check cannot run.
    Check {
        /// A directory in the project; the project root is the closest
        /// directory at or above it with a `shall.toml`.
        #[arg(default_value = ".")]
        path: PathBuf,
        /// When the check fails, replacing `[check]` of `shall.toml`:
        /// `report` never, `new` on errors that the `--compare-to` run
        /// does not have, `mature` on errors at items with a mature
        /// status, `strict` on every error or warning.
        #[arg(long, value_parser = parse_mode, value_name = "report|new|mature|strict")]
        mode: Option<Mode>,
        /// The `trace.json` of the run to compare with in mode `new`, e.g.
        /// written by `shall report` on the target branch.
        #[arg(long, value_name = "TRACE_JSON")]
        compare_to: Option<PathBuf>,
        /// How to print the defects: `text`, `github` for annotations in
        /// GitHub Actions (Forgejo Actions: in the log) and a summary in
        /// `$GITHUB_STEP_SUMMARY`, or `sarif`.
        #[arg(long, value_enum, default_value_t = output::Format::Text)]
        format: output::Format,
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
        /// Where the files come from, `sha=<commit>,date=<date>,pipeline=<id>`
        /// with every part optional, or just the commit. Shown in the
        /// report; with `[report] code_url`, locations link to the code at
        /// the commit.
        #[arg(long, value_name = "INFO", value_parser = parse_build_info)]
        build_info: Option<BuildInfo>,
    },
    /// Export the requirements for other tools.
    ///
    /// With `--oft`, writes every requirement file of the project and its
    /// imports in OpenFastTrace syntax into the output directory, for a
    /// cross-check with OFT.
    Export {
        /// A directory in the project.
        #[arg(default_value = ".")]
        path: PathBuf,
        /// Write OpenFastTrace Markdown; the only format so far.
        #[arg(long, required = true)]
        oft: bool,
        /// Output directory, created if missing; relative to the current
        /// directory.
        #[arg(long, default_value = "oft")]
        out: PathBuf,
    },
    /// Create a project from a template.
    ///
    /// Writes `shall.toml` and requirement files with an example chain
    /// into the directory, created if missing, and prints the files.
    /// Existing files are never overwritten.
    Init {
        /// Directory of the new project.
        #[arg(default_value = ".")]
        dir: PathBuf,
        /// The levels and examples to start with.
        #[arg(long, value_enum, default_value_t = init::Template::Simple)]
        template: init::Template,
        /// Path of the platform that `--template project` imports, relative
        /// to the new project.
        #[arg(long, value_name = "PATH")]
        platform: Option<String>,
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

fn parse_mode(text: &str) -> Result<Mode, String> {
    text.parse()
}

fn parse_build_info(text: &str) -> Result<BuildInfo, String> {
    BuildInfo::parse(text)
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
            compare_to,
            format,
            variant,
            workspace,
        } => check::run(&check::CheckArgs {
            path,
            mode,
            compare_to,
            format,
            variant,
            workspace,
        }),
        Command::Init {
            dir,
            template,
            platform,
        } => init::run(&init::InitArgs {
            dir,
            template,
            platform,
        })
        .map(|()| ExitCode::SUCCESS),
        Command::Export { path, oft: _, out } => {
            export::run(&path, &out).map(|()| ExitCode::SUCCESS)
        }
        Command::Report {
            path,
            out,
            variant,
            all_variants,
            build_info,
        } => report::run(&report::ReportArgs {
            path,
            out,
            variant,
            all_variants,
            build_info,
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
