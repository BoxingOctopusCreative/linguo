//! Fetching relocatable CRuby builds from spinel-coop/rv-ruby.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::Deserialize;

use crate::fetch;
use crate::versions::Version;

const RELEASE_URL: &str = "https://api.github.com/repos/spinel-coop/rv-ruby/releases/latest";
const RUBYINSTALLER_URL: &str =
    "https://api.github.com/repos/oneclick/rubyinstaller2/releases?per_page=100";

/// Platform tag used in rv-ruby asset names (homebrew convention: the
/// unprefixed macOS tag is x86_64).
fn platform() -> Result<&'static str> {
    let tag = match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => "arm64_sonoma",
        ("macos", "x86_64") => "ventura",
        ("linux", "aarch64") if cfg!(target_env = "musl") => "arm64_linux_musl",
        ("linux", "x86_64") if cfg!(target_env = "musl") => "x86_64_linux_musl",
        ("linux", "aarch64") => "arm64_linux",
        ("linux", "x86_64") => "x86_64_linux",
        (os, arch) => bail!("unsupported platform for ruby: {os}/{arch}"),
    };
    Ok(tag)
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
    /// GitHub-computed content digest, e.g. `sha256:<hex>`.
    digest: Option<String>,
}

/// How a build's archive is verified after download.
enum Verify {
    /// Look the asset up in a SHA256SUMS manifest published by the project.
    SumsManifest(Option<String>),
    /// A sha256 known up front (e.g. GitHub's asset digest).
    Sha256(Option<String>),
}

pub struct AvailableBuild {
    pub version: Version,
    asset_name: String,
    url: String,
    verify: Verify,
    /// Top-level directory inside the archive that holds the ruby prefix.
    archive_subdir: String,
    pub release_tag: String,
}

/// Parse a version out of an asset name for `platform`, e.g.
/// `ruby-3.4.9.arm64_sonoma.tar.gz`. Previews (`4.0.0-preview1`) and other
/// non-semver names don't parse and are skipped.
fn parse_asset_version(name: &str, platform: &str) -> Option<Version> {
    let suffix = format!(".{platform}.tar.gz");
    let version = name.strip_prefix("ruby-")?.strip_suffix(&suffix)?;
    version.parse().ok()
}

/// All CRuby versions available for the current platform, ascending:
/// rv-ruby's relocatable builds on unix, RubyInstaller archives on Windows.
pub fn fetch_available() -> Result<Vec<AvailableBuild>> {
    if std::env::consts::OS == "windows" {
        return fetch_rubyinstaller();
    }
    let platform = platform()?;
    let release: Release = fetch::client()?
        .get(RELEASE_URL)
        .send()
        .context("failed to query rv-ruby releases")?
        .error_for_status()
        .context("rv-ruby release query failed")?
        .json()
        .context("failed to parse rv-ruby release metadata")?;

    let sums_url = release
        .assets
        .iter()
        .find(|a| a.name == "SHA256SUMS")
        .map(|a| a.browser_download_url.clone());
    let mut builds: Vec<AvailableBuild> = release
        .assets
        .iter()
        .filter_map(|asset| {
            let version = parse_asset_version(&asset.name, platform)?;
            Some(AvailableBuild {
                version,
                asset_name: asset.name.clone(),
                url: asset.browser_download_url.clone(),
                verify: Verify::SumsManifest(sums_url.clone()),
                // rv-ruby archives are homebrew kegs.
                archive_subdir: format!("rv-ruby@{version}/{version}"),
                release_tag: release.tag_name.clone(),
            })
        })
        .collect();
    builds.sort_by_key(|b| b.version);
    Ok(builds)
}

