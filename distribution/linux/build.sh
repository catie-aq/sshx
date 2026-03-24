#!/usr/bin/env bash
# Generic Linux build & package script for sshx
# Prerequisites: Rust toolchain, protobuf compiler
#
# Usage:
#   ./build.sh                            # build for current architecture
#   ./build.sh --target x86_64-unknown-linux-musl   # static musl build
#   ./build.sh --deb                      # also produce a .deb package
#   ./build.sh --rpm                      # also produce an .rpm package
#
# Output: ./dist/

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
DIST_DIR="$SCRIPT_DIR/dist"
VERSION="0.4.1"
TARGET=""
BUILD_DEB=false
BUILD_RPM=false

while [[ $# -gt 0 ]]; do
    case "$1" in
        --target) TARGET="$2"; shift 2 ;;
        --deb) BUILD_DEB=true; shift ;;
        --rpm) BUILD_RPM=true; shift ;;
        *) echo "Unknown option: $1"; exit 1 ;;
    esac
done

# Check dependencies
command -v cargo >/dev/null 2>&1 || { echo "Error: cargo not found. Install Rust: https://rustup.rs"; exit 1; }
command -v protoc >/dev/null 2>&1 || { echo "Error: protoc not found. Install protobuf-compiler (apt) or protobuf (dnf/pacman)"; exit 1; }

rm -rf "$DIST_DIR"
mkdir -p "$DIST_DIR"

cd "$REPO_ROOT"

if [[ -n "$TARGET" ]]; then
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

ARCH=$(uname -m)
[[ -n "$TARGET" ]] && ARCH="$TARGET"
TARBALL="sshx-${VERSION}-linux-${ARCH}.tar.gz"
tar -czf "$DIST_DIR/$TARBALL" -C "$DIST_DIR" sshx sshx-server LICENSE

echo ""
echo "Build complete:"
ls -lh "$DIST_DIR/sshx" "$DIST_DIR/sshx-server"
echo "Tarball: $DIST_DIR/$TARBALL"

# --- Optional .deb package ---
if $BUILD_DEB; then
    echo ""
    echo "Building .deb package..."
    DEB_DIR="$DIST_DIR/deb"
    mkdir -p "$DEB_DIR/DEBIAN"
    mkdir -p "$DEB_DIR/usr/bin"
    mkdir -p "$DEB_DIR/usr/share/licenses/sshx"
    mkdir -p "$DEB_DIR/usr/share/doc/sshx"

    cp "$DIST_DIR/sshx" "$DIST_DIR/sshx-server" "$DEB_DIR/usr/bin/"
    cp "$REPO_ROOT/LICENSE" "$DEB_DIR/usr/share/licenses/sshx/"

    DEB_ARCH="amd64"
    [[ "$ARCH" == "aarch64" || "$ARCH" == *"aarch64"* ]] && DEB_ARCH="arm64"

    cat > "$DEB_DIR/DEBIAN/control" <<EOF
Package: sshx
Version: ${VERSION}
Section: utils
Priority: optional
Architecture: ${DEB_ARCH}
Depends: libc6, libgcc-s1
Maintainer: sshx contributors
Description: A secure web-based, collaborative terminal
 sshx lets you share your terminal with anyone via a web browser,
 with end-to-end encryption and real-time collaboration on an
 infinite canvas.
Homepage: https://github.com/ekzhang/sshx
EOF

    DEB_FILE="$DIST_DIR/sshx_${VERSION}_${DEB_ARCH}.deb"
    dpkg-deb --build "$DEB_DIR" "$DEB_FILE" 2>/dev/null || {
        echo "Warning: dpkg-deb not found, skipping .deb creation"
    }
    [[ -f "$DEB_FILE" ]] && echo "Created: $DEB_FILE"
    rm -rf "$DEB_DIR"
fi

# --- Optional .rpm package ---
if $BUILD_RPM; then
    echo ""
    echo "Building .rpm package..."
    command -v rpmbuild >/dev/null 2>&1 || { echo "Warning: rpmbuild not found, skipping .rpm creation"; exit 0; }

    RPM_ROOT="$DIST_DIR/rpmbuild"
    mkdir -p "$RPM_ROOT"/{BUILD,RPMS,SOURCES,SPECS,SRPMS}

    cat > "$RPM_ROOT/SPECS/sshx.spec" <<EOF
Name:           sshx
Version:        ${VERSION}
Release:        1%{?dist}
Summary:        A secure web-based, collaborative terminal
License:        MIT
URL:            https://github.com/ekzhang/sshx

%description
sshx lets you share your terminal with anyone via a web browser,
with end-to-end encryption and real-time collaboration on an
infinite canvas.

%install
mkdir -p %{buildroot}/usr/bin
mkdir -p %{buildroot}/usr/share/licenses/sshx
cp ${DIST_DIR}/sshx %{buildroot}/usr/bin/
cp ${DIST_DIR}/sshx-server %{buildroot}/usr/bin/
cp ${REPO_ROOT}/LICENSE %{buildroot}/usr/share/licenses/sshx/

%files
/usr/bin/sshx
/usr/bin/sshx-server
/usr/share/licenses/sshx/LICENSE
EOF

    rpmbuild --define "_topdir $RPM_ROOT" -bb "$RPM_ROOT/SPECS/sshx.spec" 2>/dev/null && {
        find "$RPM_ROOT/RPMS" -name '*.rpm' -exec cp {} "$DIST_DIR/" \;
        echo "RPM created in $DIST_DIR/"
    } || echo "Warning: rpmbuild failed, skipping .rpm creation"
    rm -rf "$RPM_ROOT"
fi
