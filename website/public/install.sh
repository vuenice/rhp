#!/usr/bin/env bash
# ==============================================================================
# RHP Installer Script
# Installs the pre-built RHP compiler and runtime CLI binary on Linux and macOS.
#
# Usage:
#   curl -fsSL https://rhp.dev/install.sh | bash
# ==============================================================================

set -e

REPO="vuenice/rhp"
INSTALL_DIR="${RHP_INSTALL_DIR:-$HOME/.rhp/bin}"

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
BLUE='\033[0;34m'
YELLOW='\033[1;33m'
BOLD='\033[1m'
NC='\033[0m'

info() {
    printf "${BLUE}==>${NC} %s\n" "$1"
}

success() {
    printf "${GREEN}==>${NC} ${BOLD}%s${NC}\n" "$1"
}

error() {
    printf "${RED}Error:${NC} %s\n" "$1" >&2
    exit 1
}

# 1. Detect OS & Architecture
OS="$(uname -s)"
ARCH="$(uname -m)"

case "$OS" in
    Linux)
        case "$ARCH" in
            x86_64)
                TARGET="x86_64-unknown-linux-gnu"
                ;;
            *)
                error "Unsupported Linux architecture: $ARCH. Please build from source via 'cargo install --git https://github.com/$REPO.git rhp'."
                ;;
        esac
        ;;
    Darwin)
        case "$ARCH" in
            arm64|aarch64)
                TARGET="aarch64-apple-darwin"
                ;;
            x86_64)
                TARGET="x86_64-apple-darwin"
                ;;
            *)
                error "Unsupported macOS architecture: $ARCH."
                ;;
        esac
        ;;
    *)
        error "Unsupported operating system: $OS. On Windows, please use WSL or download pre-compiled .zip releases from https://github.com/$REPO/releases."
        ;;
esac

info "Detected platform: ${BOLD}$TARGET${NC}"

# 2. Determine latest version from GitHub API
info "Fetching latest release information from GitHub..."
LATEST_RELEASE_JSON=$(curl -sSL "https://api.github.com/repos/$REPO/releases/latest" 2>/dev/null || true)

VERSION=$(echo "$LATEST_RELEASE_JSON" | grep '"tag_name":' | head -n 1 | cut -d '"' -f 4)

if [ -z "$VERSION" ]; then
    VERSION="v0.1.0"
    info "Could not detect latest release via API; falling back to default $VERSION"
fi

info "Installing RHP version: ${BOLD}$VERSION${NC}"

# 3. Download release archive
TARBALL_NAME="rhp-${VERSION}-${TARGET}.tar.gz"
DOWNLOAD_URL="https://github.com/$REPO/releases/download/${VERSION}/${TARBALL_NAME}"

TMP_DIR="$(mktemp -d)"
cleanup() {
    rm -rf "$TMP_DIR"
}
trap cleanup EXIT

info "Downloading ${DOWNLOAD_URL}..."
if ! curl -fL "$DOWNLOAD_URL" -o "$TMP_DIR/$TARBALL_NAME" 2>/dev/null; then
    # Try bundle tarball fallback if standalone is absent
    BUNDLE_NAME="rhp-bundle-${VERSION}-${TARGET}.tar.gz"
    BUNDLE_URL="https://github.com/$REPO/releases/download/${VERSION}/${BUNDLE_NAME}"
    info "Trying bundle archive ${BUNDLE_URL}..."
    curl -fL "$BUNDLE_URL" -o "$TMP_DIR/$TARBALL_NAME" || error "Failed to download release archive from $DOWNLOAD_URL"
fi

# 4. Extract and install binary
info "Extracting binary to ${INSTALL_DIR}..."
mkdir -p "$INSTALL_DIR"
tar -xzf "$TMP_DIR/$TARBALL_NAME" -C "$TMP_DIR"

if [ -f "$TMP_DIR/rhp" ]; then
    chmod +x "$TMP_DIR/rhp"
    mv "$TMP_DIR/rhp" "$INSTALL_DIR/rhp"
elif [ -f "$TMP_DIR/pkg_rhp/rhp" ]; then
    chmod +x "$TMP_DIR/pkg_rhp/rhp"
    mv "$TMP_DIR/pkg_rhp/rhp" "$INSTALL_DIR/rhp"
else
    error "Binary 'rhp' not found inside the downloaded archive."
fi

# Optional companion binary rhp-play if present in archive
if [ -f "$TMP_DIR/rhp-play" ]; then
    chmod +x "$TMP_DIR/rhp-play"
    mv "$TMP_DIR/rhp-play" "$INSTALL_DIR/rhp-play"
    info "Installed companion tool 'rhp-play' to $INSTALL_DIR"
fi

# 5. Configure PATH in shell startup file
ADDED_TO_PATH=false
CURRENT_PATH=":$PATH:"

if [[ "$CURRENT_PATH" != *":$INSTALL_DIR:"* ]]; then
    EXPORT_CMD="export PATH=\"$INSTALL_DIR:\$PATH\""
    
    # Detect target shell rc
    DETECTED_RC=""
    if [ -n "$ZSH_VERSION" ] || [ -f "$HOME/.zshrc" ]; then
        DETECTED_RC="$HOME/.zshrc"
    elif [ -n "$BASH_VERSION" ] || [ -f "$HOME/.bashrc" ]; then
        DETECTED_RC="$HOME/.bashrc"
    elif [ -f "$HOME/.profile" ]; then
        DETECTED_RC="$HOME/.profile"
    fi

    if [ -n "$DETECTED_RC" ] && ! grep -qs "$INSTALL_DIR" "$DETECTED_RC"; then
        printf "\n# RHP Programming Language CLI\n%s\n" "$EXPORT_CMD" >> "$DETECTED_RC"
        ADDED_TO_PATH=true
        info "Added $INSTALL_DIR to your PATH in $DETECTED_RC"
    fi
fi

echo ""
success "RHP was successfully installed!"
echo ""
echo "To verify the installation:"
if [ "$ADDED_TO_PATH" = true ]; then
    echo "  1. Reload your shell:  source $DETECTED_RC"
    echo "  2. Run:                rhp -v"
else
    echo "  Run: rhp -v"
    if [[ "$CURRENT_PATH" != *":$INSTALL_DIR:"* ]]; then
        echo ""
        echo "  Note: Ensure $INSTALL_DIR is in your PATH:"
        echo "    export PATH=\"$INSTALL_DIR:\$PATH\""
    fi
fi
echo ""
