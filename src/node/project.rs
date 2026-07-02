//! Project management: package.json (managed by npm) and running commands.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};

use crate::store;
use crate::versions::VersionReq;

const PACKAGE_JSON: &str = "package.json";

/// Nearest ancestor directory (including `start`) containing a package.json.
fn find_project_root(start: &Path) -> Option<PathBuf> {
    start
        .ancestors()
        .find(|dir| dir.join(PACKAGE_JSON).is_file())
        .map(Path::to_path_buf)
}

fn project_root() -> Result<PathBuf> {
    let cwd = std::env::current_dir()?;
    find_project_root(&cwd).context(
        "no package.json found in this directory or any parent (run `linguo node init` first)",
    )
}

/// linguo-managed bin directories for `cwd`: the project's node_modules/.bin
/// (if present) followed by the pinned toolchain.
fn managed_bin_dirs(cwd: &Path) -> Result<Vec<PathBuf>> {
    let mut dirs: Vec<PathBuf> = Vec::new();
    if let Some(root) = find_project_root(cwd) {
        let local_bin = root.join("node_modules").join(".bin");
        if local_bin.is_dir() {
            dirs.push(local_bin);
        }
    }
    let version = store::required_toolchain(super::LANGUAGE, cwd)?;
    dirs.push(super::dist::bin_dir(&super::toolchain_path(&version)?));
    Ok(dirs)
}

fn prepended_path(dirs: &[PathBuf]) -> Result<std::ffi::OsString> {
    let current = std::env::var_os("PATH").unwrap_or_default();
    std::env::join_paths(dirs.iter().cloned().chain(std::env::split_paths(&current)))
        .context("invalid PATH entry")
}

/// npm from the pinned toolchain, with that toolchain's node on PATH (npm is
/// a node script and resolves `node` through PATH).
fn npm(root: &Path) -> Result<Command> {
    let version = store::required_toolchain(super::LANGUAGE, root)?;
    let bin = super::dist::bin_dir(&super::toolchain_path(&version)?);
    let mut cmd = Command::new(bin.join("npm"));
    cmd.current_dir(root).env("PATH", prepended_path(&[bin])?);
    Ok(cmd)
}

fn run_checked(cmd: &mut Command, what: &str) -> Result<()> {
    let status = cmd
        .status()
        .with_context(|| format!("failed to run {what}"))?;
    if !status.success() {
        bail!("{what} failed");
    }
    Ok(())
}

/// A valid npm package name derived from a directory name.
fn sanitize_name(raw: &str) -> String {
    raw.to_lowercase()
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.') {
                c
            } else {
                '-'
            }
        })
        .collect()
}

pub fn init(name: Option<String>) -> Result<()> {
    let cwd = std::env::current_dir()?;
    let package_path = cwd.join(PACKAGE_JSON);
    if package_path.exists() {
        bail!("{} already exists", package_path.display());
    }

    let version = store::pick_project_version(super::LANGUAGE, &cwd)?;

    let name = match name {
        Some(name) => name,
        None => cwd
            .file_name()
            .and_then(|n| n.to_str())
            .map(sanitize_name)
            .filter(|n| !n.is_empty())
            .context("cannot derive a project name from this directory; pass one explicitly")?,
    };

    let package = serde_json::json!({
        "name": name,
        "version": "0.1.0",
        "private": true,
        "engines": { "node": format!(">={}", version.major) },
    });
    std::fs::write(&package_path, format!("{:#}\n", package))
        .with_context(|| format!("failed to write {}", package_path.display()))?;

    let req = VersionReq::Major(version.major);
    crate::config::write_pin(
        &cwd.join(crate::config::PIN_FILE),
        super::LANGUAGE,
        &req.to_string(),
    )?;

    println!("initialized project '{name}' with node {version}");
    Ok(())
}

pub fn add(specs: &[String]) -> Result<()> {
    if specs.is_empty() {
        bail!("no packages given");
    }
    let root = project_root()?;
    run_checked(npm(&root)?.arg("install").args(specs), "npm install")
}

pub fn remove(names: &[String]) -> Result<()> {
    if names.is_empty() {
        bail!("no packages given");
    }
    let root = project_root()?;
    run_checked(npm(&root)?.arg("uninstall").args(names), "npm uninstall")
}

pub fn sync() -> Result<()> {
    let root = project_root()?;
    run_checked(npm(&root)?.arg("install"), "npm install")
}

/// Print the path of the executable a command resolves to (default: node).
pub fn which(command: Option<String>) -> Result<()> {
    let cwd = std::env::current_dir()?;
    let name = command.unwrap_or_else(|| "node".to_string());
    for dir in managed_bin_dirs(&cwd)? {
        let path = dir.join(&name);
        if crate::exec::is_executable(&path) {
            println!("{}", path.display());
            return Ok(());
        }
    }
    bail!("'{name}' not found in node_modules/.bin or the pinned toolchain");
}

/// Run a command with node_modules/.bin and the pinned toolchain on PATH.
pub fn run(args: &[String]) -> Result<()> {
    let (program, rest) = args.split_first().context("no command given")?;
    let cwd = std::env::current_dir()?;
    let dirs = managed_bin_dirs(&cwd)?;

    let mut cmd = Command::new(program);
    cmd.args(rest).env("PATH", prepended_path(&dirs)?);
    crate::exec::exec(cmd, program)
}
