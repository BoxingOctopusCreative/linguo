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

/// A fetched channel manifest plus the metadata linguo needs from it.
pub struct Manifest {
    pub doc: DocumentMut,
    /// The manifest's build date (`2026-07-02`), which names dated toolchains.
    pub date: String,
    /// Raw `[pkg.rust].version`, e.g. `1.98.0-nightly (abcdef 2026-07-01)`.
    pub rust_version: String,
}

impl Manifest {
    /// The plain semver version, for release (non-prerelease) channels.
    pub fn release_version(&self) -> Result<Version> {
        self.rust_version
            .split_whitespace()
            .next()
            .unwrap_or(&self.rust_version)
            .parse()
            .with_context(|| {
                format!(
                    "unexpected rust version '{}' in manifest",
                    self.rust_version
                )
            })
    }
}

/// Fetch a channel manifest. `channel` is the manifest name (`stable`,
/// `beta`, `nightly`, `1.96`, or `1.96.1`); `date` selects a dated snapshot
/// (`dist/<date>/channel-rust-<channel>.toml`) instead of the current one.
pub fn fetch_manifest(channel: &str, date: Option<&str>) -> Result<Manifest> {
    let url = match date {
        Some(date) => format!("{DIST_BASE}/{date}/channel-rust-{channel}.toml"),
        None => format!("{DIST_BASE}/channel-rust-{channel}.toml"),
    };
    let text = fetch::client()?
        .get(&url)
        .send()
        .and_then(|r| r.error_for_status())
        .with_context(|| format!("no rust channel manifest for '{channel}' ({url})"))?
        .text()?;
    let doc: DocumentMut = text.parse().context("failed to parse channel manifest")?;

    let date = doc
        .get("date")
        .and_then(|d| d.as_str())
        .context("channel manifest has no date")?
        .to_string();
    let rust_version = doc
        .get("pkg")
        .and_then(|p| p.get("rust"))
        .and_then(|r| r.get("version"))
        .and_then(|v| v.as_str())
        .context("channel manifest has no rust version")?
        .to_string();
    Ok(Manifest {
        doc,
        date,
        rust_version,
    })
}

/// Resolve a user-facing component name to the manifest's package key:
/// several components live under `<name>-preview` in the manifests.
fn component_package(doc: &DocumentMut, name: &str) -> Result<String> {
    let pkg = doc.get("pkg").context("manifest has no packages")?;
    if pkg.get(name).is_some() {
        return Ok(name.to_string());
    }
    let preview = format!("{name}-preview");
    if pkg.get(&preview).is_some() {
        return Ok(preview);
    }
    bail!("unknown component '{name}' (not in this toolchain's manifest)");
}

/// The manifest target key a component installs from: target-independent
/// components (e.g. rust-src) live under `*`.
fn component_target_key<'a>(doc: &DocumentMut, package: &str, triple: &'a str) -> &'a str {
    let has_star = doc
        .get("pkg")
        .and_then(|p| p.get(package))
        .and_then(|c| c.get("target"))
        .and_then(|t| t.get("*"))
        .is_some();
    if has_star { "*" } else { triple }
}

/// Download and merge extra `components` (by user-facing name, for the host)
/// and `rust-std` for extra `targets` into an existing toolchain prefix.
pub fn add_components(
    doc: &DocumentMut,
    dest: &Path,
    components: &[String],
    targets: &[String],
) -> Result<()> {
    let triple = target_triple()?;
    let mut wanted: Vec<(String, String)> = Vec::new();
    for name in components {
        let package = component_package(doc, name)?;
        let key = component_target_key(doc, &package, triple).to_string();
        wanted.push((package, key));
    }
    for target in targets {
        wanted.push(("rust-std".to_string(), target.clone()));
    }
    install_packages(doc, dest, &wanted)
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

/// Download each `(package, target-key)` pair from the manifest, verify it,
/// and merge it into the toolchain prefix at `dest`.
fn install_packages(doc: &DocumentMut, dest: &Path, packages: &[(String, String)]) -> Result<()> {
    let http = fetch::client()?;
    let parent = dest
        .parent()
        .context("install destination has no parent directory")?;

    for (package, target_key) in packages {
        let (url, hash) = component_build(doc, package, target_key)?;
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

/// Download the default profile (rustc, cargo, rust-std, clippy, rustfmt)
/// plus any extra `components`/`targets`, and merge everything into `dest`
/// so that `dest/bin/cargo` and `dest/bin/rustc` exist.
pub fn install_channel(
    doc: &DocumentMut,
    dest: &Path,
    components: &[String],
    targets: &[String],
) -> Result<()> {
    let triple = target_triple()?;
    let mut packages: Vec<(String, String)> = COMPONENTS
        .iter()
        .map(|c| (c.to_string(), triple.to_string()))
        .collect();
    for name in components {
        let package = component_package(doc, name)?;
        if packages.iter().any(|(p, _)| p == &package) {
            continue;
        }
        let key = component_target_key(doc, &package, triple).to_string();
        packages.push((package, key));
    }
    for target in targets {
        if target == triple {
            continue;
        }
        packages.push(("rust-std".to_string(), target.clone()));
    }
    install_packages(doc, dest, &packages)
}

/// The directory containing executables inside an installed toolchain.
pub fn bin_dir(toolchain: &Path) -> PathBuf {
    toolchain.join("bin")
}
