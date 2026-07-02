//! `linguo status`: cross-language overview of toolchains and active pins.

use anyhow::Result;

use crate::config::{self, PinSource};
use crate::store;

pub const LANGUAGES: &[&str] = &["python", "node", "go", "terraform"];

pub fn status() -> Result<()> {
    let cwd = std::env::current_dir()?;
    for &language in LANGUAGES {
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

        match config::resolve_pin(language, &cwd)? {
            None => println!("  active: none (no version pinned)"),
            Some(pin) => {
                let source = match &pin.source {
                    PinSource::Project(path) => path.display().to_string(),
                    PinSource::Global => "global config".to_string(),
                };
                match store::find_installed(language, &pin.req)? {
                    Some(version) => {
                        println!("  active: {version} (pinned to {} by {source})", pin.req);
                    }
                    None => println!(
                        "  active: none ({} pinned by {source} but not installed — run `linguo {language} install {}`)",
                        pin.req, pin.req
                    ),
                }
            }
        }
    }
    Ok(())
}
