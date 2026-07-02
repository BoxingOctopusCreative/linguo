//! Fetching Terraform builds from releases.hashicorp.com.

use std::collections::HashMap;
use std::io::Cursor;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::Deserialize;

use crate::fetch;
use crate::versions::Version;

const INDEX_URL: &str = "https://releases.hashicorp.com/terraform/index.json";

/// (os, arch) in releases.hashicorp.com naming.
fn platform() -> Result<(&'static str, &'static str)> {
    let pair = match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => ("darwin", "arm64"),
        ("macos", "x86_64") => ("darwin", "amd64"),
        ("linux", "aarch64") => ("linux", "arm64"),
        ("linux", "x86_64") => ("linux", "amd64"),
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

/// All stable Terraform versions with a build for the current platform,
/// ascending. Prereleases (e.g. `1.16.0-alpha20260701`) don't parse as plain
/// semver and are skipped.
pub fn fetch_available() -> Result<Vec<AvailableBuild>> {
    let (os, arch) = platform()?;
    let index: Index = fetch::client()?
        .get(INDEX_URL)
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

/// Extract a zip archive into `dest`, preserving unix permissions.
fn extract_zip(archive: &[u8], dest: &Path) -> Result<()> {
    let mut zip = zip::ZipArchive::new(Cursor::new(archive)).context("failed to open archive")?;
    std::fs::create_dir_all(dest)
        .with_context(|| format!("failed to create {}", dest.display()))?;
    for i in 0..zip.len() {
        let mut file = zip.by_index(i).context("failed to read archive entry")?;
        if file.is_dir() {
            continue;
        }
        let path = dest.join(file.mangled_name());
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut out = std::fs::File::create(&path)
            .with_context(|| format!("failed to create {}", path.display()))?;
        std::io::copy(&mut file, &mut out).context("failed to extract archive entry")?;
        #[cfg(unix)]
        if let Some(mode) = file.unix_mode() {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode))?;
        }
    }
    Ok(())
}

/// Download the build, verify its checksum, and extract it so that
/// `dest/terraform` exists.
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
    extract_zip(&archive, dest)
}

/// Terraform archives hold the binary at the top level, so the toolchain
/// directory is its own bin dir.
pub fn bin_dir(toolchain: &Path) -> PathBuf {
    toolchain.to_path_buf()
}
