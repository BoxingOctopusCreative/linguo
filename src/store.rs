//! Language-agnostic toolchain store: installed versions under
//! `$LINGUO_ROOT/toolchains/<language>/<version>` plus pin handling.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

use crate::config::{self, Pin, PinSource};
use crate::versions::{Version, VersionReq};
use crate::{go, node, python, ruby, rust};

pub fn toolchain_path(language: &str, version: &Version) -> Result<PathBuf> {
    Ok(config::toolchains_dir(language)?.join(version.to_string()))
}

/// Installed toolchain versions, ascending.
pub fn installed_versions(language: &str) -> Result<Vec<Version>> {
    let dir = config::toolchains_dir(language)?;
    let entries = match std::fs::read_dir(&dir) {
        Ok(entries) => entries,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(err) => {
            return Err(err).with_context(|| format!("failed to read {}", dir.display()));
        }
    };
    let mut versions: Vec<Version> = entries
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.path().is_dir())
        .filter_map(|entry| entry.file_name().to_str().and_then(|s| s.parse().ok()))
        .collect();
    versions.sort();
    Ok(versions)
}

/// Highest installed version satisfying `req`.
pub fn find_installed(language: &str, req: &VersionReq) -> Result<Option<Version>> {
    Ok(req.best_match(installed_versions(language)?))
}

/// Parse a pin's value as a plain version request (how every language except
/// terraform interprets pins).
fn pin_req(language: &str, pin: &Pin) -> Result<VersionReq> {
    pin.raw
        .parse()
        .with_context(|| format!("invalid {language} version '{}' pinned", pin.raw))
}

/// The ecosystem's own pin-file convention for `language`, honored when no
/// linguo.toml covers it (nearest file up the tree; a file whose content
/// isn't a plain version — e.g. `lts/*` — counts as no pin).
fn fallback_pin(language: &str, cwd: &Path) -> Result<Option<Pin>> {
    match language {
        python::LANGUAGE => python::fallback_pin(cwd),
        node::LANGUAGE => node::fallback_pin(cwd),
        ruby::LANGUAGE => ruby::fallback_pin(cwd),
        go::LANGUAGE => go::fallback_pin(cwd),
        rust::LANGUAGE => rust::fallback_pin(cwd),
        _ => Ok(None),
    }
}

/// Resolve the pin for `language`: project linguo.toml, then the ecosystem's
/// own pin file, then the global config.
pub fn resolve_pin(language: &str, cwd: &Path) -> Result<Option<Pin>> {
    if let Some(pin) = config::project_pin(language, cwd)? {
        return Ok(Some(pin));
    }
    if let Some(pin) = fallback_pin(language, cwd)? {
        return Ok(Some(pin));
    }
    config::global_pin(language)
}

/// Build a Pin from a version-file's content, if it holds a plain version.
pub fn file_pin(raw: &str, path: &Path) -> Option<Pin> {
    raw.parse::<VersionReq>().ok().map(|req| Pin {
        raw: req.to_string(),
        source: PinSource::Project(path.to_path_buf()),
    })
}

/// Read the first non-empty line of a pin file like `.python-version`.
pub fn read_version_file(path: &Path) -> Result<Option<String>> {
    if !path.is_file() {
        return Ok(None);
    }
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read {}", path.display()))?;
    Ok(text
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty() && !line.starts_with('#'))
        .map(str::to_string))
}

/// Resolve the active toolchain for `cwd`: pin -> installed version.
pub fn resolve_active(language: &str, cwd: &Path) -> Result<Option<(Pin, Version)>> {
    let Some(pin) = resolve_pin(language, cwd)? else {
        return Ok(None);
    };
    let req = pin_req(language, &pin)?;
    match find_installed(language, &req)? {
        Some(version) => Ok(Some((pin, version))),
        None => Ok(None),
    }
}

/// Marker file whose mtime records the last failed auto-install attempt for
/// a pin, so an unreachable upstream doesn't hang every shell prompt.
fn auto_install_marker(language: &str, req: &str) -> Result<PathBuf> {
    Ok(config::linguo_root()?
        .join("cache")
        .join(format!("auto-install-failed-{language}-{req}")))
}

const AUTO_INSTALL_BACKOFF: std::time::Duration = std::time::Duration::from_secs(300);

pub fn auto_install_recently_failed(language: &str, req: &str) -> bool {
    auto_install_marker(language, req)
        .ok()
        .and_then(|path| path.metadata().ok())
        .and_then(|meta| meta.modified().ok())
        .and_then(|mtime| mtime.elapsed().ok())
        .is_some_and(|age| age < AUTO_INSTALL_BACKOFF)
}

