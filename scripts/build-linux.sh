#!/usr/bin/env bash
# Builds SnapCap for Linux and packages it as a .deb and a portable .tar.gz.
#
#   scripts/build-linux.sh                 # build + package
#   scripts/build-linux.sh --install-deps  # first install build dependencies (Ubuntu/Debian, uses sudo)
#
# Runtime needs only libraries every Ubuntu desktop already has (X11/Wayland client
# libraries, OpenGL, ALSA, PipeWire); codecs and the tray are compiled in.
set -euo pipefail
cd "$(dirname "$0")/.."

if [[ "${1:-}" == "--install-deps" ]]; then
  SUDO=$([[ $EUID -eq 0 ]] && echo "" || echo sudo)
  $SUDO apt-get update
  $SUDO apt-get install -y --no-install-recommends \
    build-essential pkg-config clang libclang-dev nasm curl ca-certificates \
    libxcb1-dev libxcb-randr0-dev libxcb-shm0-dev libxcb-xfixes0-dev \
    libpipewire-0.3-dev libspa-0.2-dev libwayland-dev libegl-dev libgbm-dev libdrm-dev \
    libasound2-dev libxkbcommon-dev libdbus-1-dev
fi

cargo build --release --locked
BIN=target/release/snapcap
VERSION=$(grep -m1 '^version' Cargo.toml | cut -d'"' -f2)
ARCH=$(dpkg --print-architecture 2>/dev/null || uname -m)
DIST=dist
rm -rf "$DIST" && mkdir -p "$DIST"

# Shared file tree.
ROOT="$DIST/snapcap_${VERSION}_${ARCH}"
install -Dm755 "$BIN" "$ROOT/usr/bin/snapcap"
install -Dm644 packaging/linux/snapcap.desktop "$ROOT/usr/share/applications/snapcap.desktop"
install -Dm644 LICENSE LICENSE-EXCEPTION -t "$ROOT/usr/share/doc/snapcap/"
for s in 16 32 64 128 256 512; do
  install -Dm644 "assets/icon_${s}.png" "$ROOT/usr/share/icons/hicolor/${s}x${s}/apps/snapcap.png"
done

# Portable tarball.
tar -C "$ROOT/usr" -czf "$DIST/snapcap-${VERSION}-linux-${ARCH}.tar.gz" bin share

# Debian package.
mkdir -p "$ROOT/DEBIAN"
cat > "$ROOT/DEBIAN/control" <<CONTROL
Package: snapcap
Version: ${VERSION}
Section: graphics
Priority: optional
Architecture: ${ARCH}
Depends: libc6, libxcb1, libx11-6, libxcursor1, libxrandr2, libxi6, libxkbcommon-x11-0, libegl1, libgl1,
 libasound2t64 | libasound2, libpipewire-0.3-0t64 | libpipewire-0.3-0
Maintainer: SnapCap
Description: Screenshots and screen recording
 Take screenshots of a screen, all screens or a region, and record the screen
 or a region to MP4 (H.264/AAC) or GIF, with global keyboard shortcuts.
CONTROL
dpkg-deb --build --root-owner-group "$ROOT" "$DIST/snapcap_${VERSION}_${ARCH}.deb" >/dev/null
rm -rf "$ROOT"
ls -lh "$DIST"
