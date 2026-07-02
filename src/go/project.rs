//! Project management: go.mod (managed by the go tool) and running commands.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};

use crate::store;
use crate::versions::VersionReq;

const GO_MOD: &str = "go.mod";

/// Nearest ancestor directory (including `start`) containing a go.mod.
fn find_project_root(start: &Path) -> Option<PathBuf> {
    start
        .ancestors()
        .find(|dir| dir.join(GO_MOD).is_file())
        .map(Path::to_path_buf)
}

fn project_root() -> Result<PathBuf> {
    let cwd = std::env::current_dir()?;
    find_project_root(&cwd)
        .context("no go.mod found in this directory or any parent (run `linguo go init` first)")
}

fn toolchain_bin(dir: &Path) -> Result<PathBuf> {
    let version = store::required_toolchain(super::LANGUAGE, dir)?;
    Ok(super::dist::bin_dir(&super::toolchain_path(&version)?))
}

fn prepended_path(dirs: &[PathBuf]) -> Result<std::ffi::OsString> {
    let current = std::env::var_os("PATH").unwrap_or_default();
    std::env::join_paths(dirs.iter().cloned().chain(std::env::split_paths(&current)))
        .context("invalid PATH entry")
}

/// The pinned toolchain's `go`, with its bin dir on PATH.
fn go(dir: &Path) -> Result<Command> {
    let bin = toolchain_bin(dir)?;
    let mut cmd = Command::new(bin.join(crate::exec::exe("go")));
    cmd.current_dir(dir).env("PATH", prepended_path(&[bin])?);
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

/// A plausible module path derived from a directory name.
fn sanitize_module(raw: &str) -> String {
    raw.to_lowercase()
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '/') {
                c
            } else {
                '-'
            }
        })
        .collect()
}

pub fn init(module: Option<String>) -> Result<()> {
    let cwd = std::env::current_dir()?;
    if cwd.join(GO_MOD).exists() {
        bail!("{} already exists", cwd.join(GO_MOD).display());
    }

    let version = store::pick_project_version(super::LANGUAGE, &cwd)?;

    let module = match module {
        Some(module) => module,
        None => cwd
            .file_name()
            .and_then(|n| n.to_str())
            .map(sanitize_module)
            .filter(|m| !m.is_empty())
            .context("cannot derive a module path from this directory; pass one explicitly")?,
    };

    // Not the go() helper: the pin this project will use is written below,
    // so resolve the command from the picked version directly.
    let bin = super::dist::bin_dir(&super::toolchain_path(&version)?);
    let mut cmd = Command::new(bin.join(crate::exec::exe("go")));
    cmd.current_dir(&cwd).env("PATH", prepended_path(&[bin])?);
    run_checked(cmd.args(["mod", "init", &module]), "go mod init")?;

    let req = VersionReq::MajorMinor(version.major, version.minor);
    crate::config::write_pin(
        &cwd.join(crate::config::PIN_FILE),
        super::LANGUAGE,
        &req.to_string(),
    )?;

    println!("initialized module '{module}' with go {version}");
    Ok(())
}

pub fn add(specs: &[String]) -> Result<()> {
    if specs.is_empty() {
        bail!("no packages given");
    }
    let root = project_root()?;
    run_checked(go(&root)?.arg("get").args(specs), "go get")
}

pub fn remove(names: &[String]) -> Result<()> {
    if names.is_empty() {
        bail!("no packages given");
    }
    let root = project_root()?;
    let at_none: Vec<String> = names.iter().map(|n| format!("{n}@none")).collect();
    run_checked(go(&root)?.arg("get").args(&at_none), "go get @none")
}

pub fn sync() -> Result<()> {
    let root = project_root()?;
    run_checked(go(&root)?.args(["mod", "download"]), "go mod download")
}

/// Print the path of the executable a command resolves to (default: go).
pub fn which(command: Option<String>) -> Result<()> {
    let cwd = std::env::current_dir()?;
    let name = command.unwrap_or_else(|| "go".to_string());
    if let Some(path) = crate::exec::find_in_dir(&toolchain_bin(&cwd)?, &name) {
        println!("{}", path.display());
        return Ok(());
    }
    bail!("'{name}' not found in the pinned toolchain");
}

/// Run a command with the pinned toolchain on PATH.
pub fn run(args: &[String]) -> Result<()> {
    let (program, rest) = args.split_first().context("no command given")?;
    let cwd = std::env::current_dir()?;
    let bin = toolchain_bin(&cwd)?;

    let mut cmd = Command::new(program);
    cmd.args(rest).env("PATH", prepended_path(&[bin])?);
    crate::exec::exec(cmd, program)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitizes_module_paths() {
        assert_eq!(sanitize_module("My App"), "my-app");
        assert_eq!(sanitize_module("demo.v2"), "demo.v2");
        assert_eq!(sanitize_module("Foo_Bar"), "foo_bar");
    }
}
