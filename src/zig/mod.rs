pub mod dist;
pub mod project;

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

use crate::config::Pin;
use crate::store;
use crate::versions::{Version, VersionReq};

pub const LANGUAGE: &str = "zig";

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

/// Community convention: the nearest `.zigversion` holding a plain version,
/// or build.zig.zon's `minimum_zig_version` field.
pub fn fallback_pin(cwd: &Path) -> Result<Option<Pin>> {
    for dir in cwd.ancestors() {
        let path = dir.join(".zigversion");
        if let Some(raw) = store::read_version_file(&path)? {
            return Ok(store::file_pin(&raw, &path));
        }
        let zon = dir.join("build.zig.zon");
        if zon.is_file() {
            let text = std::fs::read_to_string(&zon)
                .with_context(|| format!("failed to read {}", zon.display()))?;
            return Ok(zon_minimum_zig_version(&text).and_then(|v| store::file_pin(&v, &zon)));
        }
    }
    Ok(None)
}

/// Extract `.minimum_zig_version = "X.Y.Z"` from build.zig.zon (zon is zig
/// syntax, so this is a targeted line scan, not a full parser).
fn zon_minimum_zig_version(text: &str) -> Option<String> {
    for line in text.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix(".minimum_zig_version") {
            let value = rest.trim_start_matches(['=', ' ', '\t']);
            let value = value.trim_start_matches('"');
            if let Some(end) = value.find('"') {
                return Some(value[..end].to_string());
            }
        }
    }
    None
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
        eprintln!("zig {} is already installed", build.version);
        return Ok(());
    }
    std::fs::create_dir_all(dest.parent().unwrap())
        .with_context(|| format!("failed to create {}", dest.parent().unwrap().display()))?;

    dist::install_build(build, &dest)?;
    eprintln!("installed zig {} to {}", build.version, dest.display());
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_zon_minimum_zig_version() {
        let zon = r#".{
    .name = .demo,
    .version = "0.0.1",
    .minimum_zig_version = "0.15.2",
    .dependencies = .{},
}
"#;
        assert_eq!(zon_minimum_zig_version(zon), Some("0.15.2".to_string()));
        assert_eq!(zon_minimum_zig_version(".{ .name = .demo }"), None);
    }
}
