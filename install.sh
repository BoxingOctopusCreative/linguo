#!/bin/sh
# linguo installer for platforms not covered by the deb/rpm/MSI packages or
# the Homebrew tap:
#
#   curl -fsSL https://raw.githubusercontent.com/BoxingOctopusCreative/linguo/main/install.sh | sh
#
# Installs the latest release binary to ~/.local/bin (override with
# LINGUO_INSTALL_DIR). Pin a version with LINGUO_VERSION=0.6.0 or by passing
# it as the first argument. Updating linguo later is just re-running this,
# or your package manager if you installed a package instead.
set -eu

REPO="BoxingOctopusCreative/linguo"
INSTALL_DIR="${LINGUO_INSTALL_DIR:-$HOME/.local/bin}"
VERSION="${1:-${LINGUO_VERSION:-}}"

err() {
    printf 'install.sh: %s\n' "$*" >&2
    exit 1
}

command -v curl >/dev/null 2>&1 || err "curl is required"

os=$(uname -s)
arch=$(uname -m)
case "$os" in
    Darwin)
        case "$arch" in
            arm64) target="aarch64-apple-darwin" ;;
            x86_64) target="x86_64-apple-darwin" ;;
            *) err "unsupported macOS architecture: $arch" ;;
        esac
        ;;
    Linux)
        libc="gnu"
        if ldd --version 2>&1 | grep -qi musl; then
            libc="musl"
        fi
        case "$arch" in
            aarch64 | arm64) target="aarch64-unknown-linux-${libc}" ;;
            x86_64) target="x86_64-unknown-linux-${libc}" ;;
            *) err "unsupported Linux architecture: $arch" ;;
        esac
        ;;
    *)
        err "unsupported OS: $os (on Windows, use the MSI or zip from the releases page)"
        ;;
esac

if [ -z "$VERSION" ]; then
    VERSION=$(curl -fsSL "https://api.github.com/repos/${REPO}/releases/latest" |
        sed -n 's/.*"tag_name": *"\(v[^"]*\)".*/\1/p' | head -n 1)
    [ -n "$VERSION" ] || err "could not determine the latest release"
fi
case "$VERSION" in
    v*) ;;
    *) VERSION="v${VERSION}" ;;
esac

name="linguo-${VERSION}-${target}"
url="https://github.com/${REPO}/releases/download/${VERSION}/${name}.tar.gz"

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

echo "downloading ${url}"
curl -fSL --progress-bar -o "${tmp}/${name}.tar.gz" "$url" ||
    err "download failed (does release ${VERSION} exist?)"
curl -fsSL -o "${tmp}/${name}.tar.gz.sha256" "${url}.sha256" ||
    err "checksum download failed"

expected=$(awk '{print $1}' "${tmp}/${name}.tar.gz.sha256")
if command -v sha256sum >/dev/null 2>&1; then
    actual=$(sha256sum "${tmp}/${name}.tar.gz" | awk '{print $1}')
else
    actual=$(shasum -a 256 "${tmp}/${name}.tar.gz" | awk '{print $1}')
fi
[ "$expected" = "$actual" ] || err "checksum mismatch: expected ${expected}, got ${actual}"

tar -xzf "${tmp}/${name}.tar.gz" -C "$tmp"
mkdir -p "$INSTALL_DIR"
install -m 755 "${tmp}/${name}/linguo" "${INSTALL_DIR}/linguo"

echo "installed $("${INSTALL_DIR}/linguo" --version) to ${INSTALL_DIR}/linguo"

case ":${PATH}:" in
    *":${INSTALL_DIR}:"*) ;;
    *) echo "note: ${INSTALL_DIR} is not on your PATH; add it to your shell profile" ;;
esac
echo 'next: enable per-project activation with: eval "$(linguo activate zsh)"  # or bash / fish'
