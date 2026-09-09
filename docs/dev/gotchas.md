# Gotchas

Traps that have already cost real debugging time. Each has a symptom, because
the symptom is how you'll meet it again.

## macOS input injection

### The injector's cursor position goes stale, and the cursor jumps

*Symptom: use the Mac's own trackpad, then touch the phone — the cursor
teleports back to wherever PadRemote last left it before moving on.*

A `CGEvent` mouse event carries an **absolute point**, not a delta, so
`MacInjector` has to keep its own idea of where the cursor is; a relative move is
that point plus the delta. Nothing tells it when a hand lands on the real
trackpad, so between gestures that idea quietly goes wrong, and the next move
posts an event at the *old* position plus the delta.

`sync_from_system()` was written for this — its comment even says "in case the
user touched the real trackpad" — and then was called from nowhere at all, which
is a failure mode worth naming on its own: the fix reads as present in the source
and is dead. `Shared::drive` now calls it through `Injector::sync_cursor` when a
batch opens a gesture, which is the only safe moment (nothing is in flight) and
the only necessary one. `sessions.rs` asserts the sync arrives *before* the first
move, so the wiring cannot rot back out.

The same call re-reads `screen_bounds()`, which had the same shape of staleness:
measured once at launch, so a display plugged in later left `clamp()` pulling the
cursor back inside a rectangle that no longer described the desk.

### Modifiers must be real key events, not flags

*Symptom: Mission Control and space switching do nothing; the arrow key reaches
the focused app as a bare arrow.*

macOS tracks modifier state from Control/Command **key-down and key-up events**.
A key event that merely carries `CGEventFlagControl` arrives without the
modifier. `MacInjector::press()` presses the modifier keys around the target key
for exactly this reason.

### Arrow keys need `NumericPad`

*Symptom: as above, and setting the modifier correctly still isn't enough.*

macOS classes arrows as numeric-pad keys. Synthetic `Ctrl+Arrow` without
`CGEventFlagNumericPad` (and `SecondaryFn`) is ignored by the WindowServer.

### Volume and brightness are media keys, not keystrokes

*Symptom: the volume changes but no panel appears on screen, so the gesture
reads as having done nothing.*

`osascript -e "set volume output volume ..."` moves the slider through
CoreAudio. It changes the volume and draws nothing. The panel is drawn by macOS
in response to the *key*, so the key is what has to be sent: an
`NSEventTypeSystemDefined` event, subtype 8, with the button number packed into
`data1`. `CGEvent` cannot build one - only AppKit can attach a subtype - which
is why `objc2` is a dependency for a single message. See `media_key` in
`input/macos.rs`.

It was also a process launch, waited on, from the thread working through
touches: about a tenth of a second per step of a swipe.

### A scratch binary can post mouse events but not media keys

*Symptom: `cargo run --example …` moves the cursor, so permission looks fine,
but the same code posts a volume key and nothing happens. The installed app
does it perfectly.*

`AXIsProcessTrusted()` returns true for anything launched from a Terminal that
has Accessibility, and mouse events from such a binary really do land - so every
signal says the process is allowed. System-defined media events are filtered
more tightly, and an ad-hoc scratch binary is not the app the grant was given
to. **Test media keys through the installed app**, not through `cargo run`; the
quickest way is a temporary flag on the real binary, installed and then run
directly out of `~/Applications/PadRemote.app/Contents/MacOS/`.

### Brightness measured in `ioreg` looks stuck when it is not

*Symptom: brightness keys appear to do nothing, because
`ioreg -c AppleARMBacklight` reports the same `rawBrightness` before and after.*

That value lags and is on a different curve from the one the keys move. Read
`DisplayServicesGetBrightness` instead - and read it for the **built-in**
display, not `CGMainDisplayID()`, which is the external monitor whenever one is
plugged in. An external display usually reports `canChange=false` and a
brightness of `0.0`, which looks exactly like a broken key.

### Scroll needs gesture phases

*Symptom: scrolling feels like a notched mouse wheel — no smoothing, no
rubber-banding.*

macOS grants smooth scrolling only to wheel events carrying a scroll phase.
Events must be flagged continuous and carry `begin → continue → end`, plus
momentum phases while coasting. The field numbers (88, 99, 123) aren't in the
`core-graphics` crate and are declared in `input/macos.rs`.

### `CGEventSource::new()` is not an Accessibility check

*Symptom: the app starts happily and nothing moves.*

It succeeds for any process. Untrusted processes can *create* events; macOS
discards them at post time. Use `AXIsProcessTrusted()`. This one was especially
costly because it made a permission problem look like a gesture problem.

The grant is **per binary**: `PadRemote.app` and your terminal are different
subjects. Say which one needs enabling.

### An ad-hoc signature has no identity across rebuilds

*Symptom: the app runs fine from `cargo run`, but launching it from Spotlight
does nothing at all — no window, no menu-bar icon, no error. The Accessibility
list still shows PadRemote, ticked.*

TCC remembers a grant against the bundle's **designated requirement**. Ad-hoc
signing produces `cdhash H"…"` — the hash of that exact build — so every
`make-app.sh` run is a different subject to TCC. The old row survives, still
looking enabled in System Settings, while `AXIsProcessTrusted()` returns false
for the new binary.

It hides for a while because running from a terminal works: the *responsible
process* is then Terminal, which has its own grant. Only a Finder or Spotlight
launch makes the app answer for itself.

Compare the two to confirm it:

```sh
codesign -d -r- ~/Applications/PadRemote.app          # current requirement
sqlite3 "/Library/Application Support/com.apple.TCC/TCC.db" \
  "select hex(csreq) from access
     where service='kTCCServiceAccessibility' and client='com.padremote.desktop'"
```

Different cdhashes mean a stale grant. `make-app.sh` now notices and runs
`tccutil reset Accessibility com.padremote.desktop`, so the next launch asks
again. To make a grant stick across rebuilds, sign with a real identity instead
— its requirement is identifier-plus-certificate, which does not move:

