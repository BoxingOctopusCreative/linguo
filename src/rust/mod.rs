pub mod dist;
pub mod project;

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

use crate::config::{self, Pin, PinSource};
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

/// Resolve the rust pin: linguo.toml / global config first, then the nearest
/// rust-toolchain(.toml) whose channel is a plain version. Channel names like
/// `stable` or `nightly-*` can't map to a pinned install and are ignored.
pub fn resolve_pin(cwd: &Path) -> Result<Option<Pin>> {
    if let Some(pin) = config::resolve_pin(LANGUAGE, cwd)? {
        return Ok(Some(pin));
    }
    for dir in cwd.ancestors() {
        if let Some((channel, path)) = read_toolchain_file(dir)? {
            if channel.parse::<VersionReq>().is_ok() {
                return Ok(Some(Pin {
                    raw: channel,
                    source: PinSource::Project(path),
                }));
            }
            return Ok(None);
        }
    }
    Ok(None)
}

fn pin_req(pin: &Pin) -> Result<VersionReq> {
    pin.raw
        .parse()
        .with_context(|| format!("invalid rust version '{}' pinned", pin.raw))
}

pub fn resolve_active(cwd: &Path) -> Result<Option<(Pin, Version)>> {
    let Some(pin) = resolve_pin(cwd)? else {
        return Ok(None);
    };
    let req = pin_req(&pin)?;
    Ok(store::find_installed(LANGUAGE, &req)?.map(|version| (pin, version)))
}

/// The pinned toolchain for `dir`, or an actionable error.
pub fn required_toolchain(dir: &Path) -> Result<Version> {
    let Some(pin) = resolve_pin(dir)? else {
        bail!("no rust version pinned (run `linguo rust use <version>` or `linguo rust init`)");
    };
    let req = pin_req(&pin)?;
    store::find_installed(LANGUAGE, &req)?.with_context(|| {
        format!("rust {req} is pinned but not installed (run `linguo rust install {req}`)")
    })
}

/// Version a new project should use: pin if satisfiable, else newest installed.
fn pick_project_version(dir: &Path) -> Result<Version> {
    match resolve_pin(dir)? {
        Some(_) => required_toolchain(dir),
        None => store::installed_versions(LANGUAGE)?
            .last()
            .copied()
            .context("no rust toolchains installed (run `linguo rust install`)"),
    }
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

    if installed.is_empty() {
        println!("no rust toolchains installed (try `linguo rust install`)");
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

/// Status lines for `linguo status`, matching the generic language format.
pub fn print_status(cwd: &Path) -> Result<()> {
    println!("{LANGUAGE}");
    let installed = store::installed_versions(LANGUAGE)?;
    let toolchains = if installed.is_empty() {
        "(none)".to_string()
    } else {
        installed
            .iter()
            .map(|v| v.to_string())
            .collect::<Vec<_>>()
            .join(", ")
    };
    println!("  toolchains: {toolchains}");

    match resolve_pin(cwd)? {
        None => println!("  active: none (no version pinned)"),
        Some(pin) => {
            let source = match &pin.source {
                PinSource::Project(path) => path.display().to_string(),
                PinSource::Global => "global config".to_string(),
            };
            let req = pin_req(&pin)?;
            match store::find_installed(LANGUAGE, &req)? {
                Some(version) => {
                    println!("  active: {version} (pinned to {} by {source})", pin.raw);
                }
                None => println!(
                    "  active: none ({} pinned by {source} but not installed — run `linguo rust install {}`)",
                    pin.raw, pin.raw
                ),
            }
        }
    }
    Ok(())
}
