//! Project management: Gemfile (managed by bundler) and running commands.
//!
//! Gems use the shared per-toolchain layout: bundler installs into the
//! toolchain's default GEM_HOME and gem executables land in the toolchain's
//! bin, so putting that directory on PATH covers both ruby and gem commands.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};

use crate::store;
use crate::versions::VersionReq;

const GEMFILE: &str = "Gemfile";

/// Nearest ancestor directory (including `start`) containing a Gemfile.
fn find_project_root(start: &Path) -> Option<PathBuf> {
    start
        .ancestors()
        .find(|dir| dir.join(GEMFILE).is_file())
        .map(Path::to_path_buf)
}

fn project_root() -> Result<PathBuf> {
    let cwd = std::env::current_dir()?;
    find_project_root(&cwd)
        .context("no Gemfile found in this directory or any parent (run `linguo ruby init` first)")
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

/// The pinned toolchain's `bundle`, with that toolchain's bin on PATH.
fn bundle(root: &Path) -> Result<Command> {
    let bin = toolchain_bin(root)?;
    let bundle = if cfg!(windows) {
        "bundle.bat"
    } else {
        "bundle"
    };
    let mut cmd = Command::new(bin.join(bundle));
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

pub fn init() -> Result<()> {
    let cwd = std::env::current_dir()?;
    let gemfile_path = cwd.join(GEMFILE);
    if gemfile_path.exists() {
        bail!("{} already exists", gemfile_path.display());
    }

    let version = store::pick_project_version(super::LANGUAGE, &cwd)?;

    std::fs::write(&gemfile_path, "source \"https://rubygems.org\"\n")
        .with_context(|| format!("failed to write {}", gemfile_path.display()))?;

    let req = VersionReq::MajorMinor(version.major, version.minor);
    crate::config::write_pin(
        &cwd.join(crate::config::PIN_FILE),
        super::LANGUAGE,
        &req.to_string(),
    )?;

    println!("initialized Gemfile with ruby {version}");
    Ok(())
}

pub fn add(gems: &[String]) -> Result<()> {
    if gems.is_empty() {
        bail!("no gems given");
    }
    let root = project_root()?;
    run_checked(bundle(&root)?.arg("add").args(gems), "bundle add")
}

pub fn remove(gems: &[String]) -> Result<()> {
    if gems.is_empty() {
        bail!("no gems given");
    }
    let root = project_root()?;
    run_checked(bundle(&root)?.arg("remove").args(gems), "bundle remove")
}

pub fn sync() -> Result<()> {
    sync_in(&project_root()?)
}

/// Sync a specific project directory (used by workspace sync).
pub fn sync_in(root: &Path) -> Result<()> {
    run_checked(bundle(root)?.arg("install"), "bundle install")
}

/// Print the path of the executable a command resolves to (default: ruby).
pub fn which(command: Option<String>) -> Result<()> {
    let cwd = std::env::current_dir()?;
    let name = command.unwrap_or_else(|| "ruby".to_string());
    if let Some(path) = crate::exec::find_in_dir(&toolchain_bin(&cwd)?, &name) {
        println!("{}", path.display());
        return Ok(());
    }
    bail!("'{name}' not found in the pinned toolchain");
}

/// Run a command with the pinned toolchain (and its gem executables) on PATH.
pub fn run(args: &[String]) -> Result<()> {
    let (program, rest) = args.split_first().context("no command given")?;
    let cwd = std::env::current_dir()?;
    let bin = toolchain_bin(&cwd)?;

    let mut cmd = crate::exec::command_in(std::slice::from_ref(&bin), program);
    cmd.args(rest).env("PATH", prepended_path(&[bin])?);
    crate::exec::exec(cmd, program)
}
