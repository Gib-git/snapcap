#!/usr/bin/env bash
# Builds SnapCap.app and a drag-to-install DMG in dist/.
#
#   scripts/bundle-macos.sh               # native architecture
#   scripts/bundle-macos.sh --universal   # Apple silicon + Intel in one binary
#
# Signing: ad hoc by default, which is enough for local use. Set
# SIGN_IDENTITY="Developer ID Application: …" to sign for distribution with the hardened
# runtime (then notarize the DMG with `xcrun notarytool`).
set -euo pipefail
cd "$(dirname "$0")/.."

PRESET=release
SUFFIX=$(uname -m)
if [[ "${1:-}" == "--universal" ]]; then
  PRESET=universal
  SUFFIX=universal
fi

cmake --preset "$PRESET" -DSNAPCAP_SIGN_IDENTITY="${SIGN_IDENTITY:--}"
cmake --build --preset "$PRESET"

VERSION=$(sed -n 's/^  VERSION \([0-9.]*\)$/\1/p' CMakeLists.txt | head -1)
DIST=dist
APP="$DIST/SnapCap.app"
mkdir -p "$DIST"
rm -rf "$APP" "$DIST/SnapCap-$VERSION-macos-$SUFFIX.dmg"
cp -R "build/$PRESET/SnapCap.app" "$APP"

if [[ -n "${SIGN_IDENTITY:-}" ]]; then
  codesign --force --options runtime --timestamp --entitlements packaging/macos/SnapCap.entitlements \
    --sign "$SIGN_IDENTITY" "$APP"
fi
codesign --verify --strict "$APP"
lipo -info "$APP/Contents/MacOS/snapcap"

STAGE=$(mktemp -d)
cp -R "$APP" "$STAGE/"
ln -s /Applications "$STAGE/Applications"
hdiutil create -quiet -volname SnapCap -srcfolder "$STAGE" -ov -format UDZO "$DIST/SnapCap-$VERSION-macos-$SUFFIX.dmg"
rm -rf "$STAGE"
du -sh "$APP" "$DIST"/*.dmg
