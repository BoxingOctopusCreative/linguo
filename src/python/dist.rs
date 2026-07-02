//! Fetching CPython builds from astral-sh/python-build-standalone.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result, bail};
use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::versions::Version;

const RELEASE_URL: &str =
    "https://api.github.com/repos/astral-sh/python-build-standalone/releases/latest";

/// Rust-style target triple used in python-build-standalone asset names.
pub fn target_triple() -> Result<&'static str> {
    let triple = match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => "aarch64-apple-darwin",
        ("macos", "x86_64") => "x86_64-apple-darwin",
        ("linux", "aarch64") => "aarch64-unknown-linux-gnu",
        ("linux", "x86_64") => "x86_64-unknown-linux-gnu",
        ("windows", "x86_64") => "x86_64-pc-windows-msvc",
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

fn client() -> Result<reqwest::blocking::Client> {
    reqwest::blocking::Client::builder()
        .user_agent(concat!("linguo/", env!("CARGO_PKG_VERSION")))
        .timeout(Duration::from_secs(600))
        .connect_timeout(Duration::from_secs(15))
        .build()
        .context("failed to build HTTP client")
}

/// Look up a file's hash in a `SHA256SUMS` manifest (`<hex>  <name>` lines).
fn find_sha256(sums: &str, asset_name: &str) -> Option<String> {
    sums.lines().find_map(|line| {
        let (hash, name) = line.split_once(char::is_whitespace)?;
        (name.trim() == asset_name).then(|| hash.to_ascii_lowercase())
    })
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
    let release: Release = client()?
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
    let http = client()?;

    let expected_sha = match &build.sums_url {
        Some(url) => {
            let text = http
                .get(url)
                .send()
                .and_then(|r| r.error_for_status())
                .with_context(|| format!("failed to fetch checksums from {url}"))?
                .text()?;
            let digest = find_sha256(&text, &build.asset_name).with_context(|| {
                format!("no SHA256SUMS entry for {}", build.asset_name)
            })?;
            Some(digest)
        }
        None => None,
    };

    eprintln!("downloading {}", build.url);
    let mut response = http
        .get(&build.url)
        .send()
        .and_then(|r| r.error_for_status())
        .with_context(|| format!("failed to download {}", build.url))?;

    let staging = tempfile::tempdir_in(
        dest.parent()
            .context("install destination has no parent directory")?,
    )
    .context("failed to create staging directory")?;

    let mut archive = Vec::new();
    response
        .read_to_end(&mut archive)
        .context("download interrupted")?;

    if let Some(expected) = expected_sha {
        let actual = hex::encode(Sha256::digest(&archive));
        if actual != expected {
            bail!(
                "checksum mismatch for {}: expected {expected}, got {actual}",
                build.url
            );
        }
    } else {
        eprintln!("warning: no published checksum for this build; skipping verification");
    }

    let gz = flate2::read::GzDecoder::new(archive.as_slice());
    tar::Archive::new(gz)
        .unpack(staging.path())
        .context("failed to extract archive")?;

    // install_only archives contain a single top-level `python/` directory.
    let extracted = staging.path().join("python");
    if !extracted.is_dir() {
        bail!("unexpected archive layout: no top-level python/ directory");
    }
    std::fs::rename(&extracted, dest).with_context(|| {
        format!("failed to move extracted toolchain to {}", dest.display())
    })?;
    Ok(())
}

/// The directory containing executables inside an installed toolchain.
pub fn bin_dir(toolchain: &Path) -> PathBuf {
    if cfg!(windows) {
        toolchain.to_path_buf()
    } else {
        toolchain.join("bin")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_sha256_entries() {
        let sums = "abc123  cpython-3.12.8+20241219-aarch64-apple-darwin-install_only.tar.gz\n\
                    DEF456  other.tar.gz\n";
        assert_eq!(
            find_sha256(
                sums,
                "cpython-3.12.8+20241219-aarch64-apple-darwin-install_only.tar.gz"
            ),
            Some("abc123".to_string())
        );
        assert_eq!(find_sha256(sums, "other.tar.gz"), Some("def456".to_string()));
        assert_eq!(find_sha256(sums, "missing.tar.gz"), None);
    }

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
