//! Project management: pyproject.toml, the project venv, and running commands.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};
use toml_edit::{Array, DocumentMut, value};

use crate::config;
use crate::exec;
use crate::store;
use crate::versions::VersionReq;

const PYPROJECT: &str = "pyproject.toml";
const VENV: &str = ".venv";

/// Nearest ancestor directory (including `start`) containing a pyproject.toml.
fn find_project_root(start: &Path) -> Option<PathBuf> {
    start
        .ancestors()
        .find(|dir| dir.join(PYPROJECT).is_file())
        .map(Path::to_path_buf)
}

fn project_root() -> Result<PathBuf> {
    let cwd = std::env::current_dir()?;
    find_project_root(&cwd).context(
        "no pyproject.toml found in this directory or any parent (run `linguo python init` first)",
    )
}

pub fn venv_bin_dir(project_root: &Path) -> PathBuf {
    let venv = project_root.join(VENV);
    if cfg!(windows) {
        venv.join("Scripts")
    } else {
        venv.join("bin")
    }
}

/// Create the project venv with the pinned toolchain if it doesn't exist yet.
fn ensure_venv(root: &Path) -> Result<PathBuf> {
    let venv = root.join(VENV);
    if venv.join("pyvenv.cfg").is_file() {
        return Ok(venv);
    }
    let version = store::required_toolchain(super::LANGUAGE, root)?;
    let python =
        super::dist::bin_dir(&super::toolchain_path(&version)?).join(super::dist::python_exe());
    eprintln!("creating {} with python {version}", venv.display());
    let status = Command::new(&python)
        .args(["-m", "venv"])
        .arg(&venv)
        .status()
        .with_context(|| format!("failed to run {}", python.display()))?;
    if !status.success() {
        bail!("venv creation failed");
    }
    Ok(venv)
}

