#!/usr/bin/env bash
#
# "Nothing leaves your machine" is the claim PadRemote is built on. This is the
# check that keeps it true.
#
# The app has no cloud, no accounts, no telemetry and no analytics, and it stays
# that way not because anyone remembers but because a build that adds an HTTP
# client, a CDN script tag, or an outbound connection fails here. That matters
# more than it sounds: the failure mode of a privacy claim is not a crash, it is
# a quiet regression nobody notices for a year.
#
# What is deliberately allowed:
#   - the app *accepting* connections. It is a server; that is the whole point.
#   - `open`/`Command::new("open")`, which hands a local file to the browser.
#   - the dev-time tooling in web/scripts and tools/, which never ships.
#
#   ./tools/check-no-phone-home.sh
set -uo pipefail
cd "$(dirname "$0")/.."

fail=0
report() {
  fail=1
  echo "FAIL: $1"
  shift
  printf '  %s\n' "$@"
}

# ---------------------------------------------------------------- the desktop

# HTTP clients. None of these is in Cargo.toml, and adding one is how a
# "check for updates" or a crash reporter would arrive.
if hits=$(grep -nE '^(reqwest|ureq|hyper|isahc|curl|surf|attohttpc|awc) *=' desktop/Cargo.toml); then
  report "desktop/Cargo.toml has an HTTP client" "$hits"
fi

# Outbound sockets in the shipped app. `TcpListener` is fine - that is the
# server. `TcpStream::connect` is the app calling out to somewhere.
if hits=$(grep -rnE 'TcpStream::connect|UdpSocket::(connect|send_to)|connect_async' desktop/src); then
  report "desktop/src opens an outbound connection" "$hits"
fi

# A URL that is not documentation. Anything reachable belongs in a comment, in
# docs, or nowhere.
#
# Test modules are cut out first: `net/origin.rs` proves that a page at
# `https://example.com` is refused, and the only way to write that test is to
# name the address. That cut is only safe while `#[cfg(test)]` is the last thing
# in each file, so the check below enforces exactly that - otherwise a module
# with its tests at the top would silently exempt itself from everything here.
# `$(...)` inside `if hits=...` inherits the command's exit status, which is why
# every other check here can lean on grep returning 1 for "no match". A loop
# cannot, so this one tests the output directly.
hits=$(for f in $(find desktop/src -name '*.rs'); do
  start=$(grep -n '^#\[cfg(test)\]' "$f" | head -1 | cut -d: -f1)
  [ -z "$start" ] || awk -v s="$start" \
    'NR > s && /^(pub )?(fn|struct|enum|impl|const|static) / { print FILENAME ":" NR ": " $0 }' "$f"
done)
if [ -n "$hits" ]; then
  report "code lives after a #[cfg(test)] module, which the URL check below skips" "$hits"
fi

if hits=$(find desktop/src -name '*.rs' -exec awk '/^#\[cfg\(test\)\]/ { nextfile } { print FILENAME ":" FNR ":" $0 }' {} + \
    | grep -E 'https?://' \
    | grep -vE '://(localhost|127\.0\.0\.1|\{ip\}|\{\}|<ip>)' \
    | grep -vE '^[^:]+:[0-9]+: *(//|///|//!|\*| \*)' \
    | grep -vE 'rust-lang\.org|json-schema\.org|w3\.org|apple\.com/DTDs|padremote\.com/protocol'); then
  report "desktop/src contains a live URL" "$hits"
fi

# ------------------------------------------------------------- the phone page

# Third-party code on the phone. The page has zero runtime dependencies, which
# is why "no third-party code touches your input" is a claim and not a hope.
if node -e '
  const p = require("./web/package.json");
  const deps = Object.keys(p.dependencies || {});
  if (deps.length) { console.log(deps.join(", ")); process.exit(0); }
  process.exit(1);
' 2>/dev/null > /tmp/padremote-deps.$$; then
  report "web/package.json has runtime dependencies" "$(cat /tmp/padremote-deps.$$)"
fi
rm -f /tmp/padremote-deps.$$

# A script, stylesheet, font or image loaded from somewhere else. One <script
# src> from a CDN would put a third party in the path of every touch.
if hits=$(grep -rnE '(src|href)="https?://' web/*.html web/src 2>/dev/null); then
  report "the phone page loads something from the network" "$hits"
fi

# Calls out of the page. `fetch`, `sendBeacon` and `XMLHttpRequest` have no
# legitimate use here: the only thing the page talks to is the desktop, over the
# WebSocket it was pointed at.
if hits=$(grep -rnE '\b(fetch|sendBeacon|XMLHttpRequest|EventSource)\b' web/src 2>/dev/null); then
  report "the phone page makes HTTP requests" "$hits"
fi

# The analytics that always arrives "just to see how many people use it".
# Matched on the hostnames and API entry points, not on English words: an
# earlier version of this line flagged the word "plausible" in a comment and the
# word "amplitude" in the haptics code, and a check that cries wolf is a check
# someone deletes.
if hits=$(grep -rniE 'google-analytics\.com|googletagmanager\.com|gtag\(|plausible\.io|posthog\.(com|init)|mixpanel\.(com|init|track)|sentry\.(io|init)|datadoghq|segment\.(com|io)|amplitude\.(com|getInstance)|umami|matomo' web/src web/*.html desktop/src 2>/dev/null); then
  report "something looks like analytics" "$hits"
fi

# ------------------------------------------------------------------- the rest

if [ "$fail" -eq 0 ]; then
  echo "no-phone-home: no HTTP clients, no outbound connections, no third-party"
  echo "               code on the phone page, no analytics."
fi
exit "$fail"
