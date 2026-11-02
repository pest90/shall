//! `shall init`: create a project from a template.
//!
//! A template is a `shall.toml` and requirement files with one example
//! chain, in `templates/init/<template>/`. IDs in them are placeholders
//! `@@typ~base@@`, replaced by a generated ID `typ~base-random~1` that is
//! the same for every occurrence; `@@typ~base|other@@` is that ID with
//! the type `other`, the item a forwarding makes. `@@platform@@` is the
//! path of the platform that the `project` template imports.

use std::collections::HashMap;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Component, Path, PathBuf};

use anyhow::{Context, bail};
use clap::ValueEnum;
use shall_core::config;
use shall_core::create;
use shall_core::{ItemId, ItemType};

use crate::check;
use crate::project::shown;

/// What a new project looks like.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum Template {
    /// Requirements and design decisions: `req`, `dsn`.
    Simple,
    /// High-level, system and software requirements and design
    /// decisions: `hlr`, `sys`, `swr`, `dsn`.
    Automotive,
    /// A platform with capabilities for customer projects: `cap`, `swr`,
    /// `dsn`.
    Platform,
    /// Customer requirements `hlr` of a project that imports a platform.
    Project,
}

/// The files of each template, by path relative to the project root.
const FILES: [(Template, &[(&str, &str)]); 4] = [
    (
        Template::Simple,
        &[
            (
                "shall.toml",
                include_str!("../templates/init/simple/shall.toml"),
            ),
            (
                "reqs/requirements.md",
                include_str!("../templates/init/simple/reqs/requirements.md"),
            ),
            (
                "reqs/design.md",
                include_str!("../templates/init/simple/reqs/design.md"),
            ),
        ],
    ),
    (
        Template::Automotive,
        &[
            (
                "shall.toml",
                include_str!("../templates/init/automotive/shall.toml"),
            ),
            (
                "reqs/hlr.md",
                include_str!("../templates/init/automotive/reqs/hlr.md"),
            ),
            (
                "reqs/sys.md",
                include_str!("../templates/init/automotive/reqs/sys.md"),
            ),
            (
                "reqs/swr.md",
                include_str!("../templates/init/automotive/reqs/swr.md"),
            ),
            (
                "reqs/design.md",
                include_str!("../templates/init/automotive/reqs/design.md"),
            ),
        ],
    ),
    (
        Template::Platform,
        &[
            (
                "shall.toml",
                include_str!("../templates/init/platform/shall.toml"),
            ),
            (
                "reqs/capabilities.md",
                include_str!("../templates/init/platform/reqs/capabilities.md"),
            ),
            (
                "reqs/software.md",
                include_str!("../templates/init/platform/reqs/software.md"),
            ),
            (
                "reqs/design.md",
                include_str!("../templates/init/platform/reqs/design.md"),
            ),
        ],
    ),
    (
        Template::Project,
        &[
            (
                "shall.toml",
                include_str!("../templates/init/project/shall.toml"),
            ),
            (
                "reqs/customer.md",
                include_str!("../templates/init/project/reqs/customer.md"),
            ),
        ],
    ),
];

/// Where the `project` template looks for the platform by default: the
/// layout of a workspace with `platform/` and `projects/<name>/`.
pub const DEFAULT_PLATFORM: &str = "../../platform";

/// Arguments of `shall init`.
pub struct InitArgs {
    pub dir: PathBuf,
    pub template: Template,
    pub platform: Option<String>,
}

/// Creates the project, prints the files written and then warns about
/// what keeps `shall check` from running in it.
pub fn run(args: &InitArgs) -> anyhow::Result<()> {
    let random_index = || rand::random_range(0..create::ALPHABET.len());
    if args.platform.is_some() && args.template != Template::Project {
        bail!("--platform is only for --template project");
    }
    let platform = args.platform.as_deref().unwrap_or(DEFAULT_PLATFORM);
    if Path::new(platform)
        .components()
        .all(|part| part == Component::CurDir)
    {
        bail!("--platform must name another directory than the project");
    }
    let files = FILES
        .iter()
        .find(|(template, _)| *template == args.template)
        .expect("every template has files")
        .1;
    let contents = fill(files, &toml_string(platform), random_index)?;
    // Check everything before writing anything: no file may exist, also
    // not as a broken symbolic link, and every directory on the way must
    // be one or be missing.
    for (path, _) in &contents {
        let path = args.dir.join(path);
        if fs::symlink_metadata(&path).is_ok() {
            bail!("{} exists already", shown(&path));
        }
        for dir in path.ancestors().skip(1) {
            if fs::symlink_metadata(dir).is_ok() && !dir.is_dir() {
                bail!("{} is not a directory", shown(dir));
            }
        }
    }
    let mut written: Vec<PathBuf> = Vec::new();
    for (path, text) in &contents {
        let path = args.dir.join(path);
        if let Err(error) = create(&path, text) {
            // Leave no project behind that is only half written.
            for file in &written {
                let _ = fs::remove_file(file);
            }
            return Err(error);
        }
        written.push(path);
    }
    for path in &written {
        println!("created {}", shown(path));
    }
    let root = dunce::canonicalize(&args.dir)
        .with_context(|| format!("cannot resolve {}", shown(&args.dir)))?;
    // The files are written: what keeps `shall check` from running is only
    // a warning here.
    match check::place(&root) {
        Err(error) => eprintln!("warning: {error:#}"),
        Ok(place) if place.members.is_none() && args.template == Template::Project => {
            eprintln!(
                "warning: {} is in no workspace, but a project imports only members of \
                 its own workspace; list both in the [workspace] members of a shall.toml above",
                shown(&args.dir)
            );
        }
        Ok(_) => {}
    }
    let imported = dunce::canonicalize(root.join(platform)).ok();
    if args.template == Template::Project && imported.as_ref() == Some(&root) {
        eprintln!(
            "warning: --platform {platform} is the new project itself, which cannot import \
             itself; fix the [[import]] path in shall.toml"
        );
    } else if args.template == Template::Project
        && !root.join(platform).join(config::FILE_NAME).is_file()
    {
        eprintln!(
            "warning: no {} at {platform} relative to {}; create the platform there \
             (`shall init --template platform`) or fix the [[import]] path in shall.toml",
            config::FILE_NAME,
            shown(&args.dir)
        );
    }
    Ok(())
}

