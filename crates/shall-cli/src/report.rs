//! `shall report`: write the traced project as static files.
//!
//! For now this is `trace.json` only (see [`shall_core::export`]); the
//! HTML report follows.

use std::fs;
use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::Context;
use shall_core::export::{Document, Input};

use crate::check;

/// Arguments of `shall report`.
pub struct ReportArgs {
    pub path: PathBuf,
    pub out: PathBuf,
}

/// File name of the exported graph in the output directory.
const TRACE_JSON: &str = "trace.json";

/// Traces the project, writes the files into the output directory and
/// prints what it wrote and the summary of `shall check` to stderr.
/// Defects do not change the exit code; `shall check` decides that.
pub fn run(args: &ReportArgs) -> anyhow::Result<ExitCode> {
    let project = check::analyze(&args.path)?;
    let document = Document::new(Input {
        config: &project.config,
        items: &project.items,
        trace: &project.trace,
        tests: project
            .tests
            .as_ref()
            .map(|(results, evaluation)| (results.as_slice(), evaluation)),
        diagnostics: &project.diagnostics,
    });
    fs::create_dir_all(&args.out)
        .with_context(|| format!("cannot create {}", args.out.display()))?;
    let target = args.out.join(TRACE_JSON);
    fs::write(&target, document.to_json())
        .with_context(|| format!("cannot write {}", target.display()))?;
    eprintln!("wrote {}", target.display());
    eprintln!("{}", project.summary());
    Ok(ExitCode::SUCCESS)
}
