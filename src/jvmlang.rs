//! Shared machinery for JVM-based languages (Kotlin, Groovy, Scala): each is
//! a runtime-only toolchain whose run/which put the language's bin AND its
//! bound JDK on PATH with JAVA_HOME set. `set-jvm` binds a specific JVM per
//! language; without a binding, the directory's plain jvm pin applies.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

use crate::versions::{Version, VersionReq};
use crate::{config, jvm, store};

pub struct Build {
    pub version: Version,
    pub url: String,
    /// sha256 known up front (e.g. GitHub asset digests).
    pub sha256: Option<String>,
    /// URL of a checksum sidecar to fetch at install time (Apache-style).
    pub sha256_url: Option<String>,
    /// Top-level directory inside the archive holding the distribution.
    pub subdir: String,
    pub asset_name: String,
}

/// A JVM language definition: everything the shared commands need.
pub struct Def {
    pub language: &'static str,
    /// Default binary for `which` (e.g. `scala`).
    pub default_bin: &'static str,
    pub fetch_available: fn() -> Result<Vec<Build>>,
}

pub fn toolchain_bin(def: &Def, version: &Version) -> Result<PathBuf> {
    Ok(store::toolchain_path(def.language, version)?.join("bin"))
}

pub fn install(def: &Def, request: Option<String>) -> Result<()> {
    let builds = (def.fetch_available)()?;
    if builds.is_empty() {
        bail!("no builds available");
    }
    let build = match &request {
        Some(raw) => {
            let req: VersionReq = raw.parse()?;
            builds
                .iter()
                .rev()
                .find(|b| req.matches(&b.version))
                .with_context(|| format!("no available build matches '{raw}'"))?
        }
        None => builds.last().unwrap(),
    };

    let dest = store::toolchain_path(def.language, &build.version)?;
    if dest.exists() {
        eprintln!("{} {} is already installed", def.language, build.version);
        return Ok(());
    }
    std::fs::create_dir_all(dest.parent().unwrap())
        .with_context(|| format!("failed to create {}", dest.parent().unwrap().display()))?;

    let http = crate::fetch::client()?;
    let expected = match (&build.sha256, &build.sha256_url) {
        (Some(sha), _) => Some(sha.clone()),
        (None, Some(url)) => {
            let text = http
                .get(url)
                .send()
                .and_then(|r| r.error_for_status())
                .with_context(|| format!("failed to fetch checksum from {url}"))?
                .text()?;
            Some(
                text.split_whitespace()
                    .next()
                    .context("empty checksum file")?
                    .to_ascii_lowercase(),
            )
        }
        (None, None) => None,
    };

    eprintln!("downloading {}", build.url);
    let archive = crate::fetch::download(&http, &build.url)?;
    match expected {
        Some(expected) => crate::fetch::verify_sha256(&archive, &expected, &build.asset_name)?,
        None => eprintln!("warning: no published checksum for this build; skipping verification"),
    }
    crate::fetch::extract_archive_subdir(&archive, &build.asset_name, &build.subdir, &dest)?;
    eprintln!(
        "installed {} {} to {}",
        def.language,
        build.version,
        dest.display()
    );
    Ok(())
}

pub fn list(def: &Def, available: bool) -> Result<()> {
    if !available {
        return store::list_installed(def.language);
    }
    let builds = (def.fetch_available)()?;
    if builds.is_empty() {
        println!("no builds available");
        return Ok(());
    }
    let installed = store::installed_versions(def.language)?;
    // Show the latest release per minor line.
    let mut previous: Option<Version> = None;
    let mut latest_per_minor: Vec<Version> = Vec::new();
    for build in &builds {
        if let Some(prev) = previous
            && (prev.major, prev.minor) != (build.version.major, build.version.minor)
        {
            latest_per_minor.push(prev);
        }
        previous = Some(build.version);
    }
    latest_per_minor.extend(previous);
    for version in latest_per_minor {
        let marker = if installed.contains(&version) {
            " (installed)"
        } else {
            ""
        };
        println!("{version}{marker}");
    }
    println!("(latest release per minor line; any exact version can be installed)");
    Ok(())
}

pub fn upgrade(def: &'static Def, latest: bool, prune: bool) -> Result<()> {
    let available: Vec<Version> = (def.fetch_available)()?.iter().map(|b| b.version).collect();
    let newest = available.last().copied();
    store::upgrade(def.language, &available, newest, latest, prune, &|v| {
        install(def, Some(v.to_string()))
    })
}

/// Bind the JVM this language uses (writes `[jvm] <language> = "<req>"`).
pub fn set_jvm(def: &Def, raw: &str, global: bool) -> Result<()> {
    let req: VersionReq = raw.parse()?;
    let path = if global {
        config::linguo_root()?.join(config::GLOBAL_CONFIG)
    } else {
        std::env::current_dir()?.join(config::PIN_FILE)
    };
    config::write_jvm_binding(&path, def.language, &req.to_string())?;
    println!("bound {} to jvm {req} in {}", def.language, path.display());
    if store::find_installed(jvm::LANGUAGE, &req)?.is_none() {
        println!("note: no installed jvm matches; run `linguo jvm install {req}`");
    }
    Ok(())
}

/// The language's bin plus its resolved JDK bin, for PATH assembly.
pub fn managed_dirs(def: &Def, dir: &Path) -> Result<(Vec<PathBuf>, PathBuf)> {
    let version = store::required_toolchain(def.language, dir)?;
    let lang_bin = toolchain_bin(def, &version)?;
    let (_, java_home) = jvm::resolve_for(def.language, dir)?;
    Ok((vec![lang_bin, java_home.join("bin")], java_home))
}

/// Print the path of the executable a command resolves to.
pub fn which(def: &Def, command: Option<String>) -> Result<()> {
    let cwd = std::env::current_dir()?;
    let (dirs, _) = managed_dirs(def, &cwd)?;
    let name = command.unwrap_or_else(|| def.default_bin.to_string());
    for dir in &dirs {
        if let Some(path) = crate::exec::find_in_dir(dir, &name) {
            println!("{}", path.display());
            return Ok(());
        }
    }
    bail!(
        "'{name}' not found in the pinned {} toolchain or its jvm",
        def.language
    );
}

/// Run a command with the language and its JDK on PATH and JAVA_HOME set.
pub fn run(def: &Def, args: &[String]) -> Result<()> {
    let (program, rest) = args.split_first().context("no command given")?;
    let cwd = std::env::current_dir()?;
    let (dirs, java_home) = managed_dirs(def, &cwd)?;

    let current = std::env::var_os("PATH").unwrap_or_default();
    let path = std::env::join_paths(dirs.iter().cloned().chain(std::env::split_paths(&current)))
        .context("invalid PATH entry")?;

    let mut cmd = crate::exec::command_in(&dirs, program);
    cmd.args(rest).env("PATH", path).env("JAVA_HOME", java_home);
    crate::exec::exec(cmd, program)
}
