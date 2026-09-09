#!/usr/bin/env bash
# Build PadRemote.app and install it where Spotlight can find it.
#
# This is the first step of the "installs like Ollama" bar (plan.md §11.1). It is
# not yet signed or notarized, so Gatekeeper will still ask on a machine other
# than the one that built it - that comes with the .dmg in milestone 5.
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
CRATE="$(dirname "$HERE")"
APP_NAME="PadRemote"
# ~/Applications needs no password and is indexed by Spotlight just like
# /Applications, so installing never has to ask for one.
DEST="${1:-$HOME/Applications}"
APP="$DEST/$APP_NAME.app"
BUNDLE_ID="com.padremote.desktop"

# The cdhash of whatever is installed right now. An ad-hoc signed bundle is
# identified to TCC by this hash alone, so a change to it invalidates the
# Accessibility grant - see the re-signing note further down.
OLD_CDHASH="$(codesign -dvvv "$APP" 2>&1 | sed -n 's/^CDHash=//p' || true)"

# The phone page is compiled into the binary (desktop/build.rs), so it has to
# exist before the binary is built. Skipping this step silently is how you get
# an app that runs, shows a QR, and serves a phone nothing at all - so it is a
# hard failure here rather than a discovery made later on a phone.
WEB="$(dirname "$CRATE")/web"
if [ "${SKIP_WEB_BUILD:-0}" != "1" ]; then
  echo "==> building the phone page"
  if ! command -v npm >/dev/null 2>&1; then
    echo "npm is needed to build the phone page that ships inside the app." >&2
    echo "Install Node (https://nodejs.org, or 'brew install node') and try again." >&2
    echo "To build the app without a page anyway: SKIP_WEB_BUILD=1 $0" >&2
    exit 1
  fi
  ( cd "$WEB" && npm ci --silent && npm run build --silent )
fi
if [ ! -f "$WEB/dist/index.html" ]; then
  echo "==> WARNING: web/dist has no page in it; this app will serve nothing"
fi

echo "==> building release binary"
cargo build --release --manifest-path "$CRATE/Cargo.toml"

echo "==> assembling $APP"
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp "$CRATE/target/release/padremote" "$APP/Contents/MacOS/$APP_NAME"

VERSION="$(grep -m1 '^version' "$CRATE/Cargo.toml" | cut -d'"' -f2)"
cat > "$APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>$APP_NAME</string>
  <key>CFBundleDisplayName</key><string>$APP_NAME</string>
  <key>CFBundleIdentifier</key><string>com.padremote.desktop</string>
  <key>CFBundleExecutable</key><string>$APP_NAME</string>
  <key>CFBundleIconFile</key><string>icon</string>
  <key>CFBundleShortVersionString</key><string>$VERSION</string>
  <key>CFBundleVersion</key><string>$VERSION</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>LSMinimumSystemVersion</key><string>13.0</string>
  <!-- A menu-bar app: no Dock icon, no window on launch. -->
  <key>LSUIElement</key><true/>
  <key>NSHighResolutionCapable</key><true/>
</dict>
</plist>
PLIST

echo "==> rendering icon"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT
python3 "$HERE/make-icon.py" "$WORK/icon.png" >/dev/null
mkdir -p "$WORK/icon.iconset"
for s in 16 32 64 128 256 512; do
  sips -z $s $s "$WORK/icon.png" --out "$WORK/icon.iconset/icon_${s}x${s}.png" >/dev/null 2>&1
  sips -z $((s*2)) $((s*2)) "$WORK/icon.png" --out "$WORK/icon.iconset/icon_${s}x${s}@2x.png" >/dev/null 2>&1
done
iconutil -c icns "$WORK/icon.iconset" -o "$APP/Contents/Resources/icon.icns"

# Signing, and the reason this is fussier than it looks.
#
# The Accessibility grant is remembered against the bundle's *designated
# requirement*. For an ad-hoc signature that requirement is `cdhash H"..."` -
# the hash of this exact build - so every rebuild is, as far as TCC is
# concerned, a different app. The old grant then stays in System Settings
# looking enabled while `AXIsProcessTrusted()` returns false, which is a
# genuinely awful failure to debug: the box is ticked and the app still says
# permission is missing.
#
# Set CODESIGN_IDENTITY to a real (even self-signed) code-signing identity to
# escape that: its requirement is identifier-plus-certificate, which survives
# rebuilds, and the grant sticks for good.
if [ -n "${CODESIGN_IDENTITY:-}" ]; then
  codesign --force --deep --sign "$CODESIGN_IDENTITY" "$APP" \
    && echo "==> signed as $CODESIGN_IDENTITY (grant survives rebuilds)"
else
  codesign --force --deep --sign - "$APP" >/dev/null 2>&1 \
    && echo "==> signed (ad-hoc)" || echo "==> could not sign"
fi

NEW_CDHASH="$(codesign -dvvv "$APP" 2>&1 | sed -n 's/^CDHash=//p' || true)"

# A changed identity leaves a stale TCC row behind. Clear it so the next launch
# asks again, rather than starting up permanently unable to move the cursor.
if [ -n "$OLD_CDHASH" ] && [ "$OLD_CDHASH" != "$NEW_CDHASH" ]; then
  echo "==> identity changed since the last build; clearing the stale Accessibility grant"
  tccutil reset Accessibility "$BUNDLE_ID" >/dev/null 2>&1 \
    && echo "    cleared - macOS will ask again on the next launch" \
    || echo "    could not clear it; remove PadRemote from the Accessibility list by hand"
fi

# Nudge Spotlight, which otherwise may not notice a freshly written bundle.
touch "$APP"
mdimport "$APP" 2>/dev/null || true

echo
echo "Installed: $APP"
echo "Find it with Spotlight (Cmd-Space) by typing: $APP_NAME"
echo
echo "First launch needs Accessibility permission for THIS bundle - the grant is"
echo "per-binary, so the one your terminal has does not carry over:"
echo "  System Settings > Privacy & Security > Accessibility > enable PadRemote"
echo
echo "The phone page is inside the app - there is nothing else to start."
