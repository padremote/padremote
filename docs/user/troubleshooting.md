# Troubleshooting

Every problem listed here is one that actually happened, with the symptom that
gave it away.

## The live debug view

Before guessing, look. Menu-bar icon → **Connect a device…** → **Diagnostics**
(or the `debug ->` URL printed at startup).

It attaches as a **read-only observer**, so it never takes control away from your
phone — unlike opening the normal phone page on your Mac, which does.

It separates the three things that all *feel* the same:

| What you see | What it means |
|---|---|
| **Touch preview** looks ragged | The phone isn't capturing cleanly |
| **Connection smoothness** is orange or red | The network is delivering unevenly |
| **Recent actions** disagree with what your hand did | The recognizer is misreading you |

Useful numbers, under **Connection measurements**:

- **Average gap · ms** — time between batches. Steady 8–16 ms is smooth.
  Measured only *during* a touch, so pauses between gestures don't pollute it.
- **Gap variation · ms** — spread between fastest and slowest. High means
  stutter even if the average looks fine.
- **Peak fingers** — how many fingers the gesture is being judged on. If this
  reads 3 while you have four down, your hand is landing too raggedly to group.

## The cursor doesn't move at all

**The phone will tell you.** If the computer cannot move its own cursor, the pad
shows an amber dot and says so instead of showing a healthy connection — read
that line first, because it names the fix. Everything else on the page looks
perfectly normal in this state (the gesture readout follows your finger, the
latency figure is live), which is why the page has to say it out loud.

**Accessibility permission is missing.** The commonest cause by far: macOS
accepts injected events from an untrusted process and silently discards them.

Fix: System Settings → Privacy & Security → Accessibility → enable PadRemote.
The cursor starts moving the moment you tick the box — nothing to restart, on
either end. Remember the grant is **per app**: one granted to your terminal does
nothing for `PadRemote.app`, and a rebuilt binary needs it again.

**It was started with `--dry-run`.** Then it is reading every gesture and moving
nothing, on purpose. The phone says this too. Quit it and start it without the
flag — and check you have only one copy running: a phone pointed at a dry-run
copy on another port looks exactly like a healthy connection that does nothing.
`Connect a device…` in the menu bar always opens the copy you are looking at.

## macOS asked for Accessibility twice

Two copies were running, and each one asks for itself. That is what older
installs did: the login item started PadRemote, and the installer then opened
it a second time a few lines later.

Fixed on both sides — the installer no longer launches a copy the login item has
already started, and a copy that finds the port taken now quits before it asks
for anything. If you still see two prompts, you started a second copy yourself
(from a terminal, say); quit it and grant the permission once, to
`PadRemote.app`.

Granting it twice does no harm, and neither does dismissing the extra dialog:
the tick in System Settings is per app, not per dialog.

## The cursor drags instead of moving

Tapping and then immediately moving is starting a drag, so the cursor selects
text instead of pointing at it. That is *tap and drag*, and it ships off — if it
is happening, something turned it on.

Your Mac's dragging style is copied. If yours is *without drag lock*, tapping and
immediately moving starts a drag on your trackpad too, and the phone is doing
what the Mac does. Change the style in **System Settings → Accessibility →
Pointer Control → Trackpad Options**, or turn *Tap and drag* off in **Advanced
settings** on the phone — with `followSystem` on, though, the next reading of
your Mac's preferences turns it back on.

PadRemote up to version 1 of the config file shipped it on by mistake. Your
config is corrected the first time this version starts, and says so in the log.

## The pad says *Someone else is using this computer*

You have more than one phone or tablet connected, which is fine — they share the
computer and take turns. The cursor belongs to whoever is using it, and the
status dot turns amber on the devices that are waiting.

**To take over, just touch the pad.** The cursor comes to you as soon as the
other device stops — about a third of a second after it lifts its fingers. There
is no button to press and nothing to close.

While you are waiting, your touches are still being read (the trails still draw)
— they simply don't move anything. If nothing you do ever takes effect, check
the name in the message: it may be a forgotten tab open on another device, and
closing it hands the cursor over for good.

The device names come from each device's own **Settings → This device**, so if
two of them read "iPhone", rename one there.

## The settings button is impossible to tap

Fixed — but if you are looking at an old page, that is why. The gear used to sit
at the **top right**, which on a phone is the worst possible corner: the
browser's address bar sits over it and iOS reserves the top edge for its own
pull-down. It was also far too small.

It is now a 48-pixel target at the **bottom right**, a thumb's distance away and
clear of the browser's own chrome.

