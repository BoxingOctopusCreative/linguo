//! Project management: Cargo.toml (managed by cargo) and running commands.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};

const CARGO_TOML: &str = "Cargo.toml";

/// Nearest ancestor directory (including `start`) containing a Cargo.toml.
fn find_project_root(start: &Path) -> Option<PathBuf> {
    start
        .ancestors()
        .find(|dir| dir.join(CARGO_TOML).is_file())
        .map(Path::to_path_buf)
}

fn project_root() -> Result<PathBuf> {
    let cwd = std::env::current_dir()?;
    find_project_root(&cwd).context(
        "no Cargo.toml found in this directory or any parent (run `linguo rust init` first)",
    )
}

fn toolchain_bin(dir: &Path) -> Result<PathBuf> {
    let toolchain = super::required_toolchain(dir)?;
    Ok(super::dist::bin_dir(&super::toolchain_dir(&toolchain)?))
}

fn prepended_path(dirs: &[PathBuf]) -> Result<std::ffi::OsString> {
    let current = std::env::var_os("PATH").unwrap_or_default();
    std::env::join_paths(dirs.iter().cloned().chain(std::env::split_paths(&current)))
        .context("invalid PATH entry")
}

/// The pinned toolchain's cargo, with that toolchain's bin on PATH so cargo
/// resolves its sibling rustc.
fn cargo_in(dir: &Path, bin: PathBuf) -> Result<Command> {
    let mut cmd = Command::new(bin.join(crate::exec::exe("cargo")));
    cmd.current_dir(dir).env("PATH", prepended_path(&[bin])?);
    Ok(cmd)
}

fn cargo(root: &Path) -> Result<Command> {
    cargo_in(root, toolchain_bin(root)?)
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

pub fn init(name: Option<String>) -> Result<()> {
    let cwd = std::env::current_dir()?;
    if cwd.join(CARGO_TOML).exists() {
        bail!("{} already exists", cwd.join(CARGO_TOML).display());
    }

    let toolchain = super::pick_project_toolchain(&cwd)?;

    // Not the cargo() helper: the pin this project will use is written below.
    let bin = super::dist::bin_dir(&super::toolchain_dir(&toolchain)?);
    let mut cmd = cargo_in(&cwd, bin)?;
    cmd.arg("init");
    if let Some(name) = &name {
        cmd.args(["--name", name]);
    }
    run_checked(&mut cmd, "cargo init")?;

    crate::config::write_pin(
        &cwd.join(crate::config::PIN_FILE),
        super::LANGUAGE,
        &super::pin_value(&toolchain),
    )?;

    println!("initialized cargo project with rust {toolchain}");
    Ok(())
}

pub fn add(crates: &[String]) -> Result<()> {
    if crates.is_empty() {
        bail!("no crates given");
    }
    let root = project_root()?;
    run_checked(cargo(&root)?.arg("add").args(crates), "cargo add")
}

pub fn remove(crates: &[String]) -> Result<()> {
    if crates.is_empty() {
        bail!("no crates given");
    }
    let root = project_root()?;
    run_checked(cargo(&root)?.arg("remove").args(crates), "cargo remove")
}

pub fn sync() -> Result<()> {
    sync_in(&project_root()?)
}

/// Sync a specific project directory (used by workspace sync).
pub fn sync_in(root: &Path) -> Result<()> {
    run_checked(cargo(root)?.arg("fetch"), "cargo fetch")
}

/// Print the path of the executable a command resolves to (default: cargo).
pub fn which(command: Option<String>) -> Result<()> {
    let cwd = std::env::current_dir()?;
    let name = command.unwrap_or_else(|| "cargo".to_string());
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

    let mut cmd = crate::exec::command_in(std::slice::from_ref(&bin), program);
    cmd.args(rest).env("PATH", prepended_path(&[bin])?);
    crate::exec::exec(cmd, program)
}
