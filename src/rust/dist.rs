//! Fetching Rust toolchains from the official static.rust-lang.org dist
//! channels — the same v2 manifests rustup consumes.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::Deserialize;
use toml_edit::DocumentMut;

use crate::fetch;
use crate::versions::Version;

const DIST_BASE: &str = "https://static.rust-lang.org/dist";
const RELEASES_URL: &str = "https://api.github.com/repos/rust-lang/rust/releases?per_page=100";

/// The components a toolchain installs: rustup's default profile minus docs.
const COMPONENTS: &[&str] = &[
    "rustc",
    "cargo",
    "rust-std",
    "clippy-preview",
    "rustfmt-preview",
];

pub fn target_triple() -> Result<&'static str> {
    let triple = match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => "aarch64-apple-darwin",
        ("macos", "x86_64") => "x86_64-apple-darwin",
        ("linux", "aarch64") => "aarch64-unknown-linux-gnu",
        ("linux", "x86_64") => "x86_64-unknown-linux-gnu",
        ("windows", "x86_64") => "x86_64-pc-windows-msvc",
        ("windows", "aarch64") => "aarch64-pc-windows-msvc",
        (os, arch) => bail!("unsupported platform for rust: {os}/{arch}"),
    };
    Ok(triple)
}

#[derive(Debug, Deserialize)]
struct Release {
    tag_name: String,
}

/// Recent stable Rust versions (the releases GitHub knows about), ascending.
pub fn fetch_available() -> Result<Vec<Version>> {
    let releases: Vec<Release> = fetch::client()?
        .get(RELEASES_URL)
        .send()
        .context("failed to query rust-lang/rust releases")?
        .error_for_status()
        .context("rust release query failed")?
        .json()
        .context("failed to parse rust release list")?;
    let mut versions: Vec<Version> = releases
        .into_iter()
        .filter_map(|r| r.tag_name.parse().ok())
        .collect();
    versions.sort();
    Ok(versions)
}

/// Fetch the channel manifest for a channel name (`stable`, `1.96`, or
/// `1.96.1`) and return it with the exact version it resolved to.
pub fn fetch_manifest(channel: &str) -> Result<(Version, DocumentMut)> {
    let url = format!("{DIST_BASE}/channel-rust-{channel}.toml");
    let text = fetch::client()?
        .get(&url)
        .send()
        .and_then(|r| r.error_for_status())
        .with_context(|| format!("no rust channel manifest for '{channel}' ({url})"))?
        .text()?;
    let doc: DocumentMut = text.parse().context("failed to parse channel manifest")?;

    // [pkg.rust].version is e.g. `1.96.1 (31fca3adb 2026-06-26)`.
    let raw = doc
        .get("pkg")
        .and_then(|p| p.get("rust"))
        .and_then(|r| r.get("version"))
        .and_then(|v| v.as_str())
        .context("channel manifest has no rust version")?;
    let version: Version = raw
        .split_whitespace()
        .next()
        .unwrap_or(raw)
        .parse()
        .with_context(|| format!("unexpected rust version '{raw}' in manifest"))?;
    Ok((version, doc))
}

fn component_build<'a>(
    doc: &'a DocumentMut,
    component: &str,
    triple: &str,
) -> Result<(&'a str, &'a str)> {
    let target = doc
        .get("pkg")
        .and_then(|p| p.get(component))
        .and_then(|c| c.get("target"))
        .and_then(|t| t.get(triple))
        .with_context(|| format!("manifest has no {component} entry for {triple}"))?;
    if target.get("available").and_then(|a| a.as_bool()) != Some(true) {
        bail!("{component} is not available for {triple} in this release");
    }
    let url = target
        .get("url")
        .and_then(|u| u.as_str())
        .with_context(|| format!("no archive url for {component}/{triple}"))?;
    let hash = target
        .get("hash")
        .and_then(|h| h.as_str())
        .with_context(|| format!("no checksum for {component}/{triple}"))?;
    Ok((url, hash))
}

/// Recursively move `src`'s contents into `dst`, creating directories and
/// skipping installer metadata files.
fn merge_tree(src: &Path, dst: &Path) -> Result<()> {
    std::fs::create_dir_all(dst).with_context(|| format!("failed to create {}", dst.display()))?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let from = entry.path();
        let to = dst.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            merge_tree(&from, &to)?;
        } else if entry.file_name() != "manifest.in" {
            std::fs::rename(&from, &to)
                .with_context(|| format!("failed to place {}", to.display()))?;
        }
    }
    Ok(())
}

/// Download each component from the manifest, verify it, and merge all of
/// them into `dest` so that `dest/bin/cargo` and `dest/bin/rustc` exist.
pub fn install_channel(doc: &DocumentMut, dest: &Path) -> Result<()> {
    let triple = target_triple()?;
    let http = fetch::client()?;
    let parent = dest
        .parent()
        .context("install destination has no parent directory")?;

    for component in COMPONENTS {
        let (url, hash) = component_build(doc, component, triple)?;
        let archive_name = url.rsplit('/').next().unwrap_or(url).to_string();

        eprintln!("downloading {url}");
        let archive = fetch::download(&http, url)?;
        fetch::verify_sha256(&archive, hash, &archive_name)?;

        let staging = tempfile::tempdir_in(parent).context("failed to create staging directory")?;
        fetch::extract_archive_root(&archive, &archive_name, staging.path())?;

        // Layout: <name>-<version>-<triple>/<payload dirs> + installer files;
        // every payload directory merges into the toolchain prefix.
        let top = staging
            .path()
            .join(archive_name.trim_end_matches(".tar.gz"));
        if !top.is_dir() {
            bail!("unexpected archive layout in {archive_name}");
        }
        for entry in std::fs::read_dir(&top)? {
            let entry = entry?;
            if entry.file_type()?.is_dir() {
                merge_tree(&entry.path(), dest)?;
            }
        }
    }
    Ok(())
}

/// The directory containing executables inside an installed toolchain.
pub fn bin_dir(toolchain: &Path) -> PathBuf {
    toolchain.join("bin")
}
