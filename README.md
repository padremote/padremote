# PadRemote

Turn your phone's touchscreen into a wireless trackpad for your computer. The
computer runs a small app; the phone just opens a web page.

**Documentation is in [`docs/`](docs/README.md)** — [getting
started](docs/user/getting-started.md) and [gestures](docs/user/gestures.md) if
you want to use it, [architecture](docs/dev/architecture.md) and
[gotchas](docs/dev/gotchas.md) if you want to work on it.

The full product specification is [`plan.md`](plan.md). This repository currently
implements **milestones 0-2** of it plus QR pairing: the gesture engine, the
macOS input backend, live mirroring of the host's own trackpad settings, and a
phone page that drives the cursor over the local network.

## Install

```sh
./install.sh
```

Builds the app with the phone page inside it, installs it to `~/Applications`,
offers to start it when you log in, and launches it. Then grant Accessibility
when macOS asks, and use **Connect a device…** in the menu bar. Full walkthrough:
[getting started](docs/user/getting-started.md).

## What works today

| | |
|---|---|
| Cursor movement | relative, with a macOS-like acceleration curve |
| Tap | left click, with double- and triple-click |
| Two-finger tap | right click (three-finger tap: middle click) |
| Two-finger drag | pixel-precise scroll, natural-scroll toggle, momentum |
| Pinch | zoom, via the app-zoom backend (`⌘=` / `⌘-`) |
| Press-and-drag, tap-and-drag | click-and-hold dragging |
| Four-finger swipe | switch spaces (left/right), Mission Control (up), app windows (down) |
| Several devices at once | a phone and a tablet stay connected and take turns with the cursor |
| Settings | a page, on whichever screen is nearer — not a JSON file you have to find |
| Reconnect | automatic, 1 s → 10 s backoff |
| Installing | one command, one process, one port — the app serves the phone page itself |
| Safety | losing the connection always releases held buttons |
| Pairing | HMAC challenge over the QR's 128-bit secret; web pages refused at the handshake |
| Devices | Named on the connect page, one session each, revocable one at a time |

**It mirrors your real trackpad.** On startup — and every half-second after —
PadRemote reads your actual macOS trackpad settings and matches them, so it
behaves like the trackpad you already use rather than like some hardcoded
default. Change a switch in System Settings → Trackpad and PadRemote follows
within seconds, no restart. Set `"followSystem": false` in the config to pin
behaviour to the file instead.

**Use it from more than one device.** Scan the QR with a second phone or tablet
and both stay connected. They share the one cursor and take turns: it goes to
whoever touches next, as soon as the other one stops. The device that is waiting
says so, and names the one that has it. Nothing to press, nothing to close.

**Pairing is enforced.** The QR carries a 128-bit secret in its URL fragment,
and every connection - the trackpad, the diagnostics view and the settings page
alike - answers an `HMAC-SHA256` challenge before the desktop reads a single
touch. A connection from a web page is refused during the handshake, which
matters more than it sounds: a WebSocket is not subject to the same-origin
policy, so without that check any tab open in any browser on your machine could
have driven your cursor. Each phone then swaps the code for a key of its own and
forgets the code, so *Forget* beside a device on the connect page can revoke one
phone without disturbing the others - and it stays revoked after a restart,
because the phone no longer has the code to enrol itself again. *Forget all
devices* does the same for all of them at once, and replaces the code with it.

**One session per device.** Open the pad in a second tab and the first stands
down and says where its trackpad went, rather than the two racing each other for
the cursor. A phone and a tablet still stay connected and take turns; only a
device replacing *itself* is ever hung up on.

**One process, one port.** The page your phone loads is compiled into the app and
served on the same port the phone then opens its socket on. There is no second
server to start and nothing to leave running in a terminal — which used to be
the most common way to arrive at a phone saying "this site can't be reached"
while the menu-bar app looked perfectly healthy.

**Not yet** (milestones 3-5): TLS, PWA install, and a signed installer. The link
is still plain `ws://`, so someone already positioned to read your Wi-Fi can see
the touch stream - they cannot open a session of their own. What is defended,
what is not, and where the test for each claim lives:
[threat model](docs/dev/threat-model.md).

