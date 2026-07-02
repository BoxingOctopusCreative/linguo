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
