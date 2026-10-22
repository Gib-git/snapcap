#!/usr/bin/env bash
# End-to-end smoke test of a built SnapCap.app, driven through its command line.
#
#   scripts/smoke-macos.sh [path/to/SnapCap.app]     (default: build/debug/SnapCap.app)
#
# Uses a throwaway settings folder and save folder, so your own settings are untouched.
# Needs Screen Recording permission for the terminal running it, and an unlocked screen.
set -euo pipefail
cd "$(dirname "$0")/.."
APP="${1:-build/debug/SnapCap.app}"
BIN="$APP/Contents/MacOS/snapcap"
WORK=$(mktemp -d)
export SNAPCAP_CONFIG_DIR="$WORK/config"
mkdir -p "$SNAPCAP_CONFIG_DIR/SnapCap" "$WORK/caps"
cat > "$SNAPCAP_CONFIG_DIR/SnapCap/settings.json" <<JSON
{"save_dir": "$WORK/caps", "tray_hint_shown": true, "countdown_secs": 0, "copy_to_clipboard": false}
JSON

"$BIN" --background &
PID=$!
cleanup() { kill "$PID" 2>/dev/null || true; rm -rf "$WORK"; }
trap cleanup EXIT
sleep 2

fail() { echo "FAIL: $*"; exit 1; }
count() { find "$WORK/caps" -name "$1" | wc -l | tr -d ' '; }

"$BIN" --shot screen
sleep 2
[[ $(count '*.png') -eq 1 ]] || fail "--shot screen saved no PNG"
"$BIN" --shot all
sleep 3
[[ $(count '*.png') -eq 2 ]] || fail "--shot all saved no PNG"
for f in "$WORK"/caps/*.png; do sips -g pixelWidth -g pixelHeight "$f" | tail -2 | tr '\n' ' '; echo; done

# ~5 s of recording with a 2 s pause in the middle: the file should be ~3 s long.
"$BIN" --record screen
sleep 2
"$BIN" --pause
sleep 2
"$BIN" --pause
sleep 1.5
"$BIN" --record stop
sleep 3
MP4=$(find "$WORK/caps" -name '*.mp4' | head -1)
[[ -n "$MP4" ]] || fail "no MP4 was written"

cat > "$WORK/inspect.swift" <<'SWIFT'
import AVFoundation
let asset = AVURLAsset(url: URL(fileURLWithPath: CommandLine.arguments[1]))
let done = DispatchSemaphore(value: 0)
Task {
  let seconds = try await asset.load(.duration).seconds
  let video = try await asset.loadTracks(withMediaType: .video).count
  print(String(format: "%.2f", seconds), video)
  done.signal()
}
done.wait()
SWIFT
read -r SECONDS_LONG VIDEO_TRACKS < <(swift "$WORK/inspect.swift" "$MP4")
echo "MP4: ${SECONDS_LONG}s, $VIDEO_TRACKS video track(s)"
[[ "$VIDEO_TRACKS" -eq 1 ]] || fail "expected one video track"
awk -v s="$SECONDS_LONG" 'BEGIN { exit !(s > 2.0 && s < 4.5) }' || fail "unexpected duration ${SECONDS_LONG}s"

kill "$PID"
wait "$PID" 2>/dev/null || true
echo "PASS"
