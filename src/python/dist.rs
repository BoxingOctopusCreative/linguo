//! Fetching CPython builds from astral-sh/python-build-standalone.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::Deserialize;

use crate::fetch;
use crate::versions::Version;

const RELEASE_URL: &str =
    "https://api.github.com/repos/astral-sh/python-build-standalone/releases/latest";

/// Rust-style target triple used in python-build-standalone asset names.
pub fn target_triple() -> Result<&'static str> {
    let triple = match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => "aarch64-apple-darwin",
        ("macos", "x86_64") => "x86_64-apple-darwin",
        ("linux", "aarch64") if cfg!(target_env = "musl") => "aarch64-unknown-linux-musl",
        ("linux", "x86_64") if cfg!(target_env = "musl") => "x86_64-unknown-linux-musl",
        ("linux", "aarch64") => "aarch64-unknown-linux-gnu",
        ("linux", "x86_64") => "x86_64-unknown-linux-gnu",
        ("windows", "x86_64") => "x86_64-pc-windows-msvc",
        ("windows", "aarch64") => "aarch64-pc-windows-msvc",
        (os, arch) => bail!("unsupported platform: {os}/{arch}"),
    };
    Ok(triple)
}

#[derive(Debug, Deserialize)]
struct Release {
    tag_name: String,
    assets: Vec<Asset>,
}

#[derive(Debug, Deserialize)]
struct Asset {
    name: String,
    browser_download_url: String,
}

pub struct AvailableBuild {
    pub version: Version,
    asset_name: String,
    url: String,
    /// URL of the release-wide SHA256SUMS manifest, when the release has one.
    sums_url: Option<String>,
    pub release_tag: String,
}

/// Parse a version out of an `install_only` asset name for `triple`, e.g.
/// `cpython-3.12.8+20241219-aarch64-apple-darwin-install_only.tar.gz`.
fn parse_asset_version(name: &str, triple: &str) -> Option<Version> {
    let suffix = format!("-{triple}-install_only.tar.gz");
    let rest = name.strip_prefix("cpython-")?.strip_suffix(&suffix)?;
    let (version, _build_date) = rest.split_once('+')?;
    version.parse().ok()
}

/// All CPython versions in the latest python-build-standalone release that
/// have an `install_only` build for the current platform.
pub fn fetch_available() -> Result<Vec<AvailableBuild>> {
    let triple = target_triple()?;
    let release: Release = fetch::client()?
        .get(RELEASE_URL)
        .send()
        .context("failed to query python-build-standalone releases")?
        .error_for_status()
        .context("python-build-standalone release query failed")?
        .json()
        .context("failed to parse release metadata")?;

    let sums_url = release
        .assets
        .iter()
        .find(|a| a.name == "SHA256SUMS")
        .map(|a| a.browser_download_url.clone());
    let mut builds: Vec<AvailableBuild> = release
        .assets
        .iter()
        .filter_map(|asset| {
            let version = parse_asset_version(&asset.name, triple)?;
            Some(AvailableBuild {
                version,
                asset_name: asset.name.clone(),
                url: asset.browser_download_url.clone(),
                sums_url: sums_url.clone(),
                release_tag: release.tag_name.clone(),
            })
        })
        .collect();
    builds.sort_by_key(|b| b.version);
    Ok(builds)
}

/// Download the build, verify its checksum, and extract it so that
/// `dest/bin/python3` exists. `dest` must not already exist.
pub fn install_build(build: &AvailableBuild, dest: &Path) -> Result<()> {
    let http = fetch::client()?;

    let expected_sha = match &build.sums_url {
        Some(url) => {
            let text = http
                .get(url)
                .send()
                .and_then(|r| r.error_for_status())
                .with_context(|| format!("failed to fetch checksums from {url}"))?
                .text()?;
            let digest = fetch::find_sha256(&text, &build.asset_name)
                .with_context(|| format!("no SHA256SUMS entry for {}", build.asset_name))?;
            Some(digest)
        }
        None => None,
    };

    eprintln!("downloading {}", build.url);
    let archive = fetch::download(&http, &build.url)?;

    match expected_sha {
        Some(expected) => fetch::verify_sha256(&archive, &expected, &build.asset_name)?,
        None => eprintln!("warning: no published checksum for this build; skipping verification"),
    }

    // install_only archives contain a single top-level `python/` directory.
    fetch::extract_archive_subdir(&archive, &build.asset_name, "python", dest)
}

/// The directory containing executables inside an installed toolchain.
pub fn bin_dir(toolchain: &Path) -> PathBuf {
    if cfg!(windows) {
        toolchain.to_path_buf()
    } else {
        toolchain.join("bin")
    }
}

/// The interpreter's file name inside a toolchain or venv bin dir.
pub fn python_exe() -> &'static str {
    if cfg!(windows) {
        "python.exe"
    } else {
        "python3"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_install_only_asset_names() {
        let triple = "aarch64-apple-darwin";
        assert_eq!(
            parse_asset_version(
                "cpython-3.12.8+20241219-aarch64-apple-darwin-install_only.tar.gz",
                triple
            ),
            Some("3.12.8".parse().unwrap())
        );
        // wrong triple
        assert_eq!(
            parse_asset_version(
                "cpython-3.12.8+20241219-x86_64-unknown-linux-gnu-install_only.tar.gz",
                triple
            ),
            None
        );
        // debug/pgo variants must not match
        assert_eq!(
            parse_asset_version(
                "cpython-3.12.8+20241219-aarch64-apple-darwin-debug-full.tar.zst",
                triple
            ),
            None
        );
        assert_eq!(
            parse_asset_version(
                "cpython-3.12.8+20241219-aarch64-apple-darwin-install_only_stripped.tar.gz",
                triple
            ),
            None
        );
    }
}
