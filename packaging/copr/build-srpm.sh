#!/usr/bin/env bash
# Build a source RPM for Fedora COPR: build-srpm.sh <version> <outdir>
#
# COPR builds each SRPM in a network-isolated mock chroot, so the source
# tarball carries vendored cargo dependencies and the spec builds --offline
# (the same approach as the Launchpad PPA lane). Run this where rpmbuild is
# available (a Fedora container in CI). The finished .src.rpm lands in
# <outdir>, ready for `copr-cli build`.
set -euo pipefail

version="$1"
out="$2"
here="$(cd "$(dirname "$0")" && pwd)"
repo_root="$(cd "${here}/../.." && pwd)"

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

tarball="linguo-${version}-vendored.tar.gz"
tar -C "$work" -czf "${work}/${tarball}" "linguo-${version}"

# Render the spec.
changelog_date="$(date +'%a %b %d %Y')"
spec="${work}/linguo.spec"
sed -e "s/{{VERSION}}/${version}/g" \
    -e "s/{{CHANGELOG_DATE}}/${changelog_date}/g" \
    "${here}/linguo.spec.tmpl" > "$spec"

# Lay out an rpmbuild tree and build the SRPM.
tree="${work}/rpmbuild"
mkdir -p "${tree}/SOURCES" "${tree}/SPECS" "${tree}/SRPMS"
cp "${work}/${tarball}" "${tree}/SOURCES/"
cp "$spec" "${tree}/SPECS/"
rpmbuild --define "_topdir ${tree}" -bs "${tree}/SPECS/linguo.spec"

mkdir -p "$out"
cp "${tree}"/SRPMS/*.src.rpm "$out/"
echo "built $(ls "${tree}"/SRPMS/*.src.rpm | xargs -n1 basename) in ${out}"
