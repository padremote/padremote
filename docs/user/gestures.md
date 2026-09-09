# Gestures

## It copies your Mac's trackpad

PadRemote reads your real trackpad settings and matches them — so it behaves
like the trackpad you already use, not like some fixed default. If you have
three-finger *tap* off, PadRemote won't middle-click on a three-finger tap. If
your scrolling is natural, so is PadRemote's.

It re-reads them about twice a second, so changing a switch in **System
Settings → Trackpad** takes effect within a second or two. No restart.

To see exactly what it copied: menu-bar icon → **Settings…**, which says for
each gesture whether your computer is deciding it and which setting does.

## The gestures

| Gesture | Does | Follows your setting for |
|---|---|---|
| One finger, move | Move the cursor | (tracking speed is approximated — see below) |
| Tap | Left click | Tap to click |
| Two-finger tap | Right click | Secondary click |
| Three-finger tap | Middle click | Three finger tap |
| Four-finger tap | *unbound* — yours to assign | not a trackpad setting; your Mac has no four-finger tap |
| Two-finger double tap | *not offered* — too easily confused with a two-finger swipe | not copied |
| Two fingers, move | Scroll | Scroll direction, inertia, horizontal scroll |
| Pinch two fingers | *off by default* — too easily confused with a two-finger swipe | not copied |
| Two fingers sideways | Back / forward | Swipe between pages |
| Three fingers, left/right | Switch full-screen apps | Swipe between pages |
| Three fingers, up/down | Mission Control | Mission Control |
| Four fingers, left/right | Switch full-screen apps | Swipe between full-screen apps |
| Four fingers, up | Mission Control | Mission Control |
| Four fingers, down | App windows | Mission Control |
| Four fingers, pinch in | Launchpad | Launchpad |
| Five fingers, spread | Show Desktop | Show Desktop |
| Three or four fingers, up/down | Volume or brightness, if you assign it | not a trackpad setting — yours to set |
| **Press and hold, then move** | **Drag / select text** | always available — see below |