## Layout

```
install.sh                one command: build, install, login item, launch
protocol/v1.schema.json   the wire format both sides implement
desktop/                  the Rust app (gesture engine + macOS injection + page + server)
web/                      the phone page (Vite + TypeScript, no framework)
tools/proto/              throwaway Python prototype the engine was tuned in
docs/                     everything above in more detail
tools/                    the no-phone-home guard CI runs on every push
.github/workflows/ci.yml  fmt, clippy, tests and both builds
```

The gesture engine in `desktop/src/gesture/` is pure and OS-agnostic. The
Windows and Linux backends — input through `enigo`, settings from the Precision
Touchpad registry keys and from GNOME/KDE — are **written but have never been
run on real hardware**; they compile, their mappings are unit-tested, and CI
type-checks the Windows build on every push. See
[porting](docs/dev/porting.md).

## Run it from source

You need Rust and Node. On first run macOS will need **Accessibility**
permission for whatever runs the app (in development, your terminal):
System Settings → Privacy & Security → Accessibility.

```sh
# the page is compiled into the app, so build it first
cd web && npm install && npm run build

# then the app, which serves it - prints the URL to open on your phone
cargo run --manifest-path desktop/Cargo.toml
```

Then open the printed `http://<your-ip>:8787/#h=<your-ip>:8787&k=…` on a phone on
the same Wi-Fi. The status dot goes green and the whole screen becomes the
trackpad.

While working on the page itself, run Vite for hot reload and tell the QR where
it is — `--page-port` changes only the address in the QR:

```sh
cd web && npm run dev                                          # page on :5173
cargo run --manifest-path desktop/Cargo.toml -- --page-port 5173
```

Useful flags:

```sh
cargo run --manifest-path desktop/Cargo.toml -- --dry-run    # recognise, inject nothing
cargo run --manifest-path desktop/Cargo.toml -- --headless   # no menu-bar icon
cargo run --manifest-path desktop/Cargo.toml -- --port 9000
cargo run --manifest-path desktop/Cargo.toml -- --unpair     # revoke every paired phone
cargo run --manifest-path desktop/Cargo.toml -- --help
```

### What gets mirrored

| System Settings | PadRemote |
|---|---|
| Scrolling direction: Natural | scroll direction |
| Tap to click | tap → left click |
| Secondary click (two fingers) | two-finger tap → right click |
| Three finger tap | three-finger tap → middle click |
| Zoom in or out (pinch) | pinch → zoom |
| Scroll direction: horizontal | side-to-side scrolling |
| Swipe between pages / full-screen apps | three- and four-finger swipes |

Not mirrored, and why: **tracking and scroll speed** (Apple's acceleration curve
is private, so `sensitivity` stays manual), **Force Touch, haptics and rotate**
(no phone hardware, no synthesis path), and **three-finger drag** (three fingers
are kept for the swipes — press and hold to drag instead). The Accessibility
switch is still read, because selecting it is what turns *one-finger* dragging
off on the Mac, and that PadRemote does follow.

Swipes are produced by sending the Mission Control keyboard shortcuts (Ctrl +
arrow), since no public API can synthesize a real multi-finger swipe. If you've
disabled those shortcuts in System Settings → Keyboard, PadRemote detects it and
warns at startup rather than silently doing nothing.

### Install it as a Mac app

```sh
./install.sh
```

Builds the page, compiles it into a release binary, wraps that in
`PadRemote.app`, installs it to `~/Applications` where **Spotlight finds it**
(Cmd-Space, type "PadRemote"), offers to start it at login, and launches it.
`./desktop/packaging/make-app.sh` is the build-and-install step on its own.

It runs as a menu-bar app with no Dock icon. The menu says how many devices are
connected and offers three things: **Connect a device…**, **Settings…** and
**Quit PadRemote**. Everything else lives on the pages those two open - the QR,
the paired devices and the way to forget them are all on the connect page.

