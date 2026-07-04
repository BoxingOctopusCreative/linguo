#!/usr/bin/env bash
# Build (and optionally upload) Launchpad PPA source packages.
#
#   build-source.sh <version> <series>... [--sign KEYID --upload PPA]
#
# Launchpad builders have no network access, so the .orig tarball carries
# vendored cargo dependencies and the build runs --offline. One source
# package is produced per Ubuntu series (versioned 1.2.0-1~ppa1~noble1 etc).
# Without --sign, packages build unsigned (CI validation); with --sign and
# --upload they're signed and dput to the PPA.
set -euo pipefail

version="$1"
shift
series_list=()
sign_key=""
ppa=""
while [ $# -gt 0 ]; do
    case "$1" in
        --sign)
            sign_key="$2"
            shift 2
            ;;
        --upload)
            ppa="$2"
            shift 2
            ;;
        *)
            series_list+=("$1")
            shift
            ;;
    esac
done
[ ${#series_list[@]} -gt 0 ] || {
    echo "no series given" >&2
    exit 1
}

repo_root="$(cd "$(dirname "$0")/../.." && pwd)"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

# Assemble the upstream tree: the repo at HEAD plus vendored dependencies.
src="${work}/linguo-${version}"
mkdir -p "$src"
git -C "$repo_root" archive HEAD | tar -x -C "$src"
(cd "$src" && cargo vendor --locked vendor > /dev/null)
mkdir -p "$src/.cargo"
cat > "$src/.cargo/config.toml" <<'CARGOCFG'
[source.crates-io]
replace-with = "vendored-sources"

[source.vendored-sources]
directory = "vendor"
CARGOCFG

tar -C "$work" -czf "${work}/linguo_${version}.orig.tar.gz" "linguo-${version}"

export DEBEMAIL="ryan.draga@boxingoctop.us"
export DEBFULLNAME="Ryan Draga"

first_upload=1
for series in "${series_list[@]}"; do
    build="${work}/${series}/linguo-${version}"
    mkdir -p "${work}/${series}"
    cp "${work}/linguo_${version}.orig.tar.gz" "${work}/${series}/"
    cp -R "$src" "$build"

    cat > "${build}/debian/changelog" <<CHANGELOG
linguo (${version}-1~ppa1~${series}1) ${series}; urgency=medium

  * Release ${version}. See the GitHub release notes:
    https://github.com/BoxingOctopusCreative/linguo/releases/tag/v${version}

 -- ${DEBFULLNAME} <${DEBEMAIL}>  $(date -R)
CHANGELOG

    # Launchpad accepts the .orig.tar.gz once; later series reference it
    # with a diff-only upload (-sd) or the queue daemon rejects them.
    if [ "$first_upload" = 1 ]; then
        src_opt="-sa"
        first_upload=0
    else
        src_opt="-sd"
    fi
    if [ -n "$sign_key" ]; then
        (cd "$build" && dpkg-buildpackage -S "$src_opt" -d "-k${sign_key}")
    else
        (cd "$build" && dpkg-buildpackage -S "$src_opt" -d -us -uc)
    fi
    echo "built ${series}: $(ls "${work}/${series}"/*.changes)"

    if [ -n "$ppa" ]; then
        dput "$ppa" "${work}/${series}"/*_source.changes
    fi
done
echo "done: ${#series_list[@]} series"