Every swipe direction is assignable on its own — three fingers up and three
fingers down are two settings, not one — and each starts out following the
trackpad setting it is copied from, so the table above is what you get until you
change something. See [Settings](settings.md#gestures).

**The four-finger tap is the one gesture your Mac does not have.** Everything
else in the table copies a trackpad setting; this one has nothing to copy, so it
arrives unbound and does nothing until you give it a job. Open **Settings →
Gestures**, pick *Tap with four fingers*, and choose — Mission Control, a
screenshot, Spotlight, lock the screen, or any of the others. Nothing about your
Mac changes when you do; the choice lives in PadRemote.

**You can change your mind part-way through.** Start moving the cursor with one
finger, then lay a second finger down, and it becomes a scroll — you do not have
to lift your hand and start again, exactly as on the trackpad. The gesture is
re-measured from where your fingers are at that moment, so the page scrolls the
way you move them next, not the way the first finger happened to be going. The
trade is the same one a trackpad makes: a stray second touch during a move will
interrupt it too. Lift and start again if that happens.

A drag is the exception. Once the button is held, a second finger landing does
not take it apart — a brush against the screen must not drop what you are half
way through selecting.

Dragging deserves a note, because macOS treats it as **one choice** with several
options: *Three-Finger Drag*, *without drag lock*, *with drag lock*, or off.
PadRemote has no three-finger drag — three fingers are kept for the swipes — so
what it takes from that choice is whether **one** finger may start a drag. Pick
either drag-lock option and a tap followed by a move drags; pick Three-Finger
Drag or off and one finger only ever moves the cursor. Press and hold works
either way, and is never taken away.

## Selecting text: press and hold

On a trackpad you hold the button — or Force Click — and drag. Your phone has no
button and no pressure sensor, so **long press** stands in for both.

Rest one finger without moving. A ring charges around it; when it fills, the
button is held and you're dragging. Slide to select, lift to release.

**Start from a standstill.** The hold only counts from a finger that has not yet
moved. Once you have started steering the cursor, that touch can no longer become
a drag however long you pause — lift, put the finger back down, and hold. This is
what keeps an ordinary move that stops for a moment from grabbing whatever is
under the cursor and dragging it away.

**What you'll see**, and it is designed to be readable without looking directly
at the phone:

| | |
|---|---|
| **Charging** | Four blue brackets close in on the **corners of the screen**, growing along the edges as the hold fills, and a ring tightens at your fingertip. |
| **Armed** | A line sweeps down the screen, the brackets are thrown outward, the ring turns green where it stood, the edges bloom white, and **DRAGGING** appears clear of your hand. |
| **Dragging** | White light glows in from all four edges and pulses, and a green bracket box holds your fingertip, for as long as the button is held. |
| **Released** | A ring rings out from where you let go, so you can see the drop landed. |

The screen border is the part to watch. Your fingertip covers about a
centimetre of display — anything drawn under it is hidden by the hand doing the
gesture — so the signal that has to be unmissable is drawn where your hand
isn't.

- It takes **half a second**, deliberately: a shorter hold caught ordinary cursor
  moves that paused for a moment and turned them into selections.
- **Moving spends the hold.** The ring stops charging the instant the finger
  leaves where it landed, and does not start again until you lift. If you see no
  ring while resting, your finger already travelled — lift and press again.
- **The change at the edges is the cue to trust.** Blue brackets still closing
  in mean keep waiting; the whole rim glowing white means go. That reads out of
  the corner of your eye, which is exactly how you'll see it while watching the
  cursor on the other screen.
- **Nothing is drawn in the corners themselves.** Each bracket is cut off at
  45°, so the indicator never argues with your phone's own rounded display -
  there is no arc to match, and nothing to disappear behind the bezel.
- The white glow is light rather than an outline: brightest at the very edge of
  the glass and fading inward, with no line to look at. It runs corner to corner,
  so light pools where two edges meet. It dims the status text along the top of
  the screen while a drag is held. That is deliberate — during a drag the only
  thing that has to carry is whether the button is down.
- It pulses about once every two seconds, brightening and dimming by roughly
  half. Fast enough to catch in peripheral vision while your eyes are on the
  other screen, and never so fast that it reads as an alarm.
- **The confirmation grows out of the charge**, on purpose: the ring you were
  watching keeps its size and turns green where it stood, and the edge light
  blooms bright and settles rather than appearing from nowhere. If arming ever
  looks like a *different* animation starting, that is a bug worth reporting.
- **A buzz is a bonus, never the signal.** Android usually vibrates when the drag
  arms; iPhone cannot (iOS Safari has no vibration API), and many tablets have no
  vibration motor at all. The animation is what always arrives.
- **A click sound**, on every phone. Two clicks, in fact - one when the button
  goes down and a quieter one when it comes back up, like a real trackpad. It is
  on by default everywhere, Android included: a phone that can vibrate gets both,
  because the buzz is the half that often does not arrive. Turn it off in
  **Settings → Click sound** on the pad. On iPhone and iPad it follows the mute
  switch and never interrupts music; on Android it plays at media volume, so
  silence there means the switch rather than the ringer.
- If your device shows the ring but never buzzes, open **`/haptics.html`** on it.
  It fires the same pulse from a tap, a press, a hold and a full two seconds, and
  tells you whether the browser refused the call or handed it to the device — two
  very different problems, with a checklist for each.
- This is the one gesture **not** copied from your Mac. It substitutes for
  hardware you don't have, so no trackpad setting can take it away — otherwise a
  Mac set to three-finger drag would leave you no way to select text at all.

## What can't be copied

Three things are genuinely impossible, and the mirror report says so rather than
pretending:

- **Force Click** — your phone screen has no pressure sensor.
- **Three-finger drag** — three fingers are kept for the swipe gestures. Press
  and hold does the same job, and works whatever your Mac is set to.
- **Rotate** — macOS exposes no way for any app to synthesize a rotation
  gesture. PadRemote could detect the twist; nothing could deliver it.
- **Tracking and scroll speed** — readable, but Apple's acceleration curve is
  private, so these are approximations. Tune `sensitivity` by hand if the feel
  is off (see below).

Two more are approximated:

- **Pinch zoom** and **smart zoom** are off by default (see the table above).
  When turned on under Advanced they are sent as `⌘+` / `⌘−`, because no public
  API can synthesize a real magnify gesture. Zoom therefore steps rather than
  glides, and only works in apps that have a zoom command.

## Tuning by hand

Edit `~/Library/Application Support/PadRemote/config.json` — the settings page
prints the path at the bottom. It hot-reloads: edit and feel the change within
about half a second, no restart.

The ones worth touching first:

- **`sensitivity`** — overall cursor speed.
- **`accel.gain`** — how much faster the cursor goes when your finger moves
  fast. Lower it if the cursor feels twitchy on quick flicks.
- **`scroll.accel`** — how much further than your finger the page scrolls on a
  quick flick. `0` is strictly one-to-one; the default `1.0` lets a flick cover
  a page, like a real trackpad.
- **`tap.pressMs`** — how long to hold before a drag starts. The phone's ring
  follows this automatically.
- **`tap.tapMaxPx`** — how far a finger may stray and still count as resting.
  Past this it is a cursor move, and the hold is spent until you lift. The
  phone's ring follows this automatically too.

Sensitivity and scroll direction can also be changed live from the ⚙ button on
the phone.

Setting `"followSystem": false` stops PadRemote copying your Mac and pins it to
the config file alone.