pub fn record_auto_install_failure(language: &str, req: &str) {
    if let Ok(path) = auto_install_marker(language, req) {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::write(&path, "");
    }
}

/// resolve_active, but — when auto-install is enabled — an unsatisfied pin
/// is installed on the spot (with a failure backoff so broken networks don't
/// stall every prompt). Used by the shell hook.
pub fn resolve_active_auto(
    language: &str,
    cwd: &Path,
    install: &dyn Fn(&str) -> Result<()>,
) -> Result<Option<(Pin, Version)>> {
    if let Some(active) = resolve_active(language, cwd)? {
        return Ok(Some(active));
    }
    let Some(pin) = resolve_pin(language, cwd)? else {
        return Ok(None);
    };
    if !config::auto_install_enabled()? {
        return Ok(None);
    }
    let req = pin_req(language, &pin)?;
    if auto_install_recently_failed(language, &req.to_string()) {
        return Ok(None);
    }
    eprintln!("linguo: auto-installing {language} {req} (pinned by {})", {
        match &pin.source {
            PinSource::Project(path) => path.display().to_string(),
            PinSource::Global => "the global config".to_string(),
        }
    });
    if let Err(err) = install(&pin.raw) {
        record_auto_install_failure(language, &req.to_string());
        eprintln!("linguo: auto-install of {language} {req} failed: {err:#}");
        eprintln!("linguo: will not retry for 5 minutes");
        return Ok(None);
    }
    resolve_active(language, cwd)
}

/// The toolchain version pinned for `dir`, or an actionable error.
pub fn required_toolchain(language: &str, dir: &Path) -> Result<Version> {
    let Some(pin) = resolve_pin(language, dir)? else {
        bail!(
            "no {language} version pinned (run `linguo {language} use <version>` or `linguo {language} init`)"
        );
    };
    let req = pin_req(language, &pin)?;
    find_installed(language, &req)?.with_context(|| {
        format!(
            "{language} {req} is pinned but not installed (run `linguo {language} install {req}`)"
        )
    })
}

/// Version a new project should use: the existing pin if satisfiable,
/// otherwise the newest installed toolchain.
pub fn pick_project_version(language: &str, dir: &Path) -> Result<Version> {
    match resolve_pin(language, dir)? {
        Some(_) => required_toolchain(language, dir),
        None => installed_versions(language)?
            .last()
            .copied()
            .with_context(|| {
                format!("no {language} toolchains installed (run `linguo {language} install`)")
            }),
    }
}

/// The newest release's version at the same granularity as `req`:
/// `3.12` bumps to `3.13`, `24` to `26`, `1.96.0` to `1.96.1`.
pub fn granularity_bump(req: &VersionReq, newest: Version) -> VersionReq {
    match req {
        VersionReq::Major(_) => VersionReq::Major(newest.major),
        VersionReq::MajorMinor(..) => VersionReq::MajorMinor(newest.major, newest.minor),
        VersionReq::Exact(_) => VersionReq::Exact(newest),
    }
}

/// Replace the version line of a single-value pin file (`.nvmrc`,
/// `.python-version`, ...), preserving comments and other lines.
fn rewrite_version_file(path: &Path, value: &str) -> Result<()> {
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read {}", path.display()))?;
    let mut replaced = false;
    let mut lines: Vec<&str> = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if !replaced && !trimmed.is_empty() && !trimmed.starts_with('#') {
            lines.push(value);
            replaced = true;
        } else {
            lines.push(line);
        }
    }
    if !replaced {
        lines.push(value);
    }
    std::fs::write(path, format!("{}\n", lines.join("\n")))
        .with_context(|| format!("failed to write {}", path.display()))
}

/// Update `channel` in a rust-toolchain.toml, preserving everything else.
fn rewrite_rust_toolchain_toml(path: &Path, value: &str) -> Result<()> {
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read {}", path.display()))?;
    let mut doc: toml_edit::DocumentMut = text
        .parse()
        .with_context(|| format!("failed to parse {}", path.display()))?;
    doc["toolchain"]["channel"] = toml_edit::value(value);
    std::fs::write(path, doc.to_string())
        .with_context(|| format!("failed to write {}", path.display()))
}

