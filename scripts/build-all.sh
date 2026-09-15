#!/usr/bin/env bash
#
# Build sshx for all supported platforms using Docker.
#
# Requirements: Docker, tar, zip
# macOS targets are not built here — use a macOS machine or GitHub Actions.
#
# Usage:
#   ./scripts/build-all.sh                        # builds all targets
#   SSHX_BUILD_URL=https://my.server.com ./scripts/build-all.sh
#
# Output: static/dist/sshx-<VERSION>-<platform>.(tar.gz|zip)

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DIST="$REPO_ROOT/static/dist"
BUILDER_IMAGE="sshx-builder"

# Read version from workspace Cargo.toml
VERSION=$(grep '^version = ' "$REPO_ROOT/Cargo.toml" | head -1 | sed 's/version = "\(.*\)"/\1/')

# Resolve server URL: explicit env > .env SSHX_HOST > default
if [[ -z "${SSHX_BUILD_URL:-}" ]]; then
  if [[ -f "$REPO_ROOT/.env" ]]; then
    SSHX_HOST_VAL=$(grep '^SSHX_HOST=' "$REPO_ROOT/.env" 2>/dev/null | cut -d= -f2 | tr -d '"' || true)
    if [[ -n "$SSHX_HOST_VAL" ]]; then
      SSHX_BUILD_URL="https://$SSHX_HOST_VAL"
    fi
  fi
  SSHX_BUILD_URL="${SSHX_BUILD_URL:-https://sshx.io}"
fi

echo ""
echo "  sshx v${VERSION} — build all platforms"
echo "  server : ${SSHX_BUILD_URL}"
echo "  output : ${DIST}"
echo ""

# ── Build the builder image ──────────────────────────────────────────────────

echo "==> Building builder image (${BUILDER_IMAGE})…"
docker build -q -t "$BUILDER_IMAGE" -f "$REPO_ROOT/Dockerfile.builder" "$REPO_ROOT"
echo "    done"
echo ""

# ── Docker volumes for caching ───────────────────────────────────────────────

CARGO_CACHE_VOL="sshx-cargo-cache"
CARGO_TARGET_VOL="sshx-cargo-target"
XWIN_CACHE_VOL="sshx-xwin-cache"

docker volume create "$CARGO_CACHE_VOL"  >/dev/null 2>&1 || true
docker volume create "$CARGO_TARGET_VOL" >/dev/null 2>&1 || true
docker volume create "$XWIN_CACHE_VOL"   >/dev/null 2>&1 || true

mkdir -p "$DIST"

# ── Helper ───────────────────────────────────────────────────────────────────

run_in_builder() {
  docker run --rm \
    -v "$REPO_ROOT:/app" \
    -v "$CARGO_CACHE_VOL:/usr/local/cargo/registry" \
    -v "$CARGO_TARGET_VOL:/app/target" \
    -v "$XWIN_CACHE_VOL:/root/.xwin-cache" \
    -e SSHX_BUILD_URL="$SSHX_BUILD_URL" \
    -e XWIN_ARCH="x86,x86_64,aarch64" \
    -w /app \
    "$BUILDER_IMAGE" \
    "$@"
}

# ── Linux + FreeBSD (cargo-zigbuild) ─────────────────────────────────────────

echo "==> Linux + FreeBSD targets (cargo-zigbuild)"
echo ""

declare -A LINUX_TARGETS=(
  [x86_64-unknown-linux-musl]="linux-x86_64"
  [aarch64-unknown-linux-musl]="linux-aarch64"
  [arm-unknown-linux-musleabihf]="linux-armv6"
  [armv7-unknown-linux-musleabihf]="linux-armv7"
#  [x86_64-unknown-freebsd]="freebsd-x86_64"
)

for target in "${!LINUX_TARGETS[@]}"; do
  label="${LINUX_TARGETS[$target]}"
  out="sshx-${VERSION}-${label}.tar.gz"
  echo "  [${label}]  ${target}"
  run_in_builder cargo zigbuild --release -p sshx --target "$target"
  tar --no-xattrs -czf "$DIST/$out" \
    -C "$REPO_ROOT/target/$target/release" sshx
  echo "    → ${out}"
  echo ""
done

# ── Windows (cargo-xwin) ─────────────────────────────────────────────────────

echo "==> Windows targets (cargo-xwin)"
echo ""

declare -A WIN_TARGETS=(
  [x86_64-pc-windows-msvc]="windows-x86_64"
  [i686-pc-windows-msvc]="windows-x86"
  [aarch64-pc-windows-msvc]="windows-aarch64"
)

for target in "${!WIN_TARGETS[@]}"; do
  label="${WIN_TARGETS[$target]}"
  out="sshx-${VERSION}-${label}.zip"
  echo "  [${label}]  ${target}"
  run_in_builder cargo xwin build --release -p sshx --target "$target"
  rm -f "$DIST/$out"
  zip -X -j "$DIST/$out" "$REPO_ROOT/target/$target/release/sshx.exe"
  echo "    → ${out}"
  echo ""
done

# ── Fix ownership (Docker writes as root) ────────────────────────────────────

if [[ "$(id -u)" != "0" ]]; then
  docker run --rm \
    -v "$REPO_ROOT/target:/target" \
    -v "$DIST:/dist" \
    "$BUILDER_IMAGE" \
    chown -R "$(id -u):$(id -g)" /target /dist
fi

# ── Summary ───────────────────────────────────────────────────────────────────

echo "==> Done — artifacts in static/dist/:"
echo ""
ls -lh "$DIST"/sshx-"${VERSION}"-* 2>/dev/null | awk '{print "  "$5"\t"$9}' || true
echo ""
echo "  macOS targets (not built here):"
echo "    cargo build --release -p sshx --target aarch64-apple-darwin"
echo "    cargo build --release -p sshx --target x86_64-apple-darwin"
echo ""
