#!/usr/bin/env bash
#
# Install PadRemote. One command, from a fresh clone to a cursor on your screen.
#
#   ./install.sh                 build, install, start at login, launch
#   ./install.sh --no-login      the same, without starting at login
#   ./install.sh --uninstall     remove the app and the login item
#
# What it does, and why each step is here:
#
#   1. checks you have Rust and Node, and says how to get them if not
#   2. builds the phone page and compiles it into the app, so the app *is* the
#      product - there is no second process to remember to start
#   3. installs to ~/Applications, which needs no password
#   4. registers a login item, so a reboot does not quietly end your setup
#   5. launches it, and tells you the one permission macOS will ask for
#
# Step 4 is the one that came from being burned. The page used to be served by
# `npm run dev` in a terminal; reboot, and the menu-bar app came back while the
# page server did not. Everything looked fine on the computer and the phone said
# "this site can't be reached".
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
APP="$HOME/Applications/PadRemote.app"
AGENT_LABEL="com.padremote.desktop"
AGENT="$HOME/Library/LaunchAgents/$AGENT_LABEL.plist"

LOGIN_ITEM=ask
case "${1:-}" in
  --no-login) LOGIN_ITEM=no ;;
  --login) LOGIN_ITEM=yes ;;
  --uninstall) LOGIN_ITEM=uninstall ;;
  "") ;;
  *) echo "usage: $0 [--login | --no-login | --uninstall]" >&2; exit 2 ;;
esac

say() { printf '\n\033[1m%s\033[0m\n' "$*"; }
oops() { printf '\n\033[31m%s\033[0m\n' "$*" >&2; }

# ----------------------------------------------------------------- uninstall

remove_login_item() {
  # `bootout` on something that was never loaded is an error, not a no-op, and
  # this script should be safe to run twice.
  launchctl bootout "gui/$UID/$AGENT_LABEL" >/dev/null 2>&1 || true
  rm -f "$AGENT"
}

if [ "$LOGIN_ITEM" = uninstall ]; then
  say "Removing PadRemote"
  osascript -e 'quit app "PadRemote"' >/dev/null 2>&1 || true
  remove_login_item
  rm -rf "$APP"
  echo "Removed the app and the login item."
  echo
  echo "Your settings and pairing are kept, in case you reinstall:"
  echo "  ~/Library/Application Support/PadRemote"
  echo "Delete that folder too if you want no trace left."
  exit 0
fi

# -------------------------------------------------------------- requirements

if [ "$(uname -s)" != "Darwin" ]; then
  oops "PadRemote's input backend is macOS-only today. See docs/dev/porting.md."
  exit 1
fi

missing=0
if ! command -v cargo >/dev/null 2>&1; then
  # A shell that has never sourced the cargo env still has the toolchain.
  # shellcheck disable=SC1091
  [ -f "$HOME/.cargo/env" ] && . "$HOME/.cargo/env"
fi
if ! command -v cargo >/dev/null 2>&1; then
  oops "Rust is needed to build the app."
  echo "  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh"
  missing=1
fi
if ! command -v npm >/dev/null 2>&1; then
  oops "Node is needed to build the phone page."
  echo "  brew install node        (or https://nodejs.org)"
  missing=1
fi
[ "$missing" -eq 0 ] || exit 1

# ------------------------------------------------------------------- build it

say "Building PadRemote (this takes a few minutes the first time)"
"$HERE/desktop/packaging/make-app.sh"

if [ ! -d "$APP" ]; then
  oops "The build finished but $APP is not there. Nothing was installed."
  exit 1
fi

# --------------------------------------------------------------- login item

