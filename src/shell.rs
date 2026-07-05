//! Shell integration: `linguo activate <shell>` emits a hook that re-runs
//! `linguo env --shell <shell>` on every prompt / directory change, and `env`
//! prints the PATH updates needed for the current directory's pins.

use std::path::PathBuf;

use anyhow::Result;
use clap::ValueEnum;

use crate::config::PinSource;
use crate::{
    go, groovy, jvm, jvmlang, kotlin, node, php, python, ruby, rust, scala, terraform, tools, zig,
};

/// Env var tracking which directories linguo has prepended to PATH, so they
/// can be removed again when the active project changes.
const DIRS_VAR: &str = "__LINGUO_DIRS";
/// Env var marking that linguo set JAVA_HOME, so it can be unset on leave.
const JAVA_VAR: &str = "__LINGUO_JAVA_HOME";

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum Shell {
    Zsh,
    Bash,
    Fish,
    Powershell,
}

pub fn activate(shell: Shell) {
    let script = match shell {
        Shell::Zsh => {
            r#"_linguo_hook() {
  eval "$(command linguo env --shell zsh)"
}
typeset -ag precmd_functions
if (( ! ${precmd_functions[(I)_linguo_hook]} )); then
  precmd_functions+=(_linguo_hook)
fi
_linguo_hook"#
        }
        Shell::Bash => {
            r#"_linguo_hook() {
  eval "$(command linguo env --shell bash)"
}
case ";${PROMPT_COMMAND:-};" in
  *";_linguo_hook;"*) ;;
  *) PROMPT_COMMAND="_linguo_hook${PROMPT_COMMAND:+;$PROMPT_COMMAND}" ;;
esac
_linguo_hook"#
        }
        Shell::Fish => {
            r#"function _linguo_hook --on-variable PWD
  command linguo env --shell fish | source
end
_linguo_hook"#
        }
        Shell::Powershell => {
            r#"function global:_linguo_hook {
  $linguoEnv = (linguo env --shell powershell | Out-String)
  if ($linguoEnv.Trim()) { Invoke-Expression $linguoEnv }
}
if (-not $Global:__linguo_original_prompt) {
  $Global:__linguo_original_prompt = $function:prompt
  function global:prompt {
    _linguo_hook
    & $Global:__linguo_original_prompt
  }
}
_linguo_hook"#
        }
    };
    println!("{script}");
}

/// Directories that should be on PATH for the current directory.
fn desired_dirs() -> Result<(Vec<PathBuf>, Option<PathBuf>)> {
    let cwd = std::env::current_dir()?;
    let mut dirs = Vec::new();
    // When auto-install is enabled, unsatisfied pins install on the spot.
    let auto = |language: &str, install: &dyn Fn(&str) -> anyhow::Result<()>| {
        crate::store::resolve_active_auto(language, &cwd, install)
    };
    if let Some((pin, version)) = auto(python::LANGUAGE, &|v| python::install(Some(v.into())))? {
        if let PinSource::Project(pin_file) = &pin.source {
            let project_dir = pin_file.parent().unwrap_or(&cwd);
            let venv_bin = python::project::venv_bin_dir(project_dir);
            if venv_bin.is_dir() {
                dirs.push(venv_bin);
            }
        }
        dirs.push(python::dist::bin_dir(&python::toolchain_path(&version)?));
    }
    if let Some((pin, version)) = auto(node::LANGUAGE, &|v| node::install(Some(v.into())))? {
        if let PinSource::Project(pin_file) = &pin.source {
            let local_bin = pin_file
                .parent()
                .unwrap_or(&cwd)
                .join("node_modules")
                .join(".bin");
            if local_bin.is_dir() {
                dirs.push(local_bin);
            }
        }
        dirs.push(node::dist::bin_dir(&node::toolchain_path(&version)?));
    }
    if let Some((_, version)) = auto(ruby::LANGUAGE, &|v| ruby::install(Some(v.into())))? {
        dirs.push(ruby::dist::bin_dir(&ruby::toolchain_path(&version)?));
    }
    if let Some((_, version)) = auto(go::LANGUAGE, &|v| go::install(Some(v.into())))? {
        dirs.push(go::dist::bin_dir(&go::toolchain_path(&version)?));
    }
    if let Some((pin, version)) = auto(php::LANGUAGE, &|v| php::install(Some(v.into())))? {
        if let PinSource::Project(pin_file) = &pin.source {
            let vendor_bin = pin_file.parent().unwrap_or(&cwd).join("vendor").join("bin");
            if vendor_bin.is_dir() {
                dirs.push(vendor_bin);
            }
        }
        dirs.push(php::dist::bin_dir(&php::toolchain_path(&version)?));
    }
    if let Some((_, version)) = auto(zig::LANGUAGE, &|v| zig::install(Some(v.into())))? {
        dirs.push(zig::dist::bin_dir(&zig::toolchain_path(&version)?));
    }
    if let Some((_, toolchain)) = rust::resolve_active_auto(&cwd)? {
        dirs.push(rust::dist::bin_dir(&rust::toolchain_dir(&toolchain)?));
    }
    if let Some((_, dist, version)) = terraform::resolve_active_auto(&cwd)? {
        dirs.push(terraform::dist::bin_dir(&terraform::toolchain_path(
            dist, &version,
        )?));
    }

    // Isolated developer tools: the exposed executables of every pinned tool
    // active here. Global `[tools.<lang>]` pins apply everywhere (pipx-style);
    // a project's linguo.toml adds or overrides them within the project.
    // (List grows as tool support fans out to node/go/rust/ruby.)
    #[allow(clippy::single_element_loop)]
    for language in [python::LANGUAGE] {
        dirs.extend(tools::active_exposed_dirs(language, &cwd)?);
    }

    // JVM stack: the jvm pin itself, then the JVM languages (each pushes its
    // own bin plus its bound JDK's bin; the plain jvm pin owns JAVA_HOME).
    let mut java_home: Option<PathBuf> = None;
    if let Some((_, version)) = auto(jvm::LANGUAGE, &|v| jvm::install(Some(v.into())))? {
        let toolchain = jvm::toolchain_path(&version)?;
        dirs.push(jvm::dist::bin_dir(&toolchain));
        java_home = Some(jvm::dist::java_home(&toolchain));
    }
    for def in [&kotlin::DEF, &groovy::DEF, &scala::DEF] {
        let installer = |v: &str| jvmlang::install(def, Some(v.into()));
        if let Some((_, version)) = auto(def.language, &installer)? {
            dirs.push(jvmlang::toolchain_bin(def, &version)?);
            if let Ok((_, home)) = jvm::resolve_for(def.language, &cwd) {
                let bin = home.join("bin");
                if !dirs.contains(&bin) {
                    dirs.push(bin);
                }
                if java_home.is_none() {
                    java_home = Some(home);
                }
            }
        }
    }
    Ok((dirs, java_home))
}

