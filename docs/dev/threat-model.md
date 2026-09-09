# Threat model

PadRemote moves the cursor, clicks, drags and types shortcuts on the machine it
runs on, driven by touches arriving over a network. That is a remote-control
program, and it deserves to be described as one.

This document says what it defends against, what it does not, and — for each
claim — **where the check is that proves it**. A security property with no test
behind it is a comment, and comments do not survive refactors.

## What is being protected

| | |
|---|---|
| **The computer** | Anything that can open a session can click, drag, scroll and send Mission Control shortcuts. There is no sandbox: this is the user's own machine, with whatever is open on it. |
| **The touch stream** | Where fingers are on the pad. Not interesting on its own, but a live mirror of it (`/observe`) tells you what someone is doing. |
| **The config** | `/config` rewrites how the app behaves. Not as bad as driving the cursor, but not nothing. |

There is no camera, no microphone, no screen capture, no keyboard text, no
file access, and no account. Three files are stored, all in the user's own
config directory and all owner-only: `config.json`, the enrolment secret, and
`devices.json` — the list of paired device ids, which is what makes revoking one
of them survive a restart.

## Who the attacker is

Ordered by how likely they are to actually turn up.

### 1. A web page in a browser on this machine — *defended*

The one that matters most, because it needs nothing from the attacker except
that the user visits a page. **A WebSocket is not subject to the same-origin
policy**: there is no preflight, no CORS negotiation, no permission prompt. A
tab open in the background can connect to `ws://127.0.0.1:8787`, and before the
work in `src/net/origin.rs` it could then drive the cursor, silently.

Two things stop it, and either would be enough:

- The **`Origin` check** refuses the HTTP handshake with a 403 before a socket
  exists at all. A browser will not let a page lie about its origin, and a page
  served from a registrable domain never has an origin that passes.
- The **pairing challenge**: the page has no secret, so it cannot answer.

*Checked by:* `desktop/src/net/origin.rs` unit tests and
`desktop/tests/auth.rs::a_web_page_cannot_open_a_socket_at_all`, which asserts a
403 for `https://example.com` and for a DNS-rebinding origin.

### 2. Another device on the same Wi-Fi — *defended*

A phone, laptop or anything else on the network, including a guest on a café
network the user forgot they had joined. It can reach the port. It cannot open
a session: the desktop challenges every connection with a fresh 128-bit nonce
and reads nothing until the answer verifies.

The enrolment secret `P` is 128 bits from the OS random source, stored at mode
0600, and reaches the phone only in the fragment of the QR's URL — never in a
query string, where the page server would log it.

*Checked by:* `desktop/tests/auth.rs`, fifteen tests written from the attacker's
side. Each asserts that **nothing reached the OS**, because a server that
refuses politely and injects anyway would pass a check made on the socket alone.

### 2b. A phone that was lost, lent or stolen — *defended*

Each device holds a key of its own rather than a copy of `P`:
`HMAC-SHA256(P, "padremote:device:" + id)`, **derived on both sides and never
sent** — over a link that is still plain `ws://`, a key that is transmitted is a
key an eavesdropper has. The phone throws `P` away the moment it enrols.

*Forget* (on the connect page) revokes one of them: it drops the live session and
takes the id off the list, which survives a restart. The revoked phone still
holds its device key and gets nowhere with it, because an id that is not on the
list is checked against `P` — and it no longer has `P`. Being shown the QR again
is the way back, deliberately.

*Checked by:* `forgetting_one_device_leaves_the_others_working` and
`a_forgotten_device_cannot_re_enrol_with_the_key_it_kept` in `tests/auth.rs`,
plus the persistence tests in `src/auth/devices.rs`.

### 3. Someone reading the Wi-Fi — *partially defended, and this is the gap*

The link is **plain `ws://`**. It is not encrypted.

- They **cannot open a session.** The challenge is a fresh nonce per
  connection, so the answer they captured is worthless on the next one, and `P`
  itself never crosses the wire.
- They **can read the touch stream of a session in flight**, and could inject
  frames into an established TCP connection if they are positioned to.

