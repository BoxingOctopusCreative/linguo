# Binary formula for a personal tap. Regenerated automatically by the
# Release workflow and attached to every GitHub release as linguo.rb.
class Linguo < Formula
  desc "Cross-platform, multi-language runtime, package, and project manager"
  homepage "https://github.com/BoxingOctopusCreative/linguo"
  version "0.6.0"
  license "MPL-2.0"

  on_macos do
    on_arm do
      url "https://github.com/BoxingOctopusCreative/linguo/releases/download/v0.6.0/linguo-v0.6.0-aarch64-apple-darwin.tar.gz"
      sha256 "215a302750049773ff94817ec6e95072c6ae9a486a318b4b6d0c6dab2b79cbc2"
    end
    on_intel do
      url "https://github.com/BoxingOctopusCreative/linguo/releases/download/v0.6.0/linguo-v0.6.0-x86_64-apple-darwin.tar.gz"
      sha256 "88e17431fc4844dbf44a88a670f2d4991ff0199e023623dc01e19831e8b8abc5"
    end
  end

  on_linux do
    on_arm do
      url "https://github.com/BoxingOctopusCreative/linguo/releases/download/v0.6.0/linguo-v0.6.0-aarch64-unknown-linux-gnu.tar.gz"
      sha256 "4083668f90686d7e2780c122b180f45d39594827e0a6f357a54e6d8c17f10032"
    end
    on_intel do
      url "https://github.com/BoxingOctopusCreative/linguo/releases/download/v0.6.0/linguo-v0.6.0-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "365760b1f5c5e7793155ca9e7b8e22c401d115f3ac27720e446549468fc5919a"
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
