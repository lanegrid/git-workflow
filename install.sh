#!/bin/sh
# git-workflow installer
# Usage: curl -fsSL https://github.com/lanegrid/git-workflow/releases/latest/download/install.sh | sh

set -e

REPO="lanegrid/git-workflow"
INSTALL_DIR="${INSTALL_DIR:-$HOME/.local/bin}"

# Detect OS and architecture
detect_platform() {
    OS=$(uname -s | tr '[:upper:]' '[:lower:]')
    ARCH=$(uname -m)

    case "$OS" in
        darwin)
            case "$ARCH" in
                x86_64) PLATFORM="x86_64-apple-darwin" ;;
                arm64)  PLATFORM="aarch64-apple-darwin" ;;
                *)      echo "Unsupported architecture: $ARCH"; exit 1 ;;
            esac
            ;;
        linux)
            case "$ARCH" in
                x86_64)  PLATFORM="x86_64-unknown-linux-gnu" ;;
                aarch64) PLATFORM="aarch64-unknown-linux-gnu" ;;
                *)       echo "Unsupported architecture: $ARCH"; exit 1 ;;
            esac
            ;;
        *)
            echo "Unsupported OS: $OS"
            echo "For Windows, download manually from:"
            echo "  https://github.com/$REPO/releases/latest"
            exit 1
            ;;
    esac
}

# Get latest release version
get_latest_version() {
    curl -fsSL "https://api.github.com/repos/$REPO/releases/latest" | grep '"tag_name"' | sed -E 's/.*"([^"]+)".*/\1/'
}

main() {
    echo "Installing git-workflow..."

    detect_platform
    VERSION=$(get_latest_version)

    if [ -z "$VERSION" ]; then
        echo "Failed to get latest version"
        exit 1
    fi

    echo "  Version: $VERSION"
    echo "  Platform: $PLATFORM"
    echo "  Install to: $INSTALL_DIR"

    # Create install directory
    mkdir -p "$INSTALL_DIR"

    # Download and extract
    ARCHIVE="git-workflow-$PLATFORM.tar.gz"
    URL="https://github.com/$REPO/releases/download/$VERSION/$ARCHIVE"

    echo "  Downloading from $URL..."

    TEMP_DIR=$(mktemp -d)
    trap 'rm -rf "$TEMP_DIR"' EXIT

    curl -fsSL "$URL" -o "$TEMP_DIR/$ARCHIVE"
    tar -xzf "$TEMP_DIR/$ARCHIVE" -C "$TEMP_DIR"

    # Install binaries
    mv "$TEMP_DIR/git-workflow" "$INSTALL_DIR/"
    mv "$TEMP_DIR/gw" "$INSTALL_DIR/"
    chmod +x "$INSTALL_DIR/git-workflow" "$INSTALL_DIR/gw"

    echo ""
    echo "Installed successfully!"
    echo ""

    # Check if install dir is in PATH
    case ":$PATH:" in
        *":$INSTALL_DIR:"*) ;;
        *)
            echo "Add $INSTALL_DIR to your PATH:"
            echo ""
            echo "  export PATH=\"\$PATH:$INSTALL_DIR\""
            echo ""
            ;;
    esac

    echo "Run 'git-workflow --help' to get started."
}

main
