#!/usr/bin/env bash
# Builds SnapCap.app and a drag-to-install DMG in dist/.
#
#   scripts/bundle-macos.sh               # native architecture
#   scripts/bundle-macos.sh --universal   # Apple silicon + Intel in one binary
#
# Signing: ad-hoc by default, which is enough for local use and lets macOS remember the
# Screen Recording permission. Set SIGN_IDENTITY="Developer ID Application: …" to sign
# for distribution (then notarize the DMG with `xcrun notarytool`).
set -euo pipefail
cd "$(dirname "$0")/.."
# The Rust app is archived in archive/rust; shared assets, packaging and dist/ live at the repo root.
REPO=../..
export MACOSX_DEPLOYMENT_TARGET=13.0

VERSION=$(grep -m1 '^version' Cargo.toml | cut -d'"' -f2)
DIST=$REPO/dist
APP="$DIST/SnapCap.app"
rm -rf "$APP" "$DIST/SnapCap-$VERSION-macos.dmg"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"

if [[ "${1:-}" == "--universal" ]]; then
  rustup target add aarch64-apple-darwin x86_64-apple-darwin >/dev/null
  cargo build --release --locked --target aarch64-apple-darwin
  cargo build --release --locked --target x86_64-apple-darwin
  lipo -create -output "$APP/Contents/MacOS/snapcap" \
    target/aarch64-apple-darwin/release/snapcap target/x86_64-apple-darwin/release/snapcap
  SUFFIX=universal
else
  cargo build --release --locked
  cp target/release/snapcap "$APP/Contents/MacOS/snapcap"
  SUFFIX=$(uname -m)
fi

cp $REPO/LICENSE $REPO/LICENSE-EXCEPTION "$APP/Contents/Resources/"
sed "s/__VERSION__/$VERSION/g" $REPO/packaging/macos/Info.plist > "$APP/Contents/Info.plist"
plutil -lint "$APP/Contents/Info.plist" >/dev/null

# Icon set from the procedurally drawn icons in assets/.
ICONSET=$(mktemp -d)/SnapCap.iconset
mkdir -p "$ICONSET"
for s in 16 32 128 256 512; do
  cp "$REPO/assets/icon_${s}.png" "$ICONSET/icon_${s}x${s}.png"
  cp "$REPO/assets/icon_$((s * 2)).png" "$ICONSET/icon_${s}x${s}@2x.png"
done
iconutil -c icns "$ICONSET" -o "$APP/Contents/Resources/SnapCap.icns"

if [[ -n "${SIGN_IDENTITY:-}" ]]; then
  codesign --force --options runtime --timestamp --sign "$SIGN_IDENTITY" "$APP"
else
  codesign --force --sign - "$APP"
fi
codesign --verify --strict "$APP"

STAGE=$(mktemp -d)
cp -R "$APP" "$STAGE/"
ln -s /Applications "$STAGE/Applications"
hdiutil create -quiet -volname SnapCap -srcfolder "$STAGE" -ov -format UDZO "$DIST/SnapCap-$VERSION-macos-$SUFFIX.dmg"
du -sh "$APP" "$DIST"/*.dmg