/// Write a new value to wherever a pin came from. Single-line version files
/// and rust-toolchain(.toml) are rewritten in place; go.mod is owned by the
/// go tool and refused.
pub fn write_pin_back(language: &str, pin: &Pin, value: &str) -> Result<()> {
    match &pin.source {
        PinSource::Global => config::write_pin(
            &config::linguo_root()?.join(config::GLOBAL_CONFIG),
            language,
            value,
        ),
        PinSource::Project(path) => match path.file_name().and_then(|n| n.to_str()) {
            Some(config::PIN_FILE) => config::write_pin(path, language, value),
            Some(".python-version" | ".nvmrc" | ".node-version" | ".ruby-version")
            | Some("rust-toolchain") => rewrite_version_file(path, value),
            Some("rust-toolchain.toml") => rewrite_rust_toolchain_toml(path, value),
            Some("go.mod") => bail!(
                "the pin comes from go.mod, which the go tool owns; bump it there or run `linguo go use {value}`"
            ),
            _ => bail!("cannot update pin source {}", path.display()),
        },
    }
}

/// Uninstall installed versions older than `keep` that any of `reqs` match.
pub fn prune_older(language: &str, reqs: &[VersionReq], keep: Version) -> Result<()> {
    let stale: Vec<Version> = installed_versions(language)?
        .into_iter()
        .filter(|v| *v < keep && reqs.iter().any(|r| r.matches(v)))
        .collect();
    if stale.is_empty() {
        println!("nothing to prune");
        return Ok(());
    }
    for version in stale {
        uninstall(language, &version.to_string())?;
    }
    Ok(())
}

/// Upgrade the pinned toolchain: newest release within the pin, or — with
/// `latest` — bump the pin to the newest release at the same granularity.
/// `newest` is the version `--latest` targets (e.g. node's latest LTS).
pub fn upgrade(
    language: &str,
    available: &[Version],
    newest: Option<Version>,
    latest: bool,
    prune: bool,
    install: &dyn Fn(&str) -> Result<()>,
) -> Result<()> {
    let cwd = std::env::current_dir()?;
    let Some(pin) = resolve_pin(language, &cwd)? else {
        bail!("no {language} version pinned (run `linguo {language} use <version>`)");
    };
    let req = pin_req(language, &pin)?;

    let target_req = if latest {
        let newest = newest.context("no releases available for this platform")?;
        granularity_bump(&req, newest)
    } else {
        req
    };
    let target = target_req
        .best_match(available.iter().copied())
        .with_context(|| format!("no available release matches '{target_req}'"))?;

    if latest && target_req != req {
        write_pin_back(language, &pin, &target_req.to_string())?;
        let source = match &pin.source {
            PinSource::Project(path) => path.display().to_string(),
            PinSource::Global => "the global config".to_string(),
        };
        println!("bumped {language} pin {req} -> {target_req} in {source}");
    }

    if toolchain_path(language, &target)?.exists() {
        println!("{language} {target} is already installed and is the newest {target_req} release");
        if !latest && matches!(req, VersionReq::Exact(_)) {
            println!("note: the pin is exact; use `--latest` to bump it");
        }
    } else {
        install(&target.to_string())?;
    }

    if prune {
        prune_older(language, &[req, target_req], target)?;
    }
    Ok(())
}

