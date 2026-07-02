//! Shared HTTP plumbing for toolchain downloads.

use std::io::Read;
use std::time::Duration;

use anyhow::{Context, Result};
use indicatif::{ProgressBar, ProgressStyle};

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