This is the honest limit of where PadRemote is today, and TLS (`rustls`, with
the cert fingerprint pinned from the same QR) is what closes it — the other half
of milestone 3. Until then, the app should be treated the way you would treat
any unencrypted service on a network you do not control.

### 4. Another account on the same computer — *partially defended*

Anything that can read the user's own files can read the pairing secret, and
there is no defending against that. What is defended is accidental exposure:
the secret file is created at mode 0600, with the mode set at creation rather
than fixed afterwards, so its contents never exist world-readable even for an
instant.

The connect page carries the secret too, and used to be written to a file in
a world-listable `/tmp` for the browser to open. It is now served over loopback
instead and never written down — which also gives it an origin the app will
talk to, so it no longer needs an iframe to reach its own server.

*Checked by:* `auth.rs::a_saved_secret_comes_back_and_is_owner_only`, and
`pages.rs::the_host_gets_pairing_but_untrusted_hosts_get_no_secret` for who the
page is served to.

### 4b. A pairing nobody expected — *visible*

Authentication answers *do you hold a key*; it takes a person to answer *should
you*. The first time a device pairs, the Mac raises a notification naming it —
because a QR read over a shoulder looks exactly like a device pairing that
nobody was expecting, and silence is what would make that worth stealing. The
device then appears in the list on the connect page, with a *Forget* beside it -
which is the undo.

This is the half of Bluetooth-style pairing that PadRemote needs. The other
half — a typed number confirming the key exchange — the QR already covers, and
better: 128 bits over a channel an attacker cannot see, against a passkey's ~20.

### 5. A malicious dependency — *watched*

The realistic supply-chain risk for a small app is a crate, not the protocol.
`cargo deny` runs in CI: any RUSTSEC vulnerability fails the build at any depth,
every crate must come from crates.io, and a git dependency is refused outright.
The phone page has **zero runtime dependencies**, so no third-party code is in
the path of a touch.

*Checked by:* `desktop/deny.toml` and `tools/check-no-phone-home.sh` in the
`security` CI job.

### 6. Us — *checked, not trusted*

"No cloud, no accounts, no telemetry" is the claim a user cannot verify without
a packet capture, and the one that decays quietly: nothing crashes when an
update-checker gets added. `tools/check-no-phone-home.sh` fails the build on an
HTTP client in `Cargo.toml`, an outbound `connect` in `desktop/src`, a runtime
dependency or CDN `<script src>` in the phone page, a `fetch` anywhere in it, or
anything matching a known analytics endpoint.

### 7. The page server on the same port — *no new surface*

Since the app serves the phone page itself, port 8787 answers plain HTTP as well
as WebSocket. It is worth saying exactly what that does and does not add.

**What is handed out:** the static bundle in `web/dist`, to anyone who asks. It
is not a secret — it is the same JavaScript any visitor to a website downloads,
and every copy is identical. The pairing secret is not in it: that travels in the
QR's URL *fragment*, which a browser never sends to a server, so serving the page
gives away nothing that helps anyone connect.

**What it cannot reach:** anything outside the bundle. Responses come from a
table fixed at compile time (`desktop/build.rs`), so there is no path to
translate, no directory to escape and no file on disk to name — the traversal bug
this kind of code is famous for cannot be written here. `GET` and `HEAD` only;
everything else is a 405. `tests/pages.rs` holds the line.

**What it does not touch:** the socket. The challenge in `src/auth.rs` still
guards every connection, unchanged, and the page server runs before any of that
and hands off nothing to it. A browser that loads the page still has to answer
the challenge to move the cursor, exactly as when the page came from Vite.

The one thing that genuinely changed: with Vite gone, an attacker on the LAN can
no longer reach a Node development server with file-watching and a websocket of
its own. That is one fewer process listening, not one more.

## What is deliberately *not* defended

Saying so plainly is what makes the rest of the document worth reading.

- **A compromised machine.** If something is already running as the user, it
  can read the secret, and it did not need PadRemote to move the cursor anyway.
- **The user's own choices.** Whoever holds a paired phone can drive the
  computer. That is the product. *Forget* revokes one; *Forget all
  devices* (or `--unpair`) revokes every device, rotates the secret and drops
  the sessions already open.