To check what your phone is running, open the sheet and read the last line. It
shows the build, and the three measurements that decide the layout:

```
build 06/09/2026 16:52 · surface 390×664 @3× · visible 390×664+0,91 · layout 390×844
```

- **visible** is the area you can actually see, and where it starts — `+0,91`
  means the browser's own bar takes the top 91 points.
- **layout** is the whole display, bars included.

The pad should be exactly the *visible* size. If those two disagree and the pad
looks shifted, that line is the evidence — send it. If there is no such line at
all, the page is old: pull to refresh, or close the tab and open it again.

## Scrolling goes the wrong way

PadRemote copies your Mac's **Scrolling direction: Natural** setting, so it
should already match the trackpad you are used to. If it doesn't, this phone has
taken the setting over at some point.

Open **Settings** on the pad and look under *Natural scrolling*:

- **"matching your computer"** — the phone is following your Mac. If the
  direction is still wrong, the Mac's own setting is what you want to change:
  System Settings → Trackpad → Scroll & Zoom → Natural scrolling. PadRemote
  follows within a second, no restart.
- **"match my computer"** — this phone is overriding your Mac. Tap it to hand
  the setting back, or leave it if that is what you wanted; the override is
  local to this phone, so your tablet is unaffected either way.

Sensitivity works the same way, except that it comes from the config file rather
than from your trackpad: macOS keeps its tracking-speed curve private, so there
is nothing to copy (see [Gestures](gestures.md#what-cant-be-copied)).

## The phone doesn't buzz when a drag starts

Every iPhone and iPad is in this position: Safari has no vibration API at all,
and nothing can turn it on. Many Android tablets have no vibration motor either.

**Settings → Click sound** on the pad covers it, and is already on by default on
every device: it clicks when the button goes down and again, more quietly, when
it lets go — the same two clicks a real trackpad makes.

If you hear nothing on an iPhone or iPad, check the mute switch. The click
deliberately follows it, and deliberately never interrupts whatever you are
listening to. On Android it plays at media volume instead, so turn the volume up
rather than looking for a silent switch.

On Android, where a buzz *should* work but doesn't, open **`/haptics.html`** on
the phone: it fires the same pulse from a tap, a press, a hold and a full two
seconds, and tells you whether the browser refused the call or handed it to the
device — two very different problems, with a checklist for each.

## The pad says it hasn't been paired

The page was opened without ever being pointed at a computer — a bookmark, a
typed address, or the home-screen icon before the first scan. Nothing is broken;
this phone has simply never been told where your computer is.

Open PadRemote on your computer, choose **Connect a device…**, and scan the QR code with the
phone's camera. After that the phone remembers the address and reconnects on its
own, so this is a one-time step per phone.

## The pad says *Not paired yet*, and it was working before

Different from the one above: this phone reached your computer and was turned
away. The QR carries a pairing code as well as an address, and this phone's is
no longer the right one. There are three ways that happens:

- Somebody used **Forget all devices** on the connect page, which revokes every
  paired phone at once. Re-scan the new QR. (**Forget** on that phone's own row
  does the same to it alone.)
- The address was typed or bookmarked rather than scanned, so it never carried a
  code at all. Scan the QR.
- The phone's saved data was cleared — a wiped browser, or private browsing.
  Scan the QR again.

The pad will not keep retrying, because it would be refused identically every
time. Scanning a fresh code is what fixes it.

## The status flickers between your computer's name and "offline"

The page is failing to reach the app and retrying. This is a network problem,
not a second device — a device waiting its turn stays connected and says so.

- Check that PadRemote is still running in the menu bar.
- Check that the phone and the Mac are on the same Wi-Fi, and that the network
  isn't a guest one with client isolation turned on.
- Don't leave the normal phone page open on your Mac — use **Diagnostics** on
  the connect page instead, which observes without taking control.

## The cursor freezes until I lift my fingers

Two fingers are down, one of them lifted, and the gesture had not yet decided
what it was — so it is still waiting for the second finger to come back and say.
The remaining finger does nothing until you lift it and start again.

This is most likely right after a second finger brushes the screen while you are
moving the cursor. That second touch turns the gesture into a scroll (which is
what a trackpad does, and what lets you start scrolling without lifting your
hand), and if it then leaves before either finger has travelled, there is nothing
left to scroll with. Lift and put your finger back down.

A scroll that has actually started is not affected: lift one of the two fingers
mid-scroll and the other carries on scrolling, as it should.

## The cursor jumps when I go from the trackpad to the phone

*Fixed — this one is worth updating for.* The cursor would snap back to wherever
PadRemote last left it, then carry on from there.

The app has to remember where it put the cursor, because macOS is told a
position rather than a distance. Nothing tells it when you use the Mac's own
trackpad, so that memory went stale and the next touch on the phone corrected
the cursor to it. It now re-reads the real position at the start of every
gesture. If you still see it, you are running an older build — rebuild and
reinstall.

## Four-finger gestures don't fire

Check **peak fingers** in the debug view while you swipe.

- Reads **3** — your fingers are landing too far apart in time. They must land
  within 160 ms of each other. Try placing them more deliberately together.
- Reads **4** but nothing happens — check the Events list. If it shows
  `launchpad` when you meant Mission Control, your fingers are splaying enough to
  look like a pinch; swipe a little further and straighter.

## Vertical swipes work but horizontal don't (or vice versa)

Check **Settings…** in the menu bar. It says, gesture by gesture, which ones
your computer is deciding — your Mac may simply have that one turned off, and
PadRemote is faithfully copying it. Turn it on in System Settings → Trackpad and
PadRemote follows within a second.

## The pad is shifted up the screen after scanning the code

Fixed — but if you are looking at an old page, this is what you were seeing: on
the **first** load after a scan the pad sits too high, the trails land away from
your finger and the frame is cut off. Reloading puts it right, until the next
fresh scan.

Chrome on iPhone can keep the screen size it had *before* the Camera app handed
the link over, so the page is laid out against a size that is no longer true.
Nothing the page measures can correct that, which is why reloading worked and
nothing else did. PadRemote now shows a brief **Opening trackpad…** screen on a
scanned link, waits for the browser's bars to settle, and then loads the pad
fresh in the size it will actually have. This happens only on iPhone Chrome, and
only on the first load from a scan — typed addresses, reloads and every other
browser start straight away.

If you still see it, open the settings sheet and read the last line. An old page
will not have the fix at all; if the build is current, send the **visible** and
**layout** numbers from that line.

## "This site can't be reached", and your Mac's address hasn't changed

*Symptom on the phone: the browser never loads PadRemote's page at all, and the
address in the QR matches the menu bar's address line.*

**PadRemote is not running.** It serves the page itself, on the same port your
phone connects to, so if the app is gone there is nothing to load — the two used
to be separate processes and are not any more. Look for the ●● icon in the menu
bar; if it is not there, launch PadRemote (Cmd-Space).

If you would rather it were always there, add the login item:

```sh
./install.sh --login
```

Check from the Mac itself with `curl -I http://localhost:8787/` — a `200` means
the page is being served and the problem is between your phone and the Mac
(different Wi-Fi, or a network with client isolation), not the app.

If the page loads but says PadRemote was **built without its phone page**, the
app was compiled without `web/dist` beside it. Rebuild with `./install.sh`.

## The QR code doesn't work / it worked yesterday

*Symptom on the phone: "This site can't be reached", before PadRemote's own page
ever appears.*

Your Mac's Wi-Fi address changed. The QR encodes the address, so an old one
points at nothing — and because the phone never reaches the page, nothing on
screen says the address is what moved.

PadRemote notices within about five seconds. The connect page is drawn from
wherever your computer is at that moment, and one left open reloads itself when
the address moves — so open **Connect a device…** and re-scan whatever code is
on screen.

Your phone stays paired across the move — only the address changed, not the
pairing — so this is a re-scan, not a re-pair.

To stop it recurring, give your Mac a reserved IP in your router. (Discovery by
name, so this never matters, is planned but not built.)

The pairing code inside the QR does **not** change when your address does, so a
phone that has already been paired is still paired — it just needs to be told
where the computer moved to.

## "Port 8787 is already taken"

Another copy is already running — often `PadRemote.app` when you also started one
from a terminal. Quit the other (menu-bar icon → Quit PadRemote), or start this
one with `--port 8788`.

## Scrolling feels like a mouse wheel, not a trackpad

That was a real bug and is fixed: scroll events now carry the gesture phases
macOS needs for smooth scrolling and rubber-banding. If it comes back, something
has gone wrong with scroll phases — see [gotchas](../dev/gotchas.md).

## Zoom is steppy

Expected, and unlikely to change soon. No public macOS API can synthesize a real
magnify gesture, so zoom is sent as `⌘+` / `⌘−`. It steps, and only works in apps
that have a zoom command.
