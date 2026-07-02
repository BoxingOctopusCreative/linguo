//! Shell integration: `linguo activate <shell>` emits a hook that re-runs
//! `linguo env --shell <shell>` on every prompt / directory change, and `env`
//! prints the PATH updates needed for the current directory's pins.

use std::path::PathBuf;

use anyhow::Result;
use clap::ValueEnum;

use crate::python;

/// Env var tracking which directories linguo has prepended to PATH, so they
/// can be removed again when the active project changes.
const DIRS_VAR: &str = "__LINGUO_DIRS";

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum Shell {
    Zsh,
    Bash,
    Fish,
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
    };
    println!("{script}");
}

/// Directories that should be on PATH for the current directory.
fn desired_dirs() -> Result<Vec<PathBuf>> {
    let cwd = std::env::current_dir()?;
    let mut dirs = Vec::new();
    if let Some((pin, version)) = python::resolve_active(&cwd)? {
        if let crate::config::PinSource::Project(pin_file) = &pin.source {
            let project_dir = pin_file.parent().unwrap_or(&cwd);
            let venv_bin = python::project::venv_bin_dir(project_dir);
            if venv_bin.is_dir() {
                dirs.push(venv_bin);
            }
        }
        dirs.push(python::dist::bin_dir(&python::toolchain_path(&version)?));
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
    }
    Ok(())
}
