# PadRemote documentation

Turn your phone's touchscreen into a wireless trackpad for your Mac. The
computer runs a small app; the phone just opens a web page.

## If you want to use it

```sh
./install.sh
```

One command: builds the app with the phone page inside it, installs it to
`~/Applications`, offers to start it at login, and launches it. There is no
second server to run.

| | |
|---|---|
| [Getting started](user/getting-started.md) | Install, pair your phone, first use |
| [Settings](user/settings.md) | Changing how it behaves, from the phone or the computer |
| [Gestures](user/gestures.md) | What every gesture does, and how PadRemote copies your Mac's trackpad settings |
| [Troubleshooting](user/troubleshooting.md) | The cursor won't move, the QR is stale, the status flickers |

## If you want to work on it

| | |
|---|---|
| [Architecture](dev/architecture.md) | How the pieces fit, and which one to change |
| [Design](dev/design.md) | The one visual language every surface shares, and why it is shaped like that |
| [Gesture engine](dev/gesture-engine.md) | The state machine: touches in, intents out |
| [Trackpad mirroring](dev/trackpad-mirroring.md) | Reading the host's real settings and mapping them |
| [Protocol](dev/protocol.md) | The wire format between phone and desktop |
| [Threat model](dev/threat-model.md) | What it defends against, what it doesn't, and where each check is |
| [Development](dev/development.md) | Build, test, debug, release |
| [Gotchas](dev/gotchas.md) | **Read this before touching input injection.** Every trap that has already cost a day |
| [Porting](dev/porting.md) | What Windows and Linux would need |

## If you want to ship it

| | |
|---|---|
| [Monetization](product/monetization.md) | Free tier vs Pro, pricing, offline license keys, and why there are no ads |

## Where things stand

Milestones 0–2 of [`plan.md`](../plan.md) are built, plus the QR pairing from
milestone 3 and the trackpad mirroring that milestone 3 did not anticipate.

**Working:** cursor movement, tap and multi-finger clicks, pixel-precise scroll
with momentum, pinch zoom, three- four- and five-finger swipes and
pinches, smart zoom, back/forward, auto-reconnect, live mirroring of the host's
own trackpad preferences, **several phones or tablets connected at once** taking
turns with the cursor, and a **settings page** that replaced hand-editing JSON.
The desktop app **serves the phone page itself**, on the same port the phone
connects to, so the whole product is one process and one install command.

**Written but never run on real hardware:** the Windows and Linux halves — an
`enigo`-backed input backend, the Precision Touchpad registry reader, and the
GNOME/KDE readers. They compile (Windows is cross-checked in CI) and their
mappings are unit-tested, but nobody has yet tried them on the machines they
are for. See [porting](dev/porting.md).

**Not yet:** TLS — the link is plain `ws://`, so someone already positioned to
read your Wi-Fi can read the touch stream, though they cannot open a session of
their own. Also PWA install and a signed, notarized `.dmg` (until then the app
is ad-hoc signed, which means macOS asks for Accessibility again after every
rebuild).

**Pairing is enforced.** Every connection answers an HMAC challenge, a
connection from a web page is refused at the handshake, and each device holds a
key of its own — so one phone can be revoked without the others re-scanning, and
it stays revoked after a restart. The menu bar is a count and three items -
Connect a device…, Settings…, Quit - and the paired devices, with the button
that forgets one, are on the connect page where a list can be read before it is
acted on. See [threat model](dev/threat-model.md), which says where the test for
each claim lives.

Everything here is checked on every push: `cargo fmt`, `cargo clippy -D
warnings`, the Rust suite on macOS, the phone page's type check and headless
behaviour checks, and a `security` job that runs the attack tests, the
no-phone-home guard, the crypto vectors and `cargo deny`
(`.github/workflows/ci.yml`).
