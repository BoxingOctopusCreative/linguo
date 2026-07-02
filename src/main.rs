mod config;
mod exec;
mod fetch;
mod go;
mod node;
mod python;
mod shell;
mod status;
mod store;
mod terraform;
mod versions;

use clap::{Parser, Subcommand};

use shell::Shell;

#[derive(Parser)]
#[command(
    name = "linguo",
    version,
    about = "Multi-language runtime, package, and project manager"
)]
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
    /// Manage Node.js toolchains and projects
    Node {
        #[command(subcommand)]
        command: NodeCommand,
    },
    /// Manage Go toolchains and projects
    Go {
        #[command(subcommand)]
        command: GoCommand,
    },
    /// Manage Terraform toolchains
    #[command(alias = "tf")]
    Terraform {
        #[command(subcommand)]
        command: TerraformCommand,
    },
    /// Overview of all languages: installed toolchains and active pins
    #[command(alias = "list")]
    Status,
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
    /// Show which executable a command resolves to (default: python)
    Which { command: Option<String> },
    /// Run a command with the project venv and pinned toolchain on PATH
    Run {
        #[arg(trailing_var_arg = true, required = true)]
        args: Vec<String>,
    },
}

#[derive(Subcommand)]
enum NodeCommand {
    /// Download and install a toolchain (latest LTS if no version is given)
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
    /// Create a new project: package.json and version pin
    Init { name: Option<String> },
    /// npm install packages into the project
    Add { packages: Vec<String> },
    /// npm uninstall packages from the project
    Remove { packages: Vec<String> },
    /// Install everything package.json declares
    Sync,
    /// Show which executable a command resolves to (default: node)
    Which { command: Option<String> },
    /// Run a command with node_modules/.bin and the toolchain on PATH
    Run {
        #[arg(trailing_var_arg = true, required = true)]
        args: Vec<String>,
    },
}

#[derive(Subcommand)]
enum GoCommand {
    /// Download and install a toolchain (latest stable if no version is given)
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
    /// Create a new module: go mod init and version pin
    Init { module: Option<String> },
    /// go get packages into the module
    Add { packages: Vec<String> },
    /// Drop packages from the module (go get pkg@none)
    Remove { packages: Vec<String> },
    /// Download everything go.mod declares
    Sync,
    /// Show which executable a command resolves to (default: go)
    Which { command: Option<String> },
    /// Run a command with the pinned toolchain on PATH
    Run {
        #[arg(trailing_var_arg = true, required = true)]
        args: Vec<String>,
    },
}

#[derive(Subcommand)]
enum TerraformCommand {
    /// Download and install a toolchain (latest stable if no version is given)
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
    /// Show which executable a command resolves to (default: terraform)
    Which { command: Option<String> },
    /// Run a command with the pinned toolchain on PATH
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
            PythonCommand::Uninstall { version } => store::uninstall(python::LANGUAGE, &version),
            PythonCommand::List { available } => python::list(available),
            PythonCommand::Use { version, global } => {
                store::use_version(python::LANGUAGE, &version, global)
            }
            PythonCommand::Init { name } => python::project::init(name),
            PythonCommand::Add { packages } => python::project::add(&packages),
            PythonCommand::Remove { packages } => python::project::remove(&packages),
            PythonCommand::Sync => python::project::sync(),
            PythonCommand::Which { command } => python::project::which(command),
            PythonCommand::Run { args } => python::project::run(&args),
        },
        Command::Node { command } => match command {
            NodeCommand::Install { version } => node::install(version),
            NodeCommand::Uninstall { version } => store::uninstall(node::LANGUAGE, &version),
            NodeCommand::List { available } => node::list(available),
            NodeCommand::Use { version, global } => {
                store::use_version(node::LANGUAGE, &version, global)
            }
            NodeCommand::Init { name } => node::project::init(name),
            NodeCommand::Add { packages } => node::project::add(&packages),
            NodeCommand::Remove { packages } => node::project::remove(&packages),
            NodeCommand::Sync => node::project::sync(),
            NodeCommand::Which { command } => node::project::which(command),
            NodeCommand::Run { args } => node::project::run(&args),
        },
        Command::Go { command } => match command {
            GoCommand::Install { version } => go::install(version),
            GoCommand::Uninstall { version } => store::uninstall(go::LANGUAGE, &version),
            GoCommand::List { available } => go::list(available),
            GoCommand::Use { version, global } => {
                store::use_version(go::LANGUAGE, &version, global)
            }
            GoCommand::Init { module } => go::project::init(module),
            GoCommand::Add { packages } => go::project::add(&packages),
            GoCommand::Remove { packages } => go::project::remove(&packages),
            GoCommand::Sync => go::project::sync(),
            GoCommand::Which { command } => go::project::which(command),
            GoCommand::Run { args } => go::project::run(&args),
        },
        Command::Terraform { command } => match command {
            TerraformCommand::Install { version } => terraform::install(version),
            TerraformCommand::Uninstall { version } => {
                store::uninstall(terraform::LANGUAGE, &version)
            }
            TerraformCommand::List { available } => terraform::list(available),
            TerraformCommand::Use { version, global } => {
                store::use_version(terraform::LANGUAGE, &version, global)
            }
            TerraformCommand::Which { command } => terraform::which(command),
            TerraformCommand::Run { args } => terraform::run(&args),
        },
        Command::Status => status::status(),
        Command::Activate { shell } => {
            shell::activate(shell);
            Ok(())
        }
        Command::Env { shell } => shell::env(shell),
    }
}
