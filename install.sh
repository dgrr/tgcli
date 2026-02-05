#!/usr/bin/env bash
set -euo pipefail

# tgrs installer - downloads the appropriate binary from GitHub releases
# Usage: curl -fsSL https://raw.githubusercontent.com/dgrr/tgrs/main/install.sh | bash

REPO="dgrr/tgrs"
BINARY="tgrs"
INSTALL_DIR="${INSTALL_DIR:-}"

# Detect OS
detect_os() {
    case "$(uname -s)" in
        Linux*)  echo "linux" ;;
        Darwin*) echo "darwin" ;;
        *)       echo "unsupported" ;;
    esac
}

# Detect architecture
detect_arch() {
    case "$(uname -m)" in
        x86_64|amd64)  echo "amd64" ;;
        aarch64|arm64) echo "arm64" ;;
        *)             echo "unsupported" ;;
    esac
}

# Get latest release tag
get_latest_release() {
    curl -fsSL "https://api.github.com/repos/${REPO}/releases/latest" | \
        grep '"tag_name":' | sed -E 's/.*"([^"]+)".*/\1/'
}

# Determine install directory
get_install_dir() {
    if [ -n "$INSTALL_DIR" ]; then
        echo "$INSTALL_DIR"
    elif [ -w "/usr/local/bin" ]; then
        echo "/usr/local/bin"
    else
        mkdir -p "$HOME/.local/bin"
        echo "$HOME/.local/bin"
    fi
}

main() {
    local os=$(detect_os)
    local arch=$(detect_arch)

    if [ "$os" = "unsupported" ]; then
        echo "Error: Unsupported operating system: $(uname -s)" >&2
        exit 1
    fi

    if [ "$arch" = "unsupported" ]; then
        echo "Error: Unsupported architecture: $(uname -m)" >&2
        exit 1
    fi

    echo "Detected: ${os}-${arch}"

    # Get latest version
    local version="${VERSION:-$(get_latest_release)}"
    if [ -z "$version" ]; then
        echo "Error: Could not determine latest version" >&2
        exit 1
    fi
    echo "Version: ${version}"

    # Build download URL
    local asset_name="${BINARY}-${os}-${arch}"
    local download_url="https://github.com/${REPO}/releases/download/${version}/${asset_name}"

    # Determine install location
    local install_dir=$(get_install_dir)
    local install_path="${install_dir}/${BINARY}"

    echo "Downloading ${download_url}..."
    
    # Download to temp file
    local tmp_file=$(mktemp)
    trap "rm -f '$tmp_file'" EXIT

    if ! curl -fsSL "$download_url" -o "$tmp_file"; then
        echo "Error: Failed to download ${asset_name}" >&2
        echo "URL: ${download_url}" >&2
        exit 1
    fi

    # Install
    chmod +x "$tmp_file"
    
    if [ -w "$install_dir" ]; then
        mv "$tmp_file" "$install_path"
    else
        echo "Installing to ${install_path} (requires sudo)..."
        sudo mv "$tmp_file" "$install_path"
    fi

    echo ""
    echo "✓ Installed ${BINARY} to ${install_path}"
    echo ""

    # Check if in PATH
    if ! command -v "$BINARY" &>/dev/null; then
        echo "Note: ${install_dir} is not in your PATH."
        echo "Add it with: export PATH=\"\$PATH:${install_dir}\""
        echo ""
    fi

    echo "Get started:"
    echo "  ${BINARY} auth      # authenticate with Telegram"
    echo "  ${BINARY} sync      # sync your messages"
    echo "  ${BINARY} --help    # see all commands"
}

main "$@"
