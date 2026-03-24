#!/usr/bin/env bash
# Build script for sshx on macOS
# Prerequisites: Rust toolchain, protobuf (brew install protobuf)
#
# Usage:
#   ./build.sh                    # build for current architecture
#   ./build.sh --universal        # build universal binary (x86_64 + aarch64)
#   ./build.sh --target aarch64-apple-darwin
#
# Output: ./dist/sshx and ./dist/sshx-server

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
DIST_DIR="$SCRIPT_DIR/dist"
VERSION="0.4.1"
UNIVERSAL=false
TARGET=""

while [[ $# -gt 0 ]]; do
    case "$1" in
        --universal) UNIVERSAL=true; shift ;;
        --target) TARGET="$2"; shift 2 ;;
        *) echo "Unknown option: $1"; exit 1 ;;
    esac
done

# Check dependencies
command -v cargo >/dev/null 2>&1 || { echo "Error: cargo not found. Install Rust: https://rustup.rs"; exit 1; }
command -v protoc >/dev/null 2>&1 || { echo "Error: protoc not found. Run: brew install protobuf"; exit 1; }

rm -rf "$DIST_DIR"
mkdir -p "$DIST_DIR"

cd "$REPO_ROOT"

if $UNIVERSAL; then
    echo "Building universal binary (x86_64 + aarch64)..."
    rustup target add x86_64-apple-darwin aarch64-apple-darwin 2>/dev/null || true

    cargo build --release --target x86_64-apple-darwin
    cargo build --release --target aarch64-apple-darwin

    for bin in sshx sshx-server; do
        lipo -create \
            "target/x86_64-apple-darwin/release/$bin" \
            "target/aarch64-apple-darwin/release/$bin" \
            -output "$DIST_DIR/$bin"
    done
elif [[ -n "$TARGET" ]]; then
    echo "Building for target: $TARGET..."
    rustup target add "$TARGET" 2>/dev/null || true
    cargo build --release --target "$TARGET"
    cp "target/$TARGET/release/sshx" "target/$TARGET/release/sshx-server" "$DIST_DIR/"
else
    echo "Building for current architecture..."
    cargo build --release
    cp target/release/sshx target/release/sshx-server "$DIST_DIR/"
fi

chmod 755 "$DIST_DIR/sshx" "$DIST_DIR/sshx-server"
cp "$REPO_ROOT/LICENSE" "$DIST_DIR/"

# Create tarball
ARCH=$(uname -m)
$UNIVERSAL && ARCH="universal"
TARBALL="sshx-${VERSION}-darwin-${ARCH}.tar.gz"
tar -czf "$DIST_DIR/$TARBALL" -C "$DIST_DIR" sshx sshx-server LICENSE

echo ""
echo "Build complete:"
ls -lh "$DIST_DIR/sshx" "$DIST_DIR/sshx-server"
echo "Tarball: $DIST_DIR/$TARBALL"
