# Getting started

You need a Mac and a phone on the **same Wi-Fi**. Nothing is installed on the
phone — it just opens a web page.

## 1. Install the Mac app

```sh
./install.sh
```

That is the whole install. It checks you have Rust and Node, builds the phone
page *into* the app, installs to `~/Applications`, offers to start PadRemote
when you log in, and launches it.

It lives in the menu bar — no Dock icon, no window. Find it again with
**Cmd-Space**, type **PadRemote**.

| | |
|---|---|
| `./install.sh` | Build, install, ask about starting at login, launch |
| `./install.sh --no-login` | The same, without the login item |
| `./install.sh --login` | Add the login item without being asked |
| `./install.sh --uninstall` | Remove the app and the login item. Settings and pairing are kept |

**There is nothing else to start.** The page your phone loads is served by
PadRemote itself, on the same port your phone already connects to — so a reboot
that brings the app back brings all of it back.

> Building it yourself is the only option today. A signed, notarized `.dmg` is
> milestone 5 of [`plan.md`](../../plan.md); until then, a copy built on another
> Mac would be blocked by Gatekeeper.

## 2. Grant Accessibility permission

macOS will ask the first time. This is unavoidable — an app that moves your
cursor needs it, and macOS owns that decision.

1. **System Settings → Privacy & Security → Accessibility**
2. Enable **PadRemote**
3. Launch it again

**The grant is remembered per app.** If you also run PadRemote from a terminal
during development, that is a *separate* grant for your terminal — enabling one
does nothing for the other. PadRemote names which one it needs.

**And per build.** Without a code-signing certificate the app is signed ad-hoc,
which means macOS treats every rebuild as a different app: the box stays ticked
and the app is still refused. `./install.sh` clears the stale grant so macOS
asks again, and prints the one-time way to make grants survive rebuilds.

If permission is missing, PadRemote refuses to start rather than running
silently: without it macOS accepts every event and quietly discards it, which
looks exactly like a broken app.

## 3. Pair your phone

The menu-bar icon → **Connect a device…** opens a page with a QR code.

1. Open the **Camera** app on your phone
2. Point it at the code — don't take a photo
3. Tap the link that appears

The moment the phone connects, the code on the computer gives way to *Move a
finger on the trackpad* and a **Settings & gestures** button — pairing is done,
and the code is no longer the thing you need. **Add another device** brings it
back when a second phone or tablet needs it.

Your phone shows the trackpad: your computer's name and a green dot along the
top, and the pad itself filling the screen. Move a finger anywhere on it.

Three buttons sit along the bottom:

| Button | What it opens |
|---|---|
| **Gestures** | Every gesture, demonstrated with moving fingertips — see [Settings](settings.md) |
| **Full screen** | Hides these controls for a quieter pad; **Exit full screen** brings them back |
| **Settings** | Quick settings for this device, and a link to the full settings page |

The QR carries your Mac's current address, so if your router hands it a new one,
re-scan. The page is drawn fresh from wherever your computer is now, and one
left open notices the move and reloads itself — so the code on screen is always
the current one. Your phone stays paired across the move; only the address
changed.

### The QR is a key, not just an address

It also carries the pairing code your phone uses to prove it is allowed to
control this computer. Anything that connects without it is refused before a
single touch is read, so another device on your Wi-Fi cannot drive your cursor
just by finding the port — and neither can a web page you happen to have open.

Two things follow from that. **Don't post the QR anywhere**, or share a
screenshot of the phone page's address bar before the pad has connected: until
then it is the key to your computer.

After that first connection the phone swaps the code for a key of its own and
forgets the code — so it disappears from the address bar by itself, and one
phone can be revoked without disturbing the others. If a paired phone is lost or
lent out, use **Forget** on the connect page — see [Your devices](#your-devices).

## 4. Use it

See [Gestures](gestures.md). The short version: it behaves like the trackpad you
already have, because it reads your Mac's own trackpad settings and copies them.

## Using more than one device

Scan the same QR with a second phone or tablet and both stay connected. They
share the one cursor and take turns: it goes to whoever touches next, as soon as
the other one stops. Nothing to press, nothing to close.

The device that is waiting shows an amber dot and says who has the cursor, using
the name each device gives itself in **Settings → Device name** — rename them
there if you have two of the same phone. The menu bar counts them, and the
connect page names them.

A tablet and a phone can each keep their own sensitivity, since the settings
sheet applies to the device you are holding.

## Your devices

The connect page — menu-bar icon → **Connect a device…** — is also where the
devices live. Below is every device that has ever paired, named the way it names
itself, saying which one is connected and which one has the cursor right now.
The list stays whichever step the page is on.

Browsers paired from the same device share one row when the desktop can resolve
its Wi-Fi MAC address from the local neighbour table. The connected browser's
name takes precedence, and **Forget** revokes every browser in that row.
MAC addresses are used for grouping only; each browser still authenticates with
its own pairing key. If the address cannot be resolved (for example across a
router or VPN), pairings stay separate. Private Wi-Fi addresses can change, and
older pairings without a recorded MAC need to reconnect before they can be
grouped; stale entries can be forgotten individually.

| Button | What it does |
|---|---|
| **Forget** | Revokes that one device for good: it drops immediately, cannot reconnect, and stays locked out after a restart. The others are untouched and nobody else re-scans. |
| **Forget all devices** | The same for every device at once, plus a new pairing code. Use it if you no longer trust the QR itself — a photo of it, or a screenshot showing the phone's address bar. It takes two clicks, and the page then shows you the new code. |

A device that is paired but switched off still appears, which is the point: the
phone you want to forget is usually the one that is not here.

## The menu-bar menu

Deliberately four lines, because everything else has a better home on a page.

| Item | What it does |
|---|---|
| **_n_ devices connected** | The count, live. It says **Needs Accessibility permission** instead if that grant is missing, since nothing else would work until it is. |
| **Connect a device…** | The QR code and your paired devices — the page above |
| **Settings…** | Every setting, on a page — see [Settings](settings.md) |
| **Quit PadRemote** | |

The connect page links on to **Settings** and **Diagnostics** (a live view of
what your phone is sending — see [Troubleshooting](troubleshooting.md)).
