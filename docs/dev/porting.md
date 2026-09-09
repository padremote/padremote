# Porting to another platform

The architecture was always ready. The Windows and Linux halves are now
**written but unproven**: they compile, their pure parts are unit-tested from
any machine, and the Windows build is cross-checked on every CI run — but nobody
has yet run either on the hardware it is for.

If you have such a machine, this page is the list of what to check first.

## What is written, and what to confirm

| | Status |
|---|---|
| `input/portable.rs` | Written against `enigo`. Compile-checked for `x86_64-pc-windows-msvc`. **Never run.** |
| `sysprefs/windows.rs` | Reads the Precision Touchpad registry keys. Mapping unit-tested; the registry read itself never run. |
| `sysprefs/linux.rs` | GNOME through `gsettings`, KDE through `~/.config/kcminputrc`. Parsers unit-tested; neither desktop tried. |

**The one thing most likely to be wrong** is `ScrollDirection` on Windows. This
code reads `0` as natural scrolling — content follows the fingers. If scrolling
runs backwards on a real machine, that polarity is the reason, and
`sysprefs/windows.rs` has a test written to be flipped in one line. Run
`padremote --headless` first: the mirror report prints every value it read
beside what PadRemote did with it.

Two limits of the portable backend are inherent rather than bugs:

- **Scrolling is notch-quantised**, not pixel-precise. `enigo` scrolls in wheel
  notches; touch deltas are accumulated and spent a notch at a time
  (`PIXELS_PER_NOTCH`). A native `SendInput` backend using `WHEEL_DELTA`
  directly would fix it.
- **Gesture phases are dropped.** Nothing outside macOS has a notion of a scroll
  that has begun and not yet ended, so the phase is used only to flush the
  accumulator when the fingers lift.

## What was already portable

Everything except two modules. `gesture/`, `protocol`, `net/`, `app`, `cli`, the
phone page and the config are all OS-agnostic and stay untouched — including the
multi-device arbitration, which is expressed entirely in terms of the `Injector`
trait below.

- **`input/`** — an `Injector` trait. Implement it and nothing else changes.
- **`sysprefs/`** — produce a `HostTrackpad`. It is plain data with an
  `Option` per setting, and `HostTrackpad::read()` already has the `#[cfg]`
  split, returning an empty reading off macOS. An empty reading is safe: every
  field falls back to PadRemote's own default.

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
— not a keystroke, so each platform maps it to its own equivalent. `release_all`
is not optional: a dropped connection must never leave a button held, and with
several devices connected it is also what a device's *departure* triggers before
the cursor is handed on.

`sync_cursor` is the one method with a default, and the question it answers is
worth asking of any new platform: **does this backend move the cursor by a
delta, or by naming a point?** `enigo`'s `Coordinate::Rel` is a true delta, so
`portable.rs` implements nothing and there is nothing to go wrong. A `CGEvent`
mouse event carries an absolute point, so `macos.rs` has to keep its own idea of
where the cursor is — and nothing tells it when a hand lands on the computer's
own trackpad. Left unsynced, that idea goes stale between gestures and the next
move teleports the cursor back to wherever PadRemote last drove it. If a
platform's injection API takes coordinates, it needs this; if it takes deltas,
it does not.

## Windows

- **Injection**: `enigo` (`SendInput` underneath), in `input/portable.rs`.
- **Settings**: `sysprefs/windows.rs`, from
  `HKCU\SOFTWARE\Microsoft\Windows\CurrentVersion\PrecisionTouchPad` —
  `ScrollDirection`, `TapsEnabled`, `TwoFingerTapEnabled`,
  `ThreeFingerTapEnabled`, `PanEnabled`, `ZoomEnabled`, `CursorSpeed`, and the
  three/four-finger slide actions. Double-click speed comes from
  `Control Panel\Mouse` and is stored as a *string* of milliseconds.
- **Shortcuts**: `Win+Ctrl+Arrow` for virtual desktops, `Win+Tab` for Task View,
  `Win+D` for the desktop, `Alt+Arrow` for back and forward.
- **Permissions**: none needed. A window running as administrator will ignore
  input from a PadRemote that is not, which `permission_help()` says.
- **Zoom**: the same wall as macOS — no synthetic magnify. `Ctrl`+wheel.

## Linux

- **Injection**: `enigo` (`uinput` under Wayland, `XTEST` on X11).
- **Settings**: `sysprefs/linux.rs` asks the desktop, because the kernel and
  libinput know nothing about user preference. GNOME answers through
  `gsettings get org.gnome.desktop.peripherals.touchpad …`; KDE keeps the same
  choices in `~/.config/kcminputrc` as INI, one section per device. Anything
  else reports nothing and PadRemote's own defaults stand — which is the right
  outcome, not a failure.
- **Shortcuts**: GNOME's defaults (`Ctrl+Alt+Arrow`, `Super+W`, `Super+A`). KDE
  uses `Ctrl+F1..F4` for desktops, which no single mapping can cover; the config
  file is the escape hatch.
- **Permissions**: membership of the `input` group for `uinput`.
  `permission_help()` prints the one-line fix.
- **Building**: `enigo` pulls in X11 and D-Bus native dependencies, so a Linux
  build needs their `-dev` packages. That is also why Linux cannot be
  cross-checked from macOS the way Windows can.

## Checklist for a *new* platform

1. `input/<platform>.rs` implementing `Injector`; wire it into `input/mod.rs`'s
   `#[cfg]` selection. `portable.rs` may already cover it.
2. `sysprefs/<platform>.rs` filling whatever `HostTrackpad` fields the OS
   exposes; leave the rest `None`. **Split it the way the existing two are**: a
   `Raw` struct of the values exactly as the OS stores them, a pure `map()`, and
   a `read()` behind `#[cfg]`. The pure half is then testable from any machine,
   which is the only reason the Windows and Linux mappings have tests at all.
3. Add the platform's key names to `report()`'s `key()` calls, so the mirror
   report names *that* machine's settings rather than macOS preference keys.
4. Handle the permission model in `make_injector()`, refusing to run blind rather
   than injecting into the void. See the Accessibility entry in
   [gotchas](gotchas.md) for why that matters.
5. Run `cargo test`. The engine suites are platform-independent and should pass
   unchanged — if they don't, something leaked out of `input/`.
