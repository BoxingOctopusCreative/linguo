# Linguo

![Linguo Logo](https://assets.linguo.run/brand/linguo_wide.png)

[![Release](https://img.shields.io/github/v/release/BoxingOctopusCreative/linguo)](https://github.com/BoxingOctopusCreative/linguo/releases)
[![CI](https://github.com/BoxingOctopusCreative/linguo/actions/workflows/ci.yml/badge.svg)](https://github.com/BoxingOctopusCreative/linguo/actions/workflows/ci.yml)
[![License: MPL-2.0](https://img.shields.io/badge/license-MPL--2.0-blue)](LICENSE)

Linguo is a cross-platform, multi-language runtime, package, and project manager: think
[`uv`](https://github.com/astral-sh/uv), but for **Python, Node.js, Ruby, PHP, Rust, Go, Zig, and Terraform/OpenTofu**.

One binary manages runtime versions, per-project pins, and project workflows
for every language, with the same command shape everywhere:

```
linguo <language> <command>
```

| Language | Runtime source | Project layer |
|---|---|---|
| Python | [python-build-standalone](https://github.com/astral-sh/python-build-standalone) | pyproject.toml + pip-backed `.venv` |
| Node.js | [nodejs.org/dist](https://nodejs.org/dist) | package.json via npm |
| Ruby | [rv-ruby](https://github.com/spinel-coop/rv-ruby) relocatable builds; [RubyInstaller](https://rubyinstaller.org) on Windows | Gemfile via bundler (shared per-toolchain gems) |
| Rust | [static.rust-lang.org](https://static.rust-lang.org) dist channels | Cargo.toml via cargo |
| Go | [go.dev/dl](https://go.dev/dl) | go.mod via the go tool |
| Zig | [ziglang.org](https://ziglang.org/download) (static, musl-friendly) | build.zig.zon via the zig tool |
| PHP | [static-php-cli](https://dl.static-php.dev) builds (static); [windows.php.net](https://windows.php.net) on Windows | composer.json via bundled Composer |
| Terraform / OpenTofu | [releases.hashicorp.com](https://releases.hashicorp.com) / [get.opentofu.org](https://get.opentofu.org) | runtime-only (providers stay terraform's job) |

Every download is sha256-verified against its upstream's published checksums.
Toolchains live under `~/.linguo/toolchains/<language>/<version>` (override
with `$LINGUO_ROOT`).

Prebuilt binaries for macOS (arm64/x86_64), Linux (x64/arm64, glibc and
fully static musl), and Windows (x64) are on the
[releases page](https://github.com/BoxingOctopusCreative/linguo/releases).
On musl systems (Alpine and friends), Python, Ruby, Rust, and
Terraform/OpenTofu work natively; Node.js and Go publish no official musl
builds, so linguo points you at the distro package instead. On Windows, Ruby
comes from RubyInstaller archives (without the MSYS2 devkit, so gems with C
extensions need a separate toolchain).

## Install

Four ways in, pick one:

```sh
# Homebrew (macOS/Linux)
brew tap boxingoctopuscreative/tap && brew install linguo

# curl install script (macOS/Linux, glibc or musl)
curl -fsSL https://raw.githubusercontent.com/BoxingOctopusCreative/linguo/main/install.sh | sh

# native packages: deb, rpm, and a Windows MSI on the releases page

# from source
cargo install --path .
```

The curl script detects your platform, downloads the latest release tarball,
verifies its checksum, and installs the binary to `~/.local/bin`. Override
the destination with `LINGUO_INSTALL_DIR`, or pin a version with
`LINGUO_VERSION=0.9.0` (or `sh install.sh 0.9.0`).

In CI or anywhere GitHub API rate limits bite, set `GITHUB_TOKEN` (or
`LINGUO_GITHUB_TOKEN`): linguo and the install script authenticate their
api.github.com queries with it, and never send it anywhere else.

The tap's formula is updated automatically by the release pipeline (each
release also attaches the generated `linguo.rb`, kept at
[packaging/homebrew/linguo.rb](packaging/homebrew/linguo.rb)).

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
linguo php add monolog/monolog    # composer, bundled with every php toolchain
linguo rust add serde && linguo rust run -- cargo build
linguo go add rsc.io/quote
linguo <lang> remove <pkg>
linguo <lang> sync                # install everything the manifest declares
```

Monorepos sync in one shot: `linguo sync` at the top level finds every member
project (or honors `[workspace] members = ["services/*", "web"]` in the root
linguo.toml, globs allowed), installs any missing pinned toolchains, and runs
each member's dependency sync. Directories holding .tf files count as
toolchain-only members:

```sh
linguo sync                       # fresh clone -> every member runnable
```

Terraform and OpenTofu share one command (`linguo tf` works too); OpenTofu
versions are spelled `opentofu@<version>` and resolve the `tofu` binary:

```sh
linguo tf install opentofu@1.12
linguo tf use opentofu@1.12       # writes terraform = "opentofu@1.12"
linguo tf run -- tofu plan
```

Rust additionally understands rustup-style channels, components, and targets;
a project's `rust-toolchain.toml` `components`/`targets` arrays are honored
automatically at install time:

```sh
linguo rust install nightly            # today's; nightly-2026-07-01 for a date
linguo rust use nightly                # activates the newest installed nightly
linguo rust component add rust-analyzer rust-src
linguo rust target add wasm32-unknown-unknown
```

Zig projects work the same way (`linguo zig init/sync/run/which`); `add`
wraps `zig fetch --save`, which takes archive URLs or paths rather than
registry names.


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
the highest installed match wins. Rust pins may also be channels: `stable`,
`nightly`, `beta`, or dated builds like `nightly-2026-07-01`. Bare channel
pins resolve to the newest *installed* build of that kind, so activation
stays offline and deterministic; `linguo rust upgrade` is what moves them
forward.

Existing projects work without a `linguo.toml`: when none covers a language,
linguo honors the ecosystem's own pin file (`.python-version`, `.nvmrc` /
`.node-version`, `.ruby-version`, go.mod's `toolchain`/`go` directives,
`rust-toolchain(.toml)`, `.zigversion`, build.zig.zon's
`minimum_zig_version`, and `.php-version`), as long as it holds a plain version (or, for
rust, a channel; node aliases like `lts/*` are still ignored). Precedence:
project `linguo.toml`, then the ecosystem pin file, then the global config.

## Roadmap

Next up, in release order:

- **1.3.0 Java and JDK-based languages**: JDK management plus Kotlin,
  Groovy, and Scala.

Then, under consideration:

- **Unit-testing framework support** for the managed languages (pairs with
  developer tool management below).
- **Windows arm64 binaries**: the backends already map the targets; needs a
  release lane and CI coverage.
- **Developer tool management**: install linters, formatters, and test
  runners through linguo (`linguo python tool install ruff`,
  `linguo node tool install eslint`, `linguo go tool install golangci-lint`,
  ...), each in its own isolated environment with its executables on PATH.
  That's pipx / `uv tool` semantics, but for every managed language. Tools would pin
  and upgrade like runtimes do, so a repo can declare its lint stack the same
  way it declares its toolchains.

## Contributing

`cargo test` runs the unit suite; CI additionally runs end-to-end smoke
tests (real toolchain installs and project flows) on Linux, on musl inside
an Alpine container, and on Windows for every push, and builds the deb, rpm,
and MSI packages so packaging can't rot between releases. Releases are cut
from the Actions tab via the Release workflow, which bumps the version, tags,
builds binaries for all seven platform lanes, packages deb/rpm/MSI, publishes
with notes generated from commit messages, and pushes the regenerated
Homebrew formula to the tap.
