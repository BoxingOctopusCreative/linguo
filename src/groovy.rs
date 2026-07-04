//! Apache Groovy: binary zips from archive.apache.org, versions enumerated
//! from apache/groovy git tags, verified against Apache's .sha256 sidecars.

use anyhow::{Context, Result};
use serde::Deserialize;

use crate::jvmlang::{Build, Def};
use crate::{fetch, versions::Version};

pub const DEF: Def = Def {
    language: "groovy",
    default_bin: "groovy",
    fetch_available,
};

const TAGS_URL: &str = "https://api.github.com/repos/apache/groovy/tags?per_page=100";
const ARCHIVE_BASE: &str = "https://archive.apache.org/dist/groovy";

#[derive(Debug, Deserialize)]
struct Tag {
    name: String,
}

fn fetch_available() -> Result<Vec<Build>> {
    let http = fetch::client()?;
    let tags: Vec<Tag> = fetch::github_api_get(&http, TAGS_URL)
        .send()
        .context("failed to query Groovy tags")?
        .error_for_status()
        .context("Groovy tag query failed")?
        .json()
        .context("failed to parse Groovy tags")?;

    let mut builds: Vec<Build> = tags
        .into_iter()
        .filter_map(|t| {
            // Tags look like GROOVY_4_0_28; prerelease tags carry extra
            // parts (GROOVY_5_0_0_alpha_1) and fail the parse.
            let version: Version = t
                .name
                .strip_prefix("GROOVY_")?
                .replace('_', ".")
                .parse()
                .ok()?;
            Some(Build {
                url: format!(
                    "{ARCHIVE_BASE}/{version}/distribution/apache-groovy-binary-{version}.zip"
                ),
                sha256: None,
                // Apache publishes .sha256 sidecars; fetched at install time.
                sha256_url: Some(format!(
                    "{ARCHIVE_BASE}/{version}/distribution/apache-groovy-binary-{version}.zip.sha256"
                )),
                subdir: format!("groovy-{version}"),
                asset_name: format!("apache-groovy-binary-{version}.zip"),
                version,
            })
        })
        .collect();
    builds.sort_by_key(|b| b.version);
    builds.dedup_by_key(|b| b.version);
    Ok(builds)
}
