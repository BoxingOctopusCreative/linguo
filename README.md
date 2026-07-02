# Linguo

Linguo is a cross-platform, multi-language runtime, package, and project manager — think
[`uv`](https://github.com/astral-sh/uv), but for **Python, Node.js, Ruby, Rust, Go, and Terraform/OpenTofu**.

One binary manages runtime versions, per-project pins, and project workflows
for every language, with the same command shape everywhere:

```
linguo <language> <command>
```

| Language | Runtime source | Project layer |
|---|---|---|
| Python | [python-build-standalone](https://github.com/astral-sh/python-build-standalone) | pyproject.toml + pip-backed `.venv` |
| Node.js | [nodejs.org/dist](https://nodejs.org/dist) | package.json via npm |
| Ruby | [rv-ruby](https://github.com/spinel-coop/rv-ruby) relocatable builds | Gemfile via bundler (shared per-toolchain gems) |
| Rust | [static.rust-lang.org](https://static.rust-lang.org) dist channels | Cargo.toml via cargo |
| Go | [go.dev/dl](https://go.dev/dl) | go.mod via the go tool |
| Terraform / OpenTofu | [releases.hashicorp.com](https://releases.hashicorp.com) / [get.opentofu.org](https://get.opentofu.org) | runtime-only (providers stay terraform's job) |

Every download is sha256-verified against its upstream's published checksums.
Toolchains live under `~/.linguo/toolchains/<language>/<version>` (override
with `$LINGUO_ROOT`).

Prebuilt binaries for macOS (arm64/x86_64), Linux (x64/arm64), and Windows
(x64) are on the [releases page](https://github.com/BoxingOctopusCreative/linguo/releases).
Ruby is not yet available on Windows (no upstream relocatable builds).

## Install

Download a release binary, or build from source:

```sh
cargo install --path .
```

Then add the shell hook to your rc file so pinned runtimes activate
automatically when you `cd` into a project:

```sh
eval "$(linguo activate zsh)"   # or bash / fish
```

On Windows (PowerShell), add this to your `$PROFILE` instead:

```powershell
linguo activate powershell | Out-String | Invoke-Expression
```

## Usage

Every language gets the same runtime commands:

```sh
linguo <lang> install [version]   # sensible default: latest / latest LTS / latest stable
linguo <lang> list                # installed; --available for what's downloadable
linguo <lang> use 3.12            # pin for this directory (writes linguo.toml)
linguo <lang> use 3.12 --global   # default for everything else
linguo <lang> uninstall 3.12.4
linguo <lang> which [command]     # path a command resolves to
linguo <lang> run -- <command>    # run with the pinned toolchain on PATH
linguo status                     # cross-language overview (alias: linguo list)
```

And, where the language has a project/package layer, the uv-style project
commands (each drives the ecosystem's native tool — pip, npm, bundler, cargo,
go — rather than reimplementing it):

```sh
linguo python init                # pyproject.toml + linguo.toml pin + .venv
linguo python add "requests>=2.31"
linguo node add typescript && linguo node run -- tsc --version
linguo ruby add rails
linguo rust add serde && linguo rust run -- cargo build
linguo go add rsc.io/quote
linguo <lang> remove <pkg>
linguo <lang> sync                # install everything the manifest declares
```

Terraform and OpenTofu share one command (`linguo tf` works too); OpenTofu
versions are spelled `opentofu@<version>` and resolve the `tofu` binary:

```sh
linguo tf install opentofu@1.12
linguo tf use opentofu@1.12       # writes terraform = "opentofu@1.12"
linguo tf run -- tofu plan
```

### Version pins

Pins live in `linguo.toml`, resolved from the nearest one up the directory
tree, then the global config (`~/.linguo/config.toml`):

```toml
[runtimes]
python = "3.12"
node = "24"
rust = "1.96"
terraform = "opentofu@1.12"
```

Requests can be a major (`24`), minor (`3.12`), or exact (`1.96.1`) version;
the highest installed match wins. For Rust, a rustup-convention
`rust-toolchain.toml` whose channel is a plain version is honored as a
fallback pin when no `linguo.toml` covers rust.

## Road to 1.0

Roughly in order:

- **Ecosystem pin-file fallbacks** — read `.nvmrc`, `.python-version`,
  `.ruby-version`, and `go.mod` toolchain directives the way
  `rust-toolchain.toml` already works, so existing projects activate without
  a `linguo.toml`.
- **`linguo <lang> upgrade`** — bump a pin (and install the newer toolchain)
  in one step; prune superseded toolchains.
- **Auto-install on activation** — opt-in: entering a project with an
  unsatisfied pin installs it instead of erroring.
- **Rust channels and components** — nightly/beta toolchains, extra
  components (`rust-analyzer`, `rust-src`), and cross-compilation targets
  from the same dist manifests.
- **Ruby on more platforms** — musl Linux builds (already published by
  rv-ruby), and a Windows story (RubyInstaller-based).
- **Windows arm64 binaries** — the backends already map the targets; needs a
  release lane and CI coverage.
- **`linguo self update`** and a curl-able install script.
- **Workspace/monorepo ergonomics** — one `linguo sync` for a repo pinning
  several languages at once.

## Contributing

`cargo test` runs the unit suite; CI additionally runs an end-to-end smoke
test (real toolchain installs and project flows) on Linux and Windows for
every push. Releases are cut from the Actions tab via the Release workflow,
which tags, builds all five platform binaries, and generates notes from
commit messages.
