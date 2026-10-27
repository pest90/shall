//! Finding the project and its files.

use std::path::{Path, PathBuf};

use anyhow::Context;
use shall_core::config;

/// Returns the closest directory at or above `start` with a `shall.toml`,
/// with symbolic links resolved.
pub fn find_root(start: &Path) -> anyhow::Result<PathBuf> {
    // dunce avoids Windows `\\?\` paths, which Git does not accept.
    let start = dunce::canonicalize(start)
        .with_context(|| format!("cannot resolve {}", start.display()))?;
    let root = start
        .ancestors()
        .find(|dir| dir.join(config::FILE_NAME).is_file())
        .with_context(|| {
            format!(
                "no {} found in {} or its parents",
                config::FILE_NAME,
                start.display()
            )
        })?;
    Ok(root.to_path_buf())
}

/// Whether the path names a Markdown file, by its extension.
pub fn is_markdown(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("md") || ext.eq_ignore_ascii_case("markdown"))
}
