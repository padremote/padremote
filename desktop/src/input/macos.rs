//! macOS injection via Quartz CGEvent (plan.md section 9.10).
//!
//! Every event is posted to the HID event tap so it looks like it came from real
//! hardware and reaches every application.
//!
//! Mirrors `tools/proto/proto/inject_macos.py`, which is where the behaviour was
//! proven before this existed.

use core_graphics::display::CGDisplay;
use core_graphics::event::{
    CGEvent, CGEventFlags, CGEventTapLocation, CGEventType, CGMouseButton, EventField,
    ScrollEventUnit,
};
use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};
use core_graphics::geometry::CGPoint;

use core_foundation::base::TCFType;
use core_foundation::boolean::CFBoolean;
use core_foundation::dictionary::{CFDictionary, CFDictionaryRef};
use core_foundation::string::{CFString, CFStringRef};

use crate::gesture::{Button, ScrollPhase, Shortcut};

use super::Injector;

// Scroll event fields that core-graphics does not name. Setting these is what
// turns a synthetic wheel event into something macOS treats as a trackpad
// gesture - it is the difference between notched scrolling and smooth,
// rubber-banding scrolling.
/// `kCGScrollWheelEventIsContinuous` - pixel-precise rather than notched.
const FIELD_IS_CONTINUOUS: u32 = 88;
/// `kCGScrollWheelEventScrollPhase` - 1 began, 2 changed, 4 ended.
const FIELD_SCROLL_PHASE: u32 = 99;
/// `kCGScrollWheelEventMomentumPhase` - 0 none, 1 begin, 2 continue, 3 end.
const FIELD_MOMENTUM_PHASE: u32 = 123;

// The keys along the top of an Apple keyboard are not keystrokes at all. They
// arrive as `NSEventTypeSystemDefined` events carrying a button number, and
// macOS itself decides what each one means - which is why sending one of these
// both moves the slider *and* draws the panel on screen. The numbers are
// IOKit's, from `IOKit/hidsystem/ev_keymap.h`.
/// `NX_KEYTYPE_SOUND_UP`.
const NX_SOUND_UP: i16 = 0;
/// `NX_KEYTYPE_SOUND_DOWN`.
const NX_SOUND_DOWN: i16 = 1;
/// `NX_KEYTYPE_BRIGHTNESS_UP`.
const NX_BRIGHTNESS_UP: i16 = 2;
/// `NX_KEYTYPE_BRIGHTNESS_DOWN`.
const NX_BRIGHTNESS_DOWN: i16 = 3;
/// `NX_KEYTYPE_MUTE`.
const NX_MUTE: i16 = 7;

/// '=' -> Cmd+= zooms in.
const KEY_EQUAL: u16 = 24;
/// '-' -> Cmd+- zooms out.
const KEY_MINUS: u16 = 27;

// Arrow keys, for the Mission Control shortcuts a multi-finger swipe maps to.
const KEY_LEFT: u16 = 123;
const KEY_RIGHT: u16 = 124;
const KEY_DOWN: u16 = 125;
const KEY_UP: u16 = 126;
/// Show Desktop's default binding.
const KEY_F11: u16 = 103;
/// '[' and ']', for back and forward.
const KEY_LEFT_BRACKET: u16 = 33;
/// Virtual keycodes for the actions a gesture can be bound to. Layout
/// independent: these are physical positions, not the letters printed on them.
const KEY_SPACE: u16 = 49;
const KEY_TAB: u16 = 48;
const KEY_Q: u16 = 12;
const KEY_Z: u16 = 6;
const KEY_4: u16 = 21;
const KEY_A: u16 = 0;
const KEY_S: u16 = 1;
const KEY_F: u16 = 3;
const KEY_X: u16 = 7;
const KEY_C: u16 = 8;
const KEY_V: u16 = 9;
const KEY_W: u16 = 13;
const KEY_T: u16 = 17;
const KEY_M: u16 = 46;
const KEY_RIGHT_BRACKET: u16 = 30;

pub const ACCESSIBILITY_URL: &str =
    "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility";

// The only honest way to ask whether we may inject input. Creating a CGEvent
// source is NOT a proxy for it: that succeeds for any process, so using it made
// the app report itself healthy while macOS silently discarded every event.
#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    fn AXIsProcessTrusted() -> bool;
    fn AXIsProcessTrustedWithOptions(options: CFDictionaryRef) -> bool;
    static kAXTrustedCheckOptionPrompt: CFStringRef;
}

