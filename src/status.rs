//! `linguo status`: cross-language overview of toolchains and active pins.

use anyhow::{Context, Result};

use crate::config::PinSource;
use crate::versions::VersionReq;
use crate::{rust, store, terraform};

/// Languages whose pins are plain version requests (including ecosystem
/// pin-file fallbacks, which store::resolve_pin handles); terraform prints
/// its own section because its pins carry a distribution.
const GENERIC_LANGUAGES: &[&str] = &[
    "python", "node", "ruby", "php", "go", "zig", "jvm", "kotlin", "groovy", "scala",
];

pub fn status() -> Result<()> {
    let cwd = std::env::current_dir()?;
    for &language in GENERIC_LANGUAGES {
        println!("{language}");

        let installed = store::installed_versions(language)?;
        let toolchains = if installed.is_empty() {
            "(none)".to_string()
        } else {
            installed
                .iter()
                .map(|v| v.to_string())
                .collect::<Vec<_>>()
                .join(", ")
        };
        println!("  toolchains: {toolchains}");

        match store::resolve_pin(language, &cwd)? {
            None => println!("  active: none (no version pinned)"),
            Some(pin) => {
                let source = match &pin.source {
                    PinSource::Project(path) => path.display().to_string(),
                    PinSource::Global => "global config".to_string(),
                };
                let req: VersionReq = pin
                    .raw
                    .parse()
                    .with_context(|| format!("invalid {language} version '{}' pinned", pin.raw))?;
                match store::find_installed(language, &req)? {
                    Some(version) => {
                        println!("  active: {version} (pinned to {} by {source})", pin.raw);
                    }
                    None => println!(
                        "  active: none ({} pinned by {source} but not installed — run `linguo {language} install {}`)",
                        pin.raw, pin.raw
                    ),
                }
            }
        }
    }
    rust::print_status(&cwd)?;
    terraform::print_status(&cwd)
}