pub fn uninstall(language: &str, raw: &str) -> Result<()> {
    let req: VersionReq = raw.parse()?;
    let version = match req {
        VersionReq::Exact(v) => v,
        _ => {
            let matches: Vec<Version> = installed_versions(language)?
                .into_iter()
                .filter(|v| req.matches(v))
                .collect();
            match matches.as_slice() {
                [] => bail!("no installed version matches '{raw}'"),
                [only] => *only,
                many => bail!(
                    "'{raw}' matches multiple installed versions ({}); specify one exactly",
                    many.iter()
                        .map(|v| v.to_string())
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
            }
        }
    };
    let path = toolchain_path(language, &version)?;
    if !path.exists() {
        bail!("{language} {version} is not installed");
    }
    std::fs::remove_dir_all(&path)
        .with_context(|| format!("failed to remove {}", path.display()))?;
    println!("uninstalled {language} {version}");
    Ok(())
}

pub fn list_installed(language: &str) -> Result<()> {
    let installed = installed_versions(language)?;
    if installed.is_empty() {
        println!("no {language} toolchains installed (try `linguo {language} install`)");
        return Ok(());
    }
    let cwd = std::env::current_dir()?;
    let active = resolve_active(language, &cwd)?;
    for version in installed {
        match &active {
            Some((pin, active_version)) if *active_version == version => {
                let source = match &pin.source {
                    PinSource::Project(path) => format!("pinned by {}", path.display()),
                    PinSource::Global => "global default".to_string(),
                };
                println!("{version} * ({source})");
            }
            _ => println!("{version}"),
        }
    }
    Ok(())
}

pub fn use_version(language: &str, raw: &str, global: bool) -> Result<()> {
    let req: VersionReq = raw.parse()?;
    let path = if global {
        config::linguo_root()?.join(config::GLOBAL_CONFIG)
    } else {
        std::env::current_dir()?.join(config::PIN_FILE)
    };
    config::write_pin(&path, language, &req.to_string())?;
    println!("pinned {language} to {req} in {}", path.display());
    if find_installed(language, &req)?.is_none() {
        println!("note: no installed toolchain matches; run `linguo {language} install {req}`");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(dir: &Path, name: &str, content: &str) {
        std::fs::write(dir.join(name), content).unwrap();
    }

    #[test]
    fn granularity_bump_preserves_pin_style() {
        let newest: Version = "3.13.2".parse().unwrap();
        let bump = |s: &str| granularity_bump(&s.parse().unwrap(), newest).to_string();
        assert_eq!(bump("3"), "3");
        assert_eq!(bump("2"), "3");
        assert_eq!(bump("3.12"), "3.13");
        assert_eq!(bump("3.12.4"), "3.13.2");
    }

    #[test]
    fn version_file_rewrite_preserves_comments() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join(".nvmrc");
        std::fs::write(&path, "# pinned for CI\n22\n").unwrap();
        rewrite_version_file(&path, "24").unwrap();
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "# pinned for CI\n24\n"
        );
    }

    #[test]
    fn rust_toolchain_toml_rewrite_preserves_structure() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("rust-toolchain.toml");
        std::fs::write(
            &path,
            "[toolchain]\nchannel = \"1.96\"\ncomponents = [\"rustfmt\"]\n",
        )
        .unwrap();
        rewrite_rust_toolchain_toml(&path, "1.97").unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("channel = \"1.97\""));
        assert!(text.contains("components = [\"rustfmt\"]"));
    }

    /// One test so the LINGUO_ROOT override can't race across threads.
    #[test]
    fn fallback_pin_files_and_precedence() {
        let root = tempfile::tempdir().unwrap();
        unsafe { std::env::set_var("LINGUO_ROOT", root.path()) };

        let project = tempfile::tempdir().unwrap();
        let dir = project.path();

        // Ecosystem pin files, per language.
        write(dir, ".python-version", "3.12.4\n");
        assert_eq!(resolve_pin("python", dir).unwrap().unwrap().raw, "3.12.4");

        write(dir, ".nvmrc", "v22.1.0\n");
        assert_eq!(resolve_pin("node", dir).unwrap().unwrap().raw, "22.1.0");
        write(dir, ".nvmrc", "lts/jod\n");
        assert!(resolve_pin("node", dir).unwrap().is_none());
        std::fs::remove_file(dir.join(".nvmrc")).unwrap();
        write(dir, ".node-version", "20.11\n");
        assert_eq!(resolve_pin("node", dir).unwrap().unwrap().raw, "20.11");

        write(dir, ".ruby-version", "ruby-3.4.2\n");
        assert_eq!(resolve_pin("ruby", dir).unwrap().unwrap().raw, "3.4.2");

        write(
            dir,
            "go.mod",
            "module demo\n\ngo 1.24\n\ntoolchain go1.25.4\n",
        );
        assert_eq!(resolve_pin("go", dir).unwrap().unwrap().raw, "1.25.4");
        write(dir, "go.mod", "module demo\n\ngo 1.24\n");
        assert_eq!(resolve_pin("go", dir).unwrap().unwrap().raw, "1.24");

        write(
            dir,
            "rust-toolchain.toml",
            "[toolchain]\nchannel = \"1.96.1\"\n",
        );
        assert_eq!(resolve_pin("rust", dir).unwrap().unwrap().raw, "1.96.1");
        write(
            dir,
            "rust-toolchain.toml",
            "[toolchain]\nchannel = \"stable\"\n",
        );
        assert!(resolve_pin("rust", dir).unwrap().is_none());

        // Fallback files are found from subdirectories too.
        let nested = dir.join("a/b");
        std::fs::create_dir_all(&nested).unwrap();
        assert_eq!(
            resolve_pin("python", &nested).unwrap().unwrap().raw,
            "3.12.4"
        );

        // Project linguo.toml beats the ecosystem file...
        write(dir, "linguo.toml", "[runtimes]\npython = \"3.13\"\n");
        assert_eq!(resolve_pin("python", dir).unwrap().unwrap().raw, "3.13");

        // ...and the ecosystem file beats the global config.
        write(root.path(), "config.toml", "[runtimes]\nruby = \"3.3\"\n");
        assert_eq!(resolve_pin("ruby", dir).unwrap().unwrap().raw, "3.4.2");
        std::fs::remove_file(dir.join(".ruby-version")).unwrap();
        assert_eq!(resolve_pin("ruby", dir).unwrap().unwrap().raw, "3.3");

        unsafe { std::env::remove_var("LINGUO_ROOT") };
    }
}
