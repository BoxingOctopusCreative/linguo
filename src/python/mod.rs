pub mod dist;
pub mod project;

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

use crate::config::Pin;
use crate::store;
use crate::versions::{Version, VersionReq};

pub const LANGUAGE: &str = "python";

pub fn toolchain_path(version: &Version) -> Result<PathBuf> {
    store::toolchain_path(LANGUAGE, version)
}

pub fn resolve_active(cwd: &Path) -> Result<Option<(Pin, Version)>> {
    store::resolve_active(LANGUAGE, cwd)
}

pub fn upgrade(latest: bool, prune: bool) -> Result<()> {
    let available: Vec<Version> = dist::fetch_available()?.iter().map(|b| b.version).collect();
    let newest = available.last().copied();
    store::upgrade(LANGUAGE, &available, newest, latest, prune, &|v| {
        install(Some(v.to_string()))
    })
}

/// pyenv convention: the nearest `.python-version` holding a plain version.
pub fn fallback_pin(cwd: &Path) -> Result<Option<Pin>> {
    for dir in cwd.ancestors() {
        let path = dir.join(".python-version");
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

pub fn list(available: bool) -> Result<()> {
    if !available {
        return store::list_installed(LANGUAGE);
    }
    let builds = dist::fetch_available()?;
    if builds.is_empty() {
        println!("no builds available for this platform");
    }
    let installed = store::installed_versions(LANGUAGE)?;
    for build in builds {
        let marker = if installed.contains(&build.version) {
            " (installed)"
        } else {
            ""
        };
        println!("{}{marker}", build.version);
    }
    Ok(())
}