fn pip(root: &Path) -> Result<Command> {
    ensure_venv(root)?;
    let python = venv_bin_dir(root).join(super::dist::python_exe());
    let mut cmd = Command::new(python);
    cmd.args(["-m", "pip", "--disable-pip-version-check"]);
    cmd.current_dir(root);
    Ok(cmd)
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

/// PEP 503-style normalized package name from a requirement spec like
/// `Requests>=2.31` or `foo[extra]==1.0`.
fn spec_name(spec: &str) -> String {
    let name: String = spec
        .chars()
        .take_while(|c| c.is_alphanumeric() || matches!(c, '-' | '_' | '.'))
        .collect();
    let mut normalized = String::with_capacity(name.len());
    let mut prev_sep = false;
    for c in name.chars() {
        if matches!(c, '-' | '_' | '.') {
            if !prev_sep {
                normalized.push('-');
            }
            prev_sep = true;
        } else {
            normalized.push(c.to_ascii_lowercase());
            prev_sep = false;
        }
    }
    normalized
}

fn dependencies_array(doc: &mut DocumentMut) -> Result<&mut Array> {
    let project = doc
        .get_mut("project")
        .context("pyproject.toml has no [project] table")?;
    if project.get("dependencies").is_none() {
        project["dependencies"] = value(Array::new());
    }
    project["dependencies"]
        .as_array_mut()
        .context("project.dependencies is not an array")
}

/// Add or replace requirement specs in `[project] dependencies`.
fn add_to_dependencies(pyproject: &str, specs: &[String]) -> Result<String> {
    let mut doc: DocumentMut = pyproject
        .parse()
        .context("failed to parse pyproject.toml")?;
    let deps = dependencies_array(&mut doc)?;
    for spec in specs {
        let name = spec_name(spec);
        let existing = deps
            .iter()
            .position(|d| d.as_str().is_some_and(|s| spec_name(s) == name));
        match existing {
            Some(index) => {
                deps.replace(index, spec.as_str());
            }
            None => deps.push(spec.as_str()),
        }
    }
    Ok(doc.to_string())
}

/// Remove packages (by name) from `[project] dependencies`.
fn remove_from_dependencies(pyproject: &str, names: &[String]) -> Result<String> {
    let mut doc: DocumentMut = pyproject
        .parse()
        .context("failed to parse pyproject.toml")?;
    let deps = dependencies_array(&mut doc)?;
    let targets: Vec<String> = names.iter().map(|n| spec_name(n)).collect();
    deps.retain(|d| d.as_str().is_none_or(|s| !targets.contains(&spec_name(s))));
    Ok(doc.to_string())
}

fn read_dependencies(pyproject: &str) -> Result<Vec<String>> {
    let doc: DocumentMut = pyproject
        .parse()
        .context("failed to parse pyproject.toml")?;
    Ok(doc
        .get("project")
        .and_then(|p| p.get("dependencies"))
        .and_then(|d| d.as_array())
        .map(|deps| {
            deps.iter()
                .filter_map(|d| d.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default())
}

pub fn init(name: Option<String>) -> Result<()> {
    let cwd = std::env::current_dir()?;
    let pyproject_path = cwd.join(PYPROJECT);
    if pyproject_path.exists() {
        bail!("{} already exists", pyproject_path.display());
    }

    let version = store::pick_project_version(super::LANGUAGE, &cwd)?;

    let name = match name {
        Some(name) => name,
        None => {
            let dir_name = cwd
                .file_name()
                .and_then(|n| n.to_str())
                .context("cannot derive a project name from this directory; pass one explicitly")?;
            let sanitized = spec_name(dir_name);
            if sanitized.is_empty() {
                bail!("cannot derive a project name from this directory; pass one explicitly");
            }
            sanitized
        }
    };

    let pyproject = format!(
        r#"[project]
name = "{name}"
version = "0.1.0"
requires-python = ">={major}.{minor}"
dependencies = []
"#,
        major = version.major,
        minor = version.minor,
    );
    std::fs::write(&pyproject_path, pyproject)
        .with_context(|| format!("failed to write {}", pyproject_path.display()))?;

    let req = VersionReq::MajorMinor(version.major, version.minor);
    config::write_pin(
        &cwd.join(config::PIN_FILE),
        super::LANGUAGE,
        &req.to_string(),
    )?;
    ensure_venv(&cwd)?;

    println!("initialized project '{name}' with python {version}");
    Ok(())
}

pub fn add(specs: &[String]) -> Result<()> {
    if specs.is_empty() {
        bail!("no packages given");
    }
    let root = project_root()?;
    run_checked(pip(&root)?.arg("install").args(specs), "pip install")?;

    let path = root.join(PYPROJECT);
    let text = std::fs::read_to_string(&path)?;
    std::fs::write(&path, add_to_dependencies(&text, specs)?)
        .with_context(|| format!("failed to write {}", path.display()))?;
    println!("added {} to {}", specs.join(", "), path.display());
    Ok(())
}

pub fn remove(names: &[String]) -> Result<()> {
    if names.is_empty() {
        bail!("no packages given");
    }
    let root = project_root()?;
    run_checked(
        pip(&root)?.args(["uninstall", "-y"]).args(names),
        "pip uninstall",
    )?;

    let path = root.join(PYPROJECT);
    let text = std::fs::read_to_string(&path)?;
    std::fs::write(&path, remove_from_dependencies(&text, names)?)
        .with_context(|| format!("failed to write {}", path.display()))?;
    println!("removed {} from {}", names.join(", "), path.display());
    Ok(())
}

pub fn sync() -> Result<()> {
    sync_in(&project_root()?)
}

/// Sync a specific project directory (used by workspace sync).
pub fn sync_in(root: &Path) -> Result<()> {
    let root = root.to_path_buf();
    ensure_venv(&root)?;
    let deps = read_dependencies(&std::fs::read_to_string(root.join(PYPROJECT))?)?;
    if deps.is_empty() {
        println!("no dependencies to sync");
        return Ok(());
    }
    run_checked(pip(&root)?.arg("install").args(&deps), "pip install")?;
    println!("synced {} dependencies", deps.len());
    Ok(())
}

/// linguo-managed bin directories for `cwd` — the project venv (if any)
/// followed by the pinned toolchain — plus the venv root when one exists.
fn managed_bin_dirs(cwd: &Path) -> Result<(Vec<PathBuf>, Option<PathBuf>)> {
    let mut dirs: Vec<PathBuf> = Vec::new();
    let mut venv: Option<PathBuf> = None;
    if let Some(root) = find_project_root(cwd) {
        let venv_dir = root.join(VENV);
        if venv_dir.join("pyvenv.cfg").is_file() {
            dirs.push(venv_bin_dir(&root));
            venv = Some(venv_dir);
        }
    }
    let version = store::required_toolchain(super::LANGUAGE, cwd)?;
    dirs.push(super::dist::bin_dir(&super::toolchain_path(&version)?));
    Ok((dirs, venv))
}

/// Print the path of the executable a command resolves to (default: python).
pub fn which(command: Option<String>) -> Result<()> {
    let cwd = std::env::current_dir()?;
    let (dirs, _) = managed_bin_dirs(&cwd)?;
    let candidates = match &command {
        Some(name) => vec![name.clone()],
        None => vec!["python".to_string(), "python3".to_string()],
    };
    for dir in &dirs {
        for name in &candidates {
            if let Some(path) = exec::find_in_dir(dir, name) {
                println!("{}", path.display());
                return Ok(());
            }
        }
    }
    bail!(
        "'{}' not found in the project venv or pinned toolchain",
        candidates.join("' / '")
    );
}

/// Run a command with the project venv (if any) and pinned toolchain on PATH.
pub fn run(args: &[String]) -> Result<()> {
    let (program, rest) = args.split_first().context("no command given")?;
    let cwd = std::env::current_dir()?;
    let (mut path_dirs, venv) = managed_bin_dirs(&cwd)?;

    let current_path = std::env::var_os("PATH").unwrap_or_default();
    path_dirs.extend(std::env::split_paths(&current_path));
    let new_path = std::env::join_paths(path_dirs).context("invalid PATH entry")?;

    let mut cmd = Command::new(program);
    cmd.args(rest).env("PATH", new_path);
    if let Some(venv) = venv {
        cmd.env("VIRTUAL_ENV", venv);
    }
    exec::exec(cmd, program)
}

#[cfg(test)]
mod tests {
    use super::*;

    const BASE: &str = r#"[project]
name = "demo"
version = "0.1.0"
dependencies = []
"#;

    #[test]
    fn spec_names_normalize() {
        assert_eq!(spec_name("Requests>=2.31"), "requests");
        assert_eq!(spec_name("foo[extra]==1.0"), "foo");
        assert_eq!(spec_name("My_Pkg.Name"), "my-pkg-name");
        assert_eq!(spec_name("a--b__c"), "a-b-c");
    }

    #[test]
    fn add_appends_and_replaces() {
        let once = add_to_dependencies(BASE, &["requests>=2.31".into()]).unwrap();
        assert!(once.contains(r#""requests>=2.31""#));
        let twice = add_to_dependencies(&once, &["Requests==2.32.0".into()]).unwrap();
        assert!(twice.contains(r#""Requests==2.32.0""#));
        assert!(!twice.contains("2.31"));
    }

    #[test]
    fn remove_drops_by_normalized_name() {
        let with_deps =
            add_to_dependencies(BASE, &["requests>=2.31".into(), "flask".into()]).unwrap();
        let removed = remove_from_dependencies(&with_deps, &["Requests".into()]).unwrap();
        assert!(!removed.contains("requests"));
        assert!(removed.contains("flask"));
    }

    #[test]
    fn read_dependencies_roundtrip() {
        let text = add_to_dependencies(BASE, &["requests>=2.31".into(), "flask".into()]).unwrap();
        assert_eq!(
            read_dependencies(&text).unwrap(),
            vec!["requests>=2.31", "flask"]
        );
        assert!(read_dependencies("[tool.other]\n").unwrap().is_empty());
    }
}