install_login_item() {
  mkdir -p "$(dirname "$AGENT")" "$HOME/Library/Logs/PadRemote"
  # `--no-qr` because a pairing page opening in your browser every time you log
  # in is an annoyance, not a feature; the QR is in the menu bar when you want
  # it. No `KeepAlive`: if the app is crashing, a restart loop hides that.
  cat > "$AGENT" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key><string>$AGENT_LABEL</string>
  <key>ProgramArguments</key>
  <array>
    <string>$APP/Contents/MacOS/PadRemote</string>
    <string>--no-qr</string>
  </array>
  <key>RunAtLoad</key><true/>
  <key>StandardOutPath</key><string>$HOME/Library/Logs/PadRemote/padremote.log</string>
  <key>StandardErrorPath</key><string>$HOME/Library/Logs/PadRemote/padremote.log</string>
</dict>
</plist>
PLIST
  # `bootout` on a job with a live process is asynchronous: it returns while
  # launchd is still tearing the job down, and bootstrapping a label that has
  # not finished going away fails with "Bootstrap failed: 5: Input/output
  # error". Re-running the installer is exactly when that happens, because the
  # previous run left the job loaded and running. Wait for it to be gone.
  launchctl bootout "gui/$UID/$AGENT_LABEL" >/dev/null 2>&1 || true
  for _ in $(seq 40); do
    if ! launchctl print "gui/$UID/$AGENT_LABEL" >/dev/null 2>&1; then break; fi
    sleep 0.25
  done
  launchctl bootstrap "gui/$UID" "$AGENT"
}

case "$LOGIN_ITEM" in
  ask)
    say "Start PadRemote automatically when you log in?"
    echo "Recommended: without it, a reboot leaves your phone with nothing to connect to."
    printf "Start at login? [Y/n] "
    read -r answer </dev/tty || answer=y
    case "$answer" in [Nn]*) LOGIN_ITEM=no ;; *) LOGIN_ITEM=yes ;; esac
    ;;
esac

if [ "$LOGIN_ITEM" = yes ]; then
  say "Setting up the login item"
  install_login_item
  echo "PadRemote will start with you. Undo with: $0 --uninstall"
else
  remove_login_item
  echo
  echo "Not starting at login. Add it later with: $0 --login"
fi

# ---------------------------------------------------------------- launch it

say "Starting PadRemote"
# One copy, one prompt. The login item's `RunAtLoad` has *already* started this
# build by the time we get here, and a second launch would run a second copy -
# each of which raises its own Accessibility dialog. That is where "macOS asked
# me for Security settings twice" came from.
running() { pgrep -f "$APP/Contents/MacOS/PadRemote" >/dev/null 2>&1; }

if [ "$LOGIN_ITEM" = yes ]; then
  # launchd starts the job asynchronously, so `bootstrap` returning does not
  # mean the process exists yet. Checking immediately loses that race and
  # launches exactly the duplicate this is here to avoid.
  # `if` rather than `running && break`: under `set -e` a bare AND-list that
  # fails ends the script, so the wait would abort the install on the first tick.
  for _ in $(seq 20); do
    if running; then break; fi
    sleep 0.25
  done
  if running; then
    echo "The login item started it - this build is up."
    already_up=1
  fi
fi

if [ -z "${already_up:-}" ]; then
  # Without a login item nothing has started it, but an older copy the user
  # opened by hand may still be up; that one is the previous build.
  if running; then
    echo "Already running - restarting it so you get this build."
    osascript -e 'quit app "PadRemote"' >/dev/null 2>&1 || true
    sleep 1
  fi
  # `open` rather than running the binary: it goes through Launch Services,
  # which is what makes macOS attribute the Accessibility prompt to the bundle.
  open -a "$APP"
fi

if [ -z "${CODESIGN_IDENTITY:-}" ]; then
  cat <<'SIGNING'

One-time note about the Accessibility permission
------------------------------------------------
This app is signed ad-hoc, and macOS remembers the grant against the exact
build. So every time you rebuild, the box in System Settings stays ticked while
the app is still refused - the single most confusing failure this project has.

To make the grant survive rebuilds, create a self-signed code-signing
certificate once and build with it:

  Keychain Access > Keychain Access menu > Certificate Assistant >
    Create a Certificate…
      Name: PadRemote Dev     Identity Type: Self Signed Root
      Certificate Type: Code Signing

  Then: CODESIGN_IDENTITY="PadRemote Dev" ./install.sh

SIGNING
fi

cat <<'DONE'

Done. PadRemote is in your menu bar - look for the ●● icon.

  1. macOS will ask for Accessibility permission. It has to: an app that moves
     your cursor cannot work without it.
       System Settings > Privacy & Security > Accessibility > PadRemote
     PadRemote notices the moment you grant it. No restart.

  2. Menu bar > "Connect a device…" > scan the QR with your phone's camera.

Your phone and your Mac need to be on the same Wi-Fi. Nothing is installed on
the phone, and nothing else needs to be running on the Mac - the page your
phone loads comes from PadRemote itself.

Trouble: docs/user/troubleshooting.md
DONE
