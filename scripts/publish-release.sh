#!/usr/bin/env bash
#
# Build the sshx CLI and publish it as the latest release served by the local
# sshx-server under /releases, for `sshx update`.
#
# Requirements: Rust with the requested targets installed, gzip, python3, and
# cargo-zigbuild + zig for glibc-versioned targets (e.g. *-linux-gnu.2.28).
#
# Linux builds link against an old glibc rather than musl: musl ignores
# nsswitch, so it cannot resolve Tailscale MagicDNS names on hosts where
# systemd-resolved is only reachable through nss-resolve.
#
# Each run bumps the workspace version in Cargo.toml (patch by default), so
# clients see a newer version. Commit the bump afterwards.
#
# Usage:
#   ./scripts/publish-release.sh                          # 0.5.0 → 0.5.1, Linux x86_64 (glibc >= 2.28)
#   ./scripts/publish-release.sh --bump minor             # 0.5.1 → 0.6.0 (major|minor|patch|none)
#   ./scripts/publish-release.sh --target <triple> [...]  # other targets
#
# Output: releases/manifest.json and releases/sshx-<version>-<platform>.gz
# (the server serves ./releases relative to its working directory).

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
RELEASES="$REPO_ROOT/releases"
KEEP=3 # builds kept per platform, for rollback with `sshx update --force`

BUMP="patch"
TARGETS=()
while [[ $# -gt 0 ]]; do
  case "$1" in
    --bump) BUMP="$2"; shift 2 ;;
    --target) TARGETS+=("$2"); shift 2 ;;
    *) echo "Unknown option: $1" >&2; exit 1 ;;
  esac
done
[[ ${#TARGETS[@]} -gt 0 ]] || TARGETS=(x86_64-unknown-linux-gnu.2.28)

# Keep in sync with `platform_label` in crates/sshx/build.rs.
platform_label() {
  local target="$1" arch os
  arch="${target%%-*}"
  case "$arch" in
    i586|i686) arch="x86" ;;
    arm) arch="armv6" ;;
    armv7*) arch="armv7" ;;
  esac
  case "$target" in
    *linux*) os="linux" ;;
    *windows*) os="windows" ;;
    *apple*) os="macos" ;;
    *freebsd*) os="freebsd" ;;
    *) os="unknown" ;;
  esac
  echo "$os-$arch"
}

cd "$REPO_ROOT"

PREVIOUS=$(grep '^version = ' Cargo.toml | head -1 | sed 's/version = "\(.*\)"/\1/')
IFS=. read -r MAJOR MINOR PATCH <<< "$PREVIOUS"
case "$BUMP" in
  major) VERSION="$((MAJOR + 1)).0.0" ;;
  minor) VERSION="$MAJOR.$((MINOR + 1)).0" ;;
  patch) VERSION="$MAJOR.$MINOR.$((PATCH + 1))" ;;
  none) VERSION="$PREVIOUS" ;;
  *) echo "Invalid --bump: $BUMP (major|minor|patch|none)" >&2; exit 1 ;;
esac

if [[ "$VERSION" != "$PREVIOUS" ]]; then
  sed -i "0,/^version = \"$PREVIOUS\"/s//version = \"$VERSION\"/" Cargo.toml
  # Restore the previous version if the build fails, so a rerun bumps from the same base.
  trap 'sed -i "0,/^version = \"$VERSION\"/s//version = \"$PREVIOUS\"/" Cargo.toml; echo "  build failed — version restored to $PREVIOUS" >&2' ERR
fi

# Default server baked into the binaries: explicit env > .env SSHX_HOST > build.rs default.
if [[ -z "${SSHX_BUILD_URL:-}" && -f .env ]]; then
  SSHX_HOST_VAL=$(grep '^SSHX_HOST=' .env | cut -d= -f2 | tr -d '"' || true)
  [[ -z "$SSHX_HOST_VAL" ]] || export SSHX_BUILD_URL="https://$SSHX_HOST_VAL"
fi

echo ""
echo "  sshx v${VERSION} — publish release (from v${PREVIOUS})"
echo "  default server : ${SSHX_BUILD_URL:-(build.rs default)}"
echo "  output         : ${RELEASES}"
echo ""

mkdir -p "$RELEASES"

for target in "${TARGETS[@]}"; do
  label=$(platform_label "$target")
  triple="${target%%.[0-9]*}" # strip a glibc version suffix
  exe="sshx"
  [[ "$target" != *windows* ]] || exe="sshx.exe"
  file="sshx-${VERSION}-${label}.gz"

  echo "==> [${label}] ${target}"
  if [[ "$target" != "$triple" ]]; then
    cargo zigbuild --release -p sshx --target "$target"
  else
    cargo build --release -p sshx --target "$target"
  fi

  gzip -9 -n -c "target/$triple/release/$exe" > "$RELEASES/$file.tmp"
  mv "$RELEASES/$file.tmp" "$RELEASES/$file"
  sha=$(sha256sum "$RELEASES/$file" | cut -d' ' -f1)

  python3 - "$RELEASES/manifest.json" "$label" "$VERSION" "/releases/$file" "$sha" <<'EOF'
import json, os, sys

path, label, version, url, sha256 = sys.argv[1:]
manifest = {"platforms": {}}
if os.path.exists(path):
    with open(path) as f:
        manifest = json.load(f)
manifest["platforms"][label] = {"version": version, "url": url, "sha256": sha256}
with open(path + ".tmp", "w") as f:
    json.dump(manifest, f, indent=2)
    f.write("\n")
os.replace(path + ".tmp", path)
EOF

  # Prune older builds of this platform.
  ls -t "$RELEASES"/sshx-*-"$label".gz | tail -n +$((KEEP + 1)) | xargs -r rm --
  echo "    → ${file}"
  echo ""
done

echo "==> Published v${VERSION} — clients update with \`sshx update\`"
[[ "$VERSION" == "$PREVIOUS" ]] || echo "    Commit the version bump: git commit -am \"Release v${VERSION}\""
echo ""
