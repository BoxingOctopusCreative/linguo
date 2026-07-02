# Linguo

![Linguo Logo](https://assets.linguo.run/brand/linguo_wide.png)

Linguo is a cross-platform, multi-language runtime, package, and project manager: think
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

Download a release binary (deb/rpm/MSI packages and tarballs are on the
releases page), install via Homebrew from a personal tap (every release
attaches a ready-made `linguo.rb` formula, also kept at
[packaging/homebrew/linguo.rb](packaging/homebrew/linguo.rb)), or build from
source:

```sh
brew install <your-tap>/linguo   # after adding linguo.rb to your tap
cargo install --path .           # from a checkout
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

Optionally, let the hook install unsatisfied pins on the spot (cd into a
fresh clone and the pinned toolchains just appear). It's off by default and
gated on the machine-level config (a cloned repo can't trigger downloads by
itself), and failed attempts back off for 5 minutes so an unreachable
upstream never stalls your prompt:

```toml
# ~/.linguo/config.toml
[settings]
auto-install = true
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

# Upgrades: newest release within the pin by default, or bump the pin itself
linguo node upgrade               # pin "22" -> installs the newest 22.x
linguo node upgrade --latest      # bumps the pin (22 -> 24) and installs it,
                                  # rewriting whichever file held the pin
linguo node upgrade --prune       # also uninstall the superseded toolchains
linguo upgrade                    # all languages pinned in this directory
```

And, where the language has a project/package layer, the uv-style project
commands (each drives the ecosystem's native tool, whether pip, npm, bundler,
cargo, or go, rather than reimplementing it):

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
the highest installed match wins.

Existing projects work without a `linguo.toml`: when none covers a language,
linguo honors the ecosystem's own pin file (`.python-version`, `.nvmrc` /
`.node-version`, `.ruby-version`, go.mod's `toolchain`/`go` directives, and
`rust-toolchain(.toml)`), as long as it holds a plain version (aliases like
`lts/*` or `stable` are ignored). Precedence: project `linguo.toml`, then the
ecosystem pin file, then the global config.

## Road to 1.0

Roughly in order:

- **Rust channels and components**: nightly/beta toolchains, extra
  components (`rust-analyzer`, `rust-src`), and cross-compilation targets
  from the same dist manifests.
- **Ruby on more platforms**: musl Linux builds (already published by
  rv-ruby), and a Windows story (RubyInstaller-based).
- **Windows arm64 binaries**: the backends already map the targets; needs a
  release lane and CI coverage.
- **A curl-able install script** for platforms not covered by the
  deb/rpm/MSI packages or the Homebrew formula; updating linguo itself
  stays the package manager's job.
- **Workspace/monorepo ergonomics**: one `linguo sync` for a repo pinning
  several languages at once.

## After 1.0

- **Developer tool management**: install linters, formatters, and test
  runners through linguo (`linguo python tool install ruff`,
  `linguo node tool install eslint`, `linguo go tool install golangci-lint`,
  ...), each in its own isolated environment with its executables on PATH.
  That's pipx / `uv tool` semantics, but for every managed language. Tools would pin
  and upgrade like runtimes do, so a repo can declare its lint stack the same
  way it declares its toolchains.

## Contributing

`cargo test` runs the unit suite; CI additionally runs an end-to-end smoke
test (real toolchain installs and project flows) on Linux and Windows for
every push. Releases are cut from the Actions tab via the Release workflow,
which tags, builds all five platform binaries, and generates notes from
commit messages.
