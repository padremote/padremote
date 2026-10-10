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
# Signing and notarizing - the difference between a download that opens and one
# macOS calls possible malware.
#
# Anything downloaded carries a quarantine tag, and for a tagged app Gatekeeper
# wants two things: a Developer ID signature, and a notarization ticket from
# Apple saying it scanned that exact build. Missing either, the user gets
# "Apple could not verify PadRemote is free of malware" and has to dig through
# Privacy & Security to open it - which reads as "this is a virus" to anyone
# who has not seen it before. There is no way around that dialog short of the
# two things it asks for. Set:
#
#   CODESIGN_IDENTITY  "Developer ID Application: Name (TEAMID)", in a keychain
#
# and one way to reach Apple's notary service:
#
#   NOTARY_PROFILE     a keychain profile saved by `xcrun notarytool
#                      store-credentials` - the easy one on your own Mac
#   NOTARY_KEY, NOTARY_KEY_ID, NOTARY_ISSUER
#                      an App Store Connect API key (.p8 path, key id, issuer
#                      id) - what release.yml uses
#
# With none of them the image is ad-hoc signed, for trying the packaging out;
# macOS will warn about that copy on any other Mac. docs/dev/development.md
# ("Signing the download") has the one-time setup.
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

NOTARY_AUTH=()
if [ -n "${NOTARY_PROFILE:-}" ]; then
  NOTARY_AUTH=(--keychain-profile "$NOTARY_PROFILE")
elif [ -n "${NOTARY_KEY:-}" ]; then
  NOTARY_AUTH=(--key "$NOTARY_KEY" --key-id "${NOTARY_KEY_ID:?NOTARY_KEY needs NOTARY_KEY_ID}" \
    --issuer "${NOTARY_ISSUER:?NOTARY_KEY needs NOTARY_ISSUER}")
fi

# Signed but not notarized is refused outright rather than built. It gets the
# very same malware warning as an unsigned image, so it would look finished -
# signed, even - and still greet every user with the dialog.
NOTARIZE=0
if [ -n "${CODESIGN_IDENTITY:-}" ] || [ "${#NOTARY_AUTH[@]}" -gt 0 ]; then
  case "${CODESIGN_IDENTITY:-}" in
    "Developer ID Application"*) ;;
    *)
      echo "Notarizing needs CODESIGN_IDENTITY set to a \"Developer ID Application: ...\" identity." >&2
      echo "Found: '${CODESIGN_IDENTITY:-}'. See 'security find-identity -v -p codesigning'." >&2
      exit 1 ;;
  esac
  if [ "${#NOTARY_AUTH[@]}" -eq 0 ]; then
    echo "CODESIGN_IDENTITY is set but nothing to notarize with: set NOTARY_PROFILE," >&2
    echo "or NOTARY_KEY + NOTARY_KEY_ID + NOTARY_ISSUER. A signed image that is not" >&2
    echo "notarized still gets macOS's malware warning." >&2
    exit 1
  fi
  NOTARIZE=1
fi

STAGE="$(mktemp -d)"
WORK="$(mktemp -d)"
trap 'rm -rf "$STAGE" "$WORK"' EXIT