- **A stolen QR, before the phone that scanned it has connected.** The code is
  the key until it is swapped for a device key, which happens on first contact.
  A pairing nobody expected raises a notification on the Mac, so it is visible
  rather than silent — but the window exists.
- **Denial of service.** Nothing caps how many sockets may be open *before* the
  challenge is answered, so someone on the network can hold a lot of them and
  make the app unusable. Each is cheap — one task and a ten-second timer, no
  recognizer, no device slot — and none of it gets them control of the cursor,
  which is why the trade was made this way. Rate limiting is worth adding; it is
  not what stands between an attacker and the machine.
- **Traffic analysis.** Packet sizes and timing reveal that someone is using a
  trackpad. TLS would not hide that either.

## Verifying it yourself

None of the above has to be taken on trust.

```sh
# Every way in that should be refused, asserted against what reached the OS -
# including a revoked device trying to get back in with the key it kept.
cargo test --test auth --manifest-path desktop/Cargo.toml

# No HTTP clients, no outbound connections, no third-party code, no analytics.
./tools/check-no-phone-home.sh

# The phone's hand-written SHA-256, against FIPS 180-4, RFC 4231 and OpenSSL,
# and its half of the handshake against a fake desktop.
cd web && npm run check:hmac && npm run check:link

# Known vulnerabilities, banned crates, licenses, crate sources.
cd desktop && cargo deny check

# And the claim nobody should take on faith: watch the wire while you use it.
sudo tcpdump -i any -n 'port 8787'   # the page and the link both live here
```

## Cryptography, and why some of it is hand-written

The desktop uses RustCrypto's `hmac` and `sha2`. The phone page implements
SHA-256 itself, in `web/src/hmac.ts`, and that deserves a justification rather
than an apology.

`crypto.subtle` exists only in a **secure context**. The phone page is served
over plain `http://` from a LAN address, which is not one — only `https://` and
`localhost` qualify. On the device this app is actually for, `crypto.subtle` is
`undefined`. The choice was a hand-written hash or no authentication at all.

The usual reasons not to write your own crypto mostly do not apply to a hash:
SHA-256 has no key-dependent branches to leak timing through, no padding oracle,
no nonce to reuse. The one real risk is getting it subtly wrong, so
`npm run check:hmac` checks it against the published FIPS 180-4 and RFC 4231
vectors and differentially against Node's OpenSSL over random inputs and every
length around a block boundary — 55, 56, 63, 64 — which is where hand-written
SHA-256 goes wrong. When the link moves to `wss://`, the page becomes a secure
context and this becomes a fallback rather than the only path.

The device-key derivation is pinned to the same third-party vector on both
sides — `a_device_key_matches_the_one_the_phone_derives` in `src/auth.rs` and
the matching case in `web/scripts/check-hmac.mjs` — because if the two ever
drift apart, every device fails to pair with a bare `badAuth` and neither side's
own tests would say why.

Comparison on the desktop side goes through `verify_slice`, which is
constant-time: a comparison that returns early on the first wrong byte leaks how
much of a guess was right, one byte per attempt.

## The handshake, end to end

```
phone                                          desktop
  │                                               │
  │  HTTP upgrade, Origin: http://192.168.1.24:8787
  ├──────────────────────────────────────────────▶│  origin::allowed()?
  │                                               │  no  → 403, no socket
  │  ◀── 101 Switching Protocols ─────────────────┤  yes → socket
  │                                               │
  │  ◀── {"t":"challenge","nonce":"<16 bytes hex>"}
  │                                               │
  │  {"t":"auth","hmac":HMAC-SHA256(P, nonce)}    │
  ├──────────────────────────────────────────────▶│  constant-time verify
  │                                               │  wrong/absent/late →
  │                                               │    {"t":"error",
  │                                               │     "code":"badAuth"}, close
  │                                               │  right → join, session runs
  │  ── touch frames ────────────────────────────▶│
```

Nothing before the last line registers a device, allocates a recognizer, or
reads a touch. The wire format is in
[`protocol/v1.schema.json`](../../protocol/v1.schema.json).
