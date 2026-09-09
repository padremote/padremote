# PadRemote documentation

Turn your phone's touchscreen into a wireless trackpad for your Mac. The
computer runs a small app; the phone just opens a web page.

## Using it

| | |
|---|---|
| [**Getting started**](user/getting-started.md) | Install, pair a phone, first use |
| [**Gestures**](user/gestures.md) | What every gesture does, and how your Mac's own trackpad settings are copied |
| [**Settings**](user/settings.md) | Changing how it behaves, from the phone or the computer |
| [**Troubleshooting**](user/troubleshooting.md) | The cursor won't move, the QR is stale, the status flickers |

<p>
  <img src="img/connect.png" alt="The connect page" width="260">
  <img src="img/pad.png" alt="The trackpad on a phone" width="120">
  <img src="img/settings-gestures-swipe.png" alt="The gestures page" width="290">
</p>

## Working on it

| | |
|---|---|
| [**Architecture**](dev/architecture.md) | How the pieces fit, and which one to change |
| [**Development**](dev/development.md) | Build, test, debug, release |
| [**Design**](dev/design.md) | The one visual language every surface shares |
| [**Gesture engine**](dev/gesture-engine.md) | The state machine: touches in, intents out |
| [**Trackpad mirroring**](dev/trackpad-mirroring.md) | Reading the host's real settings and mapping them |
| [**Protocol**](dev/protocol.md) | The wire format between phone and desktop |
| [**Threat model**](dev/threat-model.md) | What it defends against, what it doesn't, and where each check is |
| [**Gotchas**](dev/gotchas.md) | **Read before touching input injection.** Every trap that has already cost a day |
| [**Porting**](dev/porting.md) | What Windows and Linux would need |

## Where things stand

**Working.** Cursor, taps and multi-finger clicks, pixel-precise scroll with
momentum, pinch zoom, three- four- and five-finger gestures, smart zoom,
back/forward, auto-reconnect, live mirroring of your Mac's trackpad settings,
several phones or tablets connected at once, and a settings page that replaced
hand-editing JSON. The desktop app serves the phone page itself, so the whole
product is one process and one install command.

**Written but never run on real hardware.** The Windows and Linux halves — an
`enigo`-backed input backend and the Precision Touchpad / GNOME / KDE settings
readers. They compile and their mappings are unit-tested; nobody has tried them
on the machines they are for. See [porting](dev/porting.md).

**Not yet.** TLS — the link is plain `ws://`, so someone already positioned to
read your Wi-Fi can read the touch stream, though they cannot open a session of
their own. Also PWA install and a signed, notarized `.dmg`; until then the app
is ad-hoc signed, which means macOS asks for Accessibility again after every
rebuild.

Milestones 0–2 of [`plan.md`](../plan.md) are built, plus QR pairing and the
trackpad mirroring that milestone 3 did not anticipate.

Everything here is checked on every push — `cargo fmt`, `cargo clippy -D
warnings`, the Rust suite, the phone page's type check and headless behaviour
checks, and a security job running the attack tests, the no-phone-home guard,
the crypto vectors and `cargo deny` ([`ci.yml`](../.github/workflows/ci.yml)).
