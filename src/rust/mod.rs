pub mod dist;
pub mod project;

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::config::Pin;
use crate::store;
use crate::versions::{Version, VersionReq};

pub const LANGUAGE: &str = "rust";

pub fn toolchain_path(version: &Version) -> Result<PathBuf> {
    store::toolchain_path(LANGUAGE, version)
}

/// Read a rustup-convention toolchain file: `rust-toolchain.toml`
/// (`[toolchain] channel = "..."`) or the legacy bare `rust-toolchain`.
/// Returns the channel string when one is declared.
fn read_toolchain_file(dir: &Path) -> Result<Option<(String, PathBuf)>> {
    let toml_path = dir.join("rust-toolchain.toml");
    if toml_path.is_file() {
        let text = std::fs::read_to_string(&toml_path)
            .with_context(|| format!("failed to read {}", toml_path.display()))?;
        let doc: toml_edit::DocumentMut = text
            .parse()
            .with_context(|| format!("failed to parse {}", toml_path.display()))?;
        if let Some(channel) = doc
            .get("toolchain")
            .and_then(|t| t.get("channel"))
            .and_then(|c| c.as_str())
        {
            return Ok(Some((channel.to_string(), toml_path)));
        }
        return Ok(None);
    }
    let legacy_path = dir.join("rust-toolchain");
    if legacy_path.is_file() {
        let text = std::fs::read_to_string(&legacy_path)
            .with_context(|| format!("failed to read {}", legacy_path.display()))?;
        let channel = text.trim().to_string();
        if !channel.is_empty() {
            return Ok(Some((channel, legacy_path)));
        }
    }
    Ok(None)
}

/// rustup convention: the nearest rust-toolchain(.toml) whose channel is a
/// plain version. Channel names like `stable` or `nightly-*` can't map to a
/// pinned install and count as no pin.
pub fn fallback_pin(cwd: &Path) -> Result<Option<Pin>> {
    for dir in cwd.ancestors() {
        if let Some((channel, path)) = read_toolchain_file(dir)? {
            return Ok(store::file_pin(&channel, &path));
        }
    }
    Ok(None)
}

pub fn resolve_active(cwd: &Path) -> Result<Option<(Pin, Version)>> {
    store::resolve_active(LANGUAGE, cwd)
}

/// Version a new project should use: pin if satisfiable, else newest installed.
fn pick_project_version(dir: &Path) -> Result<Version> {
    store::pick_project_version(LANGUAGE, dir)
}

pub fn install(request: Option<String>) -> Result<()> {
    // Channel manifests resolve minor requests server-side; only a bare
    // major needs the release list.
    let channel = match &request {
        None => "stable".to_string(),
        Some(raw) => match raw.parse::<VersionReq>()? {
            VersionReq::Major(_) => {
                let req: VersionReq = raw.parse()?;
                req.best_match(dist::fetch_available()?)
                    .with_context(|| format!("no available release matches '{raw}'"))?
                    .to_string()
            }
            _ => raw.clone(),
        },
    };

    let (version, manifest) = dist::fetch_manifest(&channel)?;
    let dest = toolchain_path(&version)?;
    if dest.exists() {
        println!("rust {version} is already installed");
        return Ok(());
    }
    std::fs::create_dir_all(dest.parent().unwrap())
        .with_context(|| format!("failed to create {}", dest.parent().unwrap().display()))?;

    // The prefix is assembled from several component archives; don't leave a
    // half-merged toolchain behind on failure.
    if let Err(err) = dist::install_channel(&manifest, &dest) {
        let _ = std::fs::remove_dir_all(&dest);
        return Err(err);
    }
    println!("installed rust {version} to {}", dest.display());
    Ok(())
}

pub fn list(available: bool) -> Result<()> {
    let installed = store::installed_versions(LANGUAGE)?;
    if available {
        let versions = dist::fetch_available()?;
        if versions.is_empty() {
            println!("no releases found");
            return Ok(());
        }
        // Show the latest release per minor line.
        let mut previous: Option<Version> = None;
        let mut latest_per_minor: Vec<Version> = Vec::new();
        for version in versions {
            if let Some(prev) = previous
                && (prev.major, prev.minor) != (version.major, version.minor)
            {
                latest_per_minor.push(prev);
            }
            previous = Some(version);
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
        return Ok(());
    }

    store::list_installed(LANGUAGE)
}
