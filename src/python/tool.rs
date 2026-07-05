//! Python developer-tool management: pipx-style isolated installs. Each tool
//! gets its own venv (built with the newest installed python), and the tool's
//! executables are copied into the store's `exposed` dir so the shell hook can
//! put just those on PATH without leaking the venv's own python/pip.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};

use crate::versions::{Version, VersionReq};
use crate::{config, store, tools};

const LANGUAGE: &str = super::LANGUAGE;

/// Split a `name[@versionreq]` spec, e.g. `ruff` or `ruff@0.6`.
fn parse_spec(spec: &str) -> Result<(String, Option<VersionReq>)> {
    match spec.split_once('@') {
        Some((name, req)) => {
            let req: VersionReq = req
                .parse()
                .with_context(|| format!("invalid version requirement in '{spec}'"))?;
            Ok((name.to_string(), Some(req)))
        }
        None => Ok((spec.to_string(), None)),
    }
}

/// Translate a linguo version request into a pip requirement specifier.
fn pip_spec(name: &str, req: &Option<VersionReq>) -> String {
    match req {
        None => name.to_string(),
        Some(VersionReq::Major(m)) => format!("{name}=={m}.*"),
        Some(VersionReq::MajorMinor(m, n)) => format!("{name}=={m}.{n}.*"),
        Some(VersionReq::Exact(v)) => format!("{name}=={v}"),
    }
}

fn venv_bin(venv: &Path) -> PathBuf {
    venv.join(if cfg!(windows) { "Scripts" } else { "bin" })
}

fn dir_files(dir: &Path) -> HashSet<String> {
    std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_file())
        .filter_map(|e| e.file_name().into_string().ok())
        .collect()
}

fn run_checked(cmd: &mut Command, what: &str) -> Result<()> {
    let status = cmd
        .status()
        .with_context(|| format!("failed to run {what}"))?;
    if !status.success() {
        bail!("{what} failed");
    }
    Ok(())
}

/// The python interpreter to build tool venvs with: the newest installed
/// toolchain (tools shouldn't ride a project's pinned version).
fn tool_python() -> Result<PathBuf> {
    let version = store::installed_versions(LANGUAGE)?
        .last()
        .copied()
        .context("no python toolchains installed (run `linguo python install` first)")?;
    Ok(super::dist::bin_dir(&super::toolchain_path(&version)?).join(super::dist::python_exe()))
}

/// Install `name` at `req` into an isolated venv and expose its executables.
/// Returns the resolved version. Skips the work if it's already installed.
fn install_tool(name: &str, req: &Option<VersionReq>) -> Result<Version> {
    let python = tool_python()?;
    let base = tools::tool_dir(LANGUAGE, name)?;
    std::fs::create_dir_all(&base)
        .with_context(|| format!("failed to create {}", base.display()))?;

    let staging = base.join(".staging");
    if staging.exists() {
        std::fs::remove_dir_all(&staging)?;
    }
    run_checked(
        Command::new(&python).args(["-m", "venv"]).arg(&staging),
        "python -m venv",
    )?;

    let bin = venv_bin(&staging);
    let before = dir_files(&bin);
    let staged_python = bin.join(super::dist::python_exe());
    run_checked(
        Command::new(&staged_python)
            .args(["-m", "pip", "install", "--disable-pip-version-check"])
            .arg(pip_spec(name, req)),
        "pip install",
    )?;

    let resolved = installed_version(&staged_python, name)?;
    let dest = tools::version_dir(LANGUAGE, name, &resolved)?;
    if dest.exists() {
        std::fs::remove_dir_all(&staging)?;
        eprintln!("python tool {name} {resolved} is already installed");
        return Ok(resolved);
    }
    std::fs::rename(&staging, &dest)
        .with_context(|| format!("failed to move tool venv into {}", dest.display()))?;

    expose_scripts(
        &venv_bin(&dest),
        &before,
        &tools::exposed_dir(LANGUAGE, name, &resolved)?,
    )?;
    eprintln!("installed python tool {name} {resolved}");
    Ok(resolved)
}

