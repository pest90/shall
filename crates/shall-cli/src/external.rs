//! External subcommands: `shall foo` runs a program `shall-foo` from the
//! `PATH`, as Git and Cargo do, so that tools can extend `shall` without
//! changing it.
//!
//! The program gets the remaining arguments and, in the environment,
//! where it runs:
//!
//! - `SHALL`: the path of the running `shall`, to call it back;
//! - `SHALL_VERSION`: its version;
//! - `SHALL_PROJECT_ROOT` and `SHALL_CONFIG`: the project at or above the
//!   current directory and its `shall.toml`, if there is one;
//! - `SHALL_WORKSPACE_ROOT`: the root of the workspace the project
//!   belongs to, or the project root without a workspace; unset if the
//!   workspace cannot be determined, e.g. because the project is no
//!   member of the workspace above it.
//!
//! Its exit code becomes the exit code of `shall`; a program killed by a
//! signal gives 128 plus the signal number, as in a shell.

use std::env;
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use anyhow::{Context, bail};
use shall_core::config;

use crate::check;
use crate::project::find_root;

/// Runs the external subcommand `args[0]` with the other arguments;
/// `builtin` are the names of the built-in commands, to suggest one for a
/// typo.
pub fn run(args: &[OsString], builtin: &[String]) -> anyhow::Result<ExitCode> {
    let (name, rest) = args.split_first().expect("clap passes the name");
    let name = name.to_string_lossy();
    if name.is_empty() || name.starts_with('-') || name.contains(['/', '\\']) {
        bail!("invalid command '{name}'; see `shall --help`");
    }
    let program = format!("shall-{name}");
    let Some(path) = find(&program, env::var_os("PATH").as_deref()) else {
        // Short names would be close to everything.
        let limit = if name.chars().count() <= 4 { 1 } else { 2 };
        let suggestion = builtin
            .iter()
            .map(String::as_str)
            .chain(["help"])
            .filter(|command| distance(command, &name) <= limit)
            .min_by_key(|command| distance(command, &name))
            .map(|command| format!("; did you mean '{command}'?"))
            .unwrap_or_default();
        bail!(
            "unknown command '{name}': no program {program} in PATH{suggestion}; see `shall --help`"
        );
    };
    let mut command = Command::new(&path);
    command.args(rest);
    command.env_remove("SHALL");
    if let Ok(exe) = env::current_exe() {
        command.env("SHALL", exe);
    }
    command.env("SHALL_VERSION", env!("CARGO_PKG_VERSION"));
    for name in ["SHALL_PROJECT_ROOT", "SHALL_CONFIG", "SHALL_WORKSPACE_ROOT"] {
        command.env_remove(name);
    }
    let cwd = env::current_dir().context("cannot read the current directory")?;
    if let Ok(root) = find_root(&cwd) {
        command.env("SHALL_CONFIG", root.join(config::FILE_NAME));
        if let Ok(workspace) = check::workspace_root(&root) {
            command.env("SHALL_WORKSPACE_ROOT", workspace);
        }
        command.env("SHALL_PROJECT_ROOT", root);
    }
    let status = command
        .status()
        .with_context(|| format!("cannot run {}", path.display()))?;
    Ok(ExitCode::from(exit_code(status)))
}

/// The exit code for the status of the program: its own if it fits into
/// a byte, 128 plus the signal number if a signal killed it, 1 otherwise,
/// e.g. for a Windows status above 255.
fn exit_code(status: std::process::ExitStatus) -> u8 {
    if let Some(code) = status.code() {
        return u8::try_from(code).unwrap_or(1);
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        if let Some(signal) = status.signal() {
            return u8::try_from(128 + signal).unwrap_or(1);
        }
    }
    1
}

/// The first executable `program` in the directories of `path`; on
/// Windows also with the extensions of `PATHEXT`.
fn find(program: &str, path: Option<&OsStr>) -> Option<PathBuf> {
    let extensions: Vec<String> = if cfg!(windows) {
        // A name with an extension, such as `foo.exe`, also as it is.
        let as_is = Path::new(program).extension().map(|_| String::new());
        as_is
            .into_iter()
            .chain(
                env::var("PATHEXT")
                    .unwrap_or_else(|_| ".EXE;.BAT;.CMD".to_owned())
                    .split(';')
                    .filter(|extension| !extension.is_empty())
                    .map(str::to_owned),
            )
            .collect()
    } else {
        vec![String::new()]
    };
    env::split_paths(path?)
        .filter(|dir| !dir.as_os_str().is_empty())
        .flat_map(|dir| {
            extensions
                .iter()
                .map(move |extension| dir.join(format!("{program}{extension}")))
        })
        .find(|candidate| is_executable(candidate))
}

/// The edit distance of two words.
fn distance(a: &str, b: &str) -> usize {
    let b: Vec<char> = b.chars().collect();
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.chars().enumerate() {
        let mut previous = row[0];
        row[0] = i + 1;
        for (j, &cb) in b.iter().enumerate() {
            let substituted = previous + usize::from(ca != cb);
            previous = row[j + 1];
            row[j + 1] = substituted.min(row[j] + 1).min(row[j + 1] + 1);
        }
    }
    row[b.len()]
}

#[cfg(unix)]
fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    path.metadata()
        .is_ok_and(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
}

#[cfg(not(unix))]
fn is_executable(path: &Path) -> bool {
    path.is_file()
}
