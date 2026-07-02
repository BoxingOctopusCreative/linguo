mod config;
mod python;
mod shell;
mod versions;

use clap::{Parser, Subcommand};

use shell::Shell;

#[derive(Parser)]
#[command(name = "linguo", version, about = "Multi-language runtime, package, and project manager")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Manage Python toolchains and projects
    Python {
        #[command(subcommand)]
        command: PythonCommand,
    },
    /// Print the shell hook (add `eval "$(linguo activate zsh)"` to your rc file)
    Activate { shell: Shell },
    /// Print PATH updates for the current directory (used by the shell hook)
    #[command(hide = true)]
    Env {
        #[arg(long)]
        shell: Shell,
    },
}

#[derive(Subcommand)]
enum PythonCommand {
    /// Download and install a toolchain (latest if no version is given)
    Install { version: Option<String> },
    /// Remove an installed toolchain
    Uninstall { version: String },
    /// List installed toolchains
    List {
        /// List versions available for download instead
        #[arg(long)]
        available: bool,
    },
    /// Pin a version for this directory (or globally)
    Use {
        version: String,
        #[arg(long)]
        global: bool,
    },
    /// Create a new project: pyproject.toml, version pin, and venv
    Init { name: Option<String> },
    /// Install packages into the project venv and add them to pyproject.toml
    Add { packages: Vec<String> },
    /// Uninstall packages and remove them from pyproject.toml
    Remove { packages: Vec<String> },
    /// Install everything pyproject.toml declares into the project venv
    Sync,
    /// Run a command with the project venv and pinned toolchain on PATH
    Run {
        #[arg(trailing_var_arg = true, required = true)]
        args: Vec<String>,
    },
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Python { command } => match command {
            PythonCommand::Install { version } => python::install(version),
            PythonCommand::Uninstall { version } => python::uninstall(&version),
            PythonCommand::List { available } => python::list(available),
            PythonCommand::Use { version, global } => python::use_version(&version, global),
            PythonCommand::Init { name } => python::project::init(name),
            PythonCommand::Add { packages } => python::project::add(&packages),
            PythonCommand::Remove { packages } => python::project::remove(&packages),
            PythonCommand::Sync => python::project::sync(),
            PythonCommand::Run { args } => python::project::run(&args),
        },
        Command::Activate { shell } => {
            shell::activate(shell);
            Ok(())
        }
        Command::Env { shell } => shell::env(shell),
    }
}
