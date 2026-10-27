//! `shall report`: write the traced project as static files.
//!
//! These are `index.html`, the HTML report (see [`shall_report`]), and
//! `trace.json`, the graph it is made from (see [`shall_core::export`]).

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

/// File name of the HTML report in the output directory.
const INDEX_HTML: &str = "index.html";

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
    for (name, content) in [
        (
            INDEX_HTML,
            shall_report::render(&document).context("cannot render the report")?,
        ),
        (TRACE_JSON, document.to_json()),
    ] {
        let target = args.out.join(name);
        fs::write(&target, content)
            .with_context(|| format!("cannot write {}", target.display()))?;
        eprintln!("wrote {}", target.display());
    }
    eprintln!("{}", project.summary());
    Ok(ExitCode::SUCCESS)
}
