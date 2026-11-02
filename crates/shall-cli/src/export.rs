//! `shall export --oft`: the requirements in OpenFastTrace syntax, see
//! [`shall_core::oft`].

use std::fs;
use std::path::Path;

use anyhow::Context;
use shall_core::oft;

use crate::check;

/// Where an exported file goes below the output directory: at its path,
/// but a file of an external component, below
/// `.shall/cache/<name>-<hash>/<commit>/`, below `components/<name>-<hash>/`.
fn exported(path: &str) -> String {
    let Some(rest) = path.strip_prefix(".shall/cache/") else {
        return path.to_owned();
    };
    let mut parts = rest.splitn(3, '/');
    match (parts.next(), parts.next(), parts.next()) {
        (Some(component), Some(_commit), Some(file)) => format!("components/{component}/{file}"),
        _ => path.to_owned(),
    }
}

/// Reads and traces the project at or above `start` and writes its
/// requirement files, and those of its imports, in OFT syntax below
/// `out`, at their paths relative to the project or workspace root.
pub fn run(start: &Path, out: &Path) -> anyhow::Result<()> {
    let project = check::analyze(start, None)?;
    let files = oft::export(&project.items, &project.trace);
    for (path, text) in &files {
        let target = out.join(exported(path));
        if let Some(dir) = target.parent() {
            fs::create_dir_all(dir).with_context(|| format!("cannot create {}", dir.display()))?;
        }
        fs::write(&target, text).with_context(|| format!("cannot write {}", target.display()))?;
        eprintln!("wrote {}", target.display());
    }
    eprintln!("{}", project.summary());
    Ok(())
}
