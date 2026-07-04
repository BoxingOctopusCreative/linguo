#!/usr/bin/env bash
# Render the AUR PKGBUILD: generate.sh <version> <sha-x86_64> <sha-aarch64> <outdir>
set -euo pipefail
version="$1"
sha_x64="$2"
sha_arm="$3"
out="$4"
here="$(cd "$(dirname "$0")" && pwd)"
mkdir -p "$out"
sed -e "s/{{VERSION}}/${version}/g" \
    -e "s/{{SHA_X86_64}}/${sha_x64}/g" \
    -e "s/{{SHA_AARCH64}}/${sha_arm}/g" \
    "${here}/PKGBUILD.tmpl" > "${out}/PKGBUILD"
echo "rendered PKGBUILD ${version} in ${out}"
