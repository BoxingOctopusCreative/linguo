//! Language-agnostic developer-tool store: isolated per-tool installs under
//! `$LINGUO_ROOT/tools/<language>/<tool>/<version>`, each exposing its
//! executables in a `<version>/exposed` directory. Which tools are active in
//! a directory comes from `[tools.<language>]` tables (global config as the
//! base, the nearest project linguo.toml overriding per tool), resolved just
//! like runtime pins. The shell hook prepends the exposed dirs of the active
//! tools; `linguo sync` installs the ones a project declares.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

use crate::config::{self, PinSource};
use crate::versions::{Version, VersionReq};

/// `$LINGUO_ROOT/tools/<language>/<tool>`, holding one dir per version.
pub fn tool_dir(language: &str, tool: &str) -> Result<PathBuf> {
    Ok(config::linguo_root()?
        .join("tools")
        .join(language)
        .join(tool))
}

pub fn version_dir(language: &str, tool: &str, version: &Version) -> Result<PathBuf> {
    Ok(tool_dir(language, tool)?.join(version.to_string()))
}

/// The directory holding a tool version's exposed executables (on PATH).
pub fn exposed_dir(language: &str, tool: &str, version: &Version) -> Result<PathBuf> {
    Ok(version_dir(language, tool, version)?.join("exposed"))
}

/// Installed versions of `tool`, ascending.
pub fn installed_versions(language: &str, tool: &str) -> Result<Vec<Version>> {
    let dir = tool_dir(language, tool)?;
    let entries = match std::fs::read_dir(&dir) {
        Ok(entries) => entries,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(err) => return Err(err).with_context(|| format!("failed to read {}", dir.display())),
    };
    let mut versions: Vec<Version> = entries
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.path().is_dir())
        .filter_map(|entry| entry.file_name().to_str().and_then(|s| s.parse().ok()))
        .collect();
    versions.sort();
    Ok(versions)
}

/// Highest installed version of `tool` satisfying `req`.
pub fn find_installed(language: &str, tool: &str, req: &VersionReq) -> Result<Option<Version>> {
    Ok(req.best_match(installed_versions(language, tool)?))
}

/// Names of every tool with at least one installed version, sorted.
pub fn installed_tools(language: &str) -> Result<Vec<String>> {
    let dir = config::linguo_root()?.join("tools").join(language);
    let entries = match std::fs::read_dir(&dir) {
        Ok(entries) => entries,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(err) => return Err(err).with_context(|| format!("failed to read {}", dir.display())),
    };
    let mut tools: Vec<String> = entries
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.path().is_dir())
        .filter_map(|entry| entry.file_name().to_str().map(str::to_string))
        .collect();
    tools.sort();
    Ok(tools)
}

/// Exposed dirs of every tool active in `cwd`, for PATH assembly by the hook.
pub fn active_exposed_dirs(language: &str, cwd: &Path) -> Result<Vec<PathBuf>> {
    let mut dirs = Vec::new();
    for pin in config::tool_pins(language, cwd)? {
        let Ok(req) = pin.raw.parse::<VersionReq>() else {
            continue;
        };
        if let Some(version) = find_installed(language, &pin.tool, &req)? {
            let exposed = exposed_dir(language, &pin.tool, &version)?;
            if exposed.is_dir() {
                dirs.push(exposed);
            }
        }
    }
    Ok(dirs)
}

/// `linguo <lang> tool list`: installed tool versions with their active pin.
pub fn list(language: &str) -> Result<()> {
    let tools = installed_tools(language)?;
    if tools.is_empty() {
        println!("no {language} tools installed (try `linguo {language} tool install <name>`)");
        return Ok(());
    }
    let cwd = std::env::current_dir()?;
    let pins = config::tool_pins(language, &cwd)?;
    for tool in tools {
        let pin = pins.iter().find(|p| p.tool == tool);
        let active = match pin {
            Some(p) => p
                .raw
                .parse::<VersionReq>()
                .ok()
                .and_then(|req| find_installed(language, &tool, &req).ok().flatten()),
            None => None,
        };
        for version in installed_versions(language, &tool)? {
            let marker = if active == Some(version) {
                let source = match &pin.unwrap().source {
                    PinSource::Project(path) => format!("pinned by {}", path.display()),
                    PinSource::Global => "global default".to_string(),
                };
                format!(" * ({source})")
            } else {
                String::new()
            };
            println!("{tool} {version}{marker}");
        }
    }
    Ok(())
}

/// `linguo <lang> tool uninstall <tool>`: remove every installed version and
/// drop its global pin (a project pin is the project's to remove).
pub fn uninstall(language: &str, tool: &str) -> Result<()> {
    let dir = tool_dir(language, tool)?;
    if !dir.exists() {
        bail!("{language} tool '{tool}' is not installed");
    }
    std::fs::remove_dir_all(&dir).with_context(|| format!("failed to remove {}", dir.display()))?;
    let global = config::linguo_root()?.join(config::GLOBAL_CONFIG);
    config::remove_tool_pin(&global, language, tool)?;
    println!("uninstalled {language} tool '{tool}'");
    Ok(())
}
