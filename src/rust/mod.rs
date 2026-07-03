pub mod dist;
pub mod project;

use std::fmt;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use anyhow::{Context, Result, bail};

use crate::config::{self, Pin, PinSource};
use crate::store;
use crate::versions::{Version, VersionReq};

pub const LANGUAGE: &str = "rust";

/// A rust toolchain request: a semver line or a rustup-style channel.
/// Bare channels (`nightly`, `beta`, `stable`) resolve to the newest
/// *installed* toolchain of that kind — activation never hits the network.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Channel {
    Semver(VersionReq),
    Stable,
    Nightly(Option<String>),
    Beta(Option<String>),
}

fn valid_date(s: &str) -> bool {
    let parts: Vec<&str> = s.split('-').collect();
    parts.len() == 3
        && parts[0].len() == 4
        && parts[1].len() == 2
        && parts[2].len() == 2
        && parts.iter().all(|p| p.bytes().all(|b| b.is_ascii_digit()))
}

impl FromStr for Channel {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self> {
        match s {
            "stable" => return Ok(Channel::Stable),
            "nightly" => return Ok(Channel::Nightly(None)),
            "beta" => return Ok(Channel::Beta(None)),
            _ => {}
        }
        if let Some(date) = s.strip_prefix("nightly-") {
            if valid_date(date) {
                return Ok(Channel::Nightly(Some(date.to_string())));
            }
            bail!("invalid nightly date '{date}' (expected YYYY-MM-DD)");
        }
        if let Some(date) = s.strip_prefix("beta-") {
            if valid_date(date) {
                return Ok(Channel::Beta(Some(date.to_string())));
            }
            bail!("invalid beta date '{date}' (expected YYYY-MM-DD)");
        }
        Ok(Channel::Semver(s.parse().with_context(|| {
            format!("invalid rust version or channel '{s}'")
        })?))
    }
}

impl fmt::Display for Channel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Channel::Semver(req) => write!(f, "{req}"),
            Channel::Stable => write!(f, "stable"),
            Channel::Nightly(None) => write!(f, "nightly"),
            Channel::Nightly(Some(date)) => write!(f, "nightly-{date}"),
            Channel::Beta(None) => write!(f, "beta"),
            Channel::Beta(Some(date)) => write!(f, "beta-{date}"),
        }
    }
}

/// An installed toolchain, named by its directory under toolchains/rust.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Toolchain {
    Release(Version),
    Nightly(String),
    Beta(String),
}

impl Toolchain {
    fn from_dir_name(name: &str) -> Option<Toolchain> {
        if let Ok(version) = name.parse::<Version>() {
            return Some(Toolchain::Release(version));
        }
        if let Some(date) = name.strip_prefix("nightly-")
            && valid_date(date)
        {
            return Some(Toolchain::Nightly(date.to_string()));
        }
        if let Some(date) = name.strip_prefix("beta-")
            && valid_date(date)
        {
            return Some(Toolchain::Beta(date.to_string()));
        }
        None
    }

    /// Ordering key: releases first (by version), then betas, then nightlies
    /// (ISO dates sort lexically).
    fn sort_key(&self) -> (u8, String) {
        match self {
            Toolchain::Release(v) => (
                0,
                format!("{:010}.{:010}.{:010}", v.major, v.minor, v.patch),
            ),
            Toolchain::Beta(d) => (1, d.clone()),
            Toolchain::Nightly(d) => (2, d.clone()),
        }
    }
}

impl fmt::Display for Toolchain {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Toolchain::Release(v) => write!(f, "{v}"),
            Toolchain::Nightly(d) => write!(f, "nightly-{d}"),
            Toolchain::Beta(d) => write!(f, "beta-{d}"),
        }
    }
}

pub fn toolchain_dir(toolchain: &Toolchain) -> Result<PathBuf> {
    Ok(config::toolchains_dir(LANGUAGE)?.join(toolchain.to_string()))
}