/// True when this process may inject input.
///
/// The grant is remembered per binary, so a `.app` bundle needs its own even on
/// a machine where the terminal already has one (plan.md section 11.1).
pub fn accessibility_trusted() -> bool {
    unsafe { AXIsProcessTrusted() }
}

/// Ask macOS to show its own "grant Accessibility" dialog, which offers the
/// user a button straight to the right settings pane.
///
/// Returns whether the permission is already granted; the dialog only appears
/// when it is not.
pub fn request_accessibility() -> bool {
    let key = unsafe { CFString::wrap_under_get_rule(kAXTrustedCheckOptionPrompt) };
    let options = CFDictionary::from_CFType_pairs(&[(key, CFBoolean::true_value())]);
    unsafe { AXIsProcessTrustedWithOptions(options.as_concrete_TypeRef()) }
}

pub fn permission_help() -> String {
    // The grant is per binary, so name which one actually needs enabling -
    // "enable PadRemote" is useless advice when the entry to tick is Terminal.
    let who = std::env::current_exe()
        .ok()
        .and_then(|p| p.file_name().map(|n| n.to_string_lossy().to_string()))
        .unwrap_or_else(|| "PadRemote".into());
    format!(
        "\n  PadRemote needs Accessibility permission to move the cursor.\n\
         \x20 Without it macOS accepts the events and quietly discards them.\n\n\
         \x20 Open:  System Settings > Privacy & Security > Accessibility\n\
         \x20 Enable: {who}   (the grant is remembered per app, so the one your\n\
         \x20         terminal may already have does not count for this one)\n\
         \x20 Deep link:  open '{ACCESSIBILITY_URL}'\n\n\
         \x20 PadRemote stays running while you do it and starts moving the cursor\n\
         \x20 the moment the box is ticked - no relaunch, no restart.\n"
    )
}

fn button_parts(button: Button) -> (CGEventType, CGEventType, CGEventType, CGMouseButton) {
    match button {
        Button::Left => (
            CGEventType::LeftMouseDown,
            CGEventType::LeftMouseUp,
            CGEventType::LeftMouseDragged,
            CGMouseButton::Left,
        ),
        Button::Right => (
            CGEventType::RightMouseDown,
            CGEventType::RightMouseUp,
            CGEventType::RightMouseDragged,
            CGMouseButton::Right,
        ),
        Button::Middle => (
            CGEventType::OtherMouseDown,
            CGEventType::OtherMouseUp,
            CGEventType::OtherMouseDragged,
            CGMouseButton::Center,
        ),
    }
}

pub struct MacInjector {
    /// The virtual cursor position.
    ///
    /// A CGEvent mouse event carries an absolute point, not a delta, so there
    /// has to be one - a relative move is this plus the delta, posted at the
    /// result. Which is also why it goes stale: nothing tells us when the user
    /// picks up the real trackpad, so `sync_cursor` re-reads it at the start of
    /// every gesture. Without that the first move of a gesture snaps the cursor
    /// back to wherever PadRemote last left it.
    x: f64,
    y: f64,
    bounds: (f64, f64, f64, f64),
    down: Vec<Button>,
}

impl MacInjector {
    pub fn new() -> anyhow::Result<Self> {
        let (x, y) = Self::current_location().unwrap_or((0.0, 0.0));
        Ok(Self {
            x,
            y,
            bounds: Self::screen_bounds(),
            down: Vec::new(),
        })
    }

    fn source() -> Option<CGEventSource> {
        CGEventSource::new(CGEventSourceStateID::HIDSystemState).ok()
    }

    pub fn current_location() -> Option<(f64, f64)> {
        let ev = CGEvent::new(Self::source()?).ok()?;
        let p = ev.location();
        Some((p.x, p.y))
    }

    /// Union of every active display, so the cursor can cross monitors.
    fn screen_bounds() -> (f64, f64, f64, f64) {
        let displays = CGDisplay::active_displays().unwrap_or_default();
        if displays.is_empty() {
            let b = CGDisplay::main().bounds();
            return (
                b.origin.x,
                b.origin.y,
                b.origin.x + b.size.width,
                b.origin.y + b.size.height,
            );
        }
        let (mut x0, mut y0) = (f64::INFINITY, f64::INFINITY);
        let (mut x1, mut y1) = (f64::NEG_INFINITY, f64::NEG_INFINITY);
        for id in displays {
            let b = CGDisplay::new(id).bounds();
            x0 = x0.min(b.origin.x);
            y0 = y0.min(b.origin.y);
            x1 = x1.max(b.origin.x + b.size.width);
            y1 = y1.max(b.origin.y + b.size.height);
        }
        (x0, y0, x1, y1)
    }