/// RubyInstaller archives from oneclick/rubyinstaller2: tags like
/// `RubyInstaller-3.4.10-1` (version + package revision), one 7z per arch,
/// verified against GitHub's asset digest. The newest package revision per
/// ruby version wins.
fn fetch_rubyinstaller() -> Result<Vec<AvailableBuild>> {
    let arch = match std::env::consts::ARCH {
        "x86_64" => "x64",
        "aarch64" => "arm",
        other => bail!("unsupported Windows architecture for ruby: {other}"),
    };
    let releases: Vec<Release> = fetch::client()?
        .get(RUBYINSTALLER_URL)
        .send()
        .context("failed to query RubyInstaller releases")?
        .error_for_status()
        .context("RubyInstaller release query failed")?
        .json()
        .context("failed to parse RubyInstaller release metadata")?;

    let mut best: std::collections::HashMap<Version, (u32, AvailableBuild)> =
        std::collections::HashMap::new();
    for release in &releases {
        let Some(rest) = release.tag_name.strip_prefix("RubyInstaller-") else {
            continue;
        };
        let Some((version_str, rev_str)) = rest.rsplit_once('-') else {
            continue;
        };
        let (Ok(version), Ok(rev)) = (version_str.parse::<Version>(), rev_str.parse::<u32>())
        else {
            continue;
        };
        let wanted = format!("rubyinstaller-{version_str}-{rev_str}-{arch}.7z");
        let Some(asset) = release.assets.iter().find(|a| a.name == wanted) else {
            continue;
        };
        let sha = asset
            .digest
            .as_deref()
            .and_then(|d| d.strip_prefix("sha256:"))
            .map(str::to_string);
        let build = AvailableBuild {
            version,
            asset_name: asset.name.clone(),
            url: asset.browser_download_url.clone(),
            verify: Verify::Sha256(sha),
            archive_subdir: format!("rubyinstaller-{version_str}-{rev_str}-{arch}"),
            release_tag: release.tag_name.clone(),
        };
        match best.get(&version) {
            Some((existing_rev, _)) if *existing_rev >= rev => {}
            _ => {
                best.insert(version, (rev, build));
            }
        }
    }
    let mut builds: Vec<AvailableBuild> = best.into_values().map(|(_, b)| b).collect();
    builds.sort_by_key(|b| b.version);
    Ok(builds)
}

/// Download the build, verify its checksum, and extract it so that
/// `dest/bin/ruby` exists. `dest` must not already exist.
pub fn install_build(build: &AvailableBuild, dest: &Path) -> Result<()> {
    let http = fetch::client()?;

    let expected_sha = match &build.verify {
        Verify::Sha256(sha) => sha.clone(),
        Verify::SumsManifest(Some(url)) => {
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
        Verify::SumsManifest(None) => None,
    };

    eprintln!("downloading {}", build.url);
    let archive = fetch::download(&http, &build.url)?;

    match expected_sha {
        Some(expected) => fetch::verify_sha256(&archive, &expected, &build.asset_name)?,
        None => eprintln!("warning: no published checksum for this build; skipping verification"),
    }

    fetch::extract_archive_subdir(&archive, &build.asset_name, &build.archive_subdir, dest)
}

/// The directory containing executables inside an installed toolchain.
/// Gem executables land here too (shared per-toolchain gem layout).
pub fn bin_dir(toolchain: &Path) -> PathBuf {
    toolchain.join("bin")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_asset_names() {
        let platform = "arm64_sonoma";
        assert_eq!(
            parse_asset_version("ruby-3.4.9.arm64_sonoma.tar.gz", platform),
            Some("3.4.9".parse().unwrap())
        );
        // wrong platform
        assert_eq!(
            parse_asset_version("ruby-3.4.9.x86_64_linux.tar.gz", platform),
            None
        );
        // previews and non-semver names must not match
        assert_eq!(
            parse_asset_version("ruby-4.0.0-preview1.arm64_sonoma.tar.gz", platform),
            None
        );
        assert_eq!(
            parse_asset_version("ruby-0.49.arm64_sonoma.tar.gz", platform),
            None
        );
    }
}
