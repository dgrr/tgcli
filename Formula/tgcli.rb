class Tgcli < Formula
  desc "Telegram CLI tool using grammers (pure Rust MTProto)"
  homepage "https://github.com/dgrr/tgcli"
  version "0.1.0"
  license "MIT"

  on_macos do
    on_arm do
      url "https://github.com/dgrr/tgcli/releases/download/v#{version}/tgcli-darwin-arm64"
      sha256 "PLACEHOLDER_DARWIN_ARM64"
    end
    on_intel do
      url "https://github.com/dgrr/tgcli/releases/download/v#{version}/tgcli-darwin-amd64"
      sha256 "PLACEHOLDER_DARWIN_AMD64"
    end
  end

  on_linux do
    on_arm do
      url "https://github.com/dgrr/tgcli/releases/download/v#{version}/tgcli-linux-arm64"
      sha256 "PLACEHOLDER_LINUX_ARM64"
    end
    on_intel do
      url "https://github.com/dgrr/tgcli/releases/download/v#{version}/tgcli-linux-amd64"
      sha256 "PLACEHOLDER_LINUX_AMD64"
    end
  end

  def install
    binary = Dir["tgcli-*"].first || "tgcli"
    bin.install binary => "tgcli"
  end

  def caveats
    <<~EOS
      If you have a tgcli sync service running, restart it to use the new version:
        launchctl kickstart -k gui/$(id -u)/com.tgcli.sync

      Or manually:
        launchctl stop com.tgcli.sync
        launchctl start com.tgcli.sync
    EOS
  end

  test do
    assert_match "tgcli", shell_output("#{bin}/tgcli --version")
  end
end
