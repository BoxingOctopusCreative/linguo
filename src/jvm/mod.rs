pub mod dist;

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

use crate::config::Pin;
use crate::store;
use crate::versions::{Version, VersionReq};

pub const LANGUAGE: &str = "jvm";

pub fn toolchain_path(version: &Version) -> Result<PathBuf> {
    store::toolchain_path(LANGUAGE, version)
}

pub fn upgrade(latest: bool, prune: bool) -> Result<()> {
    let builds = dist::fetch_available()?;
    let available: Vec<Version> = builds.iter().map(|b| b.version).collect();
    // --latest targets the newest LTS line, the sensible default for JDKs.
    let newest = builds
        .iter()
        .rev()
        .find(|b| b.lts)
        .or(builds.last())
        .map(|b| b.version);
    store::upgrade(LANGUAGE, &available, newest, latest, prune, &|v| {
        install(Some(v.to_string()))
    })
}

/// jenv convention: the nearest `.java-version` holding a plain version.
pub fn fallback_pin(cwd: &Path) -> Result<Option<Pin>> {
    for dir in cwd.ancestors() {
        let path = dir.join(".java-version");
        if let Some(raw) = store::read_version_file(&path)? {
            return Ok(store::file_pin(&raw, &path));
        }
    }
    Ok(None)
}

pub fn install(request: Option<String>) -> Result<()> {
    let builds = dist::fetch_available()?;
    if builds.is_empty() {
        bail!("no builds available for this platform");
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
        // Default to the newest LTS, like most of the Java ecosystem.
        None => builds
            .iter()
            .rev()
            .find(|b| b.lts)
            .unwrap_or(builds.last().unwrap()),
    };

    let dest = toolchain_path(&build.version)?;
    if dest.exists() {
        eprintln!("jvm {} is already installed", build.version);
        return Ok(());
    }
    std::fs::create_dir_all(dest.parent().unwrap())
        .with_context(|| format!("failed to create {}", dest.parent().unwrap().display()))?;

    dist::install_build(build, &dest)?;
    eprintln!(
        "installed jvm {} (Temurin {}) to {}",
        build.version,
        build.feature,
        dest.display()
    );
    Ok(())
}

pub fn list(available: bool) -> Result<()> {
    if !available {
        return store::list_installed(LANGUAGE);
    }
    let builds = dist::fetch_available()?;
    if builds.is_empty() {
        println!("no builds available for this platform");
        return Ok(());
    }
    let installed = store::installed_versions(LANGUAGE)?;
    for build in builds {
        let lts = if build.lts { " (LTS)" } else { "" };
        let marker = if installed.contains(&build.version) {
            " (installed)"
        } else {
            ""
        };
        println!("{}{lts}{marker}", build.version);
    }
    println!("(latest Temurin build per feature release)");
    Ok(())
}

/// The JAVA_HOME a JVM-based `language` should use in `dir`: its `[jvm]`
/// binding when one is set, else the directory's plain jvm pin.
pub fn resolve_for(language: &str, dir: &Path) -> Result<(Version, PathBuf)> {
    let req: VersionReq = match crate::config::jvm_binding(language, dir)? {
        Some(raw) => raw
            .parse()
            .with_context(|| format!("invalid jvm binding '{raw}' for {language}"))?,
        None => match store::resolve_pin(LANGUAGE, dir)? {
            Some(pin) => pin
                .raw
                .parse()
                .with_context(|| format!("invalid jvm version '{}' pinned", pin.raw))?,
            None => bail!(
                "no jvm configured for {language}: run `linguo {language} set-jvm <version>` or pin one (`linguo jvm use <version>`)"
            ),
        },
    };
    let version = store::find_installed(LANGUAGE, &req)?
        .with_context(|| format!("jvm {req} is not installed (run `linguo jvm install {req}`)"))?;
    let home = dist::java_home(&toolchain_path(&version)?);
    Ok((version, home))
}

/// Print the path of the executable a command resolves to (default: java).
pub fn which(command: Option<String>) -> Result<()> {
    let cwd = std::env::current_dir()?;
    let version = store::required_toolchain(LANGUAGE, &cwd)?;
    let bin = dist::bin_dir(&toolchain_path(&version)?);
    let name = command.unwrap_or_else(|| "java".to_string());
    if let Some(path) = crate::exec::find_in_dir(&bin, &name) {
        println!("{}", path.display());
        return Ok(());
    }
    bail!("'{name}' not found in the pinned jvm");
}

/// Run a command with the pinned JDK on PATH and JAVA_HOME set.
pub fn run(args: &[String]) -> Result<()> {
    let (program, rest) = args.split_first().context("no command given")?;
    let cwd = std::env::current_dir()?;
    let version = store::required_toolchain(LANGUAGE, &cwd)?;
    let toolchain = toolchain_path(&version)?;
    let bin = dist::bin_dir(&toolchain);

    let current = std::env::var_os("PATH").unwrap_or_default();
    let path =
        std::env::join_paths(std::iter::once(bin.clone()).chain(std::env::split_paths(&current)))
            .context("invalid PATH entry")?;

    let mut cmd = crate::exec::command_in(std::slice::from_ref(&bin), program);
    cmd.args(rest)
        .env("PATH", path)
        .env("JAVA_HOME", dist::java_home(&toolchain));
    crate::exec::exec(cmd, program)
}