/// Installed toolchains, sorted (releases ascending, then betas, nightlies).
fn installed() -> Result<Vec<Toolchain>> {
    let dir = config::toolchains_dir(LANGUAGE)?;
    let entries = match std::fs::read_dir(&dir) {
        Ok(entries) => entries,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(err) => {
            return Err(err).with_context(|| format!("failed to read {}", dir.display()));
        }
    };
    let mut toolchains: Vec<Toolchain> = entries
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.path().is_dir())
        .filter_map(|entry| {
            entry
                .file_name()
                .to_str()
                .and_then(Toolchain::from_dir_name)
        })
        .collect();
    toolchains.sort_by_key(|t| t.sort_key());
    Ok(toolchains)
}

impl Channel {
    fn matches(&self, toolchain: &Toolchain) -> bool {
        match (self, toolchain) {
            (Channel::Semver(req), Toolchain::Release(v)) => req.matches(v),
            (Channel::Stable, Toolchain::Release(_)) => true,
            (Channel::Nightly(None), Toolchain::Nightly(_)) => true,
            (Channel::Nightly(Some(want)), Toolchain::Nightly(have)) => want == have,
            (Channel::Beta(None), Toolchain::Beta(_)) => true,
            (Channel::Beta(Some(want)), Toolchain::Beta(have)) => want == have,
            _ => false,
        }
    }

    /// The newest installed toolchain this channel accepts.
    fn best_installed(&self) -> Result<Option<Toolchain>> {
        Ok(installed()?
            .into_iter()
            .filter(|t| self.matches(t))
            .max_by_key(|t| t.sort_key()))
    }
}

fn pin_channel(pin: &Pin) -> Result<Channel> {
    pin.raw
        .parse()
        .with_context(|| format!("invalid rust pin '{}'", pin.raw))
}

/// rustup convention: the nearest rust-toolchain(.toml) whose channel is a
/// plain version or a (possibly dated) channel name.
pub fn fallback_pin(cwd: &Path) -> Result<Option<Pin>> {
    for dir in cwd.ancestors() {
        if let Some((channel, path)) = read_toolchain_file(dir)? {
            if channel.parse::<Channel>().is_ok() {
                return Ok(Some(Pin {
                    raw: channel,
                    source: PinSource::Project(path),
                }));
            }
            return Ok(None);
        }
    }
    Ok(None)
}

pub fn resolve_active(cwd: &Path) -> Result<Option<(Pin, Toolchain)>> {
    let Some(pin) = store::resolve_pin(LANGUAGE, cwd)? else {
        return Ok(None);
    };
    let channel = pin_channel(&pin)?;
    Ok(channel.best_installed()?.map(|t| (pin, t)))
}

/// resolve_active with the shell hook's opt-in auto-install behavior.
pub fn resolve_active_auto(cwd: &Path) -> Result<Option<(Pin, Toolchain)>> {
    if let Some(active) = resolve_active(cwd)? {
        return Ok(Some(active));
    }
    let Some(pin) = store::resolve_pin(LANGUAGE, cwd)? else {
        return Ok(None);
    };
    if !config::auto_install_enabled()? {
        return Ok(None);
    }
    if store::auto_install_recently_failed(LANGUAGE, &pin.raw) {
        return Ok(None);
    }
    eprintln!("linguo: auto-installing rust {}", pin.raw);
    if let Err(err) = install(Some(pin.raw.clone())) {
        store::record_auto_install_failure(LANGUAGE, &pin.raw);
        eprintln!("linguo: auto-install of rust {} failed: {err:#}", pin.raw);
        eprintln!("linguo: will not retry for 5 minutes");
        return Ok(None);
    }
    resolve_active(cwd)
}

/// The pinned toolchain for `dir`, or an actionable error.
pub fn required_toolchain(dir: &Path) -> Result<Toolchain> {
    let Some(pin) = store::resolve_pin(LANGUAGE, dir)? else {
        bail!("no rust version pinned (run `linguo rust use <version>` or `linguo rust init`)");
    };
    let channel = pin_channel(&pin)?;
    channel.best_installed()?.with_context(|| {
        format!(
            "rust {} is pinned but not installed (run `linguo rust install {}`)",
            pin.raw, pin.raw
        )
    })
}

