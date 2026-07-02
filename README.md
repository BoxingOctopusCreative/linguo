# linguo

Cross-platform, multi-language runtime, package, and project manager — think
uv, but for Python, Go, Node.js, Ruby, Rust, and Terraform.

Commands follow the shape `linguo <language> <command>`. Python is the first
supported language; the others will follow the same command surface.

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
