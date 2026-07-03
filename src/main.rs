mod config;
mod exec;
mod fetch;
mod go;
mod node;
mod python;
mod ruby;
mod rust;
mod shell;
mod status;
mod store;
mod terraform;
mod versions;
mod workspace;
mod zig;

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
    /// Manage Ruby toolchains and projects
    Ruby {
        #[command(subcommand)]
        command: RubyCommand,
    },
    /// Manage Rust toolchains and projects
    Rust {
        #[command(subcommand)]
        command: RustCommand,
    },
    /// Manage Zig toolchains and projects
    Zig {
        #[command(subcommand)]
        command: ZigCommand,
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
    /// Sync every workspace member: install missing pinned toolchains, then
    /// run each member project's dependency sync
    Sync,
    /// Upgrade every language pinned in this directory
    Upgrade {
        /// Bump each pin to the newest stable release (same granularity)
        #[arg(long)]
        latest: bool,
        /// Uninstall older toolchains the pins previously matched
        #[arg(long)]
        prune: bool,
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
    /// Upgrade the pinned toolchain: newest release within the pin, or bump
    /// the pin itself with --latest
    Upgrade {
        /// Bump the pin to the newest stable release (same granularity)
        #[arg(long)]
        latest: bool,
        /// Uninstall older toolchains the pin previously matched
        #[arg(long)]
        prune: bool,
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
    /// Upgrade the pinned toolchain: newest release within the pin, or bump
    /// the pin itself with --latest
    Upgrade {
        /// Bump the pin to the newest stable release (same granularity)
        #[arg(long)]
        latest: bool,
        /// Uninstall older toolchains the pin previously matched
        #[arg(long)]
        prune: bool,
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
    /// Upgrade the pinned toolchain: newest release within the pin, or bump
    /// the pin itself with --latest
    Upgrade {
        /// Bump the pin to the newest stable release (same granularity)
        #[arg(long)]
        latest: bool,
        /// Uninstall older toolchains the pin previously matched
        #[arg(long)]
        prune: bool,
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
enum RubyCommand {
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
    /// Upgrade the pinned toolchain: newest release within the pin, or bump
    /// the pin itself with --latest
    Upgrade {
        /// Bump the pin to the newest stable release (same granularity)
        #[arg(long)]
        latest: bool,
        /// Uninstall older toolchains the pin previously matched
        #[arg(long)]
        prune: bool,
    },
    /// Create a new project: Gemfile and version pin
    Init,
    /// bundle add gems to the project
    Add { gems: Vec<String> },
    /// bundle remove gems from the project
    Remove { gems: Vec<String> },
    /// Install everything the Gemfile declares (bundle install)
    Sync,
    /// Show which executable a command resolves to (default: ruby)
    Which { command: Option<String> },
    /// Run a command with the pinned toolchain and its gems on PATH
    Run {
        #[arg(trailing_var_arg = true, required = true)]
        args: Vec<String>,
    },
}

#[derive(Subcommand)]
enum RustCommand {
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
    /// Upgrade the pinned toolchain: newest release within the pin, or bump
    /// the pin itself with --latest
    Upgrade {
        /// Bump the pin to the newest stable release (same granularity)
        #[arg(long)]
        latest: bool,
        /// Uninstall older toolchains the pin previously matched
        #[arg(long)]
        prune: bool,
    },
    /// Create a new project: cargo init and version pin
    Init { name: Option<String> },
    /// cargo add crates to the project
    Add { crates: Vec<String> },
    /// cargo remove crates from the project
    Remove { crates: Vec<String> },
    /// Download everything Cargo.toml declares (cargo fetch)
    Sync,
    /// Show which executable a command resolves to (default: cargo)
    Which { command: Option<String> },
    /// Run a command with the pinned toolchain on PATH
    Run {
        #[arg(trailing_var_arg = true, required = true)]
        args: Vec<String>,
    },
    /// Manage the active toolchain's components (rust-analyzer, rust-src, ...)
    Component {
        #[command(subcommand)]
        command: RustComponentCommand,
    },
    /// Manage the active toolchain's cross-compilation targets
    Target {
        #[command(subcommand)]
        command: RustTargetCommand,
    },
}

#[derive(Subcommand)]
enum RustComponentCommand {
    /// Install components into the active toolchain
    Add { names: Vec<String> },
}

#[derive(Subcommand)]
enum RustTargetCommand {
    /// Install rust-std for extra target triples into the active toolchain
    Add { triples: Vec<String> },
}

#[derive(Subcommand)]
enum ZigCommand {
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
    /// Upgrade the pinned toolchain: newest release within the pin, or bump
    /// the pin itself with --latest
    Upgrade {
        /// Bump the pin to the newest stable release (same granularity)
        #[arg(long)]
        latest: bool,
        /// Uninstall older toolchains the pin previously matched
        #[arg(long)]
        prune: bool,
    },
    /// Create a new project: zig init and version pin
    Init,
    /// zig fetch --save packages (archive URLs or paths) into the project
    Add { packages: Vec<String> },
    /// Fetch everything build.zig.zon declares (zig build --fetch)
    Sync,
    /// Show which executable a command resolves to (default: zig)
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
    /// Upgrade the pinned toolchain: newest release within the pin, or bump
    /// the pin itself with --latest
    Upgrade {
        /// Bump the pin to the newest stable release (same granularity)
        #[arg(long)]
        latest: bool,
        /// Uninstall older toolchains the pin previously matched
        #[arg(long)]
        prune: bool,
    },
    /// Show which executable a command resolves to (default: terraform)
    Which { command: Option<String> },
    /// Run a command with the pinned toolchain on PATH
    Run {
        #[arg(trailing_var_arg = true, required = true)]
        args: Vec<String>,
    },
}

/// Upgrade every language with a resolvable pin in the current directory.
fn upgrade_all(latest: bool, prune: bool) -> anyhow::Result<()> {
    let cwd = std::env::current_dir()?;
    let mut failures: Vec<String> = Vec::new();
    let mut any = false;
    type UpgradeFn = fn(bool, bool) -> anyhow::Result<()>;
    let languages: [(&str, UpgradeFn); 6] = [
        (python::LANGUAGE, python::upgrade),
        (node::LANGUAGE, node::upgrade),
        (ruby::LANGUAGE, ruby::upgrade),
        (go::LANGUAGE, go::upgrade),
        (rust::LANGUAGE, rust::upgrade),
        (zig::LANGUAGE, zig::upgrade),
    ];
    for (language, upgrade) in languages {
        if store::resolve_pin(language, &cwd)?.is_none() {
            continue;
        }
        any = true;
        println!("{language}:");
        if let Err(err) = upgrade(latest, prune) {
            eprintln!("  {err:#}");
            failures.push(language.to_string());
        }
    }
    if config::resolve_pin(terraform::LANGUAGE, &cwd)?.is_some() {
        any = true;
        println!("terraform:");
        if let Err(err) = terraform::upgrade(latest, prune) {
            eprintln!("  {err:#}");
            failures.push("terraform".to_string());
        }
    }
    if !any {
        println!("nothing pinned here (run `linguo <language> use <version>` first)");
    }
    if !failures.is_empty() {
        anyhow::bail!("upgrade failed for: {}", failures.join(", "));
    }
    Ok(())
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
            PythonCommand::Upgrade { latest, prune } => python::upgrade(latest, prune),
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
            NodeCommand::Upgrade { latest, prune } => node::upgrade(latest, prune),
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
            GoCommand::Upgrade { latest, prune } => go::upgrade(latest, prune),
            GoCommand::Init { module } => go::project::init(module),
            GoCommand::Add { packages } => go::project::add(&packages),
            GoCommand::Remove { packages } => go::project::remove(&packages),
            GoCommand::Sync => go::project::sync(),
            GoCommand::Which { command } => go::project::which(command),
            GoCommand::Run { args } => go::project::run(&args),
        },
        Command::Ruby { command } => match command {
            RubyCommand::Install { version } => ruby::install(version),
            RubyCommand::Uninstall { version } => store::uninstall(ruby::LANGUAGE, &version),
            RubyCommand::List { available } => ruby::list(available),
            RubyCommand::Use { version, global } => {
                store::use_version(ruby::LANGUAGE, &version, global)
            }
            RubyCommand::Upgrade { latest, prune } => ruby::upgrade(latest, prune),
            RubyCommand::Init => ruby::project::init(),
            RubyCommand::Add { gems } => ruby::project::add(&gems),
            RubyCommand::Remove { gems } => ruby::project::remove(&gems),
            RubyCommand::Sync => ruby::project::sync(),
            RubyCommand::Which { command } => ruby::project::which(command),
            RubyCommand::Run { args } => ruby::project::run(&args),
        },
        Command::Rust { command } => match command {
            RustCommand::Install { version } => rust::install(version),
            RustCommand::Uninstall { version } => rust::uninstall(&version),
            RustCommand::List { available } => rust::list(available),
            RustCommand::Use { version, global } => rust::use_version(&version, global),
            RustCommand::Upgrade { latest, prune } => rust::upgrade(latest, prune),
            RustCommand::Init { name } => rust::project::init(name),
            RustCommand::Add { crates } => rust::project::add(&crates),
            RustCommand::Remove { crates } => rust::project::remove(&crates),
            RustCommand::Sync => rust::project::sync(),
            RustCommand::Which { command } => rust::project::which(command),
            RustCommand::Run { args } => rust::project::run(&args),
            RustCommand::Component { command } => match command {
                RustComponentCommand::Add { names } => rust::component_add(&names),
            },
            RustCommand::Target { command } => match command {
                RustTargetCommand::Add { triples } => rust::target_add(&triples),
            },
        },
        Command::Zig { command } => match command {
            ZigCommand::Install { version } => zig::install(version),
            ZigCommand::Uninstall { version } => store::uninstall(zig::LANGUAGE, &version),
            ZigCommand::List { available } => zig::list(available),
            ZigCommand::Use { version, global } => {
                store::use_version(zig::LANGUAGE, &version, global)
            }
            ZigCommand::Upgrade { latest, prune } => zig::upgrade(latest, prune),
            ZigCommand::Init => zig::project::init(),
            ZigCommand::Add { packages } => zig::project::add(&packages),
            ZigCommand::Sync => zig::project::sync(),
            ZigCommand::Which { command } => zig::project::which(command),
            ZigCommand::Run { args } => zig::project::run(&args),
        },
        Command::Terraform { command } => match command {
            TerraformCommand::Install { version } => terraform::install(version),
            TerraformCommand::Uninstall { version } => terraform::uninstall(&version),
            TerraformCommand::List { available } => terraform::list(available),
            TerraformCommand::Use { version, global } => terraform::use_version(&version, global),
            TerraformCommand::Upgrade { latest, prune } => terraform::upgrade(latest, prune),
            TerraformCommand::Which { command } => terraform::which(command),
            TerraformCommand::Run { args } => terraform::run(&args),
        },
        Command::Sync => workspace::sync(),
        Command::Status => status::status(),
        Command::Upgrade { latest, prune } => upgrade_all(latest, prune),
        Command::Activate { shell } => {
            shell::activate(shell);
            Ok(())
        }
        Command::Env { shell } => shell::env(shell),
    }
}
