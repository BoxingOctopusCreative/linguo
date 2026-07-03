//! Process helpers shared by the per-language `run`/`which` commands.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result};

/// `name` with the platform's executable extension (`go` -> `go.exe`).
pub fn exe(name: &str) -> String {
    if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.to_string()
    }
}

/// Locate an executable named `name` in `dir`, trying Windows executable
/// extensions when `name` has none.
pub fn find_in_dir(dir: &Path, name: &str) -> Option<PathBuf> {
    let direct = dir.join(name);
    if is_executable(&direct) {
        return Some(direct);
    }
    if cfg!(windows) && Path::new(name).extension().is_none() {
        for ext in ["exe", "cmd", "bat"] {
            let candidate = dir.join(format!("{name}.{ext}"));
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

/// A Command for `program`, resolved against linguo-managed dirs first with
/// Windows executable extensions handled (.bat/.cmd tools like composer or
/// gem binstubs are invisible to CreateProcess's PATH search, which only
/// appends .exe). Unresolved names fall back to normal PATH search.
pub fn command_in(dirs: &[PathBuf], program: &str) -> Command {
    for dir in dirs {
        if let Some(path) = find_in_dir(dir, program) {
            return Command::new(path);
        }
    }
    Command::new(program)
}

pub fn is_executable(path: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        path.metadata()
            .is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
    }
    #[cfg(not(unix))]
    {
        path.is_file()
    }
}

/// Replace this process with `cmd` (on unix), or run it and exit with its
/// status code.
pub fn exec(mut cmd: Command, program: &str) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        Err(cmd.exec()).with_context(|| format!("failed to run {program}"))
    }
    #[cfg(not(unix))]
    {
        let status = cmd
            .status()
            .with_context(|| format!("failed to run {program}"))?;
        std::process::exit(status.code().unwrap_or(1));
    }
}
