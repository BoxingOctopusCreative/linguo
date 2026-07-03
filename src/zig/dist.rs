//! Fetching Zig toolchains from ziglang.org's release index. Zig's Linux
//! binaries are static, so one build serves glibc and musl alike.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::Deserialize;

use crate::fetch;
use crate::versions::Version;

const INDEX_URL: &str = "https://ziglang.org/download/index.json";

/// Platform key in the index, e.g. `aarch64-macos`.
fn platform() -> Result<String> {
    let os = match std::env::consts::OS {
        "macos" => "macos",
        "linux" => "linux",
        "windows" => "windows",
        other => bail!("unsupported platform for zig: {other}"),
    };
    let arch = match std::env::consts::ARCH {
        "x86_64" => "x86_64",
        "aarch64" => "aarch64",
        other => bail!("unsupported architecture for zig: {other}"),
    };
    Ok(format!("{arch}-{os}"))
}

#[derive(Debug, Deserialize)]
struct Entry {
    tarball: String,
    shasum: String,
}

pub struct AvailableBuild {
    pub version: Version,
    url: String,
    shasum: String,
}

/// All stable Zig versions with a build for the current platform, ascending.
/// The `master` nightly entry doesn't parse as semver and is skipped.
pub fn fetch_available() -> Result<Vec<AvailableBuild>> {
    let platform = platform()?;
    let index: HashMap<String, HashMap<String, serde_json::Value>> = fetch::client()?
        .get(INDEX_URL)
        .send()
        .context("failed to query ziglang.org downloads")?
        .error_for_status()
        .context("zig download index query failed")?
        .json()
        .context("failed to parse zig download index")?;

    let mut builds: Vec<AvailableBuild> = index
        .into_iter()
        .filter_map(|(name, targets)| {
            let version: Version = name.parse().ok()?;
            let entry: Entry = serde_json::from_value(targets.get(&platform)?.clone()).ok()?;
            Some(AvailableBuild {
                version,
                url: entry.tarball,
                shasum: entry.shasum.to_ascii_lowercase(),
            })
        })
        .collect();
    builds.sort_by_key(|b| b.version);
    Ok(builds)
}

/// Download the build, verify its checksum, and extract it so that
/// `dest/zig` exists (the binary lives at the archive's top level).
pub fn install_build(build: &AvailableBuild, dest: &Path) -> Result<()> {
    let archive_name = build.url.rsplit('/').next().unwrap_or(&build.url);
    eprintln!("downloading {}", build.url);
    let archive = fetch::download(&fetch::client()?, &build.url)?;
    fetch::verify_sha256(&archive, &build.shasum, archive_name)?;

    // Archives hold a single top-level dir named after the archive.
    let subdir = archive_name
        .trim_end_matches(".tar.xz")
        .trim_end_matches(".zip");
    fetch::extract_archive_subdir(&archive, archive_name, subdir, dest)
}

/// The zig binary sits at the toolchain root.
pub fn bin_dir(toolchain: &Path) -> PathBuf {
    toolchain.to_path_buf()
}
