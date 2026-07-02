//! Language-agnostic toolchain store: installed versions under
//! `$LINGUO_ROOT/toolchains/<language>/<version>` plus pin handling.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

use crate::config::{self, Pin, PinSource};
use crate::versions::{Version, VersionReq};

pub fn toolchain_path(language: &str, version: &Version) -> Result<PathBuf> {
    Ok(config::toolchains_dir(language)?.join(version.to_string()))
}

/// Installed toolchain versions, ascending.
pub fn installed_versions(language: &str) -> Result<Vec<Version>> {
    let dir = config::toolchains_dir(language)?;
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
pub fn find_installed(language: &str, req: &VersionReq) -> Result<Option<Version>> {
    Ok(req.best_match(installed_versions(language)?))
}

/// Parse a pin's value as a plain version request (how every language except
/// terraform interprets pins).
fn pin_req(language: &str, pin: &Pin) -> Result<VersionReq> {
    pin.raw
        .parse()
        .with_context(|| format!("invalid {language} version '{}' pinned", pin.raw))
}

/// Resolve the active toolchain for `cwd`: pin -> installed version.
pub fn resolve_active(language: &str, cwd: &Path) -> Result<Option<(Pin, Version)>> {
    let Some(pin) = config::resolve_pin(language, cwd)? else {
        return Ok(None);
    };
    let req = pin_req(language, &pin)?;
    match find_installed(language, &req)? {
        Some(version) => Ok(Some((pin, version))),
        None => Ok(None),
    }
}

/// The toolchain version pinned for `dir`, or an actionable error.
pub fn required_toolchain(language: &str, dir: &Path) -> Result<Version> {
    let Some(pin) = config::resolve_pin(language, dir)? else {
        bail!(
            "no {language} version pinned (run `linguo {language} use <version>` or `linguo {language} init`)"
        );
    };
    let req = pin_req(language, &pin)?;
    find_installed(language, &req)?.with_context(|| {
        format!(
            "{language} {req} is pinned but not installed (run `linguo {language} install {req}`)"
        )
    })
}

/// Version a new project should use: the existing pin if satisfiable,
/// otherwise the newest installed toolchain.
pub fn pick_project_version(language: &str, dir: &Path) -> Result<Version> {
    match config::resolve_pin(language, dir)? {
        Some(_) => required_toolchain(language, dir),
        None => installed_versions(language)?
            .last()
            .copied()
            .with_context(|| {
                format!("no {language} toolchains installed (run `linguo {language} install`)")
            }),
    }
}

pub fn uninstall(language: &str, raw: &str) -> Result<()> {
    let req: VersionReq = raw.parse()?;
    let version = match req {
        VersionReq::Exact(v) => v,
        _ => {
            let matches: Vec<Version> = installed_versions(language)?
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
    let path = toolchain_path(language, &version)?;
    if !path.exists() {
        bail!("{language} {version} is not installed");
    }
    std::fs::remove_dir_all(&path)
        .with_context(|| format!("failed to remove {}", path.display()))?;
    println!("uninstalled {language} {version}");
    Ok(())
}

pub fn list_installed(language: &str) -> Result<()> {
    let installed = installed_versions(language)?;
    if installed.is_empty() {
        println!("no {language} toolchains installed (try `linguo {language} install`)");
        return Ok(());
    }
    let cwd = std::env::current_dir()?;
    let active = resolve_active(language, &cwd)?;
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

pub fn use_version(language: &str, raw: &str, global: bool) -> Result<()> {
    let req: VersionReq = raw.parse()?;
    let path = if global {
        config::linguo_root()?.join(config::GLOBAL_CONFIG)
    } else {
        std::env::current_dir()?.join(config::PIN_FILE)
    };
    config::write_pin(&path, language, &req.to_string())?;
    println!("pinned {language} to {req} in {}", path.display());
    if find_installed(language, &req)?.is_none() {
        println!("note: no installed toolchain matches; run `linguo {language} install {req}`");
    }
    Ok(())
}