/// Toolchain a new project should use: pin if satisfiable, else the newest
/// installed release (falling back to any toolchain).
fn pick_project_toolchain(dir: &Path) -> Result<Toolchain> {
    if store::resolve_pin(LANGUAGE, dir)?.is_some() {
        return required_toolchain(dir);
    }
    let all = installed()?;
    all.iter()
        .filter(|t| matches!(t, Toolchain::Release(_)))
        .cloned()
        .next_back()
        .or_else(|| all.last().cloned())
        .context("no rust toolchains installed (run `linguo rust install`)")
}

/// The pin value a project should record for `toolchain`.
fn pin_value(toolchain: &Toolchain) -> String {
    match toolchain {
        Toolchain::Release(v) => VersionReq::MajorMinor(v.major, v.minor).to_string(),
        other => other.to_string(),
    }
}

/// Read a rustup-convention toolchain file: `rust-toolchain.toml`
/// (`[toolchain] channel = "..."`) or the legacy bare `rust-toolchain`.
fn read_toolchain_file(dir: &Path) -> Result<Option<(String, PathBuf)>> {
    let toml_path = dir.join("rust-toolchain.toml");
    if toml_path.is_file() {
        let text = std::fs::read_to_string(&toml_path)
            .with_context(|| format!("failed to read {}", toml_path.display()))?;
        let doc: toml_edit::DocumentMut = text
            .parse()
            .with_context(|| format!("failed to parse {}", toml_path.display()))?;
        if let Some(channel) = doc
            .get("toolchain")
            .and_then(|t| t.get("channel"))
            .and_then(|c| c.as_str())
        {
            return Ok(Some((channel.to_string(), toml_path)));
        }
        return Ok(None);
    }
    let legacy_path = dir.join("rust-toolchain");
    if legacy_path.is_file() {
        let text = std::fs::read_to_string(&legacy_path)
            .with_context(|| format!("failed to read {}", legacy_path.display()))?;
        let channel = text.trim().to_string();
        if !channel.is_empty() {
            return Ok(Some((channel, legacy_path)));
        }
    }
    Ok(None)
}

/// Extra components/targets a project's rust-toolchain.toml declares; both
/// empty when there's no file. Honored automatically at install time.
fn toolchain_file_extras(cwd: &Path) -> Result<(Vec<String>, Vec<String>)> {
    for dir in cwd.ancestors() {
        let path = dir.join("rust-toolchain.toml");
        if !path.is_file() {
            continue;
        }
        let text = std::fs::read_to_string(&path)
            .with_context(|| format!("failed to read {}", path.display()))?;
        let doc: toml_edit::DocumentMut = text
            .parse()
            .with_context(|| format!("failed to parse {}", path.display()))?;
        let list = |key: &str| -> Vec<String> {
            doc.get("toolchain")
                .and_then(|t| t.get(key))
                .and_then(|v| v.as_array())
                .map(|a| {
                    a.iter()
                        .filter_map(|i| i.as_str().map(str::to_string))
                        .collect()
                })
                .unwrap_or_default()
        };
        return Ok((list("components"), list("targets")));
    }
    Ok((Vec::new(), Vec::new()))
}

/// Fetch the manifest for a channel, returning the toolchain it names.
fn manifest_for(channel: &Channel) -> Result<(dist::Manifest, Toolchain)> {
    match channel {
        Channel::Semver(req) => {
            // Channel manifests resolve minor requests server-side; only a
            // bare major needs the release list.
            let name = match req {
                VersionReq::Major(_) => req
                    .best_match(dist::fetch_available()?)
                    .with_context(|| format!("no available release matches '{req}'"))?
                    .to_string(),
                other => other.to_string(),
            };
            let manifest = dist::fetch_manifest(&name, None)?;
            let version = manifest.release_version()?;
            Ok((manifest, Toolchain::Release(version)))
        }
        Channel::Stable => {
            let manifest = dist::fetch_manifest("stable", None)?;
            let version = manifest.release_version()?;
            Ok((manifest, Toolchain::Release(version)))
        }
        Channel::Nightly(date) => {
            let manifest = dist::fetch_manifest("nightly", date.as_deref())?;
            let date = manifest.date.clone();
            Ok((manifest, Toolchain::Nightly(date)))
        }
        Channel::Beta(date) => {
            let manifest = dist::fetch_manifest("beta", date.as_deref())?;
            let date = manifest.date.clone();
            Ok((manifest, Toolchain::Beta(date)))
        }
    }
}

