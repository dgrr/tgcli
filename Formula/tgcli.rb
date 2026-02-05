class Tgcli < Formula
  desc "Telegram CLI tool using grammers (pure Rust MTProto)"
  homepage "https://github.com/dgrr/tgcli"
  version "0.2.0"
  license "MIT"

  on_macos do
    on_arm do
      url "https://github.com/dgrr/tgcli/releases/download/v#{version}/tgcli-darwin-arm64"
      sha256 "1378f4f2842037a5e70096d514f925d2d7546186903699bd9985dcd11c5feabc"
    end
    on_intel do
      url "https://github.com/dgrr/tgcli/releases/download/v#{version}/tgcli-darwin-amd64"
      sha256 "d30e1d85103594839e72cc53e5f8b5d5fcab15130da7e18e509d471c06ac38c2"
    end
  end

  on_linux do
    on_arm do
      url "https://github.com/dgrr/tgcli/releases/download/v#{version}/tgcli-linux-arm64"
      sha256 "6f84a3dff99bf1eddfe2db1bfe6311849a43fcae33f505882d33071d40391ed9"
    end
    on_intel do
      url "https://github.com/dgrr/tgcli/releases/download/v#{version}/tgcli-linux-amd64"
      sha256 "bcba33db43a2c86b63f7e0baa59b61c0965378b02058e7d444ea3a0d8e8a7ed5"
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
