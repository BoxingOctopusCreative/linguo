pub mod dist;
pub mod project;

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

use crate::config::Pin;
use crate::store;
use crate::versions::{Version, VersionReq};

pub const LANGUAGE: &str = "go";

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

/// Parse a version request out of go.mod: the `toolchain go1.x.y` directive
/// wins over the `go 1.x[.y]` minimum-version directive.
fn go_mod_version(text: &str) -> Option<String> {
    let directive = |prefix: &str| {
        text.lines()
            .map(str::trim)
            .find_map(|line| line.strip_prefix(prefix))
            .map(|v| v.trim().to_string())
    };
    directive("toolchain go").or_else(|| directive("go "))
}

/// go.mod's toolchain/go directives, from the nearest go.mod up the tree.
pub fn fallback_pin(cwd: &Path) -> Result<Option<Pin>> {
    for dir in cwd.ancestors() {
        let path = dir.join("go.mod");
        if !path.is_file() {
            continue;
        }
        let text = std::fs::read_to_string(&path)
            .with_context(|| format!("failed to read {}", path.display()))?;
        return Ok(go_mod_version(&text).and_then(|raw| store::file_pin(&raw, &path)));
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
        println!("go {} is already installed", build.version);
        return Ok(());
    }
    std::fs::create_dir_all(dest.parent().unwrap())
        .with_context(|| format!("failed to create {}", dest.parent().unwrap().display()))?;

    dist::install_build(build, &dest)?;
    println!("installed go {} to {}", build.version, dest.display());
    Ok(())
}

pub fn list(available: bool) -> Result<()> {
    if !available {
        return store::list_installed(LANGUAGE);
    }
    // The full index goes back to go1; show the latest release per minor line.
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
