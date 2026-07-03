//! Fetching PHP builds: static-php-cli's static builds on unix (musl-static
//! on Linux, so every distro works), and official windows.php.net zips on
//! Windows. Composer ships into every toolchain.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::Deserialize;

use crate::fetch;
use crate::versions::Version;

const STATIC_PHP_BASE: &str = "https://dl.static-php.dev";
const STATIC_PHP_INDEX: &str = "https://dl.static-php.dev/static-php-cli/common/?format=json";
const WINDOWS_INDEX: &str = "https://downloads.php.net/~windows/releases/releases.json";
const COMPOSER_URL: &str = "https://getcomposer.org/download/latest-stable/composer.phar";

pub struct AvailableBuild {
    pub version: Version,
    url: String,
    /// sha256 when the source publishes one (windows.php.net does;
    /// static-php-cli doesn't).
    sha256: Option<String>,
}

#[derive(Debug, Deserialize)]
struct StaticPhpEntry {
    is_dir: bool,
    full_path: String,
    name: String,
}

fn fetch_static_php() -> Result<Vec<AvailableBuild>> {
    let os = match std::env::consts::OS {
        "macos" => "macos",
        "linux" => "linux",
        other => bail!("unsupported platform for php: {other}"),
    };
    let arch = match std::env::consts::ARCH {
        "x86_64" => "x86_64",
        "aarch64" => "aarch64",
        other => bail!("unsupported architecture for php: {other}"),
    };
    let suffix = format!("-cli-{os}-{arch}.tar.gz");

    let entries: Vec<StaticPhpEntry> = fetch::client()?
        .get(STATIC_PHP_INDEX)
        .send()
        .context("failed to query dl.static-php.dev")?
        .error_for_status()
        .context("static-php index query failed")?
        .json()
        .context("failed to parse static-php index")?;

    let mut builds: Vec<AvailableBuild> = entries
        .into_iter()
        .filter(|e| !e.is_dir)
        .filter_map(|e| {
            let version: Version = e
                .name
                .strip_prefix("php-")?
                .strip_suffix(&suffix)?
                .parse()
                .ok()?;
            Some(AvailableBuild {
                version,
                url: format!("{STATIC_PHP_BASE}{}", e.full_path),
                sha256: None,
            })
        })
        .collect();
    builds.sort_by_key(|b| b.version);
    Ok(builds)
}

#[derive(Debug, Deserialize)]
struct WindowsZip {
    path: String,
    sha256: String,
}

fn fetch_windows_php() -> Result<Vec<AvailableBuild>> {
    if std::env::consts::ARCH != "x86_64" {
        bail!("windows.php.net publishes x64 builds only; no official arm64 PHP for Windows yet");
    }
    let index: std::collections::HashMap<String, serde_json::Value> = fetch::client()?
        .get(WINDOWS_INDEX)
        .send()
        .context("failed to query windows.php.net releases")?
        .error_for_status()
        .context("windows php release query failed")?
        .json()
        .context("failed to parse windows php releases")?;

    let mut builds: Vec<AvailableBuild> = index
        .into_values()
        .filter_map(|branch| {
            // Non-thread-safe x64 is the CLI-appropriate variant.
            let zip: WindowsZip =
                serde_json::from_value(branch.get("nts-vs17-x64")?.get("zip")?.clone()).ok()?;
            let version: Version = branch.get("version")?.as_str()?.parse().ok()?;
            Some(AvailableBuild {
                version,
                url: format!("https://downloads.php.net/~windows/releases/{}", zip.path),
                sha256: Some(zip.sha256.to_ascii_lowercase()),
            })
        })
        .collect();
    builds.sort_by_key(|b| b.version);
    Ok(builds)
}

/// All PHP versions available for the current platform, ascending.
pub fn fetch_available() -> Result<Vec<AvailableBuild>> {
    if std::env::consts::OS == "windows" {
        fetch_windows_php()
    } else {
        fetch_static_php()
    }
}

/// Download the build (and composer), verify what can be verified, and lay
/// out the toolchain so `dest/php` (or php.exe) and `dest/composer.phar`
/// exist. Both archive styles hold their files at the top level.
pub fn install_build(build: &AvailableBuild, dest: &Path) -> Result<()> {
    let http = fetch::client()?;
    let archive_name = build.url.rsplit('/').next().unwrap_or(&build.url);

    eprintln!("downloading {}", build.url);
    let archive = fetch::download(&http, &build.url)?;
    match &build.sha256 {
        Some(expected) => fetch::verify_sha256(&archive, expected, archive_name)?,
        None => eprintln!("warning: no published checksum for this build; skipping verification"),
    }
    fetch::extract_archive_root(&archive, archive_name, dest)?;

    // Composer rides along in every toolchain, verified against its
    // published sha256.
    eprintln!("downloading {COMPOSER_URL}");
    let phar = fetch::download(&http, COMPOSER_URL)?;
    let sums = http
        .get(format!("{COMPOSER_URL}.sha256sum"))
        .send()
        .and_then(|r| r.error_for_status())
        .context("failed to fetch composer checksum")?
        .text()?;
    let expected = sums
        .split_whitespace()
        .next()
        .context("empty composer checksum")?;
    fetch::verify_sha256(&phar, expected, "composer.phar")?;
    std::fs::write(dest.join("composer.phar"), &phar)
        .with_context(|| format!("failed to write {}", dest.join("composer.phar").display()))?;

    // A `composer` wrapper so the shell hook's PATH offers it directly.
    if cfg!(windows) {
        std::fs::write(
            dest.join("composer.bat"),
            "@echo off\r\n\"%~dp0php.exe\" \"%~dp0composer.phar\" %*\r\n",
        )?;
    } else {
        let wrapper = dest.join("composer");
        std::fs::write(
            &wrapper,
            "#!/bin/sh\nexec \"$(dirname \"$0\")/php\" \"$(dirname \"$0\")/composer.phar\" \"$@\"\n",
        )?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&wrapper, std::fs::Permissions::from_mode(0o755))?;
        }
    }
    Ok(())
}

/// Both archive styles put executables at the toolchain root.
pub fn bin_dir(toolchain: &Path) -> PathBuf {
    toolchain.to_path_buf()
}

/// The php interpreter's file name.
pub fn php_exe() -> &'static str {
    if cfg!(windows) { "php.exe" } else { "php" }
}
