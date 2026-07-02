pub mod dist;
pub mod project;

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

use crate::config::Pin;
use crate::store;
use crate::versions::{Version, VersionReq};

pub const LANGUAGE: &str = "ruby";

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

/// rbenv/rvm convention: the nearest `.ruby-version` holding a plain version
/// (optionally `ruby-`-prefixed). Other engines (`jruby-...`) count as no pin.
pub fn fallback_pin(cwd: &Path) -> Result<Option<Pin>> {
    for dir in cwd.ancestors() {
        let path = dir.join(".ruby-version");
        if let Some(raw) = store::read_version_file(&path)? {
            let version = raw.strip_prefix("ruby-").unwrap_or(&raw);
            return Ok(store::file_pin(version, &path));
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
        println!("ruby {} is already installed", build.version);
        return Ok(());
    }
    std::fs::create_dir_all(dest.parent().unwrap())
        .with_context(|| format!("failed to create {}", dest.parent().unwrap().display()))?;

    dist::install_build(build, &dest)?;
    println!(
        "installed ruby {} ({}) to {}",
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
    // Several minor lines with every patch each; show the latest per minor.
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
