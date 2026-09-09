//! Windows and Linux input, through `enigo`.
//!
//! macOS talks to CGEvent directly because it needs pixel-precise scrolling and
//! explicit click state, neither of which a portable layer exposes. Everywhere
//! else `enigo` is the right trade: `SendInput` on Windows, `uinput`/`XTEST` on
//! Linux, one API, and nothing for this project to maintain per display server.
//!
//! Three things it cannot do, and how each is handled rather than hidden:
//!
//! - **Pixel scrolling.** `enigo` scrolls in notches, the unit a mouse wheel
//!   uses, so a touch surface's pixel deltas are accumulated here and spent a
//!   notch at a time. Scrolling is therefore steppier than on macOS. Nothing in
//!   this crate can fix that; a per-platform backend using `SendInput`'s
//!   `WHEEL_DELTA` directly could.
//! - **Gesture phases.** Windows and X11 have no notion of a scroll that has
//!   begun and not yet ended, so the phase is used only to flush what is left
//!   in the accumulator when the fingers lift.
//! - **Click counts.** There is no synthetic double-click flag; a double click
//!   is two clicks close together, which is what the OS is looking for anyway.
//!
//! **This code has never run.** It is written against `enigo`'s API and
//! compile-checked for `x86_64-pc-windows-msvc`, but nobody has yet tried it on
//! a real Windows or Linux machine - see `docs/dev/porting.md` before trusting
//! any of the mappings below.

use enigo::{
    Axis, Button as EButton, Coordinate, Direction, Enigo, Key, Keyboard, Mouse, Settings,
};

use crate::gesture::{Button, InputAction, ScrollPhase, Shortcut};

use super::Injector;

/// Surface pixels per wheel notch.
///
/// A notch is a big, coarse unit - three lines of text in most applications -
/// and a touch surface produces a few pixels per frame. Too small a number here
/// makes a gentle scroll leap a page at a time; too large and slow scrolling
/// does nothing at all. Roughly a finger-width of travel per notch.
const PIXELS_PER_NOTCH: f64 = 40.0;

pub struct PortableInjector {
    enigo: Enigo,
    /// Left-over pixels, kept between calls so slow scrolling still adds up.
    scroll_x: f64,
    scroll_y: f64,
    /// Buttons we have pressed and not yet released.
    held: Vec<Button>,
}

impl PortableInjector {
    pub fn new() -> anyhow::Result<Self> {
        let enigo = Enigo::new(&Settings::default())
            .map_err(|e| anyhow::anyhow!("could not open an input device: {e}"))?;
        Ok(Self {
            enigo,
            scroll_x: 0.0,
            scroll_y: 0.0,
            held: Vec::new(),
        })
    }

    /// Press, then release, a key with modifiers held around it.
    fn chord(&mut self, mods: &[Key], key: Key) {
        for m in mods {
            let _ = self.enigo.key(*m, Direction::Press);
        }
        let _ = self.enigo.key(key, Direction::Click);
        // Released in reverse order, the way a hand would leave a chord.
        for m in mods.iter().rev() {
            let _ = self.enigo.key(*m, Direction::Release);
        }
    }
}

fn button(b: Button) -> EButton {
    match b {
        Button::Left => EButton::Left,
        Button::Right => EButton::Right,
        Button::Middle => EButton::Middle,
    }
}

impl Injector for PortableInjector {
    fn move_by(&mut self, dx: f64, dy: f64) {
        // Rounded, not truncated: a slow drag is a long run of sub-pixel deltas,
        // and truncation would round every one of them to nothing.
        let (x, y) = (dx.round() as i32, dy.round() as i32);
        if x == 0 && y == 0 {
            return;
        }
        let _ = self.enigo.move_mouse(x, y, Coordinate::Rel);
    }

    fn button_down(&mut self, b: Button, clicks: u8) {
        // No synthetic click count exists here. A drag that macOS would start
        // with a double click - selecting by word - is approximated by clicking
        // once first, which is what puts most applications into that mode.
        if clicks >= 2 {
            let _ = self.enigo.button(button(b), Direction::Click);
        }
        if self.enigo.button(button(b), Direction::Press).is_ok() && !self.held.contains(&b) {
            self.held.push(b);
        }
    }

    fn button_up(&mut self, b: Button) {
        let _ = self.enigo.button(button(b), Direction::Release);
        self.held.retain(|h| *h != b);
    }

    fn click(&mut self, b: Button, count: u8) {
        for _ in 0..count.max(1) {
            let _ = self.enigo.button(button(b), Direction::Click);
        }
    }