    fn clamp(&mut self) {
        let (x0, y0, x1, y1) = self.bounds;
        self.x = self.x.clamp(x0, x1 - 1.0);
        self.y = self.y.clamp(y0, y1 - 1.0);
    }

    /// Re-seed from the real cursor, in case the user touched the real trackpad.
    ///
    /// The screen rectangle is re-read at the same time. It is the other half of
    /// the same staleness: `bounds` was measured once, at launch, and a display
    /// plugged in since then leaves `clamp` pulling the cursor back inside a
    /// rectangle that no longer describes the desk.
    pub fn sync_from_system(&mut self) {
        if let Some((x, y)) = Self::current_location() {
            self.x = x;
            self.y = y;
        }
        self.bounds = Self::screen_bounds();
    }

    fn post_key(&self, key: u16, down: bool, flags: CGEventFlags) {
        let Some(src) = Self::source() else { return };
        if let Ok(ev) = CGEvent::new_keyboard_event(src, key, down) {
            ev.set_flags(flags);
            ev.post(CGEventTapLocation::HID);
        }
    }

    /// Tap a key with modifiers held down around it.
    ///
    /// The modifiers are posted as **real key events**, not merely as flags on
    /// the key event. macOS tracks modifier state from those events, so a
    /// synthetic Ctrl+Arrow carrying only a flag arrives at the WindowServer and
    /// at apps as a bare arrow - which is exactly how Mission Control and space
    /// switching came to do nothing at all.
    fn press(&self, key: u16, flags: CGEventFlags) {
        const MODIFIERS: [(CGEventFlags, u16); 4] = [
            (CGEventFlags::CGEventFlagCommand, 55),
            (CGEventFlags::CGEventFlagShift, 56),
            (CGEventFlags::CGEventFlagControl, 59),
            (CGEventFlags::CGEventFlagAlternate, 58),
        ];
        let held: Vec<u16> = MODIFIERS
            .iter()
            .filter(|(f, _)| flags.contains(*f))
            .map(|(_, k)| *k)
            .collect();

        for k in &held {
            self.post_key(*k, true, flags);
        }
        self.post_key(key, true, flags);
        self.post_key(key, false, flags);
        // Release in reverse, with the flag already cleared.
        for k in held.iter().rev() {
            self.post_key(*k, false, CGEventFlags::empty());
        }
    }

    fn post_mouse(
        &self,
        etype: CGEventType,
        button: CGMouseButton,
        click_state: i64,
        dx: i64,
        dy: i64,
    ) {
        let Some(src) = Self::source() else { return };
        let Ok(ev) = CGEvent::new_mouse_event(src, etype, CGPoint::new(self.x, self.y), button)
        else {
            return;
        };
        if click_state > 0 {
            ev.set_integer_value_field(EventField::MOUSE_EVENT_CLICK_STATE, click_state);
        }
        if dx != 0 || dy != 0 {
            // Delta fields matter to apps that read relative motion.
            ev.set_integer_value_field(EventField::MOUSE_EVENT_DELTA_X, dx);
            ev.set_integer_value_field(EventField::MOUSE_EVENT_DELTA_Y, dy);
        }
        ev.post(CGEventTapLocation::HID);
    }
}


/// Open an app by name, without waiting for it to finish opening.
///
/// `spawn` rather than `status`: this runs inline on the connection's own task,
/// inside the injection path, so waiting for `open` to exit would stall the
/// worker thread that is reading that phone's touches - for as long as macOS
/// takes to launch an application, which is not microseconds. Nothing here
/// reads the exit code.
fn launch(app: &str) {
    match std::process::Command::new("open").arg("-a").arg(app).spawn() {
        // Reaped on a thread of its own, or it stays a zombie for the life of
        // the app.
        Ok(mut child) => {
            std::thread::spawn(move || {
                let _ = child.wait();
            });
        }
        Err(e) => tracing::debug!("could not open {app}: {e}"),
    }
}