/// The manifest that produced an installed toolchain (for component adds).
fn manifest_for_toolchain(toolchain: &Toolchain) -> Result<dist::Manifest> {
    match toolchain {
        Toolchain::Release(v) => dist::fetch_manifest(&v.to_string(), None),
        Toolchain::Nightly(d) => dist::fetch_manifest("nightly", Some(d)),
        Toolchain::Beta(d) => dist::fetch_manifest("beta", Some(d)),
    }
}

pub fn install(request: Option<String>) -> Result<()> {
    let channel: Channel = match &request {
        Some(raw) => raw.parse()?,
        None => Channel::Stable,
    };
    let (manifest, toolchain) = manifest_for(&channel)?;
    let dest = toolchain_dir(&toolchain)?;
    if dest.exists() {
        eprintln!("rust {toolchain} is already installed");
        return Ok(());
    }
    std::fs::create_dir_all(dest.parent().unwrap())
        .with_context(|| format!("failed to create {}", dest.parent().unwrap().display()))?;

    // Honor the project's declared components/targets (rustup convention).
    let cwd = std::env::current_dir()?;
    let (components, targets) = toolchain_file_extras(&cwd)?;
    if !components.is_empty() || !targets.is_empty() {
        eprintln!(
            "including rust-toolchain.toml extras (components: [{}], targets: [{}])",
            components.join(", "),
            targets.join(", ")
        );
    }

    // The prefix is assembled from several component archives; don't leave a
    // half-merged toolchain behind on failure.
    if let Err(err) = dist::install_channel(&manifest.doc, &dest, &components, &targets) {
        let _ = std::fs::remove_dir_all(&dest);
        return Err(err);
    }
    eprintln!("installed rust {toolchain} to {}", dest.display());
    Ok(())
}

pub fn uninstall(raw: &str) -> Result<()> {
    let channel: Channel = raw.parse()?;
    let matches: Vec<Toolchain> = installed()?
        .into_iter()
        .filter(|t| channel.matches(t))
        .collect();
    let toolchain = match matches.as_slice() {
        [] => bail!("no installed toolchain matches '{raw}'"),
        [only] => only.clone(),
        many => bail!(
            "'{raw}' matches multiple installed toolchains ({}); specify one exactly",
            many.iter()
                .map(|t| t.to_string())
                .collect::<Vec<_>>()
                .join(", ")
        ),
    };
    let path = toolchain_dir(&toolchain)?;
    std::fs::remove_dir_all(&path)
        .with_context(|| format!("failed to remove {}", path.display()))?;
    println!("uninstalled rust {toolchain}");
    Ok(())
}

pub fn use_version(raw: &str, global: bool) -> Result<()> {
    let channel: Channel = raw.parse()?;
    let path = if global {
        config::linguo_root()?.join(config::GLOBAL_CONFIG)
    } else {
        std::env::current_dir()?.join(config::PIN_FILE)
    };
    let normalized = channel.to_string();
    config::write_pin(&path, LANGUAGE, &normalized)?;
    println!("pinned rust to {normalized} in {}", path.display());
    if channel.best_installed()?.is_none() {
        println!("note: no installed toolchain matches; run `linguo rust install {normalized}`");
    }
    Ok(())
}

