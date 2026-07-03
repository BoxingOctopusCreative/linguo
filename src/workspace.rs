//! Workspace sync: one `linguo sync` makes every member project in a repo
//! runnable — missing pinned toolchains are installed, then each member's
//! package layer syncs through its ecosystem's native tool.
//!
//! Members come from `[workspace] members = ["services/*", "web"]` in the
//! nearest ancestor linguo.toml when declared (globs allowed); otherwise
//! they're discovered by walking the tree for project manifests, skipping
//! build and vendor directories.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

use crate::{go, node, python, ruby, rust, store, terraform};

/// Project manifests that make a directory a workspace member, and the
/// language each belongs to.
const MANIFESTS: &[(&str, &str)] = &[
    ("pyproject.toml", "python"),
    ("package.json", "node"),
    ("Gemfile", "ruby"),
    ("Cargo.toml", "rust"),
    ("go.mod", "go"),
];

/// Directories never descended into during discovery.
const SKIP_DIRS: &[&str] = &[
    "node_modules",
    "target",
    "vendor",
    "dist",
    "build",
    "__pycache__",
    ".venv",
    "venv",
];

/// `[workspace] members` from a linguo.toml, when declared.
fn workspace_members(path: &Path) -> Result<Option<Vec<String>>> {
    if !path.is_file() {
        return Ok(None);
    }
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read {}", path.display()))?;
    let doc: toml_edit::DocumentMut = text
        .parse()
        .with_context(|| format!("failed to parse {}", path.display()))?;
    Ok(doc
        .get("workspace")
        .and_then(|w| w.get("members"))
        .and_then(|m| m.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|i| i.as_str().map(str::to_string))
                .collect()
        }))
}

/// True when `dir` holds at least one project manifest (or terraform files).
fn is_member(dir: &Path) -> bool {
    member_languages(dir).is_ok_and(|langs| !langs.is_empty())
}

/// The languages a member directory has manifests for. Terraform counts
/// when the directory holds .tf files (toolchain-only member).
fn member_languages(dir: &Path) -> Result<Vec<&'static str>> {
    let mut languages = Vec::new();
    for (manifest, language) in MANIFESTS {
        if dir.join(manifest).is_file() {
            languages.push(*language);
        }
    }
    let has_tf = std::fs::read_dir(dir)
        .ok()
        .into_iter()
        .flatten()
        .filter_map(|e| e.ok())
        .any(|e| e.path().extension().is_some_and(|ext| ext == "tf"));
    if has_tf {
        languages.push(terraform::LANGUAGE);
    }
    Ok(languages)
}

/// Walk `root` collecting member directories, skipping vendor/build dirs and
/// dot-directories.
fn discover(root: &Path, members: &mut Vec<PathBuf>) -> Result<()> {
    if is_member(root) {
        members.push(root.to_path_buf());
    }
    let entries = match std::fs::read_dir(root) {
        Ok(entries) => entries,
        Err(_) => return Ok(()),
    };
    let mut subdirs: Vec<PathBuf> = entries
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|name| !name.starts_with('.') && !SKIP_DIRS.contains(&name))
        })
        .collect();
    subdirs.sort();
    for subdir in subdirs {
        discover(&subdir, members)?;
    }
    Ok(())
}

/// Resolve the workspace root and its members: a declared `[workspace]`
/// wins; otherwise discovery from `cwd`.
fn resolve_members(cwd: &Path) -> Result<(PathBuf, Vec<PathBuf>)> {
    for dir in cwd.ancestors() {
        if let Some(patterns) = workspace_members(&dir.join(crate::config::PIN_FILE))? {
            let mut members = Vec::new();
            for pattern in &patterns {
                let full = dir.join(pattern);
                let full = full.to_string_lossy();
                let mut matched = false;
                for entry in glob::glob(&full)
                    .with_context(|| format!("invalid workspace member pattern '{pattern}'"))?
                {
                    let path = entry?;
                    if path.is_dir() && is_member(&path) {
                        members.push(path);
                        matched = true;
                    }
                }
                if !matched {
                    eprintln!("warning: workspace member pattern '{pattern}' matched nothing");
                }
            }
            members.sort();
            members.dedup();
            return Ok((dir.to_path_buf(), members));
        }
    }
    let mut members = Vec::new();
    discover(cwd, &mut members)?;
    Ok((cwd.to_path_buf(), members))
}