/// Creates the file `path` with `text`, and the directories above it; a
/// file that exists, also as a link, is an error.
fn create(path: &Path, text: &str) -> anyhow::Result<()> {
    let parent = path.parent().expect("files are in the project directory");
    fs::create_dir_all(parent).with_context(|| format!("cannot create {}", shown(parent)))?;
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .with_context(|| format!("cannot create {}", shown(path)))?;
    file.write_all(text.as_bytes()).map_err(|error| {
        let _ = fs::remove_file(path);
        anyhow::Error::new(error).context(format!("cannot write {}", shown(path)))
    })
}

/// The content of a TOML basic string for `text`, without the quotes.
fn toml_string(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c if c.is_control() => out.push_str(&format!("\\u{:04X}", u32::from(c))),
            c => out.push(c),
        }
    }
    out
}

/// The files of a template with the placeholders replaced, in order.
fn fill(
    files: &[(&str, &str)],
    platform: &str,
    mut random_index: impl FnMut() -> usize,
) -> anyhow::Result<Vec<(String, String)>> {
    let mut ids: HashMap<String, ItemId> = HashMap::new();
    let mut filled = Vec::new();
    for (path, text) in files {
        let mut out = String::with_capacity(text.len());
        let mut rest = *text;
        while let Some(start) = rest.find("@@") {
            out.push_str(&rest[..start]);
            let after = &rest[start + 2..];
            let end = after
                .find("@@")
                .with_context(|| format!("unclosed placeholder in the template file {path}"))?;
            let placeholder = &after[..end];
            out.push_str(&replacement(
                placeholder,
                platform,
                &mut ids,
                &mut random_index,
            )?);
            rest = &after[end + 2..];
        }
        out.push_str(rest);
        filled.push(((*path).to_owned(), out));
    }
    Ok(filled)
}

/// The text for one placeholder; IDs are generated on first use.
fn replacement(
    placeholder: &str,
    platform: &str,
    ids: &mut HashMap<String, ItemId>,
    random_index: &mut impl FnMut() -> usize,
) -> anyhow::Result<String> {
    if placeholder == "platform" {
        return Ok(platform.to_owned());
    }
    let (key, forwarded) = match placeholder.split_once('|') {
        Some((key, typ)) => (key, Some(typ)),
        None => (placeholder, None),
    };
    let id = match ids.get(key) {
        Some(id) => id.clone(),
        None => {
            let (typ, base) = key
                .split_once('~')
                .with_context(|| format!("invalid placeholder '{placeholder}'"))?;
            let typ: ItemType = typ.parse()?;
            // Only the IDs of this template can be taken: the files are new.
            let id = create::generate_id(&typ, base, &mut *random_index, |id| {
                ids.values().any(|known| known.same_item(id))
            })?;
            ids.insert(key.to_owned(), id.clone());
            id
        }
    };
    Ok(match forwarded {
        Some(typ) => ItemId::new(typ.parse()?, id.name(), id.revision())?.to_string(),
        None => id.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use shall_core::config::Config;
    use shall_core::markdown;

    use super::*;

    /// A random source that counts up, so that the IDs are reproducible.
    fn counter() -> impl FnMut() -> usize {
        let mut next = 0;
        move || {
            next += 1;
            next
        }
    }

    #[test]
    fn every_template_is_a_valid_project() {
        for (template, files) in FILES {
            let filled = fill(files, DEFAULT_PLATFORM, counter()).unwrap();
            assert_eq!(filled[0].0, "shall.toml");
            let config = Config::parse(&filled[0].1).unwrap();
            assert!(!config.levels.is_empty(), "{template:?}");
            let mut ids = Vec::new();
            for (path, text) in &filled[1..] {
                assert!(!text.contains("@@"), "{path}");
                let parsed = markdown::parse(Path::new(path), text).unwrap();
                assert_eq!(parsed.diagnostics, [], "{template:?} {path}");
                ids.extend(parsed.items.into_iter().map(|item| item.id.to_string()));
            }
            let mut unique = ids.clone();
            unique.sort();
            unique.dedup();
            assert_eq!(unique.len(), ids.len(), "{template:?}: {ids:?}");
        }
    }

    #[test]
    fn placeholders_name_one_id() {
        let files = [(
            "a.md",
            "`@@req~a@@` `@@req~b@@` @@req~a@@ @@req~a|sys@@ @@platform@@",
        )];
        let filled = fill(&files, "../p", counter()).unwrap();
        assert_eq!(
            filled[0].1,
            "`req~a-1234~1` `req~b-5678~1` req~a-1234~1 sys~a-1234~1 ../p"
        );
        assert!(fill(&[("a.md", "@@req~a")], "", counter()).is_err());
        assert_eq!(toml_string("a\\b\"c\u{1}d"), "a\\\\b\\\"c\\u0001d");
    }
}
