pub mod dist;

use std::fmt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::str::FromStr;

use anyhow::{Context, Result, bail};

use crate::config::{self, PinSource};
use crate::store;
use crate::versions::{Version, VersionReq};

pub const LANGUAGE: &str = "terraform";

/// Which engine a version refers to. Both live under the `terraform` pin key
/// and CLI command, but are stored in separate toolchain namespaces because
/// their version numbers overlap.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Distribution {
    Terraform,
    OpenTofu,
}

impl Distribution {
    fn store_key(self) -> &'static str {
        match self {
            Distribution::Terraform => "terraform",
            Distribution::OpenTofu => "opentofu",
        }
    }

    /// The executable the distribution ships.
    fn binary(self) -> &'static str {
        match self {
            Distribution::Terraform => "terraform",
            Distribution::OpenTofu => "tofu",
        }
    }
}

/// A version spec as written by the user or in a pin: `1.13`,
/// `opentofu@1.10`, or a bare distribution name meaning "any version".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Spec {
    pub dist: Distribution,
    pub req: Option<VersionReq>,
}

impl FromStr for Spec {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self> {
        let parse_dist = |name: &str| match name {
            "terraform" => Ok(Distribution::Terraform),
            "opentofu" | "tofu" => Ok(Distribution::OpenTofu),
            other => bail!("unknown distribution '{other}' (expected terraform or opentofu)"),
        };
        if let Some((dist, version)) = s.split_once('@') {
            return Ok(Spec {
                dist: parse_dist(dist)?,
                req: Some(version.parse()?),
            });
        }
        if let Ok(dist) = parse_dist(s) {
            return Ok(Spec { dist, req: None });
        }
        Ok(Spec {
            dist: Distribution::Terraform,
            req: Some(s.parse()?),
        })
    }
}

impl fmt::Display for Spec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match (self.dist, &self.req) {
            (Distribution::Terraform, Some(req)) => write!(f, "{req}"),
            (Distribution::Terraform, None) => write!(f, "terraform"),
            (Distribution::OpenTofu, Some(req)) => write!(f, "opentofu@{req}"),
            (Distribution::OpenTofu, None) => write!(f, "opentofu"),
        }
    }
}

/// Installed-version display: plain for terraform, prefixed for opentofu.
fn display_version(dist: Distribution, version: &Version) -> String {
    match dist {
        Distribution::Terraform => version.to_string(),
        Distribution::OpenTofu => format!("opentofu@{version}"),
    }
}

pub fn toolchain_path(dist: Distribution, version: &Version) -> Result<PathBuf> {
    store::toolchain_path(dist.store_key(), version)
}

fn find_installed(spec: &Spec) -> Result<Option<Version>> {
    let installed = store::installed_versions(spec.dist.store_key())?;
    Ok(match &spec.req {
        Some(req) => req.best_match(installed),
        None => installed.last().copied(),
    })
}

fn pin_spec(pin: &config::Pin) -> Result<Spec> {
    pin.raw
        .parse()
        .with_context(|| format!("invalid terraform pin '{}'", pin.raw))
}

/// Resolve the active toolchain for `cwd`: pin -> distribution + version.
pub fn resolve_active(cwd: &Path) -> Result<Option<(config::Pin, Distribution, Version)>> {
    let Some(pin) = config::resolve_pin(LANGUAGE, cwd)? else {
        return Ok(None);
    };
    let spec = pin_spec(&pin)?;
    Ok(find_installed(&spec)?.map(|version| (pin, spec.dist, version)))
}

/// The pinned toolchain for `dir`, or an actionable error.
fn required_toolchain(dir: &Path) -> Result<(Distribution, Version)> {
    let Some(pin) = config::resolve_pin(LANGUAGE, dir)? else {
        bail!("no terraform version pinned (run `linguo terraform use <version>`)");
    };
    let spec = pin_spec(&pin)?;
    let version = find_installed(&spec)?.with_context(|| {
        format!(
            "terraform pin '{}' is not installed (run `linguo terraform install {}`)",
            pin.raw, pin.raw
        )
    })?;
    Ok((spec.dist, version))
}