/// Read the installed version of `name` from the venv via `pip show`.
fn installed_version(python: &Path, name: &str) -> Result<Version> {
    let output = Command::new(python)
        .args(["-m", "pip", "show", name])
        .output()
        .context("failed to run pip show")?;
    if !output.status.success() {
        bail!("pip show {name} failed");
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let raw = text
        .lines()
        .find_map(|l| l.strip_prefix("Version:"))
        .map(str::trim)
        .with_context(|| format!("could not determine installed version of {name}"))?;
    raw.parse()
        .with_context(|| format!("unexpected version '{raw}' for {name}"))
}

/// Copy the scripts a tool added to its venv bin (everything not present
/// before the install) into the exposed dir the hook puts on PATH.
fn expose_scripts(bin: &Path, before: &HashSet<String>, exposed: &Path) -> Result<()> {
    std::fs::create_dir_all(exposed)
        .with_context(|| format!("failed to create {}", exposed.display()))?;
    let mut count = 0;
    for entry in std::fs::read_dir(bin)?.filter_map(|e| e.ok()) {
        if !entry.path().is_file() {
            continue;
        }
        let name = entry.file_name();
        let Some(name_str) = name.to_str() else {
            continue;
        };
        if before.contains(name_str) {
            continue;
        }
        std::fs::copy(entry.path(), exposed.join(&name))
            .with_context(|| format!("failed to expose {name_str}"))?;
        count += 1;
    }
    if count == 0 {
        bail!("the package exposed no executables");
    }
    Ok(())
}

/// `linguo python tool install <spec>`: install and pin the tool globally, or
/// in this project's linguo.toml with `--project`.
pub fn install(spec: &str, project: bool) -> Result<()> {
    let (name, req) = parse_spec(spec)?;
    let resolved = install_tool(&name, &req)?;
    let pin_raw = match &req {
        Some(req) => req.to_string(),
        None => format!("{}.{}", resolved.major, resolved.minor),
    };
    let path = if project {
        std::env::current_dir()?.join(config::PIN_FILE)
    } else {
        config::linguo_root()?.join(config::GLOBAL_CONFIG)
    };
    config::write_tool_pin(&path, LANGUAGE, &name, &pin_raw)?;
    println!(
        "pinned python tool {name} to {pin_raw} in {}",
        path.display()
    );
    Ok(())
}

/// `linguo python tool upgrade [name] [--latest]`: reinstall pinned tools to
/// the newest release within their pin, or to the newest overall with
/// `--latest` (which also rewrites the pin's granularity).
pub fn upgrade(name: Option<String>, latest: bool) -> Result<()> {
    let cwd = std::env::current_dir()?;
    let pins = config::tool_pins(LANGUAGE, &cwd)?;
    let targets: Vec<config::ToolPin> = match &name {
        Some(name) => vec![
            pins.into_iter()
                .find(|p| &p.tool == name)
                .with_context(|| format!("python tool '{name}' is not pinned"))?,
        ],
        None => pins,
    };
    if targets.is_empty() {
        println!("no python tools pinned here (run `linguo python tool install <name>`)");
        return Ok(());
    }
    for pin in targets {
        let req: Option<VersionReq> = if latest {
            None
        } else {
            Some(pin.raw.parse().with_context(|| {
                format!("invalid version '{}' pinned for {}", pin.raw, pin.tool)
            })?)
        };
        let resolved = install_tool(&pin.tool, &req)?;
        if latest {
            let pin_raw = format!("{}.{}", resolved.major, resolved.minor);
            if pin_raw != pin.raw {
                let path = pin_source_path(&pin)?;
                config::write_tool_pin(&path, LANGUAGE, &pin.tool, &pin_raw)?;
                println!("bumped {} pin {} -> {pin_raw}", pin.tool, pin.raw);
            }
        }
    }
    Ok(())
}

fn pin_source_path(pin: &config::ToolPin) -> Result<PathBuf> {
    match &pin.source {
        config::PinSource::Project(path) => Ok(path.clone()),
        config::PinSource::Global => Ok(config::linguo_root()?.join(config::GLOBAL_CONFIG)),
    }
}

/// Install any tools a project declares in `[tools.python]` but hasn't got
/// installed yet. Used by workspace sync.
pub fn sync_in(dir: &Path) -> Result<()> {
    let mut installed = 0;
    for pin in config::tool_pins(LANGUAGE, dir)? {
        let req: VersionReq = pin
            .raw
            .parse()
            .with_context(|| format!("invalid version '{}' pinned for {}", pin.raw, pin.tool))?;
        if tools::find_installed(LANGUAGE, &pin.tool, &req)?.is_none() {
            install_tool(&pin.tool, &Some(req))?;
            installed += 1;
        }
    }
    if installed > 0 {
        println!("installed {installed} declared tool(s)");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_specs() {
        let (name, req) = parse_spec("ruff").unwrap();
        assert_eq!(name, "ruff");
        assert!(req.is_none());
        let (name, req) = parse_spec("ruff@0.6").unwrap();
        assert_eq!(name, "ruff");
        assert_eq!(req.unwrap().to_string(), "0.6");
    }

    #[test]
    fn pip_specs_translate_reqs() {
        assert_eq!(pip_spec("ruff", &None), "ruff");
        assert_eq!(
            pip_spec("ruff", &Some("0.6".parse().unwrap())),
            "ruff==0.6.*"
        );
        assert_eq!(
            pip_spec("ruff", &Some("0.6.9".parse().unwrap())),
            "ruff==0.6.9"
        );
        assert_eq!(
            pip_spec("black", &Some("25".parse().unwrap())),
            "black==25.*"
        );
    }
}
