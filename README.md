# linguo

Cross-platform, multi-language runtime, package, and project manager — think
uv, but for Python, Go, Node.js, Ruby, Rust, and Terraform.

Commands follow the shape `linguo <language> <command>`. Python, Node.js, Go,
and Terraform are supported so far; the other languages will follow the same
command surface.

## Install

```sh
cargo install --path .
```

Then add the shell hook to your rc file so pinned runtimes activate
automatically when you `cd` into a project:

```sh
eval "$(linguo activate zsh)"   # or bash / fish
```

## Usage

```sh
# Cross-language overview: installed toolchains + what's active here
linguo status                     # `linguo list` works too

# Runtime management (builds from python-build-standalone, sha256-verified)
linguo python install 3.12        # or omit the version for the latest
linguo python list                # --available to list downloadable versions
linguo python use 3.12            # pin for this directory (writes linguo.toml)
linguo python use 3.12 --global   # default for everything else
linguo python uninstall 3.12.13

# Project management (uv-style, backed by a .venv)
linguo python init                # pyproject.toml + linguo.toml pin + .venv
linguo python add "requests>=2.31"
linguo python remove requests
linguo python sync                # install everything pyproject.toml declares
linguo python run -- pytest       # run with the venv + toolchain on PATH
linguo python which               # path of the active python (or any command)

# Node.js works the same way (toolchains from nodejs.org/dist, sha256-verified;
# projects are plain package.json + npm, with node_modules/.bin on PATH)
linguo node install               # latest LTS if no version is given
linguo node use 24
linguo node init
linguo node add typescript
linguo node run -- tsc --version
linguo node which tsc

# So does Go (toolchains from go.dev/dl, sha256-verified; projects are plain
# go.mod managed through the pinned toolchain's go tool)
linguo go install                 # latest stable if no version is given
linguo go init my-module
linguo go add rsc.io/quote
linguo go run -- go build ./...

# Terraform is runtime-only (providers/modules stay terraform's job);
# builds come from releases.hashicorp.com, sha256-verified
linguo terraform install 1.13    # `linguo tf ...` works too
linguo tf use 1.13
linguo tf run -- terraform plan
```

Version pins live in `linguo.toml`:

```toml
[runtimes]
python = "3.12"
```

Pins are resolved from the nearest `linguo.toml` up the directory tree, then
the global config (`~/.linguo/config.toml`). Toolchains are stored under
`~/.linguo/toolchains/<language>/<version>` (override the root with
`$LINGUO_ROOT`).