pub fn install(request: Option<String>) -> Result<()> {
    let spec: Spec = match &request {
        Some(raw) => raw.parse()?,
        None => Spec {
            dist: Distribution::Terraform,
            req: None,
        },
    };
    let builds = dist::fetch_available(spec.dist)?;
    if builds.is_empty() {
        bail!("no builds available for this platform");
    }

    let build = match &spec.req {
        Some(req) => builds
            .iter()
            .rev()
            .find(|b| req.matches(&b.version))
            .with_context(|| format!("no available build matches '{}'", request.unwrap()))?,
        None => builds.last().unwrap(),
    };

    let dest = toolchain_path(spec.dist, &build.version)?;
    if dest.exists() {
        println!(
            "{} is already installed",
            display_version(spec.dist, &build.version)
        );
        return Ok(());
    }
    std::fs::create_dir_all(dest.parent().unwrap())
        .with_context(|| format!("failed to create {}", dest.parent().unwrap().display()))?;

    dist::install_build(build, &dest)?;
    println!(
        "installed {} to {}",
        display_version(spec.dist, &build.version),
        dest.display()
    );
    Ok(())
}

pub fn uninstall(raw: &str) -> Result<()> {
    let spec: Spec = raw.parse()?;
    let Some(req) = spec.req else {
        bail!("specify a version to uninstall, e.g. 1.13.5 or opentofu@1.10.6");
    };
    store::uninstall(spec.dist.store_key(), &req.to_string())
}

pub fn use_version(raw: &str, global: bool) -> Result<()> {
    let spec: Spec = raw.parse()?;
    if spec.req.is_none() {
        bail!("specify a version to pin, e.g. 1.13 or opentofu@1.10");
    }
    let path = if global {
        config::linguo_root()?.join(config::GLOBAL_CONFIG)
    } else {
        std::env::current_dir()?.join(config::PIN_FILE)
    };
    let normalized = spec.to_string();
    config::write_pin(&path, LANGUAGE, &normalized)?;
    println!("pinned terraform to {normalized} in {}", path.display());
    if find_installed(&spec)?.is_none() {
        println!(
            "note: no installed toolchain matches; run `linguo terraform install {normalized}`"
        );
    }
    Ok(())
}

/// All installed toolchains across both distributions, ascending per
/// distribution (terraform first).
fn installed() -> Result<Vec<(Distribution, Version)>> {
    let mut all = Vec::new();
    for dist in [Distribution::Terraform, Distribution::OpenTofu] {
        for version in store::installed_versions(dist.store_key())? {
            all.push((dist, version));
        }
    }
    Ok(all)
}

pub fn list(available: bool) -> Result<()> {
    if available {
        return list_available();
    }
    let installed = installed()?;
    if installed.is_empty() {
        println!("no terraform toolchains installed (try `linguo terraform install`)");
        return Ok(());
    }
    let cwd = std::env::current_dir()?;
    let active = resolve_active(&cwd)?;
    for (dist, version) in installed {
        match &active {
            Some((pin, active_dist, active_version))
                if *active_dist == dist && *active_version == version =>
            {
                let source = match &pin.source {
                    PinSource::Project(path) => format!("pinned by {}", path.display()),
                    PinSource::Global => "global default".to_string(),
                };
                println!("{} * ({source})", display_version(dist, &version));
            }
            _ => println!("{}", display_version(dist, &version)),
        }
    }
    Ok(())
}

fn list_available() -> Result<()> {
    let installed = installed()?;
    // Both indexes go back many years; show the latest release per minor line.
    for dist in [Distribution::Terraform, Distribution::OpenTofu] {
        let builds = dist::fetch_available(dist)?;
        let mut previous: Option<&dist::AvailableBuild> = None;
        let mut latest_per_minor: Vec<&dist::AvailableBuild> = Vec::new();
        for build in &builds {
            if let Some(prev) = previous
                && (prev.version.major, prev.version.minor)
                    != (build.version.major, build.version.minor)
            {
                latest_per_minor.push(prev);
            }
            previous = Some(build);
        }
        latest_per_minor.extend(previous);
        for build in latest_per_minor {
            let marker = if installed.contains(&(dist, build.version)) {
                " (installed)"
            } else {
                ""
            };
            println!("{}{marker}", display_version(dist, &build.version));
        }
    }
    println!("(latest release per minor line; any exact version can be installed)");
    Ok(())
}

