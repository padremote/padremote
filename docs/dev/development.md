# Development

## Requirements

Rust (stable), Node 18+, macOS 13+. Python 3 only for the retired prototype and
the icon generator.

## Layout

```
protocol/v1.schema.json   the wire format, source of truth
desktop/                  the Rust app
  src/cli.rs              flags and --help
  src/app.rs              the loops that run for the life of the process
  src/gesture/            touch → intent. Pure, OS-agnostic
  src/input/              intent → OS events. Only platform code
    macos.rs              CGEvent
    portable.rs           enigo, for Windows and Linux (unproven)
  src/sysprefs/           reads the host's trackpad settings
    macos.rs              CFPreferences
    windows.rs            the Precision Touchpad registry keys (unproven)
    linux.rs              GNOME gsettings, KDE kcminputrc (unproven)
  src/net/                WebSocket server
    shared.rs             the devices, and which one drives the cursor
    session.rs            one phone's conversation
    settings.rs           the settings page: config in and out
    devices.rs            the connect page: who is paired, and un-pairing
    observe.rs            the read-only telemetry feed
    pages.rs              the phone page itself, served on the same port
    status.rs             what the menu bar reads
  build.rs                compiles web/dist into the binary
  src/sync.rs             locking that survives a panic elsewhere
  src/bin/replay.rs       replay a recorded stream, with or without injection
  tests/fixtures/         eleven recorded gestures
  packaging/make-app.sh   builds PadRemote.app (page included)
web/                      the five pages Vite builds, and their checks
  src/theme.css           the one visual language: Tailwind's tokens - see design.md
  index.html              the trackpad
  config.html             the settings page that replaced editing JSON
  src/trail.ts            the canvas: sizing, finger trails
  src/trail/hold.ts       the long-press indicator and held-drag glow
  preview.html            a bench for that animation, on the computer
  src/config.css          the gesture drawings: an SVG animation engine, not styling
  src/style.css           the trackpad frame's viewport-height fallback pair
  scripts/                nine headless checks - the real modules, a stub DOM
tools/proto/              retired Python prototype (see below)
docs/                     you are here
install.sh                one command: build, install, login item, launch
.github/workflows/ci.yml  what has to pass
```

## Running

The shipped app is one process: `web/dist` is compiled into the binary by
`desktop/build.rs`, and `src/net/pages.rs` serves it on the same port as the
WebSocket. So there is nothing to start alongside it.

```sh
# build the page once, then run the app that contains it
cd web && npm install && npm run build
cargo run --manifest-path desktop/Cargo.toml
```

Startup prints the phone's URL and the debug view's, and logs how many files of
the page went into the binary. `cargo build` with no `web/dist` next to it still
works — the page table comes out empty and the app says so, on startup and in
its 404 — because CI's Rust job builds without Node and a contributor working on
the gesture engine should not need it either.

### While working on the page

Hot reload is worth having, so point the QR at Vite instead:

```sh
cd web && npm run dev                                          # page on :5173
cargo run --manifest-path desktop/Cargo.toml -- --page-port 5173
```

`--page-port` changes only what the QR says. The app still serves whatever page
was compiled into it, on its own port.

The settings page needs a computer to render at all - it is built from the
config the desktop sends - which makes looking at a change to it a rebuild and
an install. `npm run mock` is a stand-in that speaks `/config` and `/devices`
with a canned payload and never challenges, so there is no pairing to arrange:

```sh
cd web && npm run mock                                         # a fake desktop on :8788
cd web && npm run dev                                          # the page on :5173
open http://localhost:5173/config.html?h=localhost:8788
```

It is for *looking*. Behaviour is checked by `check-settings.mjs`, which needs
neither process. Note that a page served from 5173 is a different origin from
the app's own port, so a phone has no credential there - on the Mac, the menu
bar's Settings… link carries the secret in its fragment and works anywhere.

### Changing how something looks

