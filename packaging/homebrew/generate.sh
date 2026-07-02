#!/usr/bin/env bash
# Render the Homebrew formula for a release.
#
#   generate.sh <version> <dir containing the tarball .sha256 sidecars>
#
# The Release workflow runs this against the freshly built artifacts and
# attaches the result to the GitHub release as linguo.rb; the committed
# packaging/homebrew/linguo.rb is the snapshot for the newest release.
set -euo pipefail

version="$1"
dist="$2"
base="https://github.com/BoxingOctopusCreative/linguo/releases/download/v${version}"

sha() {
  awk '{print $1}' "${dist}/linguo-v${version}-$1.tar.gz.sha256"
}

cat <<EOF
# Binary formula for a personal tap. Regenerated automatically by the
# Release workflow and attached to every GitHub release as linguo.rb.
class Linguo < Formula
  desc "Cross-platform, multi-language runtime, package, and project manager"
  homepage "https://github.com/BoxingOctopusCreative/linguo"
  version "${version}"
  license "MPL-2.0"

  on_macos do
    on_arm do
      url "${base}/linguo-v${version}-aarch64-apple-darwin.tar.gz"
      sha256 "$(sha aarch64-apple-darwin)"
    end
    on_intel do
      url "${base}/linguo-v${version}-x86_64-apple-darwin.tar.gz"
      sha256 "$(sha x86_64-apple-darwin)"
    end
  end

  on_linux do
    on_arm do
      url "${base}/linguo-v${version}-aarch64-unknown-linux-gnu.tar.gz"
      sha256 "$(sha aarch64-unknown-linux-gnu)"
    end
    on_intel do
      url "${base}/linguo-v${version}-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "$(sha x86_64-unknown-linux-gnu)"
    end
  end

  livecheck do
    url :stable
    strategy :github_latest
  end

  def install
    bin.install "linguo"
    doc.install "README.md"
  end

  test do
    assert_match version.to_s, shell_output("#{bin}/linguo --version")
  end
end
EOF
