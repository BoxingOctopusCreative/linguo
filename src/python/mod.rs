pub mod dist;
pub mod project;

use std::path::PathBuf;

use anyhow::{Context, Result, bail};

use crate::config::{self, Pin, PinSource};
use crate::versions::{Version, VersionReq};

pub const LANGUAGE: &str = "python";

fn toolchains_dir() -> Result<PathBuf> {
    config::toolchains_dir(LANGUAGE)
}

pub fn toolchain_path(version: &Version) -> Result<PathBuf> {
    Ok(toolchains_dir()?.join(version.to_string()))
}

/// Installed toolchain versions, ascending.
pub fn installed_versions() -> Result<Vec<Version>> {
    let dir = toolchains_dir()?;
    let entries = match std::fs::read_dir(&dir) {
        Ok(entries) => entries,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(err) => {
            return Err(err).with_context(|| format!("failed to read {}", dir.display()));
        }
    };
    let mut versions: Vec<Version> = entries
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.path().is_dir())
        .filter_map(|entry| entry.file_name().to_str().and_then(|s| s.parse().ok()))
        .collect();
    versions.sort();
    Ok(versions)
}

/// Highest installed version satisfying `req`.
pub fn find_installed(req: &VersionReq) -> Result<Option<Version>> {
    Ok(req.best_match(installed_versions()?))
}

/// Resolve the active toolchain for the current directory: pin -> installed version.
pub fn resolve_active(cwd: &std::path::Path) -> Result<Option<(Pin, Version)>> {
    let Some(pin) = config::resolve_pin(LANGUAGE, cwd)? else {
        return Ok(None);
    };
    match find_installed(&pin.req)? {
        Some(version) => Ok(Some((pin, version))),
        None => Ok(None),
    }
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
        println!("python {} is already installed", build.version);
        return Ok(());
    }
    std::fs::create_dir_all(dest.parent().unwrap())
        .with_context(|| format!("failed to create {}", dest.parent().unwrap().display()))?;

    dist::install_build(build, &dest)?;
    println!(
        "installed python {} ({}) to {}",
        build.version,
        build.release_tag,
        dest.display()
    );
    Ok(())
}

pub fn uninstall(raw: &str) -> Result<()> {
    let req: VersionReq = raw.parse()?;
    let version = match req {
        VersionReq::Exact(v) => v,
        _ => {
            let matches: Vec<Version> = installed_versions()?
                .into_iter()
                .filter(|v| req.matches(v))
                .collect();
            match matches.as_slice() {
                [] => bail!("no installed version matches '{raw}'"),
                [only] => *only,
                many => bail!(
                    "'{raw}' matches multiple installed versions ({}); specify one exactly",
                    many.iter()
                        .map(|v| v.to_string())
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
            }
        }
    };
    let path = toolchain_path(&version)?;
    if !path.exists() {
        bail!("python {version} is not installed");
    }
    std::fs::remove_dir_all(&path)
        .with_context(|| format!("failed to remove {}", path.display()))?;
    println!("uninstalled python {version}");
    Ok(())
}

pub fn list(available: bool) -> Result<()> {
    if available {
        let builds = dist::fetch_available()?;
        if builds.is_empty() {
            println!("no builds available for this platform");
        }
        let installed = installed_versions()?;
        for build in builds {
            let marker = if installed.contains(&build.version) {
                " (installed)"
            } else {
                ""
            };
            println!("{}{marker}", build.version);
        }
        return Ok(());
    }

    let installed = installed_versions()?;
    if installed.is_empty() {
        println!("no python toolchains installed (try `linguo python install`)");
        return Ok(());
    }
    let cwd = std::env::current_dir()?;
    let active = resolve_active(&cwd)?;
    for version in installed {
        match &active {
            Some((pin, active_version)) if *active_version == version => {
                let source = match &pin.source {
                    PinSource::Project(path) => format!("pinned by {}", path.display()),
                    PinSource::Global => "global default".to_string(),
                };
                println!("{version} * ({source})");
            }
            _ => println!("{version}"),
        }
    }
    Ok(())
}

pub fn use_version(raw: &str, global: bool) -> Result<()> {
    let req: VersionReq = raw.parse()?;
    let path = if global {
        crate::config::linguo_root()?.join(config::GLOBAL_CONFIG)
    } else {
        std::env::current_dir()?.join(config::PIN_FILE)
    };
    config::write_pin(&path, LANGUAGE, &req)?;
    println!("pinned python to {req} in {}", path.display());
    if find_installed(&req)?.is_none() {
        println!("note: no installed toolchain matches; run `linguo python install {req}`");
    }
    Ok(())
}
