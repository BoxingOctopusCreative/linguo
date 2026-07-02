//! Fetching Terraform builds from releases.hashicorp.com and OpenTofu builds
//! from get.opentofu.org / GitHub releases.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::Deserialize;

use super::Distribution;
use crate::fetch;
use crate::versions::Version;

const TERRAFORM_INDEX_URL: &str = "https://releases.hashicorp.com/terraform/index.json";
const OPENTOFU_INDEX_URL: &str = "https://get.opentofu.org/tofu/api.json";

/// (os, arch) in releases.hashicorp.com naming.
fn platform() -> Result<(&'static str, &'static str)> {
    let pair = match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => ("darwin", "arm64"),
        ("macos", "x86_64") => ("darwin", "amd64"),
        ("linux", "aarch64") => ("linux", "arm64"),
        ("linux", "x86_64") => ("linux", "amd64"),
        ("windows", "x86_64") => ("windows", "amd64"),
        ("windows", "aarch64") => ("windows", "arm64"),
        (os, arch) => bail!("unsupported platform for terraform: {os}/{arch}"),
    };
    Ok(pair)
}

#[derive(Debug, Deserialize)]
struct Index {
    versions: HashMap<String, Release>,
}

#[derive(Debug, Deserialize)]
struct Release {
    version: String,
    shasums: String,
    builds: Vec<Build>,
}

#[derive(Debug, Deserialize)]
struct Build {
    os: String,
    arch: String,
    filename: String,
    url: String,
}

pub struct AvailableBuild {
    pub version: Version,
    filename: String,
    url: String,
    shasums_url: String,
}

/// All stable versions of `dist` with a build for the current platform,
/// ascending. Prereleases (e.g. `1.16.0-alpha20260701`) don't parse as plain
/// semver and are skipped.
pub fn fetch_available(dist: Distribution) -> Result<Vec<AvailableBuild>> {
    match dist {
        Distribution::Terraform => fetch_terraform(),
        Distribution::OpenTofu => fetch_opentofu(),
    }
}

fn fetch_terraform() -> Result<Vec<AvailableBuild>> {
    let (os, arch) = platform()?;
    let index: Index = fetch::client()?
        .get(TERRAFORM_INDEX_URL)
        .send()
        .context("failed to query releases.hashicorp.com")?
        .error_for_status()
        .context("terraform release index query failed")?
        .json()
        .context("failed to parse terraform release index")?;

    let mut builds: Vec<AvailableBuild> = index
        .versions
        .into_values()
        .filter_map(|release| {
            let version: Version = release.version.parse().ok()?;
            let build = release
                .builds
                .into_iter()
                .find(|b| b.os == os && b.arch == arch)?;
            Some(AvailableBuild {
                version,
                filename: build.filename,
                url: build.url,
                shasums_url: format!(
                    "https://releases.hashicorp.com/terraform/{}/{}",
                    release.version, release.shasums
                ),
            })
        })
        .collect();
    builds.sort_by_key(|b| b.version);
    Ok(builds)
}

#[derive(Debug, Deserialize)]
struct TofuIndex {
    versions: Vec<TofuVersion>,
}

#[derive(Debug, Deserialize)]
struct TofuVersion {
    id: String,
    files: Vec<String>,
}

fn fetch_opentofu() -> Result<Vec<AvailableBuild>> {
    let (os, arch) = platform()?;
    let index: TofuIndex = fetch::client()?
        .get(OPENTOFU_INDEX_URL)
        .send()
        .context("failed to query get.opentofu.org")?
        .error_for_status()
        .context("opentofu release index query failed")?
        .json()
        .context("failed to parse opentofu release index")?;

    let mut builds: Vec<AvailableBuild> = index
        .versions
        .into_iter()
        .filter_map(|release| {
            let version: Version = release.id.parse().ok()?;
            let filename = format!("tofu_{}_{os}_{arch}.zip", release.id);
            let shasums = format!("tofu_{}_SHA256SUMS", release.id);
            if !release.files.contains(&filename) || !release.files.contains(&shasums) {
                return None;
            }
            let base = format!(
                "https://github.com/opentofu/opentofu/releases/download/v{}",
                release.id
            );
            Some(AvailableBuild {
                version,
                url: format!("{base}/{filename}"),
                filename,
                shasums_url: format!("{base}/{shasums}"),
            })
        })
        .collect();
    builds.sort_by_key(|b| b.version);
    Ok(builds)
}

/// Download the build, verify its checksum, and extract it so that the
/// distribution's binary (`terraform` or `tofu`) sits at the top of `dest`.
pub fn install_build(build: &AvailableBuild, dest: &Path) -> Result<()> {
    let http = fetch::client()?;

    let sums = http
        .get(&build.shasums_url)
        .send()
        .and_then(|r| r.error_for_status())
        .with_context(|| format!("failed to fetch checksums from {}", build.shasums_url))?
        .text()?;
    let expected = fetch::find_sha256(&sums, &build.filename)
        .with_context(|| format!("no SHA256SUMS entry for {}", build.filename))?;

    eprintln!("downloading {}", build.url);
    let archive = fetch::download(&http, &build.url)?;
    fetch::verify_sha256(&archive, &expected, &build.filename)?;
    fetch::extract_archive_root(&archive, &build.filename, dest)
}

/// Terraform archives hold the binary at the top level, so the toolchain
/// directory is its own bin dir.
pub fn bin_dir(toolchain: &Path) -> PathBuf {
    toolchain.to_path_buf()
}
