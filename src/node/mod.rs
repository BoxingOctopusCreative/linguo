pub mod dist;
pub mod project;

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

use crate::config::Pin;
use crate::store;
use crate::versions::{Version, VersionReq};

pub const LANGUAGE: &str = "node";

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
        None => builds
            .iter()
            .rev()
            .find(|b| b.lts.is_some())
            .unwrap_or_else(|| builds.last().unwrap()),
    };

    let dest = toolchain_path(&build.version)?;
    if dest.exists() {
        println!("node {} is already installed", build.version);
        return Ok(());
    }
    std::fs::create_dir_all(dest.parent().unwrap())
        .with_context(|| format!("failed to create {}", dest.parent().unwrap().display()))?;

    dist::install_build(&build.version, &dest)?;
    println!("installed node {} to {}", build.version, dest.display());
    Ok(())
}

pub fn list(available: bool) -> Result<()> {
    if !available {
        return store::list_installed(LANGUAGE);
    }
    // The full index is hundreds of versions; show the latest of each major.
    let builds = dist::fetch_available()?;
    if builds.is_empty() {
        println!("no builds available for this platform");
        return Ok(());
    }
    let installed = store::installed_versions(LANGUAGE)?;
    let mut previous: Option<&dist::AvailableBuild> = None;
    let mut latest_per_major: Vec<&dist::AvailableBuild> = Vec::new();
    for build in &builds {
        if let Some(prev) = previous
            && prev.version.major != build.version.major
        {
            latest_per_major.push(prev);
        }
        previous = Some(build);
    }
    latest_per_major.extend(previous);
    for build in latest_per_major {
        let lts = build
            .lts
            .as_ref()
            .map(|name| format!(" (lts: {name})"))
            .unwrap_or_default();
        let marker = if installed.contains(&build.version) {
            " (installed)"
        } else {
            ""
        };
        println!("{}{lts}{marker}", build.version);
    }
    println!("(latest release per major line; any exact version can be installed)");
    Ok(())
}
