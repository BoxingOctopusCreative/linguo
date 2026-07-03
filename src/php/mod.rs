pub mod dist;
pub mod project;

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

use crate::config::Pin;
use crate::store;
use crate::versions::{Version, VersionReq};

pub const LANGUAGE: &str = "php";

pub fn toolchain_path(version: &Version) -> Result<PathBuf> {
    store::toolchain_path(LANGUAGE, version)
}

pub fn upgrade(latest: bool, prune: bool) -> Result<()> {
    let available: Vec<Version> = dist::fetch_available()?.iter().map(|b| b.version).collect();
    let newest = available.last().copied();
    store::upgrade(LANGUAGE, &available, newest, latest, prune, &|v| {
        install(Some(v.to_string()))
    })
}

/// phpenv convention: the nearest `.php-version` holding a plain version.
pub fn fallback_pin(cwd: &Path) -> Result<Option<Pin>> {
    for dir in cwd.ancestors() {
        let path = dir.join(".php-version");
        if let Some(raw) = store::read_version_file(&path)? {
            return Ok(store::file_pin(&raw, &path));
        }
    }
    Ok(None)
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
        eprintln!("php {} is already installed", build.version);
        return Ok(());
    }
    std::fs::create_dir_all(dest.parent().unwrap())
        .with_context(|| format!("failed to create {}", dest.parent().unwrap().display()))?;

    if let Err(err) = dist::install_build(build, &dest) {
        let _ = std::fs::remove_dir_all(&dest);
        return Err(err);
    }
    eprintln!("installed php {} to {}", build.version, dest.display());
    Ok(())
}

pub fn list(available: bool) -> Result<()> {
    if !available {
        return store::list_installed(LANGUAGE);
    }
    let builds = dist::fetch_available()?;
    if builds.is_empty() {
        println!("no builds available for this platform");
        return Ok(());
    }
    let installed = store::installed_versions(LANGUAGE)?;
    // Show the latest release per minor line.
    let mut previous: Option<Version> = None;
    let mut latest_per_minor: Vec<Version> = Vec::new();
    for build in &builds {
        if let Some(prev) = previous
            && (prev.major, prev.minor) != (build.version.major, build.version.minor)
        {
            latest_per_minor.push(prev);
        }
        previous = Some(build.version);
    }
    latest_per_minor.extend(previous);
    for version in latest_per_minor {
        let marker = if installed.contains(&version) {
            " (installed)"
        } else {
            ""
        };
        println!("{version}{marker}");
    }
    println!("(latest release per minor line; any exact version can be installed)");
    Ok(())
}
