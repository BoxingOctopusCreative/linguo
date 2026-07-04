//! Scala 3: distribution tarballs from scala/scala3 GitHub releases,
//! verified against GitHub's per-asset sha256 digests.

use anyhow::{Context, Result};
use serde::Deserialize;

use crate::jvmlang::{Build, Def};
use crate::{fetch, versions::Version};

pub const DEF: Def = Def {
    language: "scala",
    default_bin: "scala",
    fetch_available,
};

const RELEASES_URL: &str = "https://api.github.com/repos/scala/scala3/releases?per_page=100";

#[derive(Debug, Deserialize)]
struct Release {
    tag_name: String,
    prerelease: bool,
    assets: Vec<Asset>,
}

#[derive(Debug, Deserialize)]
struct Asset {
    name: String,
    browser_download_url: String,
    digest: Option<String>,
}

fn fetch_available() -> Result<Vec<Build>> {
    let http = fetch::client()?;
    let releases: Vec<Release> = fetch::github_api_get(&http, RELEASES_URL)
        .send()
        .context("failed to query Scala releases")?
        .error_for_status()
        .context("Scala release query failed")?
        .json()
        .context("failed to parse Scala releases")?;

    let mut builds: Vec<Build> = releases
        .into_iter()
        .filter(|r| !r.prerelease)
        .filter_map(|r| {
            // Tags are plain versions (3.3.8); RCs carry suffixes and fail
            // the parse.
            let version: Version = r.tag_name.parse().ok()?;
            let wanted = format!("scala3-{version}.tar.gz");
            let asset = r.assets.into_iter().find(|a| a.name == wanted)?;
            Some(Build {
                version,
                url: asset.browser_download_url,
                sha256: asset
                    .digest
                    .as_deref()
                    .and_then(|d| d.strip_prefix("sha256:"))
                    .map(str::to_string),
                sha256_url: None,
                subdir: format!("scala3-{version}"),
                asset_name: asset.name,
            })
        })
        .collect();
    builds.sort_by_key(|b| b.version);
    Ok(builds)
}