```sh
CODESIGN_IDENTITY="My Self-Signed Cert" ./desktop/packaging/make-app.sh
```

### A missing permission must not exit a menu-bar app

*Symptom: as above — Spotlight launch does nothing.*

`main` used to `?` its way out when Accessibility was missing. In a terminal
that prints a helpful paragraph; under `LSUIElement` there is no terminal, no
window and no Dock icon, so the whole experience is an app that will not start.

It now starts with `NullInjector`, brings up the tray and the server anyway, and
polls `AXIsProcessTrusted()` until the grant lands, then swaps the real backend
into `Shared::injector`. The tray says *Needs Accessibility permission* until it
does. Anything a menu-bar app cannot do is a thing it must **display**, not a
reason to quit.

### …and the phone has to be told the same thing

*Symptom: the phone connects, the trail draws, the latency readout is live, and
the cursor never moves.*

The half of the above that was missing for a while. Displaying it on the tray
answers the person sitting at the computer; the person holding the phone is
looking at a page that says everything is fine. Every signal it has — the status
dot, the gesture name, the latency figure — is driven by the socket, and the
socket is genuinely healthy. A frozen cursor is indistinguishable from a working
one until you look at the screen you are not looking at.

So `NullInjector` carries a `Blocked` reason, `ControlView` carries it to every
session, and `{"t":"control","blocked":"permission"}` puts it on the phone. Same
rule as the tray, one layer further out: **a failure the user cannot see is a
failure they will diagnose as something else**, and here they diagnose it as
their Wi-Fi.

The same field covers `--dry-run`, which produces the identical symptom, and is
worth remembering when two copies are running: a phone whose stored address
points at a `--dry-run` copy on another port looks exactly like a healthy
connection that does nothing.

### Pixel scroll is behind a feature flag

`core-graphics` only exposes `new_scroll_event` with `features = ["highsierra"]`.
Without it there is no pixel-unit scroll constructor at all.

### `pyobjc`'s scroll signature (prototype only)

`CGEventCreateScrollWheelEvent2` wants exactly `5 + wheelCount` arguments:
`(source, units, wheelCount, wheel1..wheelN, pad, pad)`.

## The gesture engine

### A multi-finger tap timed per finger fails as the count goes up

*Symptom: a four-finger tap does nothing, most of the time but not always, and
the debug view shows peak fingers 4.*

`quick` used to ask whether *this* finger had been down less than `tapMaxMs`
(200 ms), measured from its own landing. Fingers do not land together on a
phone: the last of four is routinely 150 ms behind the first, so the first
finger's budget was mostly spent waiting for its neighbours, and an ordinary tap
with any dwell at all blew it. Nothing fires — the tap is declined, and the
swipe and pinch paths decline it too because it never travelled.

Now it is measured from whichever came later, this finger's landing or the last
finger's, so the clock starts when the hand is whole. One finger is unaffected
(the two values are the same), and the movement test is still per-finger, so a
real hold is still not a tap. It was invisible for the two- and three-finger
taps because their landing spread is small enough to fit in what was left.

### A finger landing mid-move used to be thrown away

*Symptom: slide one finger, then lay a second one down to scroll — nothing
happens, and the cursor carries on following the first finger.*

"Once committed, extra fingers are ignored" is the right rule for `SCROLL`,
`ZOOM` and `DRAG`, and the wrong one for `MOVING`. One finger leaves `PENDING`
for `MOVING` as soon as it drifts `tapMaxPx`, so the commit had already happened
by the time anyone thought to add a second finger — and a MacBook reclassifies
here rather than making you lift the whole hand and start again.

The escape hatch that already existed for a late finger does not help: it is
guarded on every pointer still being within `tapMaxPx` of where it landed, which
is false by definition once the gesture *is* a cursor move.

`Recognizer::regroup` re-opens it, and the re-anchoring is the whole job — see
[gesture engine](gesture-engine.md) for why a promotion that skips it fires
back/forward instead of scrolling, and why it has to clear `tap_ok`.

### Judge on peak fingers, not current

*Symptom: four-finger gestures do nothing; the debug view shows 3 fingers.*

See [gesture engine](gesture-engine.md). Taps already worked this way; swipes
didn't, and that inconsistency was the bug.

### A pinch is opposing motion, not a distance change

*Symptom: horizontal two-finger scrolling zooms instead.*

Fingers report one at a time, so spacing changes transiently during any
two-finger gesture.

### A multi-finger pinch must beat the hand's travel

*Symptom: vertical four-finger swipes randomly open Launchpad.*

An ordinary swipe up curls and splays the fingers past the pinch threshold.

### The dragging flags are one choice, not two

*Symptom: one finger drags a selection instead of moving the cursor.*

`TrackpadThreeFingerDrag` and `Dragging` describe a single macOS setting.
`dragging` alone is wrong, and so is `three || dragging`: the condition is
`dragging && !three`, because with three-finger drag selected the Mac has
**no one-finger dragging at all**, and any move that follows a tap would
otherwise become a drag. PadRemote has no three-finger drag of its own, so that
flag exists purely to switch `drag.tapAndDrag` off.

### Deltas are consume-once

`Pointer::take()`. Reading a previous position instead double-counts a stationary
finger's last movement on every sample of the other finger.

## Configuration

### `#[serde(default)]` on a struct calls `Default::default()`

*Symptom: stack overflow, only under test.*

So `Config::default()` must not parse JSON that itself uses the attribute — it
recurses forever. The defaults are written out in Rust, and
`defaults_match_shipped_file` guards them against drifting from
`config.default.json`.

### macOS preferences are inconsistently typed

`Clicking` and `TrackpadRightClick` are **booleans**; `TrackpadPinch` and the
swipe switches are **integers** (where 0 is off and both 1 and 2 mean on).
Reading only one type makes settings look "not configured".

