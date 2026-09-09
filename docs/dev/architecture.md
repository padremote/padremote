# Architecture

## The one idea

**The phone is deliberately dumb.** It reports raw touch points and decides
nothing — not what a tap is, not what a swipe is. Every interpretation happens on
the desktop.

That is what makes the product tunable without redeploying the page, testable
without a phone, and portable: a native phone app could replace the web page
tomorrow without the desktop noticing.

## The pieces

```
┌─ phone: web page (web/src/) ──────────────────────────┐
│  surface.ts   pointer capture + gesture suppression   │
│  net.ts       WebSocket, batched once per frame       │
│  device.ts    what this phone calls itself            │
│  viewport.ts  noticing when the page really resized   │
│  trail.ts     the canvas: finger trails, a diagnostic │
│  trail/hold   the long-press indicator + drag glow    │
│  haptics.ts   vibration, where it exists              │
│  sound.ts     the click every phone can make          │
│  ui.ts        status, settings sheet, whose turn it is│
│  config.ts    the settings page, generated from the config│
│  theme.css    the one visual language - see design.md │
└───────────────────────┬───────────────────────────────┘
                        │  http:// to fetch this page, then
                        │  ws:// over your own Wi-Fi - the
                        │  same port, one connection per device
                        ▼
┌─ desktop: Rust app (desktop/src/) ────────────────────┐
│  cli.rs       flags, --help                           │
│  app.rs       the always-running loops (tick, reload) │
│  auth.rs      the pairing secret and the challenge     │
│    devices.rs   which devices are paired, and revoking │
│  net/         the one port, HTTP and WebSocket:       │
│    pages.rs     the phone page, compiled in by build.rs│
│    origin.rs    who may open a socket at all          │
│    shared.rs    devices + who drives the cursor       │
│    session.rs   one phone's conversation              │
│    settings.rs  the settings page: config in and out  │
│    devices.rs   the connect page: who is paired       │
│    observe.rs   the read-only debug feed              │
│    status.rs    what the menu bar reads               │
│  gesture/     touch → intent.  PURE, OS-agnostic      │
│  sysprefs/    reads the host's real trackpad settings │
│  input/       intent → OS events.  ONLY platform code │
│    macos.rs     CGEvent: pixel scroll, click state    │
│    portable.rs  enigo: Windows and Linux (unproven)   │
│  sync.rs      locking that survives a panic           │
│  pairing.rs   the QR, the connect page, the secret    │
│  tray.rs      menu bar: a count and three items       │
└───────────────────────────────────────────────────────┘
```

One recognizer per connected device, all of them feeding one injector. That is
the whole shape of the multi-device support, and the reason it is worth stating
in a diagram: everything about a *gesture* is per phone, and everything about
the *cursor* is shared.

**The phone page is inside the desktop app.** `desktop/build.rs` compiles
`web/dist` into the binary and `net/pages.rs` serves it, so the box on the left
of that diagram is shipped by the box on the right. The alternative - a separate
page server - is what the project had, and its failure mode is instructive: two
processes with one lifetime between them means the visible one can survive a
reboot while the invisible one does not, and the symptom appears on a device
that can say nothing useful about either. A connection arriving on the port is
sorted by its first bytes: a WebSocket upgrade goes down the path above, anything
else is a browser asking for a file.

## The layering rule

Two modules are platform-specific — `input/` and `sysprefs/` — and **nothing
else may be**. In particular `gesture/` must stay pure: no I/O, no clock of its
own, no platform calls. Every decision is a function of the samples fed in and
the timestamps they carry.

That purity buys three things:

- The engine is testable from recorded touch streams, with no phone and no Mac.
- Windows and Linux reuse it untouched ([porting](porting.md)).
- Gesture bugs are reproducible: a fixture either fires or it doesn't.

If you find yourself wanting to read a preference or post an event from
`gesture/`, that is the signal to add a field to `Config` instead and let
`sysprefs` fill it in.

## Data flow, one gesture end to end

1. **Phone** captures pointer events, including coalesced ones, and queues them.
2. Once per animation frame it packs the queue into one binary frame
   ([protocol](protocol.md)) and sends it. A finger-up flushes immediately —
   a late release is a stuck button.
3. **`net/session.rs`** decodes the frame and hands the samples to that
   device's own engine, via `net/shared.rs`, which also decides whether this
   device currently holds the cursor.
4. **`gesture/`** updates its state machine and returns zero or more
   `InputAction`s — semantic things like `Move`, `Click`, `Scroll`,
   `Shortcut(MissionControl)`.
