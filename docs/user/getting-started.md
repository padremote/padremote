# Getting started

You need a Mac and a phone on the **same Wi-Fi**. Nothing is installed on the
phone — it just opens a web page.

---

## 1. Install

```sh
./install.sh
```

That is the whole install: it builds the phone page *into* the app, installs to
`~/Applications`, offers to start PadRemote when you log in, and launches it.

It lives in the menu bar — no Dock icon, no window. Find it again with
**Cmd-Space**, type **PadRemote**.

| | |
|---|---|
| `./install.sh` | Build, install, ask about the login item, launch |
| `./install.sh --login` / `--no-login` | The same, without being asked |
| `./install.sh --uninstall` | Remove the app and the login item. Settings and pairing are kept |

> Building it yourself is the only option today. A signed, notarized `.dmg` is
> milestone 5 of [`plan.md`](../../plan.md); until then a copy built on another
> Mac would be blocked by Gatekeeper.

---

## 2. Grant Accessibility

macOS asks the first time. An app that moves your cursor needs it, and macOS
owns that decision.

**System Settings → Privacy & Security → Accessibility** → enable **PadRemote**.

Two things to expect:

- **The grant is per app.** Running PadRemote from a terminal during development
  is a *separate* grant for your terminal. PadRemote names the one it needs.
- **And per build.** Without a code-signing certificate the app is signed
  ad-hoc, so macOS treats every rebuild as a different app: the box stays ticked
  and the app is still refused. `install.sh` clears the stale grant and prints
  the one-time way to make grants survive rebuilds.

Without it, PadRemote refuses to start rather than running silently — macOS
accepts every event and quietly discards it, which looks exactly like a broken
app.

---

## 3. Pair your phone

Menu bar → **Connect a device…**

<img src="../img/connect.png" alt="The connect page: a QR code, and the devices already paired below it" width="440">

1. Open the **Camera** app on your phone
2. Point it at the code — don't take a photo
3. Tap the link that appears

The whole phone screen becomes the trackpad. Your computer's name and a green
dot sit along the top.

<img src="../img/pad.png" alt="The trackpad on a phone, connected" width="240">

Three buttons along the bottom:

| Button | What it opens |
|---|---|
| **Gestures** | Every gesture, demonstrated with moving fingertips |
| **Full screen** | Hides these controls for a quieter pad |
| **Settings** | Quick settings for this device, and a link to the full page |

### The QR is a key, not just an address

It carries the pairing code your phone uses to prove it is allowed to control
this computer. Anything that connects without it is refused before a single
touch is read — so another device on your Wi-Fi cannot drive your cursor just by
finding the port, and neither can a web page you happen to have open.

**So don't post the QR anywhere**, and don't share a screenshot of the phone's
address bar before the pad has connected: until then it is the key to your
computer. After that first connection the phone swaps the code for a key of its
own and forgets it, so the code disappears from the address bar by itself.

---

## Using more than one device

Scan the same QR with a second phone or tablet and both stay connected. They
share the one cursor and take turns: it goes to whoever touches next, as soon as
the other one stops. Nothing to press, nothing to close.

The device that is waiting shows an amber dot and names the one that has the
cursor, using the name each device gives itself in **Settings → Device name**.
A tablet and a phone can each keep their own pointer speed.

## Your devices

The connect page is also where the paired devices live: every device that has
ever paired, which one is connected, and which one has the cursor right now.

| Button | What it does |
|---|---|
| **Forget** | Revokes that one device for good — it drops immediately, cannot reconnect, and stays locked out after a restart. The others are untouched and nobody re-scans. |
| **Forget all devices** | The same for every device at once, plus a new pairing code. Use it if you no longer trust the QR itself. The page then shows you the new code. |

A device that is paired but switched off still appears, which is the point: the
phone you want to forget is usually the one that is not here.

Browsers on the same device share one row when PadRemote can resolve the Wi-Fi
MAC address from the local neighbour table, and **Forget** then revokes all of
them. Grouping only; each browser still authenticates with its own key. Across a
router or VPN, or with a private Wi-Fi address that has changed, pairings stay
separate and can be forgotten individually.

## The menu-bar menu

Deliberately four lines, because everything else has a better home on a page.

| Item | What it does |
|---|---|
| ***n* devices connected** | The count, live. It says **Needs Accessibility permission** instead if that grant is missing |
| **Connect a device…** | The QR code and your paired devices — the page above |
| **Settings…** | Every setting, on a page — see [Settings](settings.md) |
| **Quit PadRemote** | |

The connect page links on to **Settings** and **Diagnostics** — a live view of
what your phone is sending, see [Troubleshooting](troubleshooting.md).

## If the address changes

The QR carries your Mac's current address, so if your router hands it a new one,
re-scan. A connect page left open notices the move and reloads itself, so the
code on screen is always the current one. Your phone stays **paired** across the
move — only the address changed.

---

**Next:** [Gestures](gestures.md) — the short version is that it behaves like
the trackpad you already have, because it reads your Mac's settings and copies
them.