It is a utility class on the element, in the page or in the TypeScript that
builds it - [design](design.md#implementation) says what the exceptions are and
why `theme.css` is the only stylesheet anyone writes by hand.

Nothing in `npm run check` can tell you whether that worked. Tailwind generates
rules only for class names it finds spelled out in full, so a typo or a name
built from a variable produces no rule and no complaint - see
[gotchas](gotchas.md#the-interface). The browser is the check. Look at the page
you changed at a laptop width and at a phone width before calling it done; the
mock above is there so that does not cost a rebuild.

| Flag | Effect |
|---|---|
| `--dry-run` | Recognize gestures, inject nothing. Safe for engine work |
| `--headless` | No menu-bar icon. For SSH or a debugger |
| `--no-qr` | Don't open the pairing page at startup |
| `--port` | The page and the link it opens. Default 8787, and moving it moves both |
| `--page-port` | Point the QR at a page served elsewhere — `npm run dev` on 5173. Development only |
| `--help`, `--version` | An unknown flag is an error, not a shrug |

**Only one copy of the desktop app can run.** `PadRemote.app` and a `cargo run`
will fight for the port; the second reports it plainly and exits.

**Any number of phones can.** Scan the QR from a second device and both stay
connected, taking turns with the cursor — see
[protocol](protocol.md#several-devices-one-cursor).

## Testing

```sh
cargo test --manifest-path desktop/Cargo.toml        # 142 tests, no side effects
cd web && npm run check                              # types, animations, crypto vectors
```

The unattended suite never touches your cursor. The one test that does is
`#[ignore]`d and must be asked for:

```sh
cargo test --manifest-path desktop/Cargo.toml --test injection -- --ignored --nocapture
```

It moves the cursor, scrolls, zooms and switches spaces — every effect net-zero,
and it restores the cursor position before returning.

### What the suites cover

| Suite | Covers |
|---|---|
| `tests/fixtures.rs` | The eleven recorded gestures, plus parity with the Python prototype |
| `tests/swipes.rs` | Swipes, pinches, drag styles, smart zoom, ragged hands |
| `src/sysprefs/` unit tests | The host mapping, including a real Mac's exact settings |
| `src/gesture/config.rs` | Compiled defaults match `config.default.json` |
| `tests/sessions.rs` | Several devices at once: per-device engines, whose turn it is, nothing stuck on disconnect |
| `tests/pages.rs` | One port, two protocols: a browser gets the page, a handshake still becomes a socket, and neither change can break the other unnoticed |
| `tests/auth.rs` | Every way in that should be refused - no secret, a wrong one, a replayed one, touches before the handshake, a web page's origin - each asserting that **nothing reached the OS** |
| `src/auth.rs`, `src/net/origin.rs` unit tests | The HMAC and the device-key derivation against an independent implementation, the secret file's mode, and which origins are a website rather than the phone page |
| `src/auth/devices.rs` unit tests | The paired-device list: enrolled once, revocation that survives a restart, a corrupt file that does not stop the app |
| `web/scripts/check-hmac.mjs` | The hand-written SHA-256 against FIPS 180-4, RFC 4231, and Node's OpenSSL over random inputs and every block boundary |
| `web/scripts/check-link.mjs` | The phone's half of the handshake: no touches before the answer, no retry loop when unpaired, a stale socket that cannot disarm the live one, and enrolment - the credential kept only once the desktop accepts it, the QR secret discarded once it is |
| `src/cli.rs` unit tests | Flags parse, and a typo is an error |
| `src/sync.rs` unit tests | A poisoned lock does not take the app down with it |
| `web/scripts/check-hold-ring.mjs` | The phone's press ring agrees with the engine |
| `web/scripts/check-hold-intro.mjs` | The drag confirmation grows out of the charge, and nothing draws a rounded corner |
| `web/scripts/check-shake.mjs` | A shake reads as a shake - and a knock, a walk, slow rocking and a flick mid-gesture do not |
| `web/scripts/check-settings.mjs` | The settings page against a DOM stub: which page a control lands on, the hub's live subtitles, gesture assignment and inheritance, host-decided controls, and the paired-device list |
| `web/scripts/check-mobile-ui.mjs` | The touch controls and the delayed viewport: the real modules against a small event and geometry host, so a phone-only layout bug can fail on a laptop |
| `web/scripts/check-connect.mjs` | The connect page, which nothing else covers - it is served by the app, so it is neither typechecked nor built. Runs the real inline script out of `desktop/src/assets/connect.html` |
| `src/sysprefs/windows.rs`, `linux.rs` | The Windows and Linux mappings, from recorded raw values - testable because the reading and the mapping are separate |

### Writing gesture tests

Build streams that are **deliberately imperfect**. Fingers landing 150 ms apart,
one lifting early, the hand splaying mid-swipe — every multi-finger bug so far
hid behind perfectly synchronized synthetic input.

## Debugging

The live debug view is the main tool: **Diagnostics** on the connect page, or the `debug ->`
URL printed at startup. It attaches read-only, so it can stay open while a phone
drives.

It answers the question that text logs can't: whether a problem is capture,
network, or recognition. See [troubleshooting](../user/troubleshooting.md).

### The press animation, without a phone

`/preview.html` runs the real `TrailRenderer` on the computer: press and hold
anywhere with the mouse, or hit **Replay** for the scripted gesture. Nothing on
that page re-implements any drawing, so what it shows is what the phone does.

The arming beat is 380 ms long and its first frame is the one that matters, so
the page also stops on an exact millisecond - from the console:

```js
padremotePreview.at(420);   // late charge: brackets closing on their marks
padremotePreview.at(545);   // 45 ms in: the scan crossing, brackets thrown out
padremotePreview.at(1500);  // the held drag, breathing
padremotePreview.live();    // back to normal
```

Reach for this before changing anything in `trail/hold.ts`. A browser pauses
`requestAnimationFrame` in a background tab, so "wait 500 ms and look" is not
even reliable; this replays frame by frame and holds the one you asked for.

Without a phone:

```sh
cargo run --manifest-path desktop/Cargo.toml --bin replay -- \
  desktop/tests/fixtures/two_finger_scroll.json           # print actions
cargo run --manifest-path desktop/Cargo.toml --bin replay -- --inject \
  desktop/tests/fixtures/one_finger_move.json             # drive the cursor
```

`RUST_LOG=padremote=debug` raises the log level.

## Config

`~/Library/Application Support/PadRemote/config.json`, seeded from
`desktop/config.default.json` and polled about twice a second — edit it while the
app runs.

Changing a default means changing **both** files; a test enforces that.

## Packaging

```sh
./desktop/packaging/make-app.sh              # → ~/Applications/PadRemote.app
./desktop/packaging/make-app.sh /Applications
```

Builds release, assembles the bundle, generates the icon (a dependency-free PNG
writer — the icon is a few rounded rectangles), ad-hoc signs it so the
Accessibility grant survives rebuilds, and nudges Spotlight.

Not yet Developer ID signed or notarized: on another Mac, Gatekeeper still
objects. That is milestone 5.

## The Python prototype

`tools/proto/` is **retired**. It existed to tune the recognizer against a live
cursor before the Rust app did, and its output was the timings in
`config.default.json` and the eleven fixtures.

It is kept because `tests/fixtures.rs` still checks the Rust engine agrees with
it action-for-action — a useful guard that the port never silently drifted.
Gestures added since (swipes, pinches, smart zoom) are Rust-native and are
**not** in the prototype. Don't add features there.

## Before you commit

Exactly what CI runs (`.github/workflows/ci.yml`), so there are no surprises:

```sh
cd desktop
cargo fmt --check
cargo clippy --all-targets -- -D warnings   # clean, and kept that way
cargo test
cargo build --release
# The unproven halves still have to compile.
cargo check --target x86_64-pc-windows-msvc --all-targets

cd ../web
npm run check                                # types, ring, intro, crypto vectors
npm run build

# The security job. What each guards is in docs/dev/threat-model.md.
cd ..
./tools/check-no-phone-home.sh               # no HTTP client, no outbound, no analytics
cd desktop && cargo deny check               # advisories, licences, crate sources
```

`cargo deny` needs `cargo install --locked cargo-deny` once; CI uses the action
instead. Everything else runs with what the project already needs.

CI runs the Rust half on macOS: the input backend, the trackpad mirroring and
the tray are macOS-only, so a Linux runner would happily green-light a stub of
the app nobody ships.