    fn scroll_by(&mut self, dx: f64, dy: f64, phase: ScrollPhase) {
        self.scroll_x += dx;
        self.scroll_y += dy;
        let notches_x = (self.scroll_x / PIXELS_PER_NOTCH).trunc();
        let notches_y = (self.scroll_y / PIXELS_PER_NOTCH).trunc();
        self.scroll_x -= notches_x * PIXELS_PER_NOTCH;
        self.scroll_y -= notches_y * PIXELS_PER_NOTCH;

        if notches_y != 0.0 {
            let _ = self.enigo.scroll(notches_y as i32, Axis::Vertical);
        }
        if notches_x != 0.0 {
            let _ = self.enigo.scroll(notches_x as i32, Axis::Horizontal);
        }

        // The gesture is over, so nothing is coming to carry the remainder.
        // Dropping it silently loses the tail of every short scroll.
        if matches!(phase, ScrollPhase::End | ScrollPhase::MomentumEnd) {
            let (rx, ry) = (self.scroll_x, self.scroll_y);
            self.scroll_x = 0.0;
            self.scroll_y = 0.0;
            if ry.abs() > PIXELS_PER_NOTCH / 2.0 {
                let _ = self.enigo.scroll(ry.signum() as i32, Axis::Vertical);
            }
            if rx.abs() > PIXELS_PER_NOTCH / 2.0 {
                let _ = self.enigo.scroll(rx.signum() as i32, Axis::Horizontal);
            }
        }
    }

    fn zoom(&mut self, steps: i32) {
        // Ctrl and the wheel: the one zoom gesture every desktop application
        // agrees on. No OS exposes a synthetic magnify event.
        let _ = self.enigo.key(Key::Control, Direction::Press);
        let _ = self.enigo.scroll(-steps, Axis::Vertical);
        let _ = self.enigo.key(Key::Control, Direction::Release);
    }

