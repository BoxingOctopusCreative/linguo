//! Shared plumbing for toolchain downloads: HTTP, checksums, extraction.

use std::io::Read;
use std::path::Path;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use indicatif::{ProgressBar, ProgressStyle};
use sha2::{Digest, Sha256};

pub fn client() -> Result<reqwest::blocking::Client> {
    reqwest::blocking::Client::builder()
        .user_agent(concat!("linguo/", env!("CARGO_PKG_VERSION")))
        .timeout(Duration::from_secs(600))
        .connect_timeout(Duration::from_secs(15))
        .build()
        .context("failed to build HTTP client")
}

/// Download `url` into memory, showing a progress bar on stderr (hidden when
/// stderr is not a terminal).
pub fn download(http: &reqwest::blocking::Client, url: &str) -> Result<Vec<u8>> {
    let mut response = http
        .get(url)
        .send()
        .and_then(|r| r.error_for_status())
        .with_context(|| format!("failed to download {url}"))?;

    let bar = match response.content_length() {
        Some(len) => ProgressBar::new(len).with_style(
            ProgressStyle::with_template(
                "{bar:30.cyan/blue} {bytes:>10}/{total_bytes} ({bytes_per_sec}, eta {eta})",
            )
            .expect("valid progress template"),
        ),
        None => ProgressBar::new_spinner().with_style(
            ProgressStyle::with_template("{spinner} {bytes} ({bytes_per_sec})")
                .expect("valid progress template"),
        ),
    };

    let mut body = Vec::new();
    let mut chunk = [0u8; 64 * 1024];
    loop {
        let n = response.read(&mut chunk).context("download interrupted")?;
        if n == 0 {
            break;
        }
        body.extend_from_slice(&chunk[..n]);
        bar.inc(n as u64);
    }
    bar.finish_and_clear();
    Ok(body)
}

/// Look up a file's hash in a checksums manifest (`<hex>  <name>` lines, as
/// in SHA256SUMS / SHASUMS256.txt files).
pub fn find_sha256(sums: &str, asset_name: &str) -> Option<String> {
    sums.lines().find_map(|line| {
        let (hash, name) = line.split_once(char::is_whitespace)?;
        (name.trim() == asset_name).then(|| hash.to_ascii_lowercase())
    })
}

pub fn verify_sha256(bytes: &[u8], expected: &str, what: &str) -> Result<()> {
    let actual = hex::encode(Sha256::digest(bytes));
    if actual != expected {
        bail!("checksum mismatch for {what}: expected {expected}, got {actual}");
    }
    Ok(())
}

/// Unpack a .tar.gz or .zip archive (picked by `name`'s extension) into `dir`.
fn unpack(archive: &[u8], name: &str, dir: &Path) -> Result<()> {
    if name.ends_with(".tar.gz") {
        let gz = flate2::read::GzDecoder::new(archive);
        tar::Archive::new(gz)
            .unpack(dir)
            .context("failed to extract archive")
    } else if name.ends_with(".zip") {
        extract_zip(archive, dir)
    } else if name.ends_with(".7z") {
        sevenz_rust::decompress(std::io::Cursor::new(archive), dir)
            .map_err(|e| anyhow::anyhow!("failed to extract 7z archive: {e}"))
    } else {
        bail!("unsupported archive format: {name}");
    }
}

/// Extract a zip archive into `dir`, preserving unix permissions.
fn extract_zip(archive: &[u8], dir: &Path) -> Result<()> {
    let mut zip =
        zip::ZipArchive::new(std::io::Cursor::new(archive)).context("failed to open archive")?;
    std::fs::create_dir_all(dir).with_context(|| format!("failed to create {}", dir.display()))?;
    for i in 0..zip.len() {
        let mut file = zip.by_index(i).context("failed to read archive entry")?;
        if file.is_dir() {
            continue;
        }
        let path = dir.join(file.mangled_name());
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

/// Extract an archive and move its top-level `subdir` to `dest` (which must
/// not exist yet).
pub fn extract_archive_subdir(archive: &[u8], name: &str, subdir: &str, dest: &Path) -> Result<()> {
    let staging = tempfile::tempdir_in(
        dest.parent()
            .context("install destination has no parent directory")?,
    )
    .context("failed to create staging directory")?;

    unpack(archive, name, staging.path())?;

    let extracted = staging.path().join(subdir);
    if !extracted.is_dir() {
        bail!("unexpected archive layout: no top-level {subdir}/ directory");
    }
    std::fs::rename(&extracted, dest)
        .with_context(|| format!("failed to move extracted toolchain to {}", dest.display()))?;
    Ok(())
}

/// Extract an archive whose files sit at the top level directly into `dest`.
pub fn extract_archive_root(archive: &[u8], name: &str, dest: &Path) -> Result<()> {
    unpack(archive, name, dest)
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
        assert_eq!(
            find_sha256(sums, "other.tar.gz"),
            Some("def456".to_string())
        );
        assert_eq!(find_sha256(sums, "missing.tar.gz"), None);
    }
}