pub fn list(available: bool) -> Result<()> {
    if available {
        let versions = dist::fetch_available()?;
        if versions.is_empty() {
            println!("no releases found");
            return Ok(());
        }
        let installed_releases: Vec<Version> = installed()
            .unwrap_or_default()
            .into_iter()
            .filter_map(|t| match t {
                Toolchain::Release(v) => Some(v),
                _ => None,
            })
            .collect();
        // Show the latest release per minor line.
        let mut previous: Option<Version> = None;
        let mut latest_per_minor: Vec<Version> = Vec::new();
        for version in versions {
            if let Some(prev) = previous
                && (prev.major, prev.minor) != (version.major, version.minor)
            {
                latest_per_minor.push(prev);
            }
            previous = Some(version);
        }
        latest_per_minor.extend(previous);
        for version in latest_per_minor {
            let marker = if installed_releases.contains(&version) {
                " (installed)"
            } else {
                ""
            };
            println!("{version}{marker}");
        }
        println!(
            "(latest release per minor line; nightly/beta install by channel name, e.g. `linguo rust install nightly`)"
        );
        return Ok(());
    }

    let toolchains = installed()?;
    if toolchains.is_empty() {
        println!("no rust toolchains installed (try `linguo rust install`)");
        return Ok(());
    }
    let cwd = std::env::current_dir()?;
    let active = resolve_active(&cwd)?;
    for toolchain in toolchains {
        match &active {
            Some((pin, active_toolchain)) if *active_toolchain == toolchain => {
                let source = match &pin.source {
                    PinSource::Project(path) => format!("pinned by {}", path.display()),
                    PinSource::Global => "global default".to_string(),
                };
                println!("{toolchain} * ({source})");
            }
            _ => println!("{toolchain}"),
        }
    }
    Ok(())
}

/// Status lines for `linguo status`, matching the generic language format.
pub fn print_status(cwd: &Path) -> Result<()> {
    println!("{LANGUAGE}");
    let toolchains = installed()?;
    let listed = if toolchains.is_empty() {
        "(none)".to_string()
    } else {
        toolchains
            .iter()
            .map(|t| t.to_string())
            .collect::<Vec<_>>()
            .join(", ")
    };
    println!("  toolchains: {listed}");

    match store::resolve_pin(LANGUAGE, cwd)? {
        None => println!("  active: none (no version pinned)"),
        Some(pin) => {
            let source = match &pin.source {
                PinSource::Project(path) => path.display().to_string(),
                PinSource::Global => "global config".to_string(),
            };
            let channel = pin_channel(&pin)?;
            match channel.best_installed()? {
                Some(toolchain) => {
                    println!("  active: {toolchain} (pinned to {} by {source})", pin.raw);
                }
                None => println!(
                    "  active: none ({} pinned by {source} but not installed — run `linguo rust install {}`)",
                    pin.raw, pin.raw
                ),
            }
        }
    }
    Ok(())
}

/// Install extra components (e.g. rust-analyzer, rust-src) into the active
/// toolchain from its own channel manifest.
pub fn component_add(names: &[String]) -> Result<()> {
    if names.is_empty() {
        bail!("no components given");
    }
    let cwd = std::env::current_dir()?;
    let toolchain = required_toolchain(&cwd)?;
    let manifest = manifest_for_toolchain(&toolchain)?;
    dist::add_components(&manifest.doc, &toolchain_dir(&toolchain)?, names, &[])?;
    println!("added {} to rust {toolchain}", names.join(", "));
    Ok(())
}

/// Install rust-std for extra compilation targets into the active toolchain.
pub fn target_add(triples: &[String]) -> Result<()> {
    if triples.is_empty() {
        bail!("no targets given");
    }
    let cwd = std::env::current_dir()?;
    let toolchain = required_toolchain(&cwd)?;
    let manifest = manifest_for_toolchain(&toolchain)?;
    dist::add_components(&manifest.doc, &toolchain_dir(&toolchain)?, &[], triples)?;
    println!("added std for {} to rust {toolchain}", triples.join(", "));
    Ok(())
}