    fn shortcut(&mut self, shortcut: Shortcut) {
        // Semantic in, keystrokes out - the same shape as the macOS backend,
        // with each platform's own equivalents.
        #[cfg(target_os = "windows")]
        match shortcut {
            // Win+Ctrl+Arrow moves between virtual desktops.
            Shortcut::SpaceLeft => self.chord(&[Key::Meta, Key::Control], Key::LeftArrow),
            Shortcut::SpaceRight => self.chord(&[Key::Meta, Key::Control], Key::RightArrow),
            // Task View is the closest thing to Mission Control.
            // Windows has one overview, not Mission Control *and* App Expose;
            // `vocabulary` only offers the one here.
            Shortcut::MissionControl | Shortcut::AppWindows => self.chord(&[Key::Meta], Key::Tab),
            // The Start menu, which is what Launchpad is for. Search is its own
            // action below - they were both Win+S, so the menu offered the same
            // keystroke twice under two names.
            Shortcut::Launchpad => self.chord(&[], Key::Meta),
            Shortcut::ShowDesktop => self.chord(&[Key::Meta], Key::Unicode('d')),
            Shortcut::Back => self.chord(&[Key::Alt], Key::LeftArrow),
            Shortcut::Forward => self.chord(&[Key::Alt], Key::RightArrow),
            // No smart zoom anywhere but macOS; Ctrl+0 resets, which is the
            // nearest useful thing a double tap can mean.
            Shortcut::SmartZoom => self.chord(&[Key::Control], Key::Unicode('0')),
            Shortcut::VolumeUp => self.chord(&[], Key::VolumeUp),
            Shortcut::VolumeDown => self.chord(&[], Key::VolumeDown),
            Shortcut::Mute => self.chord(&[], Key::VolumeMute),
            // No brightness key here either; see the Windows arm above.
            Shortcut::BrightnessUp | Shortcut::BrightnessDown => {}
            // Brightness has no key here. Windows has no virtual keycode for
            // it at all, and enigo's `BrightnessUp`/`Down` are macOS-only - so
            // there is nothing to send, and sending the wrong thing would be
            // worse than sending nothing. It is left out of the vocabulary the
            // settings page is built from on these platforms (see
            // `gesture/config.rs`), so the only way to arrive here is a config
            // file edited by hand.
            Shortcut::BrightnessUp | Shortcut::BrightnessDown => {}
            Shortcut::ZoomIn => self.chord(&[Key::Control], Key::Unicode('=')),
            Shortcut::ZoomOut => self.chord(&[Key::Control], Key::Unicode('-')),
            Shortcut::Undo => self.chord(&[Key::Control], Key::Unicode('z')),
            Shortcut::Redo => self.chord(&[Key::Control, Key::Shift], Key::Unicode('z')),
            Shortcut::TabNext => self.chord(&[Key::Control], Key::Tab),
            Shortcut::TabPrev => self.chord(&[Key::Control, Key::Shift], Key::Tab),
            Shortcut::SwitchApps => self.chord(&[Key::Alt], Key::Tab),
            Shortcut::Spotlight => self.chord(&[Key::Meta], Key::Unicode('s')),
            Shortcut::Screenshot => self.chord(&[Key::Meta, Key::Shift], Key::Unicode('s')),
            Shortcut::LockScreen => self.chord(&[Key::Meta], Key::Unicode('l')),
            Shortcut::Copy => self.chord(&[Key::Control], Key::Unicode('c')),
            Shortcut::Cut => self.chord(&[Key::Control], Key::Unicode('x')),
            Shortcut::Paste => self.chord(&[Key::Control], Key::Unicode('v')),
            Shortcut::SelectAll => self.chord(&[Key::Control], Key::Unicode('a')),
            Shortcut::Save => self.chord(&[Key::Control], Key::Unicode('s')),
            Shortcut::Find => self.chord(&[Key::Control], Key::Unicode('f')),
            Shortcut::NewTab => self.chord(&[Key::Control], Key::Unicode('t')),
            Shortcut::CloseWindow => self.chord(&[Key::Control], Key::Unicode('w')),
            Shortcut::MinimiseWindow => self.chord(&[Key::Meta], Key::DownArrow),
            Shortcut::QuitApp => self.chord(&[Key::Alt], Key::F4),
            Shortcut::FullScreen => self.chord(&[], Key::F11),
            Shortcut::Calculator => self.chord(&[], Key::F13),
        }

        #[cfg(not(any(target_os = "windows", target_os = "macos")))]
        match shortcut {
            // GNOME's defaults. KDE uses Ctrl+F1..F4 for desktops, which no
            // single mapping can cover - the config file is the escape hatch.
            Shortcut::SpaceLeft => self.chord(&[Key::Control, Key::Alt], Key::LeftArrow),
            Shortcut::SpaceRight => self.chord(&[Key::Control, Key::Alt], Key::RightArrow),
            Shortcut::MissionControl | Shortcut::AppWindows => {
                self.chord(&[Key::Meta], Key::Unicode('w'))
            }
            Shortcut::Launchpad => self.chord(&[Key::Meta], Key::Unicode('a')),
            Shortcut::ShowDesktop => self.chord(&[Key::Control, Key::Alt], Key::Unicode('d')),
            Shortcut::Back => self.chord(&[Key::Alt], Key::LeftArrow),
            Shortcut::Forward => self.chord(&[Key::Alt], Key::RightArrow),
            Shortcut::SmartZoom => self.chord(&[Key::Control], Key::Unicode('0')),
            Shortcut::VolumeUp => self.chord(&[], Key::VolumeUp),
            Shortcut::VolumeDown => self.chord(&[], Key::VolumeDown),
            Shortcut::Mute => self.chord(&[], Key::VolumeMute),
            Shortcut::ZoomIn => self.chord(&[Key::Control], Key::Unicode('=')),
            Shortcut::ZoomOut => self.chord(&[Key::Control], Key::Unicode('-')),
            Shortcut::Undo => self.chord(&[Key::Control], Key::Unicode('z')),
            Shortcut::Redo => self.chord(&[Key::Control, Key::Shift], Key::Unicode('z')),
            Shortcut::TabNext => self.chord(&[Key::Control], Key::Tab),
            Shortcut::TabPrev => self.chord(&[Key::Control, Key::Shift], Key::Tab),
            Shortcut::SwitchApps => self.chord(&[Key::Alt], Key::Tab),
            Shortcut::Spotlight => self.chord(&[Key::Meta], Key::Unicode('a')),
            Shortcut::Screenshot => self.chord(&[], Key::Print),
            Shortcut::LockScreen => self.chord(&[Key::Meta], Key::Unicode('l')),
            Shortcut::Copy => self.chord(&[Key::Control], Key::Unicode('c')),
            Shortcut::Cut => self.chord(&[Key::Control], Key::Unicode('x')),
            Shortcut::Paste => self.chord(&[Key::Control], Key::Unicode('v')),
            Shortcut::SelectAll => self.chord(&[Key::Control], Key::Unicode('a')),
            Shortcut::Save => self.chord(&[Key::Control], Key::Unicode('s')),
            Shortcut::Find => self.chord(&[Key::Control], Key::Unicode('f')),
            Shortcut::NewTab => self.chord(&[Key::Control], Key::Unicode('t')),
            Shortcut::CloseWindow => self.chord(&[Key::Control], Key::Unicode('w')),
            Shortcut::MinimiseWindow => self.chord(&[Key::Meta], Key::Unicode('h')),
            Shortcut::QuitApp => self.chord(&[Key::Alt], Key::F4),
            Shortcut::FullScreen => self.chord(&[], Key::F11),
            Shortcut::Calculator => self.chord(&[], Key::F13),
        }
    }

    fn release_all(&mut self) {
        for b in std::mem::take(&mut self.held) {
            let _ = self.enigo.button(button(b), Direction::Release);
        }
        self.scroll_x = 0.0;
        self.scroll_y = 0.0;
    }

    fn apply(&mut self, action: InputAction) {
        // The default implementation, but going through this type's own methods
        // so the held-button bookkeeping above is never bypassed.
        match action {
            InputAction::Move { dx, dy, .. } => self.move_by(dx, dy),
            InputAction::Click { button, count, .. } => self.click(button, count),
            InputAction::ButtonDown { button, clicks } => self.button_down(button, clicks),
            InputAction::ButtonUp(b) => self.button_up(b),
            InputAction::Scroll { dx, dy, phase, .. } => self.scroll_by(dx, dy, phase),
            InputAction::Zoom { steps, .. } => self.zoom(steps),
            InputAction::Shortcut { shortcut, .. } => self.shortcut(shortcut),
        }
    }
}