fn quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

pub fn env(shell: Shell) -> Result<()> {
    let (desired, java_home) = desired_dirs()?;
    let previous: Vec<PathBuf> = std::env::var(DIRS_VAR)
        .map(|v| std::env::split_paths(&v).collect())
        .unwrap_or_default();
    let previous_java = std::env::var(JAVA_VAR).ok().filter(|v| !v.is_empty());
    let desired_java = java_home.map(|p| p.display().to_string());
    if desired == previous && desired_java == previous_java {
        return Ok(());
    }

    let current_path = std::env::var_os("PATH").unwrap_or_default();
    let kept = std::env::split_paths(&current_path).filter(|dir| !previous.contains(dir));
    let new_dirs: Vec<PathBuf> = desired.iter().cloned().chain(kept).collect();
    let dirs_value = std::env::join_paths(desired.iter().cloned())?;
    let dirs_value = dirs_value.to_string_lossy();

    match shell {
        Shell::Zsh | Shell::Bash => {
            let new_path = std::env::join_paths(&new_dirs)?;
            println!("export PATH={}", quote(&new_path.to_string_lossy()));
            if desired.is_empty() {
                println!("unset {DIRS_VAR}");
            } else {
                println!("export {DIRS_VAR}={}", quote(&dirs_value));
            }
            match &desired_java {
                Some(home) => {
                    println!("export JAVA_HOME={}", quote(home));
                    println!("export {JAVA_VAR}={}", quote(home));
                }
                None if previous_java.is_some() => println!("unset JAVA_HOME {JAVA_VAR}"),
                None => {}
            }
        }
        Shell::Fish => {
            // fish's PATH is a list variable; set each directory as an element.
            let elements: Vec<String> = new_dirs
                .iter()
                .map(|d| quote(&d.to_string_lossy()))
                .collect();
            println!("set -gx PATH {}", elements.join(" "));
            if desired.is_empty() {
                println!("set -e {DIRS_VAR}");
            } else {
                println!("set -gx {DIRS_VAR} {}", quote(&dirs_value));
            }
            match &desired_java {
                Some(home) => {
                    println!("set -gx JAVA_HOME {}", quote(home));
                    println!("set -gx {JAVA_VAR} {}", quote(home));
                }
                None if previous_java.is_some() => {
                    println!("set -e JAVA_HOME; set -e {JAVA_VAR}")
                }
                None => {}
            }
        }
        Shell::Powershell => {
            let new_path = std::env::join_paths(&new_dirs)?;
            println!("$env:PATH = {}", quote_ps(&new_path.to_string_lossy()));
            if desired.is_empty() {
                println!("Remove-Item Env:\\{DIRS_VAR} -ErrorAction SilentlyContinue");
            } else {
                println!("$env:{DIRS_VAR} = {}", quote_ps(&dirs_value));
            }
            match &desired_java {
                Some(home) => {
                    println!("$env:JAVA_HOME = {}", quote_ps(home));
                    println!("$env:{JAVA_VAR} = {}", quote_ps(home));
                }
                None if previous_java.is_some() => println!(
                    "Remove-Item Env:\\JAVA_HOME, Env:\\{JAVA_VAR} -ErrorAction SilentlyContinue"
                ),
                None => {}
            }
        }
    }
    Ok(())
}

/// PowerShell single-quoted string: only `'` needs escaping (doubled).
fn quote_ps(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}
