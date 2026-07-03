//! Shell integration: `linguo activate <shell>` emits a hook that re-runs
//! `linguo env --shell <shell>` on every prompt / directory change, and `env`
//! prints the PATH updates needed for the current directory's pins.

use std::path::PathBuf;

use anyhow::Result;
use clap::ValueEnum;

use crate::config::PinSource;
use crate::{go, node, python, ruby, rust, terraform};

/// Env var tracking which directories linguo has prepended to PATH, so they
/// can be removed again when the active project changes.
const DIRS_VAR: &str = "__LINGUO_DIRS";

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
fn desired_dirs() -> Result<Vec<PathBuf>> {
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
    if let Some((_, toolchain)) = rust::resolve_active_auto(&cwd)? {
        dirs.push(rust::dist::bin_dir(&rust::toolchain_dir(&toolchain)?));
    }
    if let Some((_, dist, version)) = terraform::resolve_active_auto(&cwd)? {
        dirs.push(terraform::dist::bin_dir(&terraform::toolchain_path(
            dist, &version,
        )?));
    }
    Ok(dirs)
}

fn quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

pub fn env(shell: Shell) -> Result<()> {
    let desired = desired_dirs()?;
    let previous: Vec<PathBuf> = std::env::var(DIRS_VAR)
        .map(|v| std::env::split_paths(&v).collect())
        .unwrap_or_default();
    if desired == previous {
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
        }
        Shell::Powershell => {
            let new_path = std::env::join_paths(&new_dirs)?;
            println!("$env:PATH = {}", quote_ps(&new_path.to_string_lossy()));
            if desired.is_empty() {
                println!("Remove-Item Env:\\{DIRS_VAR} -ErrorAction SilentlyContinue");
            } else {
                println!("$env:{DIRS_VAR} = {}", quote_ps(&dirs_value));
            }
        }
    }
    Ok(())
}

/// PowerShell single-quoted string: only `'` needs escaping (doubled).
fn quote_ps(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}
