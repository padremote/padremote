# Platform support

**PadRemote supports macOS only** — macOS 13 or later. **Linux and Windows are
not supported yet.** The app does not build for either: `desktop/build.rs` stops
any non-macOS target with a message that points here.

That is a statement about what has been proven, not about what is possible. The
architecture keeps the operating system at the edges, so a port remains a
bounded piece of work. This page is what that work would be.

## Why macOS only

For a while the repository also carried Windows and Linux halves — an
`enigo`-backed input backend (`input/portable.rs`) and settings readers for the
Precision Touchpad registry, GNOME and KDE (`sysprefs/windows.rs`,
`sysprefs/linux.rs`). They compiled, and their pure mappings were unit-tested,
but nobody ever ran them on the machines they were for. Code that has never
moved a cursor, described on the website as if it were a feature, was a promise
the project could not keep — so it was removed. It is still in git history:

```sh
git log --oneline -- desktop/src/input/portable.rs desktop/src/sysprefs/windows.rs desktop/src/sysprefs/linux.rs
```

Start there rather than from nothing, but treat it as a sketch.

## What would stay the same

Everything except the parts that call macOS directly. `gesture/`, `protocol`,
`net/` (apart from the `arp`/`ndp` probe in `net/neighbor.rs`), `cli`, the phone
page and the config are OS-agnostic — including the multi-device arbitration,
which is expressed entirely in terms of the `Injector` trait below.

What calls macOS today, and would need a counterpart:

| | macOS today |
|---|---|
| `input/` | `macos.rs` — CGEvent, pixel scroll with phases, click state, media keys |
| `sysprefs/` | `macos.rs` — CFPreferences, producing a `HostTrackpad` |
| `tray.rs` | the menu-bar item |
| `app.rs` | `local_ip()` through `ipconfig getifaddr`; the Accessibility watch |
| `lib.rs` | `host_name()` through `scutil` |
| `net/shared.rs` | the new-device notification, through `osascript` |
| `net/neighbor.rs` | `arp` / `ndp` for grouping a device's addresses |
| `install.sh`, `packaging/` | `~/Applications`, a LaunchAgent, ad-hoc code signing |

## The `Injector` trait

```rust
fn move_by(&mut self, dx: f64, dy: f64);
/// `clicks` is the click count the drag starts with: 2 makes macOS select by
/// word, which is what double-click-and-drag does.
fn button_down(&mut self, button: Button, clicks: u8);
fn button_up(&mut self, button: Button);
fn click(&mut self, button: Button, count: u8);
fn scroll_by(&mut self, dx: f64, dy: f64, phase: ScrollPhase);
fn zoom(&mut self, steps: i32);
fn shortcut(&mut self, shortcut: Shortcut);
fn release_all(&mut self);
/// Defaulted to nothing; only a backend that posts absolute positions
/// implements it. Called once, at the start of every gesture.
fn sync_cursor(&mut self) {}
```

`Shortcut` is deliberately semantic — `MissionControl`, `SpaceLeft`, `Launchpad`
— not a keystroke, so a backend maps it to its own system's equivalent.
`release_all` is not optional: a dropped connection must never leave a button
held, and with several devices connected it is also what a device's *departure*
triggers before the cursor is handed on.

`sync_cursor` is the one method with a default, and the question it answers is
worth asking of any new backend: **does it move the cursor by a delta, or by
naming a point?** A `CGEvent` mouse event carries an absolute point, so
`macos.rs` has to keep its own idea of where the cursor is — and nothing tells
it when a hand lands on the computer's own trackpad. Left unsynced, that idea
goes stale between gestures and the next move teleports the cursor back to
wherever PadRemote last drove it. An API that takes true deltas has nothing to
do here.

## What was learned about Linux and Windows

Notes from the removed code, kept because they cost time to find. None of it has
been confirmed on real hardware.

**Windows**

- Injection through `SendInput`. `enigo` scrolls in wheel notches, so touch
  deltas had to be accumulated and spent a notch at a time; a native backend
  using `WHEEL_DELTA` directly would scroll smoothly. There are no scroll
  phases to send.
- Settings live under
  `HKCU\SOFTWARE\Microsoft\Windows\CurrentVersion\PrecisionTouchPad` —
  `ScrollDirection`, `TapsEnabled`, `TwoFingerTapEnabled`,
  `ThreeFingerTapEnabled`, `PanEnabled`, `ZoomEnabled`, `CursorSpeed`, and the
  three/four-finger slide actions. Double-click speed is in `Control Panel\Mouse`,
  stored as a *string* of milliseconds. The polarity of `ScrollDirection` was
  the likeliest thing to be wrong.
- No permission prompt, but a window running as administrator ignores input
  from a process that is not.
- One overview (Task View) rather than Mission Control *and* App Exposé, and no
  brightness key — the settings page would have to offer fewer actions.

**Linux**

- Injection through `uinput` (works under Wayland) or `XTEST` (X11 only).
  `uinput` needs membership of the `input` group.
- Touchpad preferences belong to the desktop, not the kernel: GNOME answers
  through `gsettings get org.gnome.desktop.peripherals.touchpad …`, KDE keeps
  them in `~/.config/kcminputrc`. Anything else should report nothing and leave
  PadRemote's defaults standing.
- Workspace shortcuts differ by desktop (GNOME `Ctrl+Alt+Arrow`, KDE
  `Ctrl+F1..F4`), and `tray-icon` pulls in GTK, whose bindings `cargo deny`
  flags as unmaintained.

## Checklist for adding a platform

1. Lift the guard in `desktop/build.rs` for that target, and put the
   `#[cfg(target_os = …)]` selection back into `input/mod.rs`,
   `sysprefs/mod.rs` and every row of the table above.
2. `input/<platform>.rs` implementing `Injector`, and handle the permission
   model in `make_injector()` — refuse to run blind rather than injecting into
   the void. See the Accessibility entry in [gotchas](gotchas.md).
3. `sysprefs/<platform>.rs` filling whatever `HostTrackpad` fields the OS
   exposes, the rest `None`. Split it as a `Raw` struct of values exactly as the
   OS stores them, a pure `map()`, and a `read()` behind `#[cfg]`, so the
   mapping is testable from any machine. Give `report()` that system's own
   setting names.
4. Narrow `Config::vocabulary()` to what that system can actually do, and give
   the phone page that system's names for each action.
5. A CI job on that system, running `cargo test`. The engine suites are
   platform-independent and should pass unchanged — if they don't, something
   leaked out of `input/`.
6. Run it on real hardware, and only then change the README, `docs/README.md`
   and the website to say it is supported.
