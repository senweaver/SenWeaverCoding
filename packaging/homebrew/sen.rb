# Homebrew formula for SenWeaverCoding CLI
# Install: brew install senweaver/tap/sen
# Or:      brew tap senweaver/tap && brew install sen
#
# To publish: create a repo github.com/senweaver/homebrew-tap and place this file
# as Formula/sen.rb. The release workflow auto-updates the SHA and URL.

class Sen < Formula
  desc "Autonomous AI agent runtime and CLI code editor built in Rust"
  homepage "https://github.com/senweaver/SenWeaverCoding"
  version "0.1.0"
  license "MIT"

  on_macos do
    on_arm do
      url "https://github.com/senweaver/SenWeaverCoding/releases/download/v#{version}/sen-aarch64-apple-darwin.tar.gz"
      sha256 "PLACEHOLDER_SHA256_ARM64"
    end
    on_intel do
      url "https://github.com/senweaver/SenWeaverCoding/releases/download/v#{version}/sen-x86_64-apple-darwin.tar.gz"
      sha256 "PLACEHOLDER_SHA256_X64"
    end
  end

  on_linux do
    on_arm do
      url "https://github.com/senweaver/SenWeaverCoding/releases/download/v#{version}/sen-aarch64-unknown-linux-gnu.tar.gz"
      sha256 "PLACEHOLDER_SHA256_LINUX_ARM64"
    end
    on_intel do
      url "https://github.com/senweaver/SenWeaverCoding/releases/download/v#{version}/sen-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "PLACEHOLDER_SHA256_LINUX_X64"
    end
  end

  def install
    bin.install "sen"
  end

  test do
    assert_match "sen", shell_output("#{bin}/sen --version")
  end
end
