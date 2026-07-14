#!/bin/bash
set -e

# Configuration
REPO="hiro-o918/rinkaku"
BINARY_NAME="rinkaku"
# Use provided INSTALL_DIR or default to /usr/local/bin
INSTALL_DIR="${INSTALL_DIR:-/usr/local/bin}"

# Detect OS and Architecture
OS=$(uname -s)
ARCH=$(uname -m)

# Determine target triple. Linux uses the statically linked musl build so
# the binary runs regardless of the host's glibc version.
case $OS in
  Linux)  OS_SUFFIX="unknown-linux-musl" ;;
  Darwin) OS_SUFFIX="apple-darwin" ;;
  *) echo "Error: Unsupported OS: $OS"; exit 1 ;;
esac

case $ARCH in
  x86_64) ARCH_SUFFIX="x86_64" ;;
  arm64|aarch64) ARCH_SUFFIX="aarch64" ;;
  *) echo "Error: Unsupported Architecture: $ARCH"; exit 1 ;;
esac

TARGET="${ARCH_SUFFIX}-${OS_SUFFIX}"

FILE_NAME="${BINARY_NAME}-${TARGET}.tar.gz"

# Determine download URL (latest or specified version)
if [ -n "$VERSION" ]; then
  DOWNLOAD_URL="https://github.com/${REPO}/releases/download/${VERSION}/${FILE_NAME}"
  echo "Downloading $BINARY_NAME ($VERSION) for $TARGET..."
else
  DOWNLOAD_URL="https://github.com/${REPO}/releases/latest/download/${FILE_NAME}"
  echo "Downloading $BINARY_NAME (latest) for $TARGET..."
fi

# Create a temporary directory
TMP_DIR=$(mktemp -d)

# Download the archive
if ! curl -sL "$DOWNLOAD_URL" -o "$TMP_DIR/archive.tar.gz"; then
    echo "Error: Failed to download release from $DOWNLOAD_URL"
    rm -rf "$TMP_DIR"
    exit 1
fi

# Extract the binary
tar -xzf "$TMP_DIR/archive.tar.gz" -C "$TMP_DIR"
# Find the binary in extracted directory
EXTRACTED_BINARY=$(find "$TMP_DIR" -name "$BINARY_NAME" -type f | head -1)
if [ -z "$EXTRACTED_BINARY" ]; then
    echo "Error: Binary not found in archive"
    rm -rf "$TMP_DIR"
    exit 1
fi
chmod +x "$EXTRACTED_BINARY"

# Check write permissions for INSTALL_DIR
if [ ! -d "$INSTALL_DIR" ]; then
    echo "Directory $INSTALL_DIR does not exist. Creating it..."
    if ! mkdir -p "$INSTALL_DIR"; then
        echo "Error: Failed to create directory $INSTALL_DIR (permission denied?)"
        rm -rf "$TMP_DIR"
        exit 1
    fi
fi

echo "Installing to $INSTALL_DIR..."

# Move binary (use sudo if not writable)
if [ -w "$INSTALL_DIR" ]; then
    mv "$EXTRACTED_BINARY" "$INSTALL_DIR/"
else
    echo "Requires sudo permissions to write to $INSTALL_DIR..."
    sudo mv "$EXTRACTED_BINARY" "$INSTALL_DIR/"
fi

# Cleanup
rm -rf "$TMP_DIR"

echo "Successfully installed $BINARY_NAME to $INSTALL_DIR!"
