# Settings

Tap **Settings** at the bottom of the trackpad for quick changes on this device.
Choose **All settings & gesture guide** for the full settings page, or open
**Settings…** from the computer’s menu-bar icon.

## Full settings

The page opens on your connected devices, with the rest behind three cards:

| Section | What you can do |
|---|---|
| **Basic settings** | Adjust pointer and scrolling speed, natural scrolling, and press-and-drag |
| **Gestures** | Choose what each gesture does |
| **Advanced settings** | Adjust acceleration, zoom, timings, and other specialist options |

Each card says what is currently set behind it — the pointer speed, how many
gestures are assigned — so most questions are answered without opening
anything.

**‹ Settings** at the top left returns to that list from any page, and **‹
Trackpad** returns to the trackpad from the list. Your browser's Back button
does the same.

Changes apply live. The status shows **Saving…** while a change is being sent,
then **Saved on your computer** after the computer confirms it. Keep the
computer connected to save changes.

### Connected devices

The list at the top of the page: every device paired with this computer, the
ones connected right now first, and the device currently moving the cursor
named as such.

Un-pairing is done on the computer: open Settings from the menu-bar icon and
each device gets a **Forget** button, with **Forget all devices…** under the
list. A device you forget has to scan the QR code again. Opened from a phone
the list is there to read — a phone cannot revoke another phone's pairing, or
its own.

### Match my computer

In Basics, **Match my computer** uses your computer’s trackpad preferences for
the settings it controls. Those controls explain where their values come from.
Turn this off to customize them in PadRemote.

Open **View computer settings** to see which settings PadRemote matches,
approximates, or cannot reproduce.

### Gestures

Open **Gestures** to see everything the trackpad can do. The page is three
columns: the gestures on the left, the one you have picked demonstrated in the
middle, and everything it can be set to on the right.

Every row demonstrates itself: a small trackpad beside the name shows the
fingers doing it, so one finger or three, sideways or up and down, can be told
apart at a glance without reading a word. It is the same drawing as the large
one in the middle, only smaller — a row and the demonstration beside it never
show a gesture two different ways.

The rows are grouped by the hand that makes them — **Pointer & taps**, then
**2-finger swipes**, **3-finger swipes** and **4-finger swipes**.

**Each direction is its own setting.** A three-finger swipe up and a
three-finger swipe down are two rows, two demonstrations and two actions, so
Mission Control upward and the volume downward is a thing you can have. The four
directions of one hand sit together in the list, in the order left, right, up,
down.

A direction nobody has touched reads **Use existing setting**, and says what
following currently does — *Use existing setting · Mission Control*. It follows
the older paired setting for that axis, which is the one your computer's
trackpad preferences are copied into, so an existing setup keeps working and
**Match my computer** keeps reaching every direction. Choose anything else and
only that direction stops following; the other three carry on as they were.

Every action is listed at once rather than hidden in a menu, with a search box
above them for the long lists. The few worth suggesting come first under
**Recommended**, and the rest are listed alphabetically under **Other actions** —
a tap can be bound to more than twenty things, from Mission Control and Spotlight
to Copy, Close window and Lock screen.

Actions are named for the one direction you are setting: **Mission Control**,
**Volume up**, **Brightness down**, **Desktop left**. Only the actions a gesture can actually perform
are offered, so nothing you pick can be quietly ignored — a sideways swipe is
never offered Mission Control, which travels up and down.

Dots on a small trackpad move the way your fingers would, on a slight curve
because that is how fingertips actually land, and blur behind themselves as
they cross the pad. The demonstrations loop on their own — there is nothing to
press — and with reduced motion enabled they hold a single frame, where the
blur left behind the fingers is what says which way the gesture goes. On a
narrow screen the three columns stack: a picker, then the demonstration, then
the actions.

Actions are named the way your computer names them, so a Mac offers Mission
Control and App windows while Windows offers Task View.

While **Match my computer** is on, your computer's own trackpad settings decide
the gestures they cover — the two-finger and three-finger taps — and those rows
are greyed out with a note naming the setting that decides them. The same
settings are what an untouched swipe direction follows, so a switch changed in
System Settings changes what those directions do. Turn **Match my computer** off
in Basics to choose something else, including actions your computer has no
concept of, like the volume.

It is the same on Basics: pointer speed and scrolling direction are greyed out
while matching is on, because your computer keeps writing those and PadRemote
could not hold a different value even if you set one. Wherever a control is
greyed out, the note under it names the setting responsible, and the switch that
releases it is the same one.

### Which gestures exist

Every gesture using more than one finger is a **swipe** — two, three or four
fingers, up and down or left and right — plus taps with one, two or three
fingers. Two fingers up and down is scrolling, which is not assignable: it is
tuned in Basics instead.

Pinching and double-tapping are not offered, and pinch zoom ships **off**. A
pinch is the same two fingers moving as a two-finger swipe, and on a surface the
size of a palm PadRemote has to tell them apart from a few millimetres of
difference. It gets that wrong often enough that an ordinary scroll would zoom,
which is worse than not having the gesture. Unlike most settings this one is
*not* copied from your computer: whether a pinch belongs on a trackpad depends
on how big the trackpad is, and your phone's answer differs from your Mac's.

Launchpad and Show Desktop need a four-finger pinch and a five-finger spread, so
they are not offered either. All of these remain under **Advanced** for anyone
who wants them back.

### Restore defaults

**Advanced → Restore defaults** resets the computer’s PadRemote configuration
after confirmation. Device-specific pointer speed and scrolling overrides are
managed separately in quick settings.

## Quick settings on your phone

| Control | What it changes |
|---|---|
| **Device name** | The name shown when several devices share a computer |
| **Pointer speed** | A speed override for this device |
| **Natural scrolling** | A scrolling-direction override for this device |
| **Click sound** | Sound feedback when you press and release a drag |
| **Full screen** | A quieter trackpad with the app controls hidden |

Pointer speed and natural scrolling show **Matching your computer** until you
change them. Once changed, that control becomes **Use my computer’s setting**
and hands the setting back. A phone and tablet can keep different overrides.

This is a per-device override and is separate from **Match my computer** on the
full settings page, which decides what the computer itself follows.

## Full screen

Tap **Full screen** in the trackpad toolbar, or use **Toggle** in quick settings.
Use **Exit full screen** to bring the controls back.

Shaking can also toggle full screen when the browser permits motion access.
Quick settings explains when motion is unavailable and offers **Allow shake**
when permission can be requested. A plain HTTP connection does not provide
motion access.

The browser decides whether the page can use native full screen. When it
cannot, PadRemote hides its own controls. On an iPhone, adding the page to the
Home Screen provides a way to open it without the usual browser bars.

## Configuration file

You can also edit
`~/Library/Application Support/PadRemote/config.json`.
The settings page prints the path at the bottom, so you can open it from there.
PadRemote reloads edits while running.

The first save from the settings page expands the file to include every current
setting. Existing effective values stay the same; unrecognized keys are removed.

The settings page requires a connected computer. Sample-data demo mode and
animated gesture previews are no longer included.