impl Injector for MacInjector {
    fn move_by(&mut self, dx: f64, dy: f64) {
        self.x += dx;
        self.y += dy;
        self.clamp();
        // While a button is held the event type must be *Dragged*, or apps will
        // not track the drag.
        let (etype, button) = match self.down.first() {
            Some(&b) => {
                let (_, _, drag, btn) = button_parts(b);
                (drag, btn)
            }
            None => (CGEventType::MouseMoved, CGMouseButton::Left),
        };
        self.post_mouse(etype, button, 0, dx as i64, dy as i64);
    }

    fn button_down(&mut self, button: Button, clicks: u8) {
        let (down, _, _, btn) = button_parts(button);
        // The click state travels with the press: a drag begun from a double
        // click selects by word, exactly as it would from a real trackpad.
        self.post_mouse(down, btn, clicks.max(1) as i64, 0, 0);
        if !self.down.contains(&button) {
            self.down.push(button);
        }
    }

    fn button_up(&mut self, button: Button) {
        let (_, up, _, btn) = button_parts(button);
        self.post_mouse(up, btn, 1, 0, 0);
        self.down.retain(|b| *b != button);
    }

    fn click(&mut self, button: Button, count: u8) {
        let (down, up, _, btn) = button_parts(button);
        self.post_mouse(down, btn, count as i64, 0, 0);
        self.post_mouse(up, btn, count as i64, 0, 0);
    }

    fn scroll_by(&mut self, dx: f64, dy: f64, phase: ScrollPhase) {
        let Some(src) = Self::source() else { return };
        // Pixel units keep scrolling smooth instead of line-quantised.
        let Ok(ev) =
            CGEvent::new_scroll_event(src, ScrollEventUnit::PIXEL, 2, dy as i32, dx as i32, 0)
        else {
            return;
        };
        ev.set_integer_value_field(FIELD_IS_CONTINUOUS, 1);
        let (scroll_phase, momentum_phase) = match phase {
            ScrollPhase::Begin => (1, 0),
            ScrollPhase::Continue => (2, 0),
            ScrollPhase::End => (4, 0),
            // While coasting, the scroll phase must be cleared and the momentum
            // phase set, or apps see two conflicting gestures.
            ScrollPhase::Momentum => (0, 2),
            ScrollPhase::MomentumEnd => (0, 3),
        };
        ev.set_integer_value_field(FIELD_SCROLL_PHASE, scroll_phase);
        ev.set_integer_value_field(FIELD_MOMENTUM_PHASE, momentum_phase);
        ev.post(CGEventTapLocation::HID);
    }

    /// App-level zoom: Cmd+= / Cmd+- (plan.md section 5 - the reliable backend).
    /// No OS exposes a universal synthetic magnify event, so v1 approximates.
    fn zoom(&mut self, steps: i32) {
        let key = if steps > 0 { KEY_EQUAL } else { KEY_MINUS };
        for _ in 0..steps.abs() {
            self.press(key, CGEventFlags::CGEventFlagCommand);
        }
    }

