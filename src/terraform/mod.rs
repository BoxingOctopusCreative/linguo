pub mod dist;

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};

use crate::config::Pin;
use crate::store;
use crate::versions::{Version, VersionReq};

pub const LANGUAGE: &str = "terraform";

pub fn toolchain_path(version: &Version) -> Result<PathBuf> {
    store::toolchain_path(LANGUAGE, version)
}

pub fn resolve_active(cwd: &Path) -> Result<Option<(Pin, Version)>> {
    store::resolve_active(LANGUAGE, cwd)
}

pub fn install(request: Option<String>) -> Result<()> {
    let builds = dist::fetch_available()?;
    if builds.is_empty() {
        bail!("no builds available for this platform");
    }

    let build = match &request {
        Some(raw) => {
            let req: VersionReq = raw.parse()?;
            builds
                .iter()
                .rev()
                .find(|b| req.matches(&b.version))
                .with_context(|| format!("no available build matches '{raw}'"))?
        }
        None => builds.last().unwrap(),
    };

    let dest = toolchain_path(&build.version)?;
    if dest.exists() {
        println!("terraform {} is already installed", build.version);
        return Ok(());
    }
    std::fs::create_dir_all(dest.parent().unwrap())
        .with_context(|| format!("failed to create {}", dest.parent().unwrap().display()))?;

    dist::install_build(build, &dest)?;
    println!(
        "installed terraform {} to {}",
        build.version,
        dest.display()
    );
    Ok(())
}

pub fn list(available: bool) -> Result<()> {
    if !available {
        return store::list_installed(LANGUAGE);
    }
    // The full index goes back to 0.1; show the latest release per minor line.
    let builds = dist::fetch_available()?;
    if builds.is_empty() {
        println!("no builds available for this platform");
        return Ok(());
    }
    let installed = store::installed_versions(LANGUAGE)?;
    let mut previous: Option<&dist::AvailableBuild> = None;
    let mut latest_per_minor: Vec<&dist::AvailableBuild> = Vec::new();
    for build in &builds {
        if let Some(prev) = previous
            && (prev.version.major, prev.version.minor)
                != (build.version.major, build.version.minor)
        {
            latest_per_minor.push(prev);
        }
        previous = Some(build);
    }
    latest_per_minor.extend(previous);
    for build in latest_per_minor {
        let marker = if installed.contains(&build.version) {
            " (installed)"
        } else {
            ""
        };
        println!("{}{marker}", build.version);
    }
    println!("(latest release per minor line; any exact version can be installed)");
    Ok(())
}

fn toolchain_bin(dir: &Path) -> Result<PathBuf> {
    let version = store::required_toolchain(LANGUAGE, dir)?;
    Ok(dist::bin_dir(&toolchain_path(&version)?))
}

/// Print the path of the executable a command resolves to (default: terraform).
pub fn which(command: Option<String>) -> Result<()> {
    let cwd = std::env::current_dir()?;
    let name = command.unwrap_or_else(|| "terraform".to_string());
    let path = toolchain_bin(&cwd)?.join(&name);
    if crate::exec::is_executable(&path) {
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

    let current = std::env::var_os("PATH").unwrap_or_default();
    let path = std::env::join_paths(std::iter::once(bin).chain(std::env::split_paths(&current)))
        .context("invalid PATH entry")?;

    let mut cmd = Command::new(program);
    cmd.args(rest).env("PATH", path);
    crate::exec::exec(cmd, program)
}
