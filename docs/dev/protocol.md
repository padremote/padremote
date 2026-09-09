# Protocol

Schema: [`protocol/v1.schema.json`](../../protocol/v1.schema.json) — the source
of truth. Implementations: `desktop/src/protocol.rs` and `web/src/protocol.ts`.

One WebSocket carries both streams, distinguished by frame type.

## Paths

| Path | Role |
|---|---|
| `/` | A phone. Gets its own recognizer and takes turns with the cursor |
| `/config` | A settings page. Reads and writes the config; never touches the cursor |
| `/observe` | Read-only telemetry. Never claims control |

## Binary frames — touch samples, phone → desktop

Little-endian. One frame per animation frame, batching everything captured since
the last.

```
u8  version = 1
u8  count                    number of samples, 1..=255
per sample (14 bytes):
  u32 t_ms                   performance.now(), truncated
  u8  pointerId              stable per finger, mod 256
  u8  phase                  0 down, 1 move, 2 up, 3 cancel
  f32 x, f32 y               normalized 0-1 across the surface
```

Frame length must be exactly `2 + 14 * count`; anything else is dropped rather
than killing the connection.

Coordinates are normalized so the desktop can scale by the real surface size from
`welcome` — physical deltas are what acceleration needs.

## The handshake

Every connection, on every path, starts the same way: the desktop sends
`challenge` with a fresh 128-bit nonce, and reads nothing at all until the phone
answers `auth`. A wrong answer, no answer within ten seconds, or any other
message first, and the connection is closed with `error: badAuth` — before a
device slot is taken or a touch is read.

**Which key signs the answer is the whole design**, because it is what makes
revoking one phone possible:

| `device` field | Signed with | Meaning |
|---|---|---|
| an id the desktop has paired | that device's key | the normal case |
| an id it has *not* | `P`, the QR secret | enrolment; the id joins the list |
| absent | `P` | authenticate without enrolling — the replay tool, and the desktop's own settings and diagnostics pages |

`P` is the enrolment secret, 128 bits, which the QR carries in its URL
*fragment* (`#k=`) so it never reaches a page server. A device key is
`HMAC-SHA256(P, "padremote:device:" + id)` truncated to 16 bytes — **derived on
both sides, never sent**, because the link is still plain `ws://` and a key sent
over it is a key an eavesdropper has.

The phone throws `P` away once it has enrolled and keeps only its own key. That
is what makes *Forget* final: a revoked phone still holds a device key,
so enrolment refuses to accept one, and the phone has no `P` left to enrol with
again. Being shown the QR a second time is the way back, and it is meant to be.

## Text frames — JSON control messages

Phone → desktop:

```json
{"t":"welcome","v":1,"name":"iPhone","surface":{"wpx":390,"hpx":669,"dpr":3}}
{"t":"settings","sensitivity":1.0,"naturalScroll":true}
{"t":"settings","follow":["naturalScroll"]}
{"t":"auth","hmac":"…"}
```

`auth` is the **first** message the phone sends, in reply to the `challenge`
below, and nothing else is read until it arrives. See
[threat model](threat-model.md) for what that is defending against.

Desktop → phone:

```json
{"t":"challenge","nonce":"9f2c…"}
{"t":"state","gesture":"idle|move|scroll|drag|zoom","fingers":0,
 "name":"jarvis","pressMs":500,"tapMaxPx":10}
{"t":"echo","tMs":12345}
{"t":"control","active":true,"holder":"Samsung tablet","devices":2}
{"t":"settings","sensitivity":1.0,"naturalScroll":true,
 "following":{"sensitivity":true,"naturalScroll":true}}
{"t":"error","code":"badAuth|unpaired|version|busy"}
```

`state` is sent once on connect, carrying `name`, `pressMs` and `tapMaxPx`, and
then again — gesture and finger count only — on every change of gesture.
Greeting on connect rather than after the first touch means the phone can name
the computer before anything is touched.