/// Status lines for `linguo status`, matching the generic language format.
pub fn print_status(cwd: &Path) -> Result<()> {
    println!("{LANGUAGE}");
    let installed = installed()?;
    let toolchains = if installed.is_empty() {
        "(none)".to_string()
    } else {
        installed
            .iter()
            .map(|(dist, v)| display_version(*dist, v))
            .collect::<Vec<_>>()
            .join(", ")
    };
    println!("  toolchains: {toolchains}");

    match config::resolve_pin(LANGUAGE, cwd)? {
        None => println!("  active: none (no version pinned)"),
        Some(pin) => {
            let source = match &pin.source {
                PinSource::Project(path) => path.display().to_string(),
                PinSource::Global => "global config".to_string(),
            };
            let spec = pin_spec(&pin)?;
            match find_installed(&spec)? {
                Some(version) => println!(
                    "  active: {} (pinned to {} by {source})",
                    display_version(spec.dist, &version),
                    pin.raw
                ),
                None => println!(
                    "  active: none ({} pinned by {source} but not installed — run `linguo terraform install {}`)",
                    pin.raw, pin.raw
                ),
            }
        }
    }
    Ok(())
}

fn toolchain_bin(dir: &Path) -> Result<(Distribution, PathBuf)> {
    let (dist, version) = required_toolchain(dir)?;
    Ok((dist, dist::bin_dir(&toolchain_path(dist, &version)?)))
}

/// Print the path of the executable a command resolves to (default: the
/// active distribution's binary — terraform or tofu).
pub fn which(command: Option<String>) -> Result<()> {
    let cwd = std::env::current_dir()?;
    let (dist, bin) = toolchain_bin(&cwd)?;
    let name = command.unwrap_or_else(|| dist.binary().to_string());
    let path = bin.join(&name);
    if crate::exec::is_executable(&path) {
        println!("{}", path.display());
        return Ok(());
    }
    bail!("'{name}' not found in the pinned toolchain");
}

/// Run a command with the pinned toolchain on PATH.
pub fn run(args: &[String]) -> Result<()> {
    let (program, rest) = args.split_first().context("no command given")?;
    let cwd = std::env::current_dir()?;
    let (_, bin) = toolchain_bin(&cwd)?;

    let current = std::env::var_os("PATH").unwrap_or_default();
    let path = std::env::join_paths(std::iter::once(bin).chain(std::env::split_paths(&current)))
        .context("invalid PATH entry")?;

    let mut cmd = Command::new(program);
    cmd.args(rest).env("PATH", path);
    crate::exec::exec(cmd, program)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(s: &str) -> Spec {
        s.parse().unwrap()
    }

    #[test]
    fn parses_specs() {
        assert_eq!(
            spec("1.13"),
            Spec {
                dist: Distribution::Terraform,
                req: Some("1.13".parse().unwrap())
            }
        );
        assert_eq!(
            spec("opentofu@1.10"),
            Spec {
                dist: Distribution::OpenTofu,
                req: Some("1.10".parse().unwrap())
            }
        );
        assert_eq!(spec("tofu@1.10"), spec("opentofu@1.10"));
        assert_eq!(
            spec("terraform@1.13.5"),
            Spec {
                dist: Distribution::Terraform,
                req: Some("1.13.5".parse().unwrap())
            }
        );
        assert_eq!(
            spec("opentofu"),
            Spec {
                dist: Distribution::OpenTofu,
                req: None
            }
        );
        assert!("nomad@1.0".parse::<Spec>().is_err());
        assert!("not-a-version".parse::<Spec>().is_err());
    }

    #[test]
    fn spec_display_normalizes() {
        assert_eq!(spec("1.13").to_string(), "1.13");
        assert_eq!(spec("terraform@1.13").to_string(), "1.13");
        assert_eq!(spec("tofu@1.10").to_string(), "opentofu@1.10");
        assert_eq!(spec("opentofu@1.10.6").to_string(), "opentofu@1.10.6");
    }
}