The global domain needs the real `kCFPreferencesAnyApplication` constant — a
`CFString` of that name is just an app id that doesn't exist.

## Networking

### A stale socket's `onclose` must not touch the live one

*Symptom: the phone's status flickers between connected and offline several times
a second; the server logs dozens of connections a minute.*

If `onclose` clears the current socket without checking it is still current, one
eviction spawns two sockets, which evict each other forever. Guard with
`if (this.ws !== ws) return;`, and refuse to open a second socket while one is
connecting or open.

### Vite HMR leaves the old module's socket alive

Hot reload re-runs the module without unloading the old one. `import.meta.hot
?.dispose()` must stop the old link, or two fight for control during
development.

### Opening the phone page on the Mac joins as another device

It no longer steals the cursor — it queues for it like any other device — but it
does take a turn the moment you touch it. Use `/debug.html`, which attaches as
an observer and never claims control.

### One recognizer cannot serve two devices

*Symptom: with a phone and a tablet connected, the live one's gestures break in
ways the recognizer tests never reproduce — a scroll that thinks it has three
fingers, a drag that never lifts, a tap on one device ending a drag on the
other.*

Pointer ids are only unique **within** a device: both send a finger 1. A single
shared `Recognizer` therefore merged two hands into one state machine, where an
extra finger promotes a gesture to something else entirely and an `UP` from the
wrong device ends the other's drag. Every connection gets its own recognizer
(`Device` in `net/shared.rs`); nothing about gesture state is global.

The same applies to anything derived from a device: surface geometry and the
live settings from the settings sheet are per device too. A tablet and a phone
are different sizes, and one `set_surface` for both silently rescaled every
delta.

### Eviction was the "two devices are laggy" bug

*Symptom (historical): with two phones open, both flicker between connected and
offline about once a second and neither trackpad works. The desktop log shows an
endless alternation of "taking over from the previous session".*

Two mechanisms, each correct alone and lethal together: the desktop handed
control to the newest connection, and the page reconnects on its own after any
close. The evicted page reconnected, evicted the phone that took over, whose
close triggered *its* reconnect — and because the backoff resets on every
successful open, it never damped out. Every takeover also called `release_all()`,
so both phones kept having their gesture cut out from under them.

The first fix was to *announce* the eviction (`{"t":"error","code":"superseded"}`)
so the loser would stand down instead of retrying. That stopped the tug of war
but left the real request unanswered: people want to use both devices. Now
nobody is evicted at all — everyone stays connected and the cursor is handed
around. The page still understands `superseded` for older desktops, and a bare
close still means "retry", which is the right reading of a dropped Wi-Fi link.

### Work done "in case someone is watching" is work done sixty times a second

*Symptom: two connected phones both feel worse than one, with nothing in the
profile that looks like a bottleneck.*

The session used to build the `/observe` telemetry line — a nested JSON document
with an array per touch sample, and a `dev.label()` clone taken under a mutex —
on every batch from every device, then hand it to a broadcast channel that
dropped it because the debug page was closed. `Shared::publish` really is free
when nobody is listening; *constructing its argument* is not, and that is the
half that ran. `Shared::observed()` is now asked first.

The general shape is worth remembering, because the comment on `publish` said
"silently does nothing when nobody is watching" and was true about the wrong
thing. A guard on the cheap end of an expensive expression guards nothing.

### The heartbeat must not share a task with anything that blocks

*Symptom: a scroll coasting to a stop stutters about twice a second, and a
little worse every five.*

`spawn_maintenance` ran three jobs on one clock: `Shared::tick` every 16 ms,
which is what integrates momentum scroll, plus the config re-read every 30th
tick and the LAN address check every 300th. Those last two are not cheap and
not ours — `HostTrackpad::read` makes about twenty-five round trips to
`cfprefsd`, `Config::load` touches the disk, and `local_ip` spawns `ipconfig`.
Sequential on one task, each of them pushed the next tick late, and a tick that
arrives late is a visibly stepped scroll.

They are two tasks now, and the slow one puts its syscalls on `spawn_blocking`.
The rule that came out of it: **anything on the 16 ms clock must be pure
arithmetic over state we already hold.** If it reads a file, a preference or a
process, it belongs on the other task.

The same reasoning retired three blocking `.status()` calls that ran on runtime
worker threads — the pairing notification's `osascript`, and the `open -a` for
the Calculator and Launchpad shortcuts. Waiting for an app to launch, inline in
the injection path, stalls the thread reading that phone's touches. They
`spawn` and are reaped on a thread of their own.

### Handover must happen at a gesture boundary, and not instantly

*Symptom: the cursor sticks down after the other device takes over; or a
double-tap on one device turns into two single taps because the other device
grabbed the cursor in the gap.*

A device's actions are only injected while it holds the cursor, so a takeover
part-way through a gesture injects the *second half* of one — a `ButtonUp` with
no `ButtonDown`, a scroll that never ends. `Shared::claim` therefore only grants
the cursor to a recognizer that was idle before the batch and has a `DOWN` in
it.

The grace period is the other half. "Idle" happens in the middle of ordinary
gestures too: between the halves of a double-tap, and between the tap and the
drag it arms. Letting another device in there breaks sequences every trackpad
supports, so the holder keeps its claim after it goes quiet. Momentum
deliberately does *not* count as busy — a flick can coast for seconds — but a
takeover resets the loser's recognizer so the coast cannot resume later.

**How long that claim lasts is asked, not assumed.** It was a flat 350 ms after
any gesture, which is right for a tap and pure delay for everything else: a
scroll or a plain move cannot be the first half of a double-tap, so there is
nothing to protect and nothing to wait for. Charging them the grace anyway is
what made picking up the other device feel like the app had not noticed.
`Recognizer::follow_up_ms` answers it exactly — the three timestamps it needs
were already being kept for the gestures themselves — and `HANDOVER_GRACE` is
now the ceiling on that answer rather than the answer itself.

