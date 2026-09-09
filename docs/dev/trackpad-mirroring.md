# Trackpad mirroring

`desktop/src/sysprefs/` — read the host's real trackpad settings and make
PadRemote behave the same way.

## Why

Hardcoded gesture mappings are wrong for almost everybody. The development Mac
had three-finger *tap* off while PadRemote middle-clicked on it, and three-finger
*drag* on while PadRemote had no such gesture. Both were bugs the user felt as
"it doesn't work like my trackpad".

## Shape

```
sysprefs::macos ──reads──▶ HostTrackpad ──apply_to──▶ Config ──┬──▶ engine
  (CFPreferences)          (plain data)            (OS-agnostic)│
                                                                │
                          phone's settings sheet ──overrides────┘
                            (per device, optional)
```

Reading and mapping are separate on purpose:

- `HostTrackpad` is plain `Option`-typed data, so the mapping is unit-testable
  without touching the real system.
- A future Windows or Linux reader only has to produce the same struct.
- `None` means "the OS didn't tell us", which must leave PadRemote's own default
  alone rather than forcing it off.

## The phone may override, but must never assume

The last arrow above is where the mirroring was quietly defeated for a long
time. The phone's settings sheet stored a *value* for every setting and sent all
of them the moment it connected - so the sheet's own defaults (`naturalScroll:
true`, `sensitivity: 1`) landed on top of whatever the Mac had really said. A
Mac with natural scrolling off scrolled the wrong way, and every line of the
reading and mapping above was correct.

Three rules keep it honest, and `tests/sessions.rs` pins each one:

- **A phone that has not been told cannot know.** It sends an override only for
  a setting the user actually changed, and fills its sheet in from the
  `settings` message the desktop sends on connect.
- **An override is remembered separately from the config** (`Overrides` in
  `net/shared.rs`), because the config is rebuilt wholesale every time the host
  is re-read - twice a second. Overrides stored in the `Config` were silently
  undone on the next poll, which reads as a setting that will not stay set.
- **Giving an override back is its own message.** `{"t":"settings","follow":
  ["naturalScroll"]}` - absence means "no opinion", which is not the same
  request, and `Option` cannot tell the two apart.

Every connected phone is re-told whenever the host or the config file changes,
so flipping a switch in System Settings updates the sheet in the user's hand.

## Reading

`CFPreferences`, not shelling out to `defaults`: this is polled about twice a
second, and `cfprefsd` — not the files on disk — is the authority.

Domains, in precedence order:

1. `com.apple.AppleMultitouchTrackpad` (built-in)
2. `com.apple.driver.AppleBluetoothMultitouch.trackpad` (Magic Trackpad)
3. `kCFPreferencesAnyApplication` for system-wide keys

Two traps live here — mixed boolean/integer types, and the global domain
constant. See [gotchas](gotchas.md).

## Mapping

Mostly one key to one field. The exception is dragging, which is **one macOS
choice spread across two flags** — and which PadRemote reads only to answer a
single question: may one finger start a drag?

| `TrackpadThreeFingerDrag` | `Dragging` | Means | `drag.tapAndDrag` |
|---|---|---|---|
| 1 | – | Three-Finger Drag; one-finger dragging **off** | `false` |
| 0 | 1 | Dragging on (with or without drag lock) | `true` |
| 0 | 0 | Dragging off | `false` |

`TrackpadThreeFingerDrag` is therefore read but never reproduced: PadRemote has
no three-finger drag, and keeps three fingers for the swipes. Its report row says
`not possible` for that reason. Getting the table wrong makes one-finger moves
turn into drags.

`DragLock` is not read at all. It only distinguishes *with* from *without* drag
lock, and PadRemote has neither — both mean the same one-finger dragging here.

## The mirror report

`HostTrackpad::report()` returns one row per setting: the System Settings name,
the underlying key, the value here, and PadRemote's status — `mirrored`,
`approximated (why)`, `not possible (why)`, or `handled by macOS`.

It exists because "does it mirror my trackpad?" was unanswerable from outside,
and that cost several rounds of guessing. It is rendered three ways: a table at
startup, rows in the settings page beside the setting each one decides, and rows
in the debug view.

**A test asserts every field appears in the report.** That is not ceremony: an
earlier summary printed 8 of 11 fields and made a real settings change look like
a no-op.

## Adding a setting

1. Add an `Option<bool>` (or `Option<f64>`) field to `HostTrackpad`.
2. Read it in `macos::read()` — one line.
3. Map it in `apply_to`, touching `Config` only when the value is `Some`.
4. Add a row to `report()` with an honest status. The completeness test will
   fail until you do.
5. If it needs a new gesture, build that in `gesture/` and add a `Config` field
   plus a `bindings` entry.

## Deliberately not mirrored

- **Tracking and scroll speed** — readable, but Apple's curve is private, so any
  mapping is a guess. `sensitivity` stays manual.
- **Force Click** — no pressure sensor on a phone.
- **Rotate** — no public way to synthesize the gesture.
- **Spring-loading** — nothing to do: macOS springs folders open by itself from
  the real drag events we send.

All four appear in the report with their reason. Silently dropping them is what
made the earlier version feel broken.

A four-finger tap is a different case again, and does not belong in that list:
macOS has no such gesture, so there is no setting to read, nothing to map and
nothing to explain. `bindings.fourFingerTap` is simply PadRemote's own, never
written by the mirror and unbound until somebody chooses something. If you add
another gesture the Mac does not have, this is the shape to copy — a binding
with a vocabulary and no `HostTrackpad` field behind it.