Two things to expect about the Accessibility grant. macOS remembers it **per
binary**, so the app bundle needs its own, separate from the one your terminal
has — the app detects this on first launch and points you at the right pane. And
it remembers it **per build**: with no code-signing certificate the bundle is
signed ad-hoc, whose identity is the hash of that exact binary, so a rebuild is a
different app as far as macOS is concerned and the old grant is dead while still
looking enabled. `install.sh` clears the stale one and prints the one-time
Keychain Access recipe that makes grants stick. Developer ID signing and a `.dmg`
come with milestone 5.

### Tuning

**Open the settings page** — menu bar → *Settings…*, or the `settings ->`
address printed at startup, or *All settings* in the phone's own sheet. It is
the same page from the phone or the computer, it is live on both at once, and it
saves as you change something. Anything your computer's own trackpad decides is
shown locked, with the setting that decides it named beside it.

Underneath, behaviour still comes from
`~/Library/Application Support/PadRemote/config.json` (seeded from
`desktop/config.default.json`) and is **hot-reloaded** — editing it by hand
works exactly as before.

The phone's settings sheet is an **override** of that, not a copy of it. Each
control shows the computer's own value and says "matching your computer"
underneath; change one and it becomes yours — on that device only, and it
survives the computer re-reading its settings — until you tap "match my
computer" to hand it back.

## Tests

```sh
cargo test --manifest-path desktop/Cargo.toml          # engine, protocol, sessions - unattended
cargo test --manifest-path desktop/Cargo.toml --test injection -- --ignored   # drives the real cursor
cd web && npm run check                                # types + the two hold-animation checks
cd tools/proto && .venv/bin/python make_fixtures.py    # regenerate + check fixtures
```

`cargo test` covers the eleven recorded gestures, the swipe gestures, the
protocol, the system-mirroring mapping — including a case built from a real
Mac's settings, where three-finger tap is off while PadRemote's own default maps
it to middle click, which is exactly the mismatch the mirroring exists to fix —
and how two devices share one cursor without corrupting each other's gestures.

`npm run check` type-checks the page and then drives the *real* renderer
headlessly through a long press, asserting both that the ring agrees with the
engine and that the drag confirmation grows out of it frame by frame. CI runs
all of it (`.github/workflows/ci.yml`), with `cargo fmt --check` and
`cargo clippy -- -D warnings`.

Every v1 gesture has a recorded touch stream in `desktop/tests/fixtures/`. The
suite asserts both the specified behaviour and that the Rust engine still agrees
action-for-action with the Python prototype the timings were tuned against.

To watch a gesture without a phone:

```sh
cargo run --manifest-path desktop/Cargo.toml --bin replay -- desktop/tests/fixtures/two_finger_scroll.json
cargo run --manifest-path desktop/Cargo.toml --bin replay -- --inject desktop/tests/fixtures/one_finger_move.json
```

## Design notes

- The phone is deliberately dumb: it reports **raw touch points** and decides
  nothing. All interpretation happens on the desktop, so behaviour can be tuned
  without redeploying the page.
- Every surface shares one visual language, and it is taken from the long-press
  indicator: chamfered corners rather than rounded ones, corner brackets rather
  than boxes, tracked monospace for anything the machine is stating. See
  [design](docs/dev/design.md).
- Touch samples travel as 14-byte binary records batched once per animation
  frame; control messages are JSON on the same socket.
- The page ignores **mouse** pointers by default. Opened on the very computer it
  controls, the cursor it moves would pass back over the page as mouse input and
  feed itself — a runaway loop. Add `?mouse=1` (with `--dry-run`) to drive it
  from a desktop browser deliberately.
- Each connected device gets its **own** gesture engine on the desktop, and they
  take turns driving the one cursor - handing it over only at the start of a
  gesture, once the previous device has been idle for a moment. Sharing one
  engine merged two hands into one state machine; handing over mid-gesture
  injected half of one.
- The page is served over plain http, because an https page may not open a
  `ws://` socket. Wake Lock and PWA install need a secure context and therefore
  arrive with the TLS work; so does `crypto.subtle`, which is why the phone
  answers the pairing challenge with a hand-written SHA-256 checked against the
  published vectors (`web/src/hmac.ts`, `npm run check:hmac`).