/// Install the pinned toolchain for `language` in `dir` if it's missing.
/// Returns false when no pin covers the language (member gets skipped).
fn ensure_toolchain(language: &str, dir: &Path) -> Result<bool> {
    match language {
        "rust" => {
            if rust::resolve_active(dir)?.is_some() {
                return Ok(true);
            }
            match store::resolve_pin(language, dir)? {
                Some(pin) => rust::install(Some(pin.raw)).map(|_| true),
                None => Ok(false),
            }
        }
        "terraform" => {
            if terraform::resolve_active(dir)?.is_some() {
                return Ok(true);
            }
            match crate::config::resolve_pin(language, dir)? {
                Some(pin) => terraform::install(Some(pin.raw)).map(|_| true),
                None => Ok(false),
            }
        }
        _ => {
            if store::resolve_active(language, dir)?.is_some() {
                return Ok(true);
            }
            match store::resolve_pin(language, dir)? {
                Some(pin) => {
                    let raw = Some(pin.raw);
                    match language {
                        "python" => python::install(raw)?,
                        "node" => node::install(raw)?,
                        "ruby" => ruby::install(raw)?,
                        "go" => go::install(raw)?,
                        other => bail!("no installer for {other}"),
                    }
                    Ok(true)
                }
                None => Ok(false),
            }
        }
    }
}

/// Run the language's package-layer sync for a member directory. Terraform
/// members are toolchain-only.
fn sync_language(language: &str, dir: &Path) -> Result<()> {
    match language {
        "python" => python::project::sync_in(dir),
        "node" => node::project::sync_in(dir),
        "ruby" => ruby::project::sync_in(dir),
        "rust" => rust::project::sync_in(dir),
        "go" => go::project::sync_in(dir),
        "terraform" => Ok(()),
        other => bail!("no sync for {other}"),
    }
}

pub fn sync() -> Result<()> {
    let cwd = std::env::current_dir()?;
    let (root, members) = resolve_members(&cwd)?;
    if members.is_empty() {
        println!(
            "no workspace members found under {} (nothing with a project manifest)",
            root.display()
        );
        return Ok(());
    }

    let mut failures: Vec<String> = Vec::new();
    for member in &members {
        let display = member
            .strip_prefix(&root)
            .ok()
            .filter(|p| !p.as_os_str().is_empty())
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| ".".to_string());
        for language in member_languages(member)? {
            println!("{display}: {language}");
            match ensure_toolchain(language, member) {
                Ok(true) => {
                    if let Err(err) = sync_language(language, member) {
                        eprintln!("  {err:#}");
                        failures.push(format!("{display} ({language})"));
                    }
                }
                Ok(false) => {
                    println!("  no {language} version pinned; skipped");
                }
                Err(err) => {
                    eprintln!("  {err:#}");
                    failures.push(format!("{display} ({language})"));
                }
            }
        }
    }
    if !failures.is_empty() {
        bail!("sync failed for: {}", failures.join(", "));
    }
    println!("workspace in sync ({} members)", members.len());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn touch(path: &Path) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, "").unwrap();
    }

    /// Separator-agnostic relative member names for assertions.
    fn names(members: &[PathBuf], root: &Path) -> Vec<String> {
        members
            .iter()
            .map(|m| {
                m.strip_prefix(root)
                    .unwrap()
                    .display()
                    .to_string()
                    .replace('\\', "/")
            })
            .collect()
    }

    #[test]
    fn discovery_finds_members_and_skips_vendor_dirs() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        touch(&root.join("api/pyproject.toml"));
        touch(&root.join("web/package.json"));
        touch(&root.join("web/node_modules/dep/package.json"));
        touch(&root.join("infra/main.tf"));
        touch(&root.join(".hidden/Cargo.toml"));
        touch(&root.join("tools/cli/go.mod"));

        let mut members = Vec::new();
        discover(root, &mut members).unwrap();
        assert_eq!(
            names(&members, root),
            vec!["api", "infra", "tools/cli", "web"]
        );
    }

    #[test]
    fn declared_members_win_and_glob() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        std::fs::write(
            root.join("linguo.toml"),
            "[workspace]\nmembers = [\"services/*\", \"web\"]\n",
        )
        .unwrap();
        touch(&root.join("services/a/pyproject.toml"));
        touch(&root.join("services/b/package.json"));
        touch(&root.join("services/fixtures/README.md")); // not a member
        touch(&root.join("web/package.json"));
        touch(&root.join("unlisted/Cargo.toml")); // declared members win

        let nested = root.join("services/a");
        let (found_root, members) = resolve_members(&nested).unwrap();
        assert_eq!(found_root, root);
        assert_eq!(
            names(&members, root),
            vec!["services/a", "services/b", "web"]
        );
    }

    #[test]
    fn member_languages_detects_terraform() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        touch(&root.join("main.tf"));
        touch(&root.join("Cargo.toml"));
        let langs = member_languages(root).unwrap();
        assert!(langs.contains(&"terraform"));
        assert!(langs.contains(&"rust"));
    }
}