/// Upgrade the pinned toolchain, channel-aware: semver pins behave like
/// every other language; a bare `nightly`/`beta` pin moves to today's build;
/// dated pins need --latest to move (which rewrites the pin's date).
pub fn upgrade(latest: bool, prune: bool) -> Result<()> {
    let cwd = std::env::current_dir()?;
    let Some(pin) = store::resolve_pin(LANGUAGE, &cwd)? else {
        bail!("no rust version pinned (run `linguo rust use <version>`)");
    };
    let channel = pin_channel(&pin)?;

    // Which channel to fetch: dated pins stay put unless --latest bumps them.
    let fetch_channel = match (&channel, latest) {
        (Channel::Semver(_), true) => Channel::Stable,
        (Channel::Nightly(_), true) => Channel::Nightly(None),
        (Channel::Beta(_), true) => Channel::Beta(None),
        _ => channel.clone(),
    };
    let (_, target) = manifest_for(&fetch_channel)?;

    // The pin value the target implies, at the pin's own granularity.
    let new_pin: Channel = match (&channel, &target) {
        (Channel::Semver(req), Toolchain::Release(v)) => {
            Channel::Semver(store::granularity_bump(req, *v))
        }
        (Channel::Nightly(Some(_)), Toolchain::Nightly(d)) => Channel::Nightly(Some(d.clone())),
        (Channel::Beta(Some(_)), Toolchain::Beta(d)) => Channel::Beta(Some(d.clone())),
        _ => channel.clone(),
    };
    if latest && new_pin != channel {
        store::write_pin_back(LANGUAGE, &pin, &new_pin.to_string())?;
        println!("bumped rust pin {} -> {new_pin}", pin.raw);
    }

    if toolchain_dir(&target)?.exists() {
        println!("rust {target} is already installed and is the newest matching build");
        if !latest
            && matches!(
                channel,
                Channel::Semver(VersionReq::Exact(_))
                    | Channel::Nightly(Some(_))
                    | Channel::Beta(Some(_))
            )
        {
            println!("note: the pin is exact; use `--latest` to bump it");
        }
    } else {
        install(Some(target.to_string()))?;
    }

    if prune {
        let keep = target.sort_key();
        let stale: Vec<Toolchain> = installed()?
            .into_iter()
            .filter(|t| t.sort_key() < keep && (channel.matches(t) || new_pin.matches(t)))
            .collect();
        if stale.is_empty() {
            println!("nothing to prune");
        }
        for toolchain in stale {
            let path = toolchain_dir(&toolchain)?;
            std::fs::remove_dir_all(&path)
                .with_context(|| format!("failed to remove {}", path.display()))?;
            println!("uninstalled rust {toolchain}");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ch(s: &str) -> Channel {
        s.parse().unwrap()
    }

    #[test]
    fn parses_channels() {
        assert_eq!(ch("stable"), Channel::Stable);
        assert_eq!(ch("nightly"), Channel::Nightly(None));
        assert_eq!(
            ch("nightly-2026-07-01"),
            Channel::Nightly(Some("2026-07-01".to_string()))
        );
        assert_eq!(
            ch("beta-2026-07-01"),
            Channel::Beta(Some("2026-07-01".to_string()))
        );
        assert!(matches!(ch("1.96"), Channel::Semver(_)));
        assert!("nightly-July-1".parse::<Channel>().is_err());
        assert!("lts".parse::<Channel>().is_err());
    }

    #[test]
    fn channel_display_roundtrips() {
        for s in [
            "stable",
            "nightly",
            "nightly-2026-07-01",
            "beta",
            "1.96",
            "1.96.1",
        ] {
            assert_eq!(ch(s).to_string(), s);
        }
    }

    #[test]
    fn toolchain_dir_names_roundtrip() {
        for s in ["1.96.1", "nightly-2026-07-01", "beta-2026-06-15"] {
            assert_eq!(Toolchain::from_dir_name(s).unwrap().to_string(), s);
        }
        assert!(Toolchain::from_dir_name("nightly").is_none());
        assert!(Toolchain::from_dir_name("garbage").is_none());
    }

    #[test]
    fn channel_matching() {
        let release = Toolchain::Release("1.96.1".parse().unwrap());
        let nightly = Toolchain::Nightly("2026-07-01".to_string());
        assert!(ch("1.96").matches(&release));
        assert!(ch("stable").matches(&release));
        assert!(!ch("nightly").matches(&release));
        assert!(ch("nightly").matches(&nightly));
        assert!(ch("nightly-2026-07-01").matches(&nightly));
        assert!(!ch("nightly-2026-06-30").matches(&nightly));
        assert!(!ch("stable").matches(&nightly));
    }
}
