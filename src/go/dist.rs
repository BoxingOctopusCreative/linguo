//! Fetching Go toolchains from go.dev/dl.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::Deserialize;

use crate::fetch;
use crate::versions::Version;

const INDEX_URL: &str = "https://go.dev/dl/?mode=json&include=all";

/// (os, arch) in go.dev/dl naming.
fn platform() -> Result<(&'static str, &'static str)> {
    let pair = match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => ("darwin", "arm64"),
        ("macos", "x86_64") => ("darwin", "amd64"),
        ("linux", "aarch64") => ("linux", "arm64"),
        ("linux", "x86_64") => ("linux", "amd64"),
        (os, arch) => bail!("unsupported platform for go: {os}/{arch}"),
    };
    Ok(pair)
}

#[derive(Debug, Deserialize)]
struct Release {
    version: String,
    stable: bool,
    files: Vec<File>,
}

#[derive(Debug, Deserialize)]
struct File {
    filename: String,
    os: String,
    arch: String,
    kind: String,
    sha256: String,
}

pub struct AvailableBuild {
    pub version: Version,
    filename: String,
    sha256: String,
}

/// Parse `go1.23.4` (also older `go1.4` / `go1`, padding with zeros).
fn parse_go_version(raw: &str) -> Option<Version> {
    let numbers = raw.strip_prefix("go")?;
    let mut parts = numbers.split('.');
    let mut next = || -> Option<u32> {
        match parts.next() {
            Some(p) => p.parse().ok(),
            None => Some(0),
        }
    };
    let version = Version {
        major: next()?,
        minor: next()?,
        patch: next()?,
    };
    parts.next().is_none().then_some(version)
}

/// All stable Go versions with a binary archive for the current platform,
/// ascending.
pub fn fetch_available() -> Result<Vec<AvailableBuild>> {
    let (os, arch) = platform()?;
    let index: Vec<Release> = fetch::client()?
        .get(INDEX_URL)
        .send()
        .context("failed to query go.dev/dl")?
        .error_for_status()
        .context("go.dev/dl index query failed")?
        .json()
        .context("failed to parse go.dev/dl version index")?;

    let mut builds: Vec<AvailableBuild> = index
        .into_iter()
        .filter(|release| release.stable)
        .filter_map(|release| {
            let version = parse_go_version(&release.version)?;
            let file = release.files.into_iter().find(|f| {
                f.os == os && f.arch == arch && f.kind == "archive" && f.filename.ends_with(".tar.gz")
            })?;
            Some(AvailableBuild {
                version,
                filename: file.filename,
                sha256: file.sha256.to_ascii_lowercase(),
            })
        })
        .collect();
    builds.sort_by_key(|b| b.version);
    Ok(builds)
}

/// Download the build, verify its checksum, and extract it so that
/// `dest/bin/go` exists. `dest` must not already exist.
pub fn install_build(build: &AvailableBuild, dest: &Path) -> Result<()> {
    let url = format!("https://go.dev/dl/{}", build.filename);
    eprintln!("downloading {url}");
    let archive = fetch::download(&fetch::client()?, &url)?;
    fetch::verify_sha256(&archive, &build.sha256, &build.filename)?;
    // go.dev archives contain a single top-level `go/` directory.
    fetch::extract_tar_gz_subdir(&archive, "go", dest)
}

/// The directory containing executables inside an installed toolchain.
pub fn bin_dir(toolchain: &Path) -> PathBuf {
    toolchain.join("bin")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(s: &str) -> Version {
        s.parse().unwrap()
    }

    #[test]
    fn parses_go_versions() {
        assert_eq!(parse_go_version("go1.23.4"), Some(v("1.23.4")));
        assert_eq!(parse_go_version("go1.4"), Some(v("1.4.0")));
        assert_eq!(parse_go_version("go1"), Some(v("1.0.0")));
        assert_eq!(parse_go_version("1.23.4"), None);
        assert_eq!(parse_go_version("go1.23.4.5"), None);
        assert_eq!(parse_go_version("go1.21rc2"), None);
    }
}
