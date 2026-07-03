#!/usr/bin/env bash
# Render the Chocolatey package sources: generate.sh <version> <msi-sha256> <outdir>
set -euo pipefail
version="$1"
checksum="$2"
out="$3"
here="$(cd "$(dirname "$0")" && pwd)"
mkdir -p "${out}/tools"
sed -e "s/{{VERSION}}/${version}/g" "${here}/linguo.nuspec.tmpl" > "${out}/linguo.nuspec"
sed -e "s/{{VERSION}}/${version}/g" -e "s/{{CHECKSUM}}/${checksum}/g" \
    "${here}/tools/chocolateyinstall.ps1.tmpl" > "${out}/tools/chocolateyinstall.ps1"
echo "rendered chocolatey package ${version} in ${out}"