`pressMs` and `tapMaxPx` are the two numbers the phone's long-press ring is drawn
from. They are sent rather than assumed because the ring is a preview of the
engine's decision: it must finish exactly when the drag begins, and stop dead
exactly when the engine gives up on the hold. See
[gotchas](gotchas.md#the-press-ring-must-not-restart-itself-mid-move).

`echo` carries back the timestamp of the sample that produced the most recent
action, so the phone can display touch-to-cursor latency. **Sent at most ten
times a second, not once per batch.** Nothing but that readout reads it, and
answering every touch frame with a frame of our own doubled what the link
carried — on Wi-Fi a small frame costs nearly the airtime of a large one, so
with two phones touching at once half of everything on the channel was a number
nobody can read that fast.

`settings` runs **both ways, and they mean different things**. Desktop to phone
is *what this device is actually being driven with*, sent on connect and
whenever the computer's trackpad settings or config file change; `following`
says which of those values are the computer's own. Phone to desktop is an
**override** of one of them, and `follow` hands one back.

The asymmetry is the point. The desktop mirrors the user's real trackpad, so a
phone that has no opinion about a setting **must not send one** - a page that
sent its sheet's stored defaults on connect silently overrode the mirroring, and
a Mac with natural scrolling off scrolled the wrong way with nothing on the
desktop to blame. An override is per device, and survives the host being
re-read; see
[trackpad mirroring](trackpad-mirroring.md#the-phone-may-override-but-must-never-assume).

## Several devices, one cursor

Any number of phones and tablets (up to eight) may be connected at the same
time. Each connection gets its **own** recognizer, so they never share finger
ids, surface geometry or settings — a tablet's finger 1 and a phone's finger 1
are different fingers, and each device keeps its own sensitivity.

The computer still has one cursor, so one device drives it at a time:

- The cursor goes to whoever starts a gesture while nobody holds it.
- It changes hands **only at the start of a gesture**, and only once the holder
  has stopped in a way that cannot be continued. Idle is not the same as
  finished: the gap inside a double-tap, and the pause between a tap and the
  drag it arms, are moments with no finger down and a gesture very much in
  progress. The holder's own recognizer is asked how much of that window is
  left (`Recognizer::follow_up_ms`), capped at 350 ms. A gesture that arms
  nothing — a scroll, a plain move — releases the cursor the moment the finger
  lifts, because there is nothing left to protect.
- A device that is not driving is still read: its touches are recognised, its
  `state` messages still arrive, its telemetry still reaches the debug page.
  Nothing it does is injected.

`{"t":"control","active":…,"holder":…,"devices":…}` is sent on connect and on
every change, so a device that is waiting its turn can say *whose* turn it is
instead of looking dead. A page that is told `active:false` **must keep its
connection open and keep sending touches**: taking over is done by touching the
pad, not by reconnecting.

Both rules exist for the same reason. Mid-gesture handover would inject half a
gesture — a `ButtonUp` with no `ButtonDown`, a scroll that never ends — and an
instant handover would let the other device steal the cursor inside a
double-tap. When the holder disconnects, its buttons are released and the cursor
is free immediately.

## When the computer can move nothing at all

The same message carries the other reason a touch moves nothing, and it is not
about taking turns: `blocked` is present when **this computer will move nothing
for anybody**.

| `blocked` | What it means | The fix |
|---|---|---|
| *(absent)* | Input is really being injected. | — |
| `"permission"` | macOS has not granted Accessibility. It accepts every event the app posts and silently discards them. | Tick the box; see [`input::permission_help`]. |
| `"dryRun"` | Started with `--dry-run`: everything is recognised, nothing is injected, on purpose. | Restart without the flag. |

This field exists because the failure is otherwise **invisible from the phone**.
The socket is up, the gesture readout follows every finger, the latency figure
is live — and the cursor sits perfectly still. There is nothing on the page to
distinguish that from a working trackpad, so the user concludes the app is
broken and goes to check their Wi-Fi, when the actual fix is a checkbox on the
computer they are holding the phone next to.

The phone **must keep sending touches** while blocked. `permission` clears
without a reconnect: the desktop watches for the grant (`app::spawn_permission_watch`),
swaps the backend in under the running server, and publishes a fresh `control` —
so the page that has been apologising has to be able to stop.

`superseded` is the legacy of the previous design, in which a single shared
recognizer meant the newest connection had to evict the rest. Current desktops
never send it. `busy` is sent, and the socket closed, when eight devices are
already connected.

`tests/sessions.rs` covers each rule.

## The settings page — `/config`

A third role on the same socket, beside the phone and the observer. On connect
the desktop sends one message with everything a settings page needs:

```json
{"t":"config","computer":"jarvis","path":"…/config.json",
 "file":{…}, "effective":{…}, "followSystem":true,
 "host":[{"setting":"Scrolling direction: Natural","value":"on",
          "status":"mirrored","detail":""}, …],
 "decidedBy":{"scroll.natural":"Scrolling direction: Natural", …},
 "vocabulary":{"bindings.oneTap":["none","leftClick", …], …}}
```

Three of those fields exist so the page can *explain itself* rather than show a
control that quietly does nothing:

- `file` is what the user has chosen; `effective` is what the engine is running,
  which differs wherever the host's trackpad settings were mirrored on top.
- `decidedBy` maps a config field to the host setting that overrides it, so a
  locked control can name its reason. It is sent rather than known by the page
  because a copy in the page drifted the day a setting was renamed.
- `vocabulary` is the values each binding accepts, so a menu can never offer an
  action the engine would silently ignore.

The page sends back `{"t":"setConfig","config":{…}}` — the whole config, because
the file is the serialised form of one struct and a partial merge is how two
writers corrupt one — or `{"t":"reset"}`. A successful write is not answered
directly: it comes back as a fresh `config` message to *every* open page, and
to the phones as a `settings` message. Only a failure gets a direct
`{"t":"error","detail":"…"}`.

Writing goes through the file on disk, deliberately: the app already watches it,
and anything that changed only memory would be undone by the next reload.

## Telemetry — desktop → observer

Not in the schema; a debugging channel, free to change.

On connect, a `hello` with the computer name and the full mirror report. Then one
`batch` per touch frame:

```json
{"t":"batch","device":"iPhone","deviceId":1,"held":true,
 "samples":2,"gapMs":8.4,"gesture":"move","fingers":1,
 "peakFingers":1,"actions":["move"],"movePx":6.4,
 "points":[[0,1,0.31,0.42]]}
```

`gapMs` is null between gestures — only gaps *within* a continuous touch mean
anything, and including idle time made the jitter figure meaningless.

## Changing the protocol

Update the schema first, then both implementations. `frame_round_trip_and_rejection`
in `tests/fixtures.rs` covers encode/decode and malformed input.

Bump `version` for anything incompatible: the desktop already rejects a mismatched
`welcome` with `{"t":"error","code":"version"}`.