# Send one file to Apple, wait for the verdict, and staple the ticket onto it.
#
# Stapling is what makes the first open work offline: without it Gatekeeper has
# to look the ticket up online, and a Mac that cannot reach Apple at that moment
# shows the warning after all.
notarize() {
  local file="$1" upload="$1"
  if [ -d "$file" ]; then
    # notarytool takes an archive, not a folder. ditto rather than zip: it keeps
    # the bundle's symlinks and extended attributes, which zip would change and
    # so break the signature Apple is being asked to check. It goes in WORK, not
    # next to the app, or it would end up inside the disk image.
    upload="$WORK/$(basename "$file").zip"
    ditto -c -k --keepParent "$file" "$upload"
  fi
  echo "==> notarizing $(basename "$file") (usually a few minutes)"
  # The exit code is not the verdict, and a non-zero one would end the script
  # under `set -e` before Apple's log could be fetched - so read the status out
  # of the JSON either way.
  xcrun notarytool submit "$upload" "${NOTARY_AUTH[@]}" --wait --output-format json \
    > "$WORK/notary.json" || true
  local status id
  # A submission that never got as far as Apple leaves no JSON at all, which
  # must read as "no status", not crash the parser.
  status="$(python3 -c 'import json,sys
try: print(json.load(sys.stdin).get("status",""))
except ValueError: pass' < "$WORK/notary.json")"
  id="$(python3 -c 'import json,sys
try: print(json.load(sys.stdin).get("id",""))
except ValueError: pass' < "$WORK/notary.json")"
  if [ "$status" != "Accepted" ]; then
    echo "Apple did not accept $(basename "$file"): ${status:-no status}. Its log says why:" >&2
    if [ -n "$id" ]; then
      xcrun notarytool log "$id" "${NOTARY_AUTH[@]}" >&2 || true
    else
      cat "$WORK/notary.json" >&2
    fi
    exit 1
  fi
  xcrun stapler staple "$file"
}

# Universal, because a download cannot know which Mac it lands on: an arm64-only
# app on an Intel Mac does not fail helpfully, Finder just shows it crossed out.
# STAGE_ONLY keeps make-app.sh from treating this scratch folder as an install -
# no Accessibility reset, no Spotlight entry for a copy about to be deleted.
# CODESIGN_IDENTITY passes straight through to it.
UNIVERSAL=1 STAGE_ONLY=1 "$HERE/make-app.sh" "$STAGE"

# The app is notarized on its own before it goes in the image, not just as part
# of the image. The image's ticket covers the image; once someone has dragged
# the app out and ejected it, the app needs a ticket stapled to itself.
if [ "$NOTARIZE" = "1" ]; then
  notarize "$STAGE/PadRemote.app"
fi

ln -s /Applications "$STAGE/Applications"

echo "==> writing $DMG"
mkdir -p "$OUT"
# HFS+ rather than APFS so the image still opens on the oldest macOS the app
# claims to support; UDZO is the compressed read-only format every Mac mounts.
hdiutil create -volname PadRemote -srcfolder "$STAGE" -fs HFS+ -format UDZO \
  -ov "$DMG" >/dev/null

if [ "$NOTARIZE" = "1" ]; then
  codesign --force --sign "$CODESIGN_IDENTITY" --timestamp "$DMG"
  notarize "$DMG"

  # Ask Gatekeeper itself, the same question a downloaded copy will face.
  # "accepted" with "source=Notarized Developer ID" is the whole point of this
  # script, so anything else fails the build instead of reaching a user.
  echo "==> checking what Gatekeeper makes of it"
  spctl --assess --type execute -vv "$STAGE/PadRemote.app" 2>&1 | tee "$WORK/app.spctl"
  spctl --assess --type open --context context:primary-signature -vv "$DMG" 2>&1 | tee "$WORK/dmg.spctl"
  if ! grep -q "Notarized Developer ID" "$WORK/app.spctl" \
    || ! grep -q "Notarized Developer ID" "$WORK/dmg.spctl"; then
    echo "Gatekeeper does not see a notarized Developer ID build; not shipping this." >&2
    exit 1
  fi
else
  echo "==> not notarized: macOS on any other Mac will warn that it cannot check this for malware"
fi

# After stapling, never before: the ticket is written into the image, so a
# checksum taken earlier would not match the file people download.
# It names the file without its directory, so `shasum -c` works from wherever
# the two files were downloaded to.
( cd "$OUT" && shasum -a 256 PadRemote.dmg > PadRemote.dmg.sha256 )

echo
echo "Built: $DMG ($(du -h "$DMG" | cut -f1 | tr -d ' '))"
echo "       $(cat "$OUT/PadRemote.dmg.sha256")"