5. **`input/`** turns each action into real OS events — but only for the device
   that holds the cursor. Everyone else's actions are recognised and dropped.
6. `net/session.rs` publishes telemetry for anyone watching on `/observe`, and
   echoes the sample's timestamp so the phone can show latency.

## Configuration

`Config` (in `gesture/config.rs`) is the single source of tuning, and it is
OS-agnostic. It is assembled from two places:

```
config.default.json ──► user's config.json ──► + host trackpad settings ──► Config
   (compiled in)          (hot-reloaded)          (sysprefs, ~2×/second)
```

`followSystem: false` stops the third step. See
[trackpad mirroring](trackpad-mirroring.md).

## Sessions

**Several devices may be connected at once, and they take turns with the
cursor.** Each connection gets its own `Recognizer`, held in a `Device`
alongside its name and a "mid-gesture" flag; `Shared` owns the map of devices,
the injector, and the arbiter that decides whose actions reach the OS.

The rule is in `Shared::claim`: the cursor goes to whoever starts a gesture, and
changes hands only at the *start* of a gesture, once the previous holder has
stopped in a way that cannot be continued. How long that is comes from the
holder's own recognizer — `follow_up_ms`, the remaining double-tap or
tap-and-drag window — with `HANDOVER_GRACE` (350 ms) as the ceiling rather than
the answer. A scroll or a plain move arms nothing and hands the cursor straight
on; charging them the full grace was pure delay, and it read as the app not
having noticed you pick up the other device. Everyone else's touches are still
recognised — their state messages and telemetry keep flowing — and simply not
injected. The two conditions are not decoration: taking over mid-gesture would
inject half a gesture, and taking over instantly would steal the cursor inside
the gap in a double-tap.

The start of a gesture is also the moment `Shared::drive` calls
`Injector::sync_cursor`, and for the same reason it is the only safe moment to
change hands: nothing is in flight to disturb. The other hand that might have
moved the cursor is not another phone at all but the computer's own trackpad,
which no device on this diagram hears about — see
[porting](porting.md#the-injector-trait) for which backends have to care.

Per-device recognizers are the other half. One shared recognizer meant two
phones interleaved their finger ids into a single state machine, so a tap on the
tablet ended the phone's drag and every extra finger promoted the other device's
gesture to something it wasn't. That, plus the eviction below, is what "two
devices at once is laggy and buggy" actually was.

The desktop no longer evicts anybody. It used to hand control to the newest
connection and close the rest, which the closed page answered with a reconnect —
evicting the phone that had just taken over, once per backoff, forever. Phones
are now told who is driving with `{"t":"control",…}` and keep their connections;
`{"t":"error","code":"superseded"}` is only understood, never sent. See
[protocol](protocol.md#several-devices-one-cursor).

Observers (`/observe`) are exempt — they never claim control, which is why the
debug view can be left open on the Mac.

## Where to make a change

| You want to | Go to |
|---|---|
| Change what a gesture *means* | `gesture/mod.rs`, then a fixture or test |
| Change how an action reaches the OS | `input/macos.rs` |
| Copy another host setting | `sysprefs/` — reader *and* mapping *and* report |
| Change the wire format | `protocol/v1.schema.json`, then both `protocol.rs` and `protocol.ts` |
| Change who may connect | `auth.rs` or `net/origin.rs`, then `tests/auth.rs` — and re-read [threat model](threat-model.md) |
| Change what the phone shows | `web/src/` |
| Add a setting the phone can override | `protocol` both sides, `Overrides` in `net/shared.rs`, then `ui.ts` |
| Add something to the menu bar | `tray.rs` — but read its header first: nearly everything belongs on a page instead |
| Change the connect page | `src/assets/connect.html`, served by `pairing.rs`; its live half is `net/devices.rs` and `web/scripts/check-connect.mjs` |
| Change how devices take turns with the cursor | `net/shared.rs` (`claim`), then `tests/sessions.rs` |
| Change a command-line flag | `cli.rs` — the tests there are the documentation |
| Add a setting to the settings page | Nothing: the form is generated from the config. Add a hint in `config.ts` if the name is not enough |
| Change how anything *looks* | The utility classes on the element, in its page or in the `.ts` that builds it; `web/src/theme.css` only for a token or a control default — see [design](design.md) |
| Change the long-press animation | `web/src/trail/hold.ts` — look at it on `/preview.html`, then `npm run check:intro` |
| Change the press feedback (buzz, click) | `web/src/haptics.ts`, `web/src/sound.ts` |