It reads the phone's clock, not the desktop's, and that distinction is
load-bearing: `feed` carries `performance.now()` from the device while `tick`
carries the desktop's own clock, so `last_fed_t` is written by the first and
never by the second. A field that mixed them would read as a wild jump every
time a phone reconnected — which is also why a marker landing in the *future*
counts as expired rather than as a full window.

### A poisoned mutex takes the app deaf, permanently

*Symptom: after one unrelated panic the cursor never moves again, while the menu
bar still says a phone is connected. Restarting fixes it; nothing else does.*

`Mutex::lock().unwrap()` panics for every later caller once any thread has
panicked while holding the lock. The state behind these locks - a recognizer, a
device list, an injector - is rebuilt by the next touch or the next connection,
so refusing to work at all is strictly worse than carrying on. `sync.rs` takes a
poisoned lock anyway and logs it once; use `.locked()`, not `.lock().unwrap()`.

### The phone's own defaults can defeat the mirroring

*Symptom: scrolling goes the opposite way to the user's trackpad. The desktop's
mirror report says `Scrolling direction: Natural ... mirrored`, and it is right -
the value it read is correct and the value the engine uses is not.*

The phone stored a value for every setting in its sheet and sent all of them on
connect, so `naturalScroll: true` - a *default*, not a choice - was written over
the mirrored value a few milliseconds after the link came up. Nothing in the
mirroring was wrong, which is exactly why it survived so long: every test of
`HostTrackpad` passed, and the debug page showed the right number.

