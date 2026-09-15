#!/usr/bin/env bash
# Build PadRemote.dmg - the download for anyone who does not want to build it.
#
#   ./desktop/packaging/make-dmg.sh            → dist/PadRemote.dmg (+ .sha256)
#   ./desktop/packaging/make-dmg.sh <out-dir>
#
# The image holds two things: PadRemote.app and a shortcut to /Applications, so
# installing is dragging one onto the other - the install every Mac user
# already knows, and the one plan.md §11.1 asks for.
#
# The file is always called PadRemote.dmg, never PadRemote-0.1.0.dmg. GitHub
# serves the newest release's asset at
#   https://github.com/padremote/padremote/releases/latest/download/PadRemote.dmg
# only while the name stays put, and that one link is what the README, the wiki
# and the website hand out. A version in the name would break all three on the
# next release. The version lives in the release's tag and in the app's
# Info.plist instead.
#
# Still ad-hoc signed, not Developer ID signed or notarized, so on first open
# macOS says it cannot check the app for malware and the user has to allow it
# once in Privacy & Security. docs/user/getting-started.md walks through that.
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
CRATE="$(dirname "$HERE")"
ROOT="$(dirname "$CRATE")"
OUT="${1:-$ROOT/dist}"
DMG="$OUT/PadRemote.dmg"

if [ "$(uname -s)" != "Darwin" ]; then
  echo "A .dmg can only be built on a Mac (hdiutil)." >&2
  exit 1
fi

STAGE="$(mktemp -d)"
trap 'rm -rf "$STAGE"' EXIT

# Universal, because a download cannot know which Mac it lands on: an arm64-only
# app on an Intel Mac does not fail helpfully, Finder just shows it crossed out.
# STAGE_ONLY keeps make-app.sh from treating this scratch folder as an install -
# no Accessibility reset, no Spotlight entry for a copy about to be deleted.
UNIVERSAL=1 STAGE_ONLY=1 "$HERE/make-app.sh" "$STAGE"

ln -s /Applications "$STAGE/Applications"

echo "==> writing $DMG"
mkdir -p "$OUT"
# HFS+ rather than APFS so the image still opens on the oldest macOS the app
# claims to support; UDZO is the compressed read-only format every Mac mounts.
hdiutil create -volname PadRemote -srcfolder "$STAGE" -fs HFS+ -format UDZO \
  -ov "$DMG" >/dev/null

# The checksum names the file without its directory, so `shasum -c` works from
# wherever the two files were downloaded to.
( cd "$OUT" && shasum -a 256 PadRemote.dmg > PadRemote.dmg.sha256 )

echo
echo "Built: $DMG ($(du -h "$DMG" | cut -f1 | tr -d ' '))"
echo "       $(cat "$OUT/PadRemote.dmg.sha256")"
