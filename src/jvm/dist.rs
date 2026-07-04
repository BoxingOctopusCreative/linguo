//! Fetching Eclipse Temurin JDKs from the Adoptium API. The API serves
//! per-platform tarballs (zips on Windows) with inline sha256 checksums,
//! including native alpine-linux (musl) builds.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::Deserialize;

use crate::fetch;
use crate::versions::Version;

const API_BASE: &str = "https://api.adoptium.net/v3";

/// (os, architecture) in Adoptium API naming.
fn platform() -> Result<(&'static str, &'static str)> {
    let os = match std::env::consts::OS {
        "macos" => "mac",
        "linux" if cfg!(target_env = "musl") => "alpine-linux",
        "linux" => "linux",
        "windows" => "windows",
        other => bail!("unsupported platform for jvm: {other}"),
    };
    let arch = match std::env::consts::ARCH {
        "x86_64" => "x64",
        "aarch64" => "aarch64",
        other => bail!("unsupported architecture for jvm: {other}"),
    };
    Ok((os, arch))
}

#[derive(Debug, Deserialize)]
struct AvailableReleases {
    available_releases: Vec<u32>,
    available_lts_releases: Vec<u32>,
}

#[derive(Debug, Deserialize)]
struct Asset {
    release_name: String,
    binary: Binary,
    version: VersionData,
}

#[derive(Debug, Deserialize)]
struct Binary {
    package: Package,
}

#[derive(Debug, Deserialize)]
struct Package {
    name: String,
    link: String,
    checksum: String,
}

#[derive(Debug, Deserialize)]
struct VersionData {
    semver: String,
}

pub struct AvailableBuild {
    pub version: Version,
    pub feature: u32,
    pub lts: bool,
    url: String,
    checksum: String,
    asset_name: String,
    /// Top-level directory inside the archive, e.g. `jdk-21.0.11+10`.
    release_name: String,
}

/// Parse Adoptium's semver (`21.0.11+10.0.LTS`) down to plain X.Y.Z.
fn parse_semver(raw: &str) -> Option<Version> {
    raw.split(['+', '-']).next()?.parse().ok()
}

/// The latest Temurin build of every available feature release for this
/// platform, ascending.
pub fn fetch_available() -> Result<Vec<AvailableBuild>> {
    let (os, arch) = platform()?;
    let http = fetch::client()?;

    let releases: AvailableReleases = http
        .get(format!("{API_BASE}/info/available_releases"))
        .send()
        .context("failed to query the Adoptium API")?
        .error_for_status()
        .context("Adoptium available_releases query failed")?
        .json()
        .context("failed to parse Adoptium release info")?;

    let mut builds = Vec::new();
    for feature in &releases.available_releases {
        let url = format!(
            "{API_BASE}/assets/latest/{feature}/hotspot?os={os}&architecture={arch}&image_type=jdk&vendor=eclipse"
        );
        let assets: Vec<Asset> = match http.get(&url).send().and_then(|r| r.error_for_status()) {
            Ok(response) => response.json().unwrap_or_default(),
            // Not every feature release has builds for every platform.
            Err(_) => continue,
        };
        let Some(asset) = assets.into_iter().next() else {
            continue;
        };
        let Some(version) = parse_semver(&asset.version.semver) else {
            continue;
        };
        builds.push(AvailableBuild {
            version,
            feature: *feature,
            lts: releases.available_lts_releases.contains(feature),
            url: asset.binary.package.link,
            checksum: asset.binary.package.checksum.to_ascii_lowercase(),
            asset_name: asset.binary.package.name,
            release_name: asset.release_name,
        });
    }
    builds.sort_by_key(|b| b.version);
    Ok(builds)
}

/// Download the build, verify its checksum, and extract it so that
/// `java_home(dest)/bin/java` exists.
pub fn install_build(build: &AvailableBuild, dest: &Path) -> Result<()> {
    eprintln!("downloading {}", build.url);
    let archive = fetch::download(&fetch::client()?, &build.url)?;
    fetch::verify_sha256(&archive, &build.checksum, &build.asset_name)?;
    fetch::extract_archive_subdir(&archive, &build.asset_name, &build.release_name, dest)
}

/// JAVA_HOME inside an installed toolchain: macOS bundles nest the JDK under
/// Contents/Home.
pub fn java_home(toolchain: &Path) -> PathBuf {
    let nested = toolchain.join("Contents").join("Home");
    if nested.is_dir() {
        nested
    } else {
        toolchain.to_path_buf()
    }
}

/// The directory containing java/javac inside an installed toolchain.
pub fn bin_dir(toolchain: &Path) -> PathBuf {
    java_home(toolchain).join("bin")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_adoptium_semver() {
        assert_eq!(
            parse_semver("21.0.11+10.0.LTS"),
            Some("21.0.11".parse().unwrap())
        );
        assert_eq!(parse_semver("8.0.442+6"), Some("8.0.442".parse().unwrap()));
        assert_eq!(
            parse_semver("25.0.1-beta+9"),
            Some("25.0.1".parse().unwrap())
        );
        assert_eq!(parse_semver("garbage"), None);
    }
}
