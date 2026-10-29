//! `shall report`: write the traced project as static files.
//!
//! These are `index.html`, the HTML report (see [`shall_report`]),
//! `trace.json`, the graph it is made from (see [`shall_core::export`]),
//! the defects as `defects.txt` and `junit.xml` for CI (see
//! [`shall_report::defects`]), and `badge.svg`, `graph.dot` and
//! `graph.mmd` (see [`shall_report::diagram`]).
//! With `--all-variants`, every variant gets them in a directory of its
//! own, and `index.html` above links them.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Context, bail};
use shall_core::config;
use shall_core::export::{BuildInfo, Document, Input};

use crate::check::{self, Project};

/// Arguments of `shall report`.
pub struct ReportArgs {
    pub path: PathBuf,
    pub out: PathBuf,
    pub variant: Option<String>,
    pub all_variants: bool,
    pub build_info: Option<BuildInfo>,
}

/// File name of the exported graph in the output directory.
const TRACE_JSON: &str = "trace.json";

/// File name of the HTML report in the output directory.
const INDEX_HTML: &str = "index.html";

/// File name of the defects as text in the output directory.
const DEFECTS_TXT: &str = "defects.txt";

/// File name of the defects as JUnit XML in the output directory.
const JUNIT_XML: &str = "junit.xml";

/// File name of the badge for a README in the output directory.
const BADGE_SVG: &str = "badge.svg";

/// File names of the diagram of the project as Graphviz DOT and Mermaid.
const GRAPH_DOT: &str = "graph.dot";
const GRAPH_MMD: &str = "graph.mmd";

/// Traces the project, writes the files into the output directory and
/// prints what it wrote and the summary of `shall check` to stderr.
/// Defects do not change the exit code; `shall check` decides that.
pub fn run(args: &ReportArgs) -> anyhow::Result<ExitCode> {
    if !args.all_variants {
        let project = check::analyze(&args.path, args.variant.as_deref())?;
        write(&args.out, &document(&project, args.build_info.as_ref()))?;
        eprintln!("{}", project.summary());
        return Ok(ExitCode::SUCCESS);
    }
    let read = check::read_project(&args.path)?;
    if read.config().variants.is_empty() {
        bail!("--all-variants: {} has no [variant.*]", config::FILE_NAME);
    }
    let projects = read
        .config()
        .variants
        .keys()
        .map(|name| read.trace(Some(name)))
        .collect::<anyhow::Result<Vec<Project>>>()?;
    let documents: Vec<Document> = projects
        .iter()
        .map(|project| document(project, args.build_info.as_ref()))
        .collect();
    for (project, document) in projects.iter().zip(&documents) {
        let name = project.variant.as_deref().expect("a variant is traced");
        write(&args.out.join(name), document)?;
    }
    let overview =
        shall_report::render_overview(&documents).context("cannot render the overview")?;
    write_file(&args.out.join(INDEX_HTML), &overview)?;
    for project in &projects {
        eprintln!("{}", project.summary());
    }
    Ok(ExitCode::SUCCESS)
}

fn document<'a>(project: &'a Project, build: Option<&'a BuildInfo>) -> Document<'a> {
    Document::new(Input {
        config: &project.config,
        items: &project.items,
        trace: &project.trace,
        tests: project
            .tests
            .as_ref()
            .map(|(results, evaluation)| (results.as_slice(), evaluation)),
        diagnostics: &project.diagnostics,
        variant: project.variant.as_deref().map(|name| {
            let variant = &project.config.variants[name];
            (name, variant)
        }),
        build,
    })
}

/// Writes the report, `trace.json`, `defects.txt`, `junit.xml`, the badge
/// and the diagrams of a document into `dir`.
fn write(dir: &Path, document: &Document) -> anyhow::Result<()> {
    let html = shall_report::render(document).context("cannot render the report")?;
    write_file(&dir.join(INDEX_HTML), &html)?;
    write_file(&dir.join(TRACE_JSON), &document.to_json())?;
    write_file(
        &dir.join(DEFECTS_TXT),
        &shall_report::defects::text(document),
    )?;
    write_file(
        &dir.join(JUNIT_XML),
        &shall_report::defects::junit(document),
    )?;
    write_file(
        &dir.join(BADGE_SVG),
        &shall_report::diagram::badge(document),
    )?;
    write_file(&dir.join(GRAPH_DOT), &shall_report::diagram::dot(document))?;
    write_file(
        &dir.join(GRAPH_MMD),
        &shall_report::diagram::mermaid(document),
    )
}

/// Writes a file, creating its directory if missing, and says so on
/// stderr.
fn write_file(target: &Path, content: &str) -> anyhow::Result<()> {
    if let Some(dir) = target.parent() {
        fs::create_dir_all(dir).with_context(|| format!("cannot create {}", dir.display()))?;
    }
    fs::write(target, content).with_context(|| format!("cannot write {}", target.display()))?;
    eprintln!("wrote {}", target.display());
    Ok(())
}