A client may override what the computer says. It may not *assume* it. The page
now stores `null` for anything it has not been told to change, sends nothing on
connect unless the user has actually overridden something, and fills the sheet
in from the desktop's `settings` message. See
[trackpad mirroring](trackpad-mirroring.md#the-phone-may-override-but-must-never-assume).

### An overridden setting must outlive a config reload

*Symptom: a setting changed on the phone snaps back on its own within a second.*

The host is re-read about twice a second and the whole `Config` rebuilt from it,
which used to overwrite each device's recognizer wholesale. Anything the user had
chosen on the phone lived in that same struct and went with it. Overrides are
kept beside the recognizer now and re-applied on top of every rebuild.

### A JSON object is a *sorted* map by default

*Symptom: the settings page lists every setting alphabetically - `accel`,
`bindings`, `drag`, `scroll` - however the config struct is declared.*

`serde_json::json!` puts a serialised struct into a `Map`, and without the
`preserve_order` feature that map is a `BTreeMap`. Field order is lost the
moment a struct becomes a `Value`, so a page that builds its form from the JSON
shows the fields in whatever order the alphabet happens to give. The feature is
on in `Cargo.toml`; the order someone thought about the settings in is worth
keeping all the way to the browser.

### The same table written twice drifts within a day

*Symptom: a control on the settings page that should be locked - because the
host decides it - is editable, silently, and changing it does nothing.*

The page needs to know which config field each mirrored setting decides. It had
its own copy of that table. A setting renamed on the Rust side ("Secondary click
in a corner" → "Secondary click (corner)") simply stopped matching, and the
control stopped explaining itself with no error anywhere.

The table now lives in `HostTrackpad::controls()`, beside `apply_to` where the
relationship actually is, and is *sent* to the page. A test asserts every name
in it is a row the report really produces - a name that matches nothing is a lie
the user cannot see.

### An observer is not a phone

*Symptom: the tray says a phone is connected whenever `/debug.html` is open.*

`serve` used to set the link status on every accepted socket, before the
handshake had even decided whether the connection was a controller or a watcher.
Only the phone path may touch it.

### A `file://` page has no origin, so it cannot talk to the app

*Symptom: the connect page can show a QR but not a live device list, and every
attempt to open a socket from it is refused during the handshake.*

The page used to be written into `/tmp` and opened from there. A `file://` page
sends `Origin: null` - not "no origin", which is what a native client sends and
what `net::origin` lets through, but the opaque origin a sandboxed frame gets for
free. It is refused, correctly. Working around it cost an iframe served from
loopback and a `postMessage` relay that could only carry a number, and left a
copy of the pairing secret in a world-listable directory for as long as the file
existed.

The page is served over loopback by the app instead (`net::pages`, only to this
computer). It is then same-origin with the server it talks to, needs no relay,
is never written to disk, and gets `crypto.subtle` for the challenge - which
`http://localhost` counts as a secure context for, and a LAN address does not.

### A menu is the wrong shape for a list

*Symptom: "which of these three is the iPad?" is a question a submenu cannot
answer.*

Forgetting devices lived in the tray. A menu has to be held open, so a list that
changes redraws underneath the pointer; it cannot show what is offline, so the
device somebody actually wants to revoke - the phone that is not here - was
missing entirely; and revoking is a decision people want to read before they
make. It is all on the connect page now, and the tray is a count and three
items. New state belongs on a page unless there is nowhere else it can go.

## The interface

The pages are built out of Tailwind utility classes - see
[design](design.md#implementation) for why, and for what is left in
`theme.css`. These are the traps that came with that.

### A class name that is not written out in full generates nothing

*Symptom: one element on an otherwise correct page sits at browser defaults -
unpadded, square, the wrong grey - and nothing anywhere reports a problem. It
typechecks, `npm run check` passes, the build is clean.*

Tailwind does not parse the markup. It scans the files named by `@source` in
`theme.css` - `web/*.html` and `web/src/**/*.ts` - for anything that *looks
like* a class name, and generates a rule for each one it finds. A name that is
only ever assembled at runtime is never in that text:

```ts
// `tag` is in the text, so it is generated. Whatever `tone` evaluates to is
// not, so if it were a utility rather than a component class it would be a
// word in an attribute and nothing else.
el.className = `tag ${status.tone}`;
```

There is no error for this, and there cannot be: the scanner has no way to know
the difference between a name it should have found and a word it should ignore.
The browser is the only place it shows, which is half of why every UI change is
looked at in one.

The way out is not a lookup table of complete class names. There are two, and
which one you want depends on whether the varying part is a *state* or a
*place*:

- **A state is a component class in `theme.css` with one whole word appended** -
  `dot connected`, `tag mirrored`, `tag not-possible`. `ui.ts` and `config.ts`
  splice the desktop's own vocabulary into those strings, so the half that
  varies is the protocol's word rather than a utility, and the rules for it are
  written once where they can be read.
- **A place is an arbitrary descendant variant on the container**, which keeps
  the rule in the markup where the rest of the page's rules are. The mirroring
  table does this: `config.ts` emits bare `<td class="val">` and `<div
  class="why">`, and `config.html` dresses them from the table element -
  `[&_.val]:text-fg [&_.why]:text-faint`. Nothing is generated at runtime, so
  everything is scannable, and a reader of the table's markup can see what its
  rows will look like without opening the TypeScript.

Both beat a `class` attribute assembled from a variable, which is the only
thing that cannot work.

### The connect page is not built, scanned, or themed

*Symptom: a utility class added to the connect page does nothing; or a colour
changed in `theme.css` is picked up everywhere except the pairing screen, which
quietly drifts a shade off from the rest of the product.*

`desktop/src/assets/connect.html` is `include_str!`d by `pairing.rs`, because
only the app can draw the QR and hand the page its key. That puts it outside
both build systems at once: Vite never sees it, and it is not in `@source`, so
Tailwind generates nothing for it. It carries its own inline `<style>` with its
own copy of the palette, and that copy is kept in step by hand.

So a change to the shared theme is two edits, and the second one is easy to
forget because nothing fails without it. Its behaviour - the device list, the
two-tap Forget all, the stale-address reload - is covered by
`web/scripts/check-connect.mjs`, which pulls the real inline script out of the
real file and runs it; its *appearance* is only ever covered by looking.

### Two of the stylesheets are not styling

*Symptom: `@apply` in `config.css` does nothing, or a fifth stylesheet appears
because "the theme file is for tokens".*

`theme.css` is the only Tailwind entry point on the project. `config.css` and
`style.css` are plain CSS imported alongside it, and each exists for a reason
that is not "styling that did not fit":

- `config.css` is an animation engine. `offset-path`, `stroke-dashoffset`,
  `transform-box`, dash arrays in `pathLength` units, twenty drawings running
  phase-shifted on markup `gesturepad.ts` generates. Tailwind has no utilities
  for most of that and it would not be clearer if it did.
- `style.css` holds the trackpad frame's height, because it is a *pair* of
  declarations rather than a value: `height: var(--app-h, 100vh)` and then
  `height: min(100svh, ...)`, the first being the fallback for a browser that
  does not know `svh`. A class can carry either line; it cannot carry one and
  then the other, which is the whole point of writing it twice.

Anything that is not one of those two shapes is a utility class on the element,
or a component class in `theme.css` if TypeScript writes the name.

## The web page

### A phone browser has not settled on a size when your code runs

*Symptom: on the **first** load of the pad, everything is drawn shifted up the
screen — the trails land in the wrong place, the edge frame is off. Reloading
fixes it, permanently, until the page is opened fresh again.*

Safari opens with the address bar expanded and collapses it a moment later;
Chrome does the same. The desktop's own log makes it plain — on one load the
surface was announced eleven times as the page settled:

```
Mac: surface 1440x778 @2x     <- measured here, in the constructor
Mac: surface 1440x770 @2x
...
Mac: surface 1440x722 @2x     <- what it actually is
```

Anything measured once in a constructor is measured against the first of those.
The canvas backing store was then 56 px taller than the box it is drawn into,
so every coordinate was scaled and shifted. A reload "fixes" it only because the
second load starts with the browser chrome already where it ends up.

Three things are needed, and no two of them are sufficient:

- **`ResizeObserver` on the element**, not `window.resize`. iOS fires the window
  event inconsistently for the address-bar transition, because that transition
  changes the *visual* viewport without necessarily relaying out the page.
- **`visualViewport` events**, for the inverse case: chrome sliding over a page
  whose layout has not changed at all, so the element's box never moves.
- **The visible rectangle has an origin, not just a size.** This is the one that
  took three attempts, and the first two failed for the same reason: they fixed
  the height and left `inset: 0` alone.

  The phone page uses the browser's default viewport fit and keeps its app
  frame in normal document flow. Do not position that frame using visual
  viewport offsets: on the affected iPhone Chrome session, both fixed and
  document-positioned frames left content behind the browser bars.

  The normal frame is capped at `100svh` (browser bars expanded), with the
  visual viewport height allowed to shrink it for a keyboard but never enlarge
  it beyond that cap. Immersive mode uses `100dvh` so it can use the extra space
  when bars collapse. All controls, artwork and the surface share this frame.
  The standalone preview still uses its separate fixed-overlay coordinates.

  Watch document scrolling as well as visual viewport events. Repeat the
  measurement briefly after geometry events and keyboard dismissal, not only
  on startup: a transition can publish its final size after its last event.
  The mobile regression checks cover delayed settling; browser chrome
  positioning still needs verification on a physical iPhone.

  A QR-code first navigation is a *fifth* thing, and it is not a measurement
  bug at all. Symptom: the pad is shifted on the first load after a scan, and a
  reload is correct. Chrome on iPhone can retain the native viewport it held
  before the Camera app handed the link over, so the document is laid out
  against a size that is already wrong when the first line of script runs.
  Nothing measured *inside* that document can correct it - which is exactly why
  the reload worked and the in-page fixes did not.

  **Do not attempt this in the page again.** Each of these was tried and
  failed on the affected device, and re-deriving them costs a physical-phone
  session every time:

  - Resizing or re-measuring the existing page - the whole `ResizeObserver` /
    `visualViewport` apparatus above is correct and does not touch this.
  - Resetting document scroll. A `keepPadAtTop()` helper (disabling history
    scroll restoration, resetting scroll at startup, load, pageshow and
    foregrounding) was written for this and removed again: it fixed no QR
    launch. Keeping `html` and `body` content-sized is still right on its own
    merits - a `height: 100%` document leaves room for a scroll offset - but it
    is not this bug.
  - Blaming the scanner. Compare full URLs first: QR links carry `#h=...`, so a
    scan and a typed address are not the same navigation.

  What works is a second document. `launchPad()` in `web/src/launch.ts` holds
  the pad back on a scanned first load, waits for the browser bars to go quiet,
  and then `location.replace()`s the same URL with `?__pad_launch=1` added -
  a real navigation, which Chrome lays out against the true viewport. `boot.ts`
  is the entry point and only imports `main.ts` once that has been decided, so
  no socket, surface or canvas is ever built on the throwaway landing page.

  The guards are the load-bearing part. It engages only for iPhone Chrome
  (`CriOS`) with an `#h=` fragment on a `navigate` - not a reload, not
  back/forward, not any other browser. The marker lives in the query string
  rather than storage, so the second pass cannot loop when `localStorage` and
  `sessionStorage` both throw. A 700 ms quiet timer settles the bars and a 3 s
  deadline caps it, because a bar animation can emit `resize` indefinitely.
  `npm run check:launch` drives the real bootstrap against a fake clock and
  covers all of that.

  `resolveLink()` also stopped calling `history.replaceState()` to strip the
  fragment during initial load - a URL mutation that happened only on QR
  navigations - which additionally preserves pairing on reload when storage is
  blocked.

  Simulated tests cannot reproduce native Chrome UI. Whether the shift is gone
  is a physical-iPhone check, and the settings sheet's `visible` / `layout`
  line is the reading that settles it.

There is a fourth thing, which is not a mechanism but a habit: **put the build
stamp on screen**. Half the time a fix "does not work" on a phone, the phone is
holding a page from before it - and nothing on the screen said so. The pad's
settings sheet now shows the build and the surface size it believes it has,
which turns an argument into a reading.

And then **guard on the size actually having changed**. The watcher fires for
anything that *might* have moved, including a visual-viewport scroll, which on a
phone can be continuous - without the guard that is a glare bitmap rebuilt every
frame and a `welcome` message on the socket per scroll event.

### Motion sensors need a secure context, and the LAN page is not one

*Symptom: shake detection does nothing on the phone, and `DeviceMotionEvent` is
not merely unpermitted but `undefined`.*

Every browser gates `DeviceMotionEvent` and `DeviceOrientationEvent` behind a
secure context. PadRemote serves the phone page over plain `http` on the local
network until the TLS work in milestone 3, so on a phone there is nothing to
ask permission *for*. `shakeSupport()` reports `insecure` separately from
`unavailable` for exactly this reason: telling someone to allow motion access
would send them hunting through iOS Settings for a switch that changes nothing.

The feature starts working on its own the day the page is served over `https`.

### iPhone Safari has no Fullscreen API

*Symptom: `requestFullscreen` is undefined on iPhone, present on iPad.*

Apple has never shipped the Fullscreen API for elements on iPhone - only
`<video>` has `webkitEnterFullscreen`. A web page cannot take the screen there,
and no amount of feature detection changes the answer.

What is left is worth doing anyway, and `immersive.ts` does all three: hide
everything the page itself draws, scroll the document by one pixel (a document
that has been scrolled is one Safari collapses its bars for), and say - once,
in words - that **Add to Home Screen** is the only route to a real full screen,
because the manifest asks for `display: standalone`.

### A `<canvas>` is a replaced element

*Symptom: the finger trails don't appear.*

`position: fixed; inset: 0` does **not** size it — it keeps its intrinsic
300×150 and sits in a corner. State `width`/`height` explicitly.

### A settings page must never invent a value

*Symptom: a setting changed on the settings page snaps back; or the page shows
a control at a value the engine is not using.*

Two rules, both learned the hard way and both now enforced by tests:

- **The page shows what the desktop sends**, never what it stored. The phone's
  own sheet broke the mirroring for weeks by sending its stored defaults on
  connect (see "The phone's own defaults can defeat the mirroring").
- **A write goes to the file, not to memory.** The config is re-read from disk
  about twice a second; anything applied only in memory is undone by the next
  poll. `Shared::write_config` writes the file *and* applies the result, so the
  page feels immediate without lying about where the value now lives.

### Two animations meeting is where the glitch is

*Symptom: the long press "flickers" or "doesn't match" at the exact moment the
drag starts, and nobody can say which frame is wrong.*

The charge-up and the held-drag glow are drawn by different code paths, so
nothing stops them disagreeing at the seam - and everything about that instant
is deliberate, so any disagreement reads as a bug. The four that were there:
the ring cut from blue to green, its stroke jumped from 7 px to 3.5 px, the
screen border blinked out, and a green flash appeared in front of a white glare
that had not existed a frame earlier.

The rule is that the confirmation must *grow out of* the charge: same radius,
same stroke width, same colour on the arming frame, then travelling to the armed
look over one intro. `npm run check:intro` drives the real renderer and asserts
exactly that, frame by frame.

Two things it also pins, both found by *looking* at the animation stopped on one
frame (`preview.html`, then `padremotePreview.at(545)`) rather than by reading
the code:

- **No rounded corner, anywhere.** The screen frame used to be a rounded
  rectangle, whose radius was a guess at the phone's own display radius - wrong
  on most phones - and which read as chrome the page had always had rather than
  as a state. It is four chamfered brackets now, and the check counts `arcTo`
  and `roundRect` calls to keep it that way.
- **No flat wash over the screen.** A rectangle of colour laid over everything
  is unmissable, which was its job, but it is also the one moment in the gesture
  with no structure in it - and it costs a fill of every pixel on the display on
  exactly the frames where cursor movement matters most. A line crossing the
  screen covers the same ground for a fraction of the cost.

### Stroking segment-by-segment looks like beads

Each segment gets its own round cap. For a tapered trail, build one filled
polygon down each side of the path.

### The feedback loop

*Symptom: the renderer freezes solid.*

If the page is open on the machine it controls, the cursor it moves passes over
the page, the browser reports mouse input, and the page feeds it back. Mouse
pointers are ignored by default; `?mouse=1` opts in, and should be paired with
`--dry-run`.

### Haptics barely exist on the web

*Symptom: `navigator.vibrate` does nothing on an iPhone.*

**iOS Safari does not implement the Vibration API at all** — it is undefined, and
no user gesture unlocks it. Android Chrome implements it properly.

The only iOS avenue is a side effect: since 17.4, toggling an
`<input type="checkbox" switch>` plays a light system haptic. `haptics.ts` uses
it as a bonus, never as the mechanism. Anything a user must notice needs
**visual** feedback as its primary channel — hence the long-press ring.

The element must be off-screen, not `display:none`: a hidden control emits
nothing.

So the press feedback is a **sound** as well as an animation (`sound.ts`) — on
every phone, not only the ones with no vibration motor — and three things about
it are not optional:

- **Create the `AudioContext` inside a real user gesture.** One built anywhere
  else is born `suspended`, so the *first* click is swallowed and every later
  one works - the most confusing failure available. It is primed on
  `touchstart`, and again when the setting is switched on.
- **Ask for the `ambient` audio session** (`navigator.audioSession.type`,
  Safari 16.4+). The default behaves like playback: it would duck or interrupt
  whatever the user is listening to, for a 4 ms tick. Ambient mixes, and stays
  under the phone's mute switch - which is exactly where a click belongs.
- **Ramp the first half-millisecond.** A buffer that starts at full amplitude is
  a DC step, and on a phone speaker that is an audible pop in front of the click
  it is meant to be.

It used to default on where there was no Vibration API and off where there was,
so that no device got both. That was wrong on Android, and wrong for the one
gesture it was written for: the buzz it deferred to is `navigator.vibrate`,
which Chrome refuses without a user activation from a *completed* tap, and a
long press never lifts the finger (see the next section for the other half of
that). So an Android phone was handed no feedback at all beyond the ring, on the
grounds that it had a motor it was not allowed to use. It now defaults on
everywhere; a phone that does manage to buzz gets both, which is what a real
trackpad does, and the switch is in the sheet for anyone who wants silence.

### A prevented `touchstart` silently disables vibration on Android

*Symptom: the long-press ring completes and the drag works, but the phone never
buzzes — and `navigator.vibrate` works fine on other sites in the same browser.*

Chrome will only vibrate a page that holds **user activation**, and it grants
that from the tap gesture Blink synthesises out of a touch sequence. A
`touchstart` that has been `preventDefault()`ed suppresses that gesture, so a
page that swallows every `touchstart` — which `suppressBrowserGestures` used to
do, on every platform — never becomes activated. `navigator.vibrate` then
returns `false` forever, throws nothing, and logs nothing the phone can see.

The touch-event hammer is now applied **only on iOS**, which is the only browser
that needed it: `touch-action: none` already stops scrolling, double-tap zoom and
pull-to-refresh on Android, and iOS has no vibration to lose.

Two related traps, both worth knowing before blaming the page:

- **Sub-20 ms pulses may not be felt.** The spec accepts any duration, but a
  vibration motor needs time to spin up, and Samsung's One UI renders a very
  short request as nothing. `TAP_PATTERN` is 30 ms for that reason.
- **`navigator.vibrate` returns a boolean.** `false` means the browser refused
  the call. The debug page's Haptics panel reports it next to
  `navigator.userActivation.hasBeenActive`, which is what separates "no motor"
  from "refused".

### An uncancelled touch costs a compatibility mouse event every frame

*Symptom: cursor movement that was smooth becomes rough, with no change to the
network, the desktop or the sample rate.*

Dropping `preventDefault()` from the pad's touch pointer events - done to let a
tap earn the user activation Chrome wants before it will vibrate - looked free,
because `touch-action: none` and `user-select: none` already suppress scrolling,
zoom and selection. It was not. An uncancelled touch also makes the browser
synthesise the whole compatibility mouse stream: a `mousedown`, a `mousemove`
**per frame**, a `mouseup` and a `click`. Each of those costs a hit-test and a
page-wide `:hover` style recalculation, landing on exactly the frames that have
to be capturing and flushing touch samples.

Activation is sticky, so it only has to be earned once. `Surface.swallow()` now
lets a single touch through - the first, while
`navigator.userActivation.hasBeenActive` is still false - and cancels every
touch after it exactly as before. Where `navigator.userActivation` is missing
the flag starts *true*: iOS has no Vibration API to win, so its input path never
deviates from the one already known to be smooth.

Worth measuring rather than guessing: a synthetic frame-cost harness showed
ordinary cursor movement at an identical 57 canvas operations per frame before
and after the animation work, which ruled the animation out and left this.

### Screen-wide feedback must be gated, not just drawn

*Symptom: the long-press border lights up while merely moving the cursor slowly.*

Positioning a cursor precisely keeps a finger nearly still, which looks a lot
like the start of a hold. The screen-edge border is also the most expensive
thing the renderer draws - a long stroked path, on every frame. Drawing it from
the moment a hold looks possible meant paying for it throughout ordinary use.

It now appears only past `BORDER_FROM` (half the charge) and sweeps the full
perimeter across the remaining half. Cheaper, quieter, and it reads better: the
border arriving *means* "nearly there" instead of being constant chatter.

Two related economies in the same pass: a dashed stroke is much dearer than a
solid one, so `strokePerimeter` skips `setLineDash` entirely at `fraction >= 1`
(the armed state, drawn on every frame of every drag); and the arming flash is a
flat `fillRect` rather than a full-screen radial gradient, which was the single
most expensive thing drawn - across millions of device pixels, on the frames
right after a drag starts.

### A still finger sends no events, so nothing can wait for one

*Symptom: the long-press ring appears sometimes and not others, and holding
perfectly still is the case that fails.*

The worst version of this bug, because it punishes doing the gesture *well*.
`flush()` skips an empty queue, and a motionless finger produces no
`pointermove` - so after the initial Down no further batch is ever sent.
`updateHold` waited for a second sample before starting its clock (the Down was
rejected outright, because liveness was read from `points`, which is still empty
at that moment). The ring therefore appeared only when the finger happened to
shake, and a steady hand got nothing at all.

Two rules came out of it:

- **Start the clock on the Down sample**, and drive the animation from the frame
  clock alone. A motionless finger is now the case it handles best.
- **Never infer touch state from `points`.** That map is a rendering structure:
  it lags a batch behind and keeps lifted fingers while their trails fade.
  Deciding "is exactly one finger down" from it was wrong in both directions - a
  synthetic suite had the old code miss a genuine single-finger hold *and* arm a
  drag with two fingers down. A dedicated `down` set answers the question.

`clear()` must empty that set too: a link that drops mid-touch never delivers
the Up, and one stale id makes `down.size !== 1` true forever.

### The trail canvas must stay full-bleed

*Symptom: every trail is drawn slightly below where the finger actually is.*

Touch samples are normalized against `#surface` and multiplied back up by the
*canvas* size, so the two elements have to be exactly the same box. Insetting
the canvas by the safe area - to keep the screen-edge hold indicator clear of
the notch - displaced every trail on an iPhone by the height of the notch.

The canvas stays `inset: 0`. What gets inset is the drawing: `env()` is not
readable from script, so `:root` carries the four insets as `--sa-*` custom
properties and `resize()` reads them back.

### An armed drag must not re-enter the charging state

*Symptom: the long-press ring fills, turns over — and immediately starts
charging again for the whole of the drag.*

`updateHold` cancelled the hold whenever the finger moved more than a fingertip,
which is right up until the moment the drag arms and dead wrong after it: from
then on, moving is the entire point, and the desktop holds the button down until
the finger lifts. The phone therefore spent nearly every drag showing a
restarting charge-up while the desktop had the button held.

Measured on a synthetic 40-frame drag, the indicator was armed for **3 frames
before the fix and 40 after**. Once `armed` is set, movement now carries the
ring with the finger; only a lift, a cancel or a second finger ends it.

### A long press cannot earn its own user activation

*Symptom: the ring completes, the drag works, the phone stays silent — and
`googlechrome.github.io/samples/vibration/` buzzes fine on the same device.*

The deeper half of the problem above, and the half that survives fixing
`touchstart`. Chrome requires **sticky user activation** before it will vibrate,
and it grants that from a *completed tap* — a finger going down is explicitly
not a gesture. A long press buzzes at the half-second mark with the finger still
down, so on a freshly loaded page whose first interaction is a hold, there has
never been a tap and `vibrate()` returns `false`.

It is sticky, so exactly one tap anywhere on the document fixes it for the life
of the page — which is why this looks intermittent and why the Chrome sample,
being a button, never shows it.

Nothing can manufacture activation, so the app reports it instead: `buzz()`
returns the boolean, and `onPressArmed` puts *"Tap the pad once to let this
browser vibrate"* in the hint line when the call is refused on a device that
does have the API.

`/haptics.html` is the standalone probe. It fires the same pulse from a click,
a `touchstart`, a `touchend` and a real half-second hold, printing what
`vibrate()` returned and the activation state at the moment of each call.
Reload between runs — activation is sticky, so a second run without one tells
you nothing.

### The press ring must not restart itself mid-move

*Symptom: while you steer the cursor, pausing draws a full charge-up ring and
buzzes — but nothing is ever selected, and the desktop never presses the button.*

The ring is a *preview* of the engine's decision, and previews can lie. The
engine spends the hold the moment a finger passes `tapMaxPx`: `Pending` becomes
`Moving`, and `Moving` has no path back, so no amount of resting will arm a drag
on that touch. The phone instead re-anchored the hold at wherever the finger had
stopped and restarted its clock, so every pause during a move charged a fresh
ring to completion.

Both halves looked right in isolation, which is why it survived: the engine test
passes, the ring animates beautifully, and only a user holding the phone ever
sees the two disagree. Measure stillness from where the finger **landed**, not
from a rolling position, and once it is spent keep it spent until the finger
lifts.

`tapMaxPx` now travels to the phone in the `state` greeting alongside `pressMs`,
so a retuned host cannot reintroduce the disagreement. The rule is pinned from
both sides: `desktop/tests/press_after_move.rs` and `npm run check:hold`.

### A full-screen effect must be cached and clipped

*Symptom: the cursor stutters during a drag, and only during a drag.*

The drag glare is a smooth falloff over the whole rim of the screen. Drawn
honestly it is 54 one-pixel strokes around the perimeter — fine once, ruinous
at 60 fps on the frames where cursor movement matters most. It is built into an
offscreen canvas on resize and blitted after that.

The blit is then clipped to four edge strips rather than done full-screen. The
glare is transparent across the entire middle, and compositing that emptiness
costs exactly as much as compositing light: about five times the pixels, for
nothing. The same reasoning already applies to the arming flash, which is a flat
fill rather than the gradient it looks like.

### A long hold accumulates jitter

*Symptom: long-press-to-drag never fires.*

A resting finger is never truly still. Over half a second its `path_px` adds up
to more travel than a deliberate move, so judging the hold by path length blocks
every long press. Judge it by **distance from the start** instead, and let the
duration do the work of making it deliberate.

### `setPointerCapture` can throw

If the pointer is already gone. Losing the touch sample to an exception is worse
than losing capture — wrap it.