    /// Swipes become the standard Mission Control shortcuts.
    ///
    /// There is no public way to synthesize a real multi-finger swipe, so this
    /// sends the keystroke the gesture is bound to. That only works while those
    /// shortcuts are enabled - `sysprefs::macos::space_shortcuts_enabled`
    /// checks, so a user who turned them off is told rather than left guessing.
    fn shortcut(&mut self, shortcut: Shortcut) {
        use CGEventFlags as F;
        // Arrow keys must carry the flags a real keyboard sends. macOS classes
        // them as numeric-pad keys, and the WindowServer ignores a synthetic
        // Ctrl+Arrow that lacks NumericPad - which is why Mission Control and
        // space switching silently did nothing.
        let arrow = F::CGEventFlagControl | F::CGEventFlagNumericPad | F::CGEventFlagSecondaryFn;
        let (key, flags) = match shortcut {
            Shortcut::SpaceLeft => (KEY_LEFT, arrow),
            Shortcut::SpaceRight => (KEY_RIGHT, arrow),
            Shortcut::MissionControl => (KEY_UP, arrow),
            Shortcut::AppWindows => (KEY_DOWN, arrow),
            Shortcut::ShowDesktop => (KEY_F11, F::CGEventFlagSecondaryFn),
            Shortcut::Back => (KEY_LEFT_BRACKET, F::CGEventFlagCommand),
            Shortcut::Forward => (KEY_RIGHT_BRACKET, F::CGEventFlagCommand),
            // No public smart-zoom event exists, so approximate with app zoom.
            Shortcut::SmartZoom => (KEY_EQUAL, F::CGEventFlagCommand),
            // Volume and brightness are media keys, not keystrokes; see
            // `media_key`, which is also where the note about why this is not
            // `osascript` any more lives.
            Shortcut::VolumeUp => return media_key(NX_SOUND_UP),
            Shortcut::VolumeDown => return media_key(NX_SOUND_DOWN),
            Shortcut::BrightnessUp => return media_key(NX_BRIGHTNESS_UP),
            Shortcut::BrightnessDown => return media_key(NX_BRIGHTNESS_DOWN),
            Shortcut::ZoomIn => (KEY_EQUAL, F::CGEventFlagCommand),
            Shortcut::ZoomOut => (KEY_MINUS, F::CGEventFlagCommand),
            Shortcut::Undo => (KEY_Z, F::CGEventFlagCommand),
            Shortcut::Redo => (KEY_Z, F::CGEventFlagCommand | F::CGEventFlagShift),
            // Ctrl-Tab is the app's own tab cycle, not the window switcher.
            Shortcut::TabNext => (KEY_TAB, F::CGEventFlagControl),
            Shortcut::TabPrev => (KEY_TAB, F::CGEventFlagControl | F::CGEventFlagShift),
            // One press and release: the switcher shows and commits, which is
            // what a swipe or a tap can express. Holding it open would need the
            // gesture to still be in progress, and a tap has already ended.
            Shortcut::SwitchApps => (KEY_TAB, F::CGEventFlagCommand),
            Shortcut::Spotlight => (KEY_SPACE, F::CGEventFlagCommand),
            Shortcut::Screenshot => (KEY_4, F::CGEventFlagCommand | F::CGEventFlagShift),
            Shortcut::LockScreen => (KEY_Q, F::CGEventFlagCommand | F::CGEventFlagControl),
            Shortcut::Copy => (KEY_C, F::CGEventFlagCommand),
            Shortcut::Cut => (KEY_X, F::CGEventFlagCommand),
            Shortcut::Paste => (KEY_V, F::CGEventFlagCommand),
            Shortcut::SelectAll => (KEY_A, F::CGEventFlagCommand),
            Shortcut::Save => (KEY_S, F::CGEventFlagCommand),
            Shortcut::Find => (KEY_F, F::CGEventFlagCommand),
            Shortcut::NewTab => (KEY_T, F::CGEventFlagCommand),
            Shortcut::CloseWindow => (KEY_W, F::CGEventFlagCommand),
            Shortcut::MinimiseWindow => (KEY_M, F::CGEventFlagCommand),
            Shortcut::QuitApp => (KEY_Q, F::CGEventFlagCommand),
            Shortcut::FullScreen => (KEY_F, F::CGEventFlagCommand | F::CGEventFlagControl),
            Shortcut::Calculator => {
                // No hotkey to send; opening the app always works, as Launchpad
                // already does.
                launch("Calculator");
                return;
            }
            // The mute key is itself a toggle, so nothing has to read the
            // current state to work out which way to go - which is what the
            // scripting bridge was here for.
            Shortcut::Mute => return media_key(NX_MUTE),
            Shortcut::Launchpad => {
                // Launchpad's hotkey is often unassigned, and `open` always
                // works, so skip the keystroke entirely for this one.
                launch("Launchpad");
                return;
            }
        };
        self.press(key, flags);
    }

    fn release_all(&mut self) {
        for b in std::mem::take(&mut self.down) {
            let (_, up, _, btn) = button_parts(b);
            self.post_mouse(up, btn, 1, 0, 0);
        }
    }

    fn sync_cursor(&mut self) {
        self.sync_from_system();
    }
}

