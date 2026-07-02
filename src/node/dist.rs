//! Fetching Node.js builds from nodejs.org/dist.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::Deserialize;

use crate::fetch;
use crate::versions::Version;

const INDEX_URL: &str = "https://nodejs.org/dist/index.json";

/// (`files` key in index.json, filename suffix in dist archives).
fn platform() -> Result<(&'static str, &'static str)> {
    let pair = match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => ("osx-arm64-tar", "darwin-arm64"),
        ("macos", "x86_64") => ("osx-x64-tar", "darwin-x64"),
        ("linux", "aarch64") => ("linux-arm64", "linux-arm64"),
        ("linux", "x86_64") => ("linux-x64", "linux-x64"),
        (os, arch) => bail!("unsupported platform for node: {os}/{arch}"),
    };
    Ok(pair)
}

#[derive(Debug, Deserialize)]
struct IndexEntry {
    version: String,
    files: Vec<String>,
    lts: Lts,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum Lts {
    Codename(String),
    NotLts(#[allow(dead_code)] bool),
}

pub struct AvailableBuild {
    pub version: Version,
    pub lts: Option<String>,
}

/// All Node.js versions with a build for the current platform, ascending.
pub fn fetch_available() -> Result<Vec<AvailableBuild>> {
    let (file_key, _) = platform()?;
    let index: Vec<IndexEntry> = fetch::client()?
        .get(INDEX_URL)
        .send()
        .context("failed to query nodejs.org/dist")?
        .error_for_status()
        .context("nodejs.org/dist index query failed")?
        .json()
        .context("failed to parse nodejs.org version index")?;

    let mut builds: Vec<AvailableBuild> = index
        .into_iter()
        .filter(|entry| entry.files.iter().any(|f| f == file_key))
        .filter_map(|entry| {
            let version: Version = entry.version.strip_prefix('v')?.parse().ok()?;
            let lts = match entry.lts {
                Lts::Codename(name) => Some(name),
                Lts::NotLts(_) => None,
            };
            Some(AvailableBuild { version, lts })
        })
        .collect();
    builds.sort_by_key(|b| b.version);
    Ok(builds)
}

/// Download the build, verify it against the release's SHASUMS256.txt, and
/// extract it so that `dest/bin/node` exists. `dest` must not already exist.
pub fn install_build(version: &Version, dest: &Path) -> Result<()> {
    let (_, suffix) = platform()?;
    let dirname = format!("node-v{version}-{suffix}");
    let archive_name = format!("{dirname}.tar.gz");
    let base = format!("https://nodejs.org/dist/v{version}");
    let http = fetch::client()?;

    let sums_url = format!("{base}/SHASUMS256.txt");
    let sums = http
        .get(&sums_url)
        .send()
        .and_then(|r| r.error_for_status())
        .with_context(|| format!("failed to fetch checksums from {sums_url}"))?
        .text()?;
    let expected = fetch::find_sha256(&sums, &archive_name)
        .with_context(|| format!("no SHASUMS256.txt entry for {archive_name}"))?;

    let url = format!("{base}/{archive_name}");
    eprintln!("downloading {url}");
    let archive = fetch::download(&http, &url)?;
    fetch::verify_sha256(&archive, &expected, &archive_name)?;
    fetch::extract_tar_gz_subdir(&archive, &dirname, dest)
}

/// The directory containing executables inside an installed toolchain.
pub fn bin_dir(toolchain: &Path) -> PathBuf {
    if cfg!(windows) {
        toolchain.to_path_buf()
    } else {
        toolchain.join("bin")
    }
}
