//! Input injection: turning `InputAction`s into real OS input events.
//!
//! This is the ONLY platform-specific part of the app (plan.md section 16).
//! The gesture engine, the protocol, the config and the pairing flow are all
//! shared; a new platform implements this trait and nothing else.
//!
//! Windows will back this with `SendInput` and Linux with `uinput`/`XTEST`,
//! both reachable through `enigo`; macOS talks to CGEvent directly because it
//! needs pixel-precise scrolling and explicit click state, which the portable
//! layer does not expose.

use crate::gesture::{Button, InputAction, ScrollPhase, Shortcut};

pub trait Injector: Send {
    /// Move the cursor by a relative delta, in screen pixels.
    fn move_by(&mut self, dx: f64, dy: f64);
    /// `clicks` is the click count the drag starts with: 2 makes macOS
    /// select by word, which is what double-click-and-drag does.
    fn button_down(&mut self, button: Button, clicks: u8);
    fn button_up(&mut self, button: Button);
    /// A click carrying `count` so the OS recognises double- and triple-clicks.
    fn click(&mut self, button: Button, count: u8);
    /// Scroll by a pixel delta, tagged with where it sits in the gesture.
    /// The phase is what makes the OS treat this as a trackpad, not a wheel.
    fn scroll_by(&mut self, dx: f64, dy: f64, phase: ScrollPhase);
    /// Positive steps zoom in, negative zoom out.
    fn zoom(&mut self, steps: i32);
    /// A whole-desktop action from a multi-finger swipe.
    fn shortcut(&mut self, shortcut: Shortcut);
    /// Release everything currently held. Called whenever a session ends, so a
    /// dropped connection can never leave a button stuck down.
    fn release_all(&mut self);

    /// Find out where the cursor actually is, before a fresh gesture moves it.
    ///
    /// A backend that posts *absolute* positions has to be told. The user may
    /// have picked up the real trackpad since the last touch, and a backend
    /// still holding the position it last drove to would teleport the cursor
    /// back there on the first move of the next gesture - which is exactly what
    /// a phone that jumps rather than carries on looks like. Backends that send
    /// true relative deltas have nothing to do here, so this defaults to
    /// nothing.
    fn sync_cursor(&mut self) {}

    /// Why nothing handed to this backend will reach the screen, when that is
    /// the case. `None` - the normal answer - means input really is injected.
    ///
    /// The phone cannot work this out for itself, and that is the whole point:
    /// the socket is up, the gestures are recognised, the latency readout is
    /// live, and the cursor sits perfectly still. A backend that swallows
    /// everything has to say so, or the app looks broken in the one way that
    /// leaves the user nothing to try.
    fn blocked(&self) -> Option<Blocked> {
        None
    }

    fn apply(&mut self, action: InputAction) {
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

/// Why a computer is connected but moving nothing.
///
/// Two reasons, and they need different words on the phone: one is a box the
/// user has to tick, the other is a flag they asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Blocked {
    /// macOS has not granted Accessibility. It accepts every event this app
    /// posts and discards them all - see `macos::permission_help`.
    Permission,
    /// `--dry-run`: recognise everything, inject nothing, deliberately.
    DryRun,
}

impl Blocked {
    /// The code that goes on the wire, and that `ui.ts` switches on.
    pub fn code(self) -> &'static str {
        match self {
            Self::Permission => "permission",
            Self::DryRun => "dryRun",
        }
    }
}

#[cfg(target_os = "macos")]
pub mod macos;
#[cfg(not(target_os = "macos"))]
pub mod portable;

#[cfg(target_os = "macos")]
pub use macos::MacInjector as PlatformInjector;
#[cfg(target_os = "macos")]
pub use macos::{accessibility_trusted, permission_help, request_accessibility, MacInjector};

#[cfg(not(target_os = "macos"))]
pub use portable::PortableInjector as PlatformInjector;

/// Whether we are allowed to inject input at all.
///
/// macOS gates this behind an Accessibility grant and silently discards every
/// event until it is given, which is why the whole app is built around asking.
/// Windows needs nothing. Linux needs write access to `/dev/uinput`, which is a
/// file permission rather than a prompt - and one this cannot usefully test in
/// advance, because the failure only appears when the device is opened. So both
/// answer "yes, try it", and a real failure surfaces where it happens.
#[cfg(not(target_os = "macos"))]
pub fn accessibility_trusted() -> bool {
    true
}

#[cfg(not(target_os = "macos"))]
pub fn request_accessibility() -> bool {
    true
}

/// What to tell a user whose input is going nowhere.
///
/// Every platform gets an answer, because "no input backend" with no
/// explanation is the single least useful thing this app can say.
#[cfg(target_os = "windows")]
pub fn permission_help() -> String {
    "\nPadRemote could not open an input device.\n\
     \x20 Windows needs no permission for this, so the usual cause is another\n\
     \x20 program holding exclusive input, or PadRemote running with less\n\
     \x20 privilege than the window you are trying to control - a window run as\n\
     \x20 administrator ignores input from a program that is not.\n"
        .to_string()
}

#[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
pub fn permission_help() -> String {
    "\nPadRemote could not open an input device.\n\
     \x20 On Linux it writes through /dev/uinput, which usually means joining\n\
     \x20 the input group:\n\
     \x20\n\
     \x20   sudo usermod -aG input $USER\n\
     \x20\n\
     \x20 Then log out and back in - group membership is picked up at login.\n\
     \x20 On Wayland, X11-only fallbacks will not work at all; uinput does.\n"
        .to_string()
}

/// A backend that swallows everything, for tests and `--dry-run`.
///
/// It carries *why* it swallows: this is the backend the app runs on when
/// macOS has not granted Accessibility, and a phone driving a computer in that
/// state has to be told rather than left to conclude the app is broken.
pub struct NullInjector(Blocked);

impl NullInjector {
    pub fn new(why: Blocked) -> Self {
        Self(why)
    }
}

impl Injector for NullInjector {
    fn blocked(&self) -> Option<Blocked> {
        Some(self.0)
    }

    fn move_by(&mut self, _dx: f64, _dy: f64) {}
    fn button_down(&mut self, _button: Button, _clicks: u8) {}
    fn button_up(&mut self, _button: Button) {}
    fn click(&mut self, _button: Button, _count: u8) {}
    fn scroll_by(&mut self, _dx: f64, _dy: f64, _phase: ScrollPhase) {}
    fn zoom(&mut self, _steps: i32) {}
    fn shortcut(&mut self, _shortcut: Shortcut) {}
    fn release_all(&mut self) {}
}
