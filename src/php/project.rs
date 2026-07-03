//! Project management: composer.json driven through the toolchain's bundled
//! composer.phar, with per-project vendor/ (composer's own convention).

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};

use crate::store;
use crate::versions::VersionReq;

const COMPOSER_JSON: &str = "composer.json";

/// Nearest ancestor directory (including `start`) containing a composer.json.
fn find_project_root(start: &Path) -> Option<PathBuf> {
    start
        .ancestors()
        .find(|dir| dir.join(COMPOSER_JSON).is_file())
        .map(Path::to_path_buf)
}

fn project_root() -> Result<PathBuf> {
    let cwd = std::env::current_dir()?;
    find_project_root(&cwd).context(
        "no composer.json found in this directory or any parent (run `linguo php init` first)",
    )
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

/// Composer, invoked as `php composer.phar` so no shebang/wrapper concerns
/// exist on any platform.
fn composer(root: &Path) -> Result<Command> {
    let bin = toolchain_bin(root)?;
    let mut cmd = Command::new(bin.join(super::dist::php_exe()));
    cmd.arg(bin.join("composer.phar"))
        .current_dir(root)
        .env("PATH", prepended_path(&[bin])?);
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
    let path = cwd.join(COMPOSER_JSON);
    if path.exists() {
        bail!("{} already exists", path.display());
    }

    let version = store::pick_project_version(super::LANGUAGE, &cwd)?;

    std::fs::write(&path, "{\n    \"require\": {}\n}\n")
        .with_context(|| format!("failed to write {}", path.display()))?;

    let req = VersionReq::MajorMinor(version.major, version.minor);
    crate::config::write_pin(
        &cwd.join(crate::config::PIN_FILE),
        super::LANGUAGE,
        &req.to_string(),
    )?;

    println!("initialized composer.json with php {version}");
    Ok(())
}

pub fn add(packages: &[String]) -> Result<()> {
    if packages.is_empty() {
        bail!("no packages given");
    }
    let root = project_root()?;
    run_checked(
        composer(&root)?.arg("require").args(packages),
        "composer require",
    )
}

pub fn remove(packages: &[String]) -> Result<()> {
    if packages.is_empty() {
        bail!("no packages given");
    }
    let root = project_root()?;
    run_checked(
        composer(&root)?.arg("remove").args(packages),
        "composer remove",
    )
}

pub fn sync() -> Result<()> {
    sync_in(&project_root()?)
}

/// Sync a specific project directory (used by workspace sync).
pub fn sync_in(root: &Path) -> Result<()> {
    run_checked(composer(root)?.arg("install"), "composer install")
}

/// linguo-managed bin dirs: the project's vendor/bin (if present) followed
/// by the toolchain.
fn managed_bin_dirs(cwd: &Path) -> Result<Vec<PathBuf>> {
    let mut dirs: Vec<PathBuf> = Vec::new();
    if let Some(root) = find_project_root(cwd) {
        let vendor_bin = root.join("vendor").join("bin");
        if vendor_bin.is_dir() {
            dirs.push(vendor_bin);
        }
    }
    dirs.push(toolchain_bin(cwd)?);
    Ok(dirs)
}

/// Print the path of the executable a command resolves to (default: php).
pub fn which(command: Option<String>) -> Result<()> {
    let cwd = std::env::current_dir()?;
    let name = command.unwrap_or_else(|| "php".to_string());
    for dir in managed_bin_dirs(&cwd)? {
        if let Some(path) = crate::exec::find_in_dir(&dir, &name) {
            println!("{}", path.display());
            return Ok(());
        }
    }
    bail!("'{name}' not found in vendor/bin or the pinned toolchain");
}

/// Run a command with vendor/bin and the pinned toolchain on PATH.
pub fn run(args: &[String]) -> Result<()> {
    let (program, rest) = args.split_first().context("no command given")?;
    let cwd = std::env::current_dir()?;
    let dirs = managed_bin_dirs(&cwd)?;

    let mut cmd = Command::new(program);
    cmd.args(rest).env("PATH", prepended_path(&dirs)?);
    crate::exec::exec(cmd, program)
}