/// Press and release one of the keys along the top of an Apple keyboard.
///
/// These are not keystrokes and `CGEvent` cannot build one. A media key is an
/// `NSEventTypeSystemDefined` event with the `NX_SUBTYPE_AUX_CONTROL_BUTTONS`
/// subtype, and the button number is packed into `data1` next to a nibble
/// saying whether it is going down or coming back up. Only AppKit can construct
/// an event with a subtype, which is the entire reason `objc2` is a dependency
/// - one message, sent by hand, and then the resulting event goes through the
/// same HID tap as everything else in this file.
///
/// # Why not `osascript`
///
/// This used to be `set volume output volume (... + 6)`, with a comment
/// claiming the scripting bridge gave "the on-screen feedback". It does not.
/// That route moves the slider through CoreAudio, which changes the volume and
/// draws nothing - so a swipe made the sound quieter with no sign that anything
/// had happened, which reads as the gesture having failed rather than as the
/// volume having moved. Letting macOS handle the key means macOS draws its own
/// panel, the same one a real keyboard gets.
///
/// It was also slow in a way that mattered. `osascript` is a process launch,
/// and it was launched from `apply` - the thread working through touches - and
/// waited on. A tenth of a second, per step, on the path whose whole job is to
/// stay ahead of a finger. Posting an event takes microseconds.
///
/// Brightness goes through the same door and gets the same panel. There is no
/// public API that sets display brightness *and* shows the indicator; the key
/// is the indicator.
fn media_key(key: i16) {
    use objc2::encode::{Encode, Encoding, RefEncode};
    use objc2::rc::{autoreleasepool, Retained};
    use objc2::runtime::{AnyClass, AnyObject};

    // `CGPoint` from `core-graphics` cannot be sent as a message argument: the
    // runtime has to be told the shape of a struct it is putting on the stack,
    // and that crate has no reason to say. Two doubles named the way
    // Objective-C names them, which is all the encoding is.
    #[repr(C)]
    struct Point {
        x: f64,
        y: f64,
    }
    unsafe impl Encode for Point {
        const ENCODING: Encoding =
            Encoding::Struct("CGPoint", &[Encoding::Double, Encoding::Double]);
    }

    // `-[NSEvent CGEvent]` returns a `CGEventRef`, and the runtime checks that
    // claim against what the call site says it expects - a plain `*mut c_void`
    // is rejected outright, which is the runtime doing its job. This is the
    // same opaque pointer `core-graphics` wraps, spelled the way Objective-C
    // spells it; nothing is ever read through it.
    #[repr(C)]
    struct OpaqueCGEvent {
        _private: [u8; 0],
    }
    unsafe impl RefEncode for OpaqueCGEvent {
        const ENCODING_REF: Encoding = Encoding::Pointer(&Encoding::Struct("__CGEvent", &[]));
    }

    let Some(class) = AnyClass::get(c"NSEvent") else {
        return;
    };
    // Nothing here runs inside AppKit's own run loop, so the autoreleased event
    // has no pool to land in and the runtime would grumble on stderr about
    // leaking it. One pool around the pair is cheaper than either.
    autoreleasepool(|_| {
        // 0xa is "going down" and 0xb is "coming up", and each has to be said
        // twice: once in the modifier flags and once in `data1` beside the
        // button number. A press with no release leaves the key held as far as
        // macOS is concerned, which on a volume key means it keeps repeating.
        for phase in [0xa_isize, 0xb_isize] {
            let event: Option<Retained<AnyObject>> = unsafe {
                objc2::msg_send![
                    class,
                    otherEventWithType: 14_usize, // NSEventTypeSystemDefined
                    location: Point { x: 0.0, y: 0.0 },
                    modifierFlags: (phase as usize) << 8,
                    timestamp: 0.0_f64,
                    windowNumber: 0_isize,
                    context: std::ptr::null_mut::<AnyObject>(),
                    subtype: 8_i16, // NX_SUBTYPE_AUX_CONTROL_BUTTONS
                    data1: ((key as isize) << 16) | (phase << 8),
                    data2: -1_isize,
                ]
            };
            let Some(event) = event else { return };
            let cg: *mut OpaqueCGEvent = unsafe { objc2::msg_send![&*event, CGEvent] };
            if cg.is_null() {
                return;
            }
            // Posted rather than wrapped in `core_graphics::CGEvent`, whose
            // wrapper releases what it holds - and this pointer belongs to the
            // `NSEvent` it came from, which will release it itself.
            //
            // 0 is `kCGHIDEventTap`, written out rather than named through the
            // `core-graphics` enum so that this declaration says exactly what
            // the framework's own header says.
            unsafe { CGEventPost(0, cg.cast()) };
        }
    });
}

#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGEventPost(tap: u32, event: *mut std::ffi::c_void);
}
