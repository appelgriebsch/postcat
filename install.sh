#!/usr/bin/env bash
# postcat installer
#
# Downloads the latest (or a specific) postcat release from GitHub and
# installs the binary.
#
# Usage:
#   curl -fsSL https://raw.githubusercontent.com/egoist/postcat/main/install.sh | bash
#
# Environment variables:
#   POSTCAT_VERSION    Install a specific version, e.g. "0.3.0" (default: latest)
#   POSTCAT_INSTALL_DIR  Where to put the binary (default: $HOME/.local/bin, falls back to /usr/local/bin)

set -euo pipefail

REPO="egoist/postcat"
BIN_NAME="postcat"

info() { printf '\033[1;34m==>\033[0m %s\n' "$1"; }
error() { printf '\033[1;31merror:\033[0m %s\n' "$1" >&2; exit 1; }

need_cmd() {
  command -v "$1" >/dev/null 2>&1 || error "required command '$1' not found"
}

need_cmd curl
need_cmd tar
need_cmd mktemp

detect_target() {
  local os arch
  os=$(uname -s)
  arch=$(uname -m)

  case "$os" in
    Darwin)
      case "$arch" in
        arm64) echo "aarch64-apple-darwin" ;;
        x86_64) echo "x86_64-apple-darwin" ;;
        *) error "unsupported macOS architecture: $arch" ;;
      esac
      ;;
    Linux)
      case "$arch" in
        x86_64|amd64) echo "x86_64-unknown-linux-gnu" ;;
        aarch64|arm64) echo "aarch64-unknown-linux-gnu" ;;
        *) error "unsupported Linux architecture: $arch" ;;
      esac
      ;;
    MINGW*|MSYS*|CYGWIN*)
      error "please use the Windows binary from https://github.com/${REPO}/releases instead of this script"
      ;;
    *)
      error "unsupported OS: $os"
      ;;
  esac
}

TARGET=$(detect_target)
info "Detected target: $TARGET"

VERSION="${POSTCAT_VERSION:-}"
if [ -z "$VERSION" ]; then
  info "Looking up latest release..."
  VERSION=$(curl -fsSL "https://api.github.com/repos/${REPO}/releases/latest" |
    grep '"tag_name"' | head -n1 | sed -E 's/.*"v?([^"]+)".*/\1/')
  [ -n "$VERSION" ] || error "could not determine latest version"
fi
info "Installing postcat $VERSION"

ASSET="postcat-${VERSION}-${TARGET}.tar.gz"
URL="https://github.com/${REPO}/releases/download/v${VERSION}/${ASSET}"

TMP_DIR=$(mktemp -d)
trap 'rm -rf "$TMP_DIR"' EXIT

info "Downloading $URL"
curl -fsSL "$URL" -o "$TMP_DIR/$ASSET" || error "download failed — does version $VERSION exist for $TARGET?"

info "Extracting..."
tar -xzf "$TMP_DIR/$ASSET" -C "$TMP_DIR"

INSTALL_DIR="${POSTCAT_INSTALL_DIR:-}"
if [ -z "$INSTALL_DIR" ]; then
  if [ -d "$HOME/.local/bin" ] || mkdir -p "$HOME/.local/bin" 2>/dev/null; then
    INSTALL_DIR="$HOME/.local/bin"
  else
    INSTALL_DIR="/usr/local/bin"
  fi
fi
mkdir -p "$INSTALL_DIR"

if [ -w "$INSTALL_DIR" ]; then
  install -m 755 "$TMP_DIR/$BIN_NAME" "$INSTALL_DIR/$BIN_NAME"
else
  info "Elevated permissions required to write to $INSTALL_DIR"
  sudo install -m 755 "$TMP_DIR/$BIN_NAME" "$INSTALL_DIR/$BIN_NAME"
fi

info "Installed postcat to $INSTALL_DIR/$BIN_NAME"

case ":$PATH:" in
  *":$INSTALL_DIR:"*) ;;
  *)
    printf '\n\033[1;33mwarning:\033[0m %s is not on your PATH.\n' "$INSTALL_DIR"
    printf 'Add this to your shell profile:\n\n  export PATH="%s:$PATH"\n\n' "$INSTALL_DIR"
    ;;
esac

info "Run 'postcat' to get started."
