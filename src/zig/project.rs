//! Project management: build.zig/build.zig.zon, driven through the zig tool.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};

use crate::store;

/// Nearest ancestor directory (including `start`) containing a build.zig.
fn find_project_root(start: &Path) -> Option<PathBuf> {
    start
        .ancestors()
        .find(|dir| dir.join("build.zig").is_file())
        .map(Path::to_path_buf)
}

fn project_root() -> Result<PathBuf> {
    let cwd = std::env::current_dir()?;
    find_project_root(&cwd)
        .context("no build.zig found in this directory or any parent (run `linguo zig init` first)")
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

/// The pinned toolchain's zig, with its dir on PATH.
fn zig(dir: &Path) -> Result<Command> {
    let bin = toolchain_bin(dir)?;
    let mut cmd = Command::new(bin.join(crate::exec::exe("zig")));
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

pub fn init() -> Result<()> {
    let cwd = std::env::current_dir()?;
    if cwd.join("build.zig").exists() {
        bail!("{} already exists", cwd.join("build.zig").display());
    }

    let version = store::pick_project_version(super::LANGUAGE, &cwd)?;

    // Not the zig() helper: the pin this project will use is written below.
    let bin = super::dist::bin_dir(&super::toolchain_path(&version)?);
    let mut cmd = Command::new(bin.join(crate::exec::exe("zig")));
    cmd.current_dir(&cwd).env("PATH", prepended_path(&[bin])?);
    run_checked(cmd.arg("init"), "zig init")?;

    let req = crate::versions::VersionReq::MajorMinor(version.major, version.minor);
    crate::config::write_pin(
        &cwd.join(crate::config::PIN_FILE),
        super::LANGUAGE,
        &req.to_string(),
    )?;

    println!("initialized zig project with zig {version}");
    Ok(())
}

/// `zig fetch --save` a package (zig's package manager takes URLs or paths).
pub fn add(packages: &[String]) -> Result<()> {
    if packages.is_empty() {
        bail!("no packages given (zig fetch takes archive URLs or paths)");
    }
    let root = project_root()?;
    for package in packages {
        run_checked(
            zig(&root)?.args(["fetch", "--save"]).arg(package),
            "zig fetch --save",
        )?;
    }
    Ok(())
}

pub fn sync() -> Result<()> {
    sync_in(&project_root()?)
}

/// Sync a specific project directory (used by workspace sync): fetch the
/// dependency tree without building.
pub fn sync_in(root: &Path) -> Result<()> {
    run_checked(zig(root)?.args(["build", "--fetch"]), "zig build --fetch")
}

/// Print the path of the executable a command resolves to (default: zig).
pub fn which(command: Option<String>) -> Result<()> {
    let cwd = std::env::current_dir()?;
    let name = command.unwrap_or_else(|| "zig".to_string());
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
