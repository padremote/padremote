//! Gesture engine: touch samples in, `InputAction`s out (plan.md section 9).
//!
//! Pure and deterministic. No I/O, no platform calls, no clock of its own -
//! every decision is a function of the samples fed in and the timestamps they
//! carry. That is what makes it testable from recorded streams and what lets
//! Windows and Linux reuse it untouched (section 16).
//!
//! State machine (section 9.8):
//!
//! ```text
//! IDLE -1 down-> PENDING -move-> MOVING ---------------------+
//!                       \-hold>pressMs-> DRAG                |
//!      -2 down-> TWO_PENDING -parallel move-> SCROLL         | all up
//!                           \-distance change-> ZOOM         |
//!                           \-quick up-> RIGHTCLICK          |
//! IDLE -1 down/up quick-> TAP -> click                       |
//! MOVING -2nd finger down-> TWO_PENDING (re-anchored)        |
//! any <------------------------------------------------------+
//!     (all fingers up -> release held buttons -> IDLE)
//! ```
//!
//! This is a port of `tools/proto/proto/recognizer.py`, kept expression-for-
//! expression identical so both implementations agree on the shared fixtures in
//! `tests/fixtures/`.

pub mod accel;
pub mod config;

pub use config::Config;

/// Touch phases, matching the wire protocol.
pub const DOWN: u8 = 0;
pub const MOVE: u8 = 1;
pub const UP: u8 = 2;
pub const CANCEL: u8 = 3;

/// Fingers landing within this window are one gesture, not a sequence.
///
/// Generous on purpose. Four fingers never land together on a phone the way
/// they do on a laptop trackpad - the spread between index and little finger
/// routinely puts the last one 80-150 ms behind the first, and a tighter window
/// silently demoted four-finger gestures to three.
const GROUP_WINDOW_MS: u32 = 160;

/// Momentum scroll decay per 1/60 s once the fingers lift.
///
/// A real trackpad coasts for a long time; 0.94 stopped almost immediately and
/// felt like dragging a page rather than flicking it.
const MOMENTUM_DECAY: f64 = 0.972;
/// Below this launch speed (surface px/s) a scroll simply stops.
const MOMENTUM_MIN_LAUNCH: f64 = 90.0;
/// Coasting ends once it slows to this (surface px/s).
const MOMENTUM_STOP: f64 = 4.0;

/// Speed at which scroll acceleration reaches its knee, in surface px/s.
const SCROLL_SPEED_REF: f64 = 700.0;
/// Gain for a slow, deliberate scroll: close to one-to-one with the finger.
const SCROLL_BASE: f64 = 0.85;
/// Ceiling, so a fast flick covers pages without becoming uncontrollable.
const SCROLL_MAX: f64 = 6.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TouchSample {
    pub t_ms: u32,
    pub pointer_id: u8,
    pub phase: u8,
    /// Normalized 0-1 across the surface.
    pub x: f32,
    pub y: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Button {
    Left,
    Right,
    Middle,
}

impl Shortcut {
    /// The action a *tap* can be bound to.
    ///
    /// Only the ones that mean something on their own: a swipe binds a pair
    /// (louder and quieter, back and forward), and half of a pair is not a
    /// thing a single tap can express.
    pub fn from_binding(binding: &str) -> Option<Self> {
        Some(match binding {
            "missionControl" => Self::MissionControl,
            "appWindows" => Self::AppWindows,
            "showDesktop" => Self::ShowDesktop,
            "launchpad" => Self::Launchpad,
            "switchApps" => Self::SwitchApps,
            "spotlight" => Self::Spotlight,
            "screenshot" => Self::Screenshot,
            "lockScreen" => Self::LockScreen,
            "mute" => Self::Mute,
            "smartZoom" => Self::SmartZoom,
            "desktopLeft" => Self::SpaceLeft,
            "desktopRight" => Self::SpaceRight,
            "volumeUp" => Self::VolumeUp,
            "volumeDown" => Self::VolumeDown,
            "brightnessUp" => Self::BrightnessUp,
            "brightnessDown" => Self::BrightnessDown,
            "zoomIn" => Self::ZoomIn,
            "zoomOut" => Self::ZoomOut,
            "previousTab" => Self::TabPrev,
            "nextTab" => Self::TabNext,
            "undo" => Self::Undo,
            "redo" => Self::Redo,
            "back" => Self::Back,
            "forward" => Self::Forward,
            "copy" => Self::Copy,
            "cut" => Self::Cut,
            "paste" => Self::Paste,
            "selectAll" => Self::SelectAll,
            "save" => Self::Save,
            "find" => Self::Find,
            "newTab" => Self::NewTab,
            "closeWindow" => Self::CloseWindow,
            "minimiseWindow" => Self::MinimiseWindow,
            "quitApp" => Self::QuitApp,
            "fullScreen" => Self::FullScreen,
            "calculator" => Self::Calculator,
            _ => return None,
        })
    }
}

impl Button {
    fn from_binding(binding: &str) -> Option<Self> {
        match binding {
            "leftClick" => Some(Self::Left),
            "rightClick" => Some(Self::Right),
            "middleClick" => Some(Self::Middle),
            _ => None, // "none", or anything unrecognised, binds to nothing
        }
    }
}

/// Where a scroll event sits in the life of the gesture.
///
/// This is what separates "a real trackpad" from "a mouse wheel". macOS only
/// gives smooth scrolling, rubber-banding and overscroll to wheel events that
/// carry a gesture phase; a phaseless event is treated as a notched wheel, and
/// that is exactly what feels wrong.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScrollPhase {
    Begin,
    Continue,
    End,
    /// Coasting after the fingers left.
    Momentum,
    MomentumEnd,
}

impl ScrollPhase {
    /// Tag used by the shared fixtures.
    pub fn name(self) -> &'static str {
        match self {
            Self::Begin => "begin",
            Self::Continue => "continue",
            Self::End => "end",
            Self::Momentum => "momentum",
            Self::MomentumEnd => "momentumEnd",
        }
    }
}

/// A whole-desktop action a multi-finger swipe asks for.
///
/// Semantic rather than a keystroke: the engine says what the user meant, and
/// each platform's injector decides how to produce it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shortcut {
    /// Move one space/desktop to the left.
    SpaceLeft,
    SpaceRight,
    MissionControl,
    /// All windows of the current application.
    AppWindows,
    Launchpad,
    ShowDesktop,
    /// Back / forward between pages.
    Back,
    Forward,
    /// Double-tap zoom.
    SmartZoom,
    /// System output volume.
    VolumeUp,
    VolumeDown,
    Mute,
    /// Display brightness. macOS only: it is a media key there, and neither
    /// Windows nor Linux has a keystroke that any machine reliably answers.
    BrightnessUp,
    BrightnessDown,
    /// Application zoom, as a pair a swipe can drive.
    ZoomIn,
    ZoomOut,
    Undo,
    Redo,
    /// Previous / next tab in the frontmost app.
    TabPrev,
    TabNext,
    /// Hold-free application switch.
    SwitchApps,
    Spotlight,
    Screenshot,
    LockScreen,
    Copy,
    Cut,
    Paste,
    SelectAll,
    Save,
    Find,
    NewTab,
    CloseWindow,
    MinimiseWindow,
    QuitApp,
    FullScreen,
    Calculator,
}

impl Shortcut {
    pub fn name(self) -> &'static str {
        match self {
            Self::SpaceLeft => "spaceLeft",
            Self::SpaceRight => "spaceRight",
            Self::MissionControl => "missionControl",
            Self::AppWindows => "appWindows",
            Self::Launchpad => "launchpad",
            Self::ShowDesktop => "showDesktop",
            Self::Back => "back",
            Self::Forward => "forward",
            Self::SmartZoom => "smartZoom",
            Self::VolumeUp => "volumeUp",
            Self::VolumeDown => "volumeDown",
            Self::Mute => "mute",
            Self::BrightnessUp => "brightnessUp",
            Self::BrightnessDown => "brightnessDown",
            Self::ZoomIn => "zoomIn",
            Self::ZoomOut => "zoomOut",
            Self::Undo => "undo",
            Self::Redo => "redo",
            Self::TabPrev => "tabPrev",
            Self::TabNext => "tabNext",
            Self::SwitchApps => "switchApps",
            Self::Spotlight => "spotlight",
            Self::Screenshot => "screenshot",
            Self::LockScreen => "lockScreen",
            Self::Copy => "copy",
            Self::Cut => "cut",
            Self::Paste => "paste",
            Self::SelectAll => "selectAll",
            Self::Save => "save",
            Self::Find => "find",
            Self::NewTab => "newTab",
            Self::CloseWindow => "closeWindow",
            Self::MinimiseWindow => "minimiseWindow",
            Self::QuitApp => "quitApp",
            Self::FullScreen => "fullScreen",
            Self::Calculator => "calculator",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum InputAction {
    Move {
        dx: f64,
        dy: f64,
        t_ms: u32,
    },
    Click {
        button: Button,
        count: u8,
        t_ms: u32,
    },
    ButtonDown {
        button: Button,
        clicks: u8,
    },
    ButtonUp(Button),
    Scroll {
        dx: f64,
        dy: f64,
        t_ms: u32,
        phase: ScrollPhase,
    },
    Zoom {
        steps: i32,
        t_ms: u32,
    },
    Shortcut {
        shortcut: Shortcut,
        t_ms: u32,
    },
}

impl InputAction {
    /// Stable tag used by the shared fixtures.
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Move { .. } => "move",
            Self::Click { .. } => "click",
            Self::ButtonDown { .. } => "button_down",
            Self::ButtonUp(_) => "button_up",
            Self::Scroll { .. } => "scroll",
            Self::Zoom { .. } => "zoom",
            Self::Shortcut { .. } => "shortcut",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    Idle,
    Pending,
    Moving,
    Drag,
    TwoPending,
    Scroll,
    Zoom,
    MultiPending,
    /// Intent spent; wait for every finger to lift.
    Dead,
}

impl State {
    /// Coarse name reported to the phone for its status UI.
    pub fn gesture_name(self) -> &'static str {
        match self {
            Self::Moving => "move",
            Self::Drag => "drag",
            Self::Scroll => "scroll",
            Self::Zoom => "zoom",
            _ => "idle",
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct Pointer {
    id: u8,
    start_t: u32,
    start_x: f64,
    start_y: f64,
    x: f64,
    y: f64,
    /// Movement seen since a gesture handler last consumed it. Deltas are
    /// consumed exactly once: a stationary finger must contribute nothing when
    /// the *other* finger reports a sample, or two-finger scroll double-counts.
    dx: f64,
    dy: f64,
    path_px: f64,
    last_t: u32,
}

impl Pointer {
    fn take(&mut self) -> (f64, f64) {
        let d = (self.dx, self.dy);
        self.dx = 0.0;
        self.dy = 0.0;
        d
    }
}

pub struct Recognizer {
    pub cfg: Config,
    surface_wpx: f64,
    surface_hpx: f64,
    state: State,
    /// Insertion-ordered, so "the first two fingers" is well defined.
    pointers: Vec<Pointer>,
    held_button: Option<Button>,
    gesture_start_t: u32,
    armed_drag: bool,
    acc_x: f64,
    acc_y: f64,
    scroll_acc_x: f64,
    scroll_acc_y: f64,
    speed_ema: f64,
    /// Smoothed scroll speed, for the scroll acceleration curve.
    scroll_speed_ema: f64,
    zoom_ref_dist: f64,
    zoom_acc: f64,
    momentum_vx: f64,
    momentum_vy: f64,
    momentum_t: u32,
    scroll_vx: f64,
    scroll_vy: f64,
    scroll_started: bool,
    /// Click count the held drag was started with. Two means the OS selects by
    /// word, which is what "double-click and drag" does on a real trackpad.
    held_clicks: u8,
    /// Click count of the tap that armed a drag.
    armed_clicks: u8,
    /// Finger spread when a multi-finger gesture began, for pinch detection.
    multi_ref_spread: f64,
    /// A multi-finger tap is only judged once the LAST finger lifts, so the peak
    /// finger count and the "still tap-like" flag outlive the pointers.
    max_fingers: usize,
    tap_ok: bool,
    /// When the most recent finger of this gesture landed - which is when the
    /// hand became whole, and so when a multi-finger tap starts being timed.
    last_down_t: u32,
    // Click bookkeeping survives resets so double-click and tap-and-drag work.
    last_click_t: i64,
    last_click_button: Button,
    click_state: u8,
    tap_ended_t: i64,
    /// When the last two-finger tap ended, for smart zoom.
    two_finger_tap_t: i64,
    /// Timestamp of the last sample fed in, in the *phone's* clock.
    ///
    /// Written by `feed` and deliberately not by `tick`: the samples carry the
    /// phone's `performance.now()` while the tick carries the desktop's own
    /// clock, and a field that mixed the two would read as a wild jump every
    /// time a device connected. Only ever compared against the other `_t`
    /// fields here, which come from the same phone.
    last_fed_t: u32,
}

impl Recognizer {
    pub fn new(cfg: Config, surface_wpx: f64, surface_hpx: f64) -> Self {
        Self {
            cfg,
            surface_wpx,
            surface_hpx,
            state: State::Idle,
            pointers: Vec::new(),
            held_button: None,
            gesture_start_t: 0,
            armed_drag: false,
            acc_x: 0.0,
            acc_y: 0.0,
            scroll_acc_x: 0.0,
            scroll_acc_y: 0.0,
            speed_ema: 0.0,
            scroll_speed_ema: 0.0,
            zoom_ref_dist: 0.0,
            zoom_acc: 0.0,
            momentum_vx: 0.0,
            momentum_vy: 0.0,
            momentum_t: 0,
            scroll_vx: 0.0,
            scroll_vy: 0.0,
            scroll_started: false,
            held_clicks: 1,
            armed_clicks: 1,
            multi_ref_spread: 0.0,
            max_fingers: 0,
            tap_ok: true,
            last_down_t: 0,
            last_click_t: -10_000,
            last_click_button: Button::Left,
            click_state: 1,
            tap_ended_t: -10_000,
            two_finger_tap_t: -10_000,
            last_fed_t: 0,
        }
    }

    // ------------------------------------------------------------- lifecycle

    pub fn set_surface(&mut self, wpx: f64, hpx: f64) {
        if wpx > 0.0 && hpx > 0.0 {
            self.surface_wpx = wpx;
            self.surface_hpx = hpx;
        }
    }

    pub fn state(&self) -> State {
        self.state
    }

    pub fn finger_count(&self) -> usize {
        self.pointers.len()
    }

    /// Nothing in flight: no fingers down, no gesture resolving, no button held.
    ///
    /// This is the seam a second device is allowed to take the cursor at. A
    /// handover anywhere else would inject the second half of one device's
    /// gesture - a `ButtonUp` with no `ButtonDown`, or a scroll that never
    /// ended - so control only ever changes hands here.
    ///
    /// Momentum deliberately does not count as busy: a flick can coast for
    /// seconds, and making the other device wait it out would feel broken.
    pub fn is_idle(&self) -> bool {
        self.state == State::Idle && self.pointers.is_empty() && self.held_button.is_none()
    }

    /// How much longer this device could still be *extending* the gesture it
    /// just finished, in milliseconds. Zero when nothing is pending.
    ///
    /// This is the other half of [`is_idle`](Self::is_idle), and it exists for
    /// handover. Idle is not the same as finished: the gap between the two
    /// halves of a double-tap, and the pause between a tap and the drag it
    /// arms, are both moments with no finger down and a gesture very much in
    /// progress. Another device taking the cursor there breaks a sequence
    /// every trackpad supports.
    ///
    /// The desktop used to answer this with a flat 350 ms after *any* gesture,
    /// which is right for a tap and pure delay for everything else - finishing
    /// a scroll or a plain move arms nothing at all, and made picking up the
    /// other device feel broken for a third of a second for no reason. Asking
    /// the recognizer instead costs nothing and is exact: the three timestamps
    /// below are already kept for the gestures themselves.
    ///
    /// Measured from the last sample fed in, not from now - this type has no
    /// clock of its own and must not grow one. The caller holds the wall-clock
    /// end of the comparison.
    pub fn follow_up_ms(&self) -> u32 {
        let window = self.cfg.tap.double_tap_ms as i64;
        // The most recent thing that could still be continued: a tap that may
        // become a double-tap or arm a drag, a click that may gain a second,
        // or a two-finger tap that may become a smart zoom.
        let latest = self
            .tap_ended_t
            .max(self.last_click_t)
            .max(self.two_finger_tap_t);
        let since = self.last_fed_t as i64 - latest;
        // Negative means the marker is in the future, which a reconnected phone
        // restarting its clock can genuinely produce. Treat it as expired
        // rather than reserving the cursor for the length of the whole window.
        if since < 0 || since >= window {
            return 0;
        }
        (window - since) as u32
    }

    /// Most fingers seen at once during the gesture in progress.
    ///
    /// This is what a multi-finger gesture is judged on, so it is worth being
    /// able to see: it is the difference between a four-finger swipe and a
    /// three-finger one when the hand lands unevenly.
    pub fn peak_fingers(&self) -> usize {
        self.max_fingers
    }

    /// Normalized surface coordinates -> surface pixels. The phone sends 0-1 so
    /// the desktop can scale by the real surface size reported in `welcome`;
    /// physical deltas are what the acceleration curve needs.
    fn to_px(&self, x: f32, y: f32) -> (f64, f64) {
        (x as f64 * self.surface_wpx, y as f64 * self.surface_hpx)
    }

    /// Drop all gesture state, releasing any held button into `out`.
    fn reset(&mut self, out: &mut Vec<InputAction>) {
        if let Some(b) = self.held_button.take() {
            out.push(InputAction::ButtonUp(b));
        }
        self.held_clicks = 1;
        self.state = State::Idle;
        self.pointers.clear();
        self.gesture_start_t = 0;
        self.armed_drag = false;
        self.acc_x = 0.0;
        self.acc_y = 0.0;
        self.scroll_acc_x = 0.0;
        self.scroll_acc_y = 0.0;
        self.speed_ema = 0.0;
        self.scroll_speed_ema = 0.0;
        self.zoom_ref_dist = 0.0;
        self.zoom_acc = 0.0;
        self.momentum_vx = 0.0;
        self.momentum_vy = 0.0;
        self.momentum_t = 0;
        self.scroll_vx = 0.0;
        self.scroll_vy = 0.0;
        self.scroll_started = false;
        self.multi_ref_spread = 0.0;
        self.max_fingers = 0;
        self.tap_ok = true;
        self.last_down_t = 0;
    }

    /// Connection lost or gesture cancelled: never leave a button or drag stuck.
    pub fn release_all(&mut self) -> Vec<InputAction> {
        let mut out = Vec::new();
        self.reset(&mut out);
        out
    }

    // ----------------------------------------------------------------- input

    pub fn feed(&mut self, samples: &[TouchSample]) -> Vec<InputAction> {
        let mut out = Vec::new();
        if let Some(last) = samples.last() {
            self.last_fed_t = last.t_ms;
        }
        for s in samples {
            match s.phase {
                DOWN => self.on_down(*s),
                MOVE => self.on_move(*s, &mut out),
                UP => self.on_up(*s, &mut out),
                _ => self.on_cancel(*s, &mut out),
            }
        }
        out
    }

    /// Momentum scroll continuation (section 9.4). Safe to call at any rate.
    pub fn tick(&mut self, t_ms: u32) -> Vec<InputAction> {
        let mut out = Vec::new();
        if self.momentum_vx == 0.0 && self.momentum_vy == 0.0 {
            return out;
        }
        let dt = (t_ms.saturating_sub(self.momentum_t)) as f64 / 1000.0;
        if dt <= 0.0 {
            return out;
        }
        self.momentum_t = t_ms;
        let decay = MOMENTUM_DECAY.powf(dt * 60.0);
        self.momentum_vx *= decay;
        self.momentum_vy *= decay;
        if self.momentum_vx.abs() < MOMENTUM_STOP && self.momentum_vy.abs() < MOMENTUM_STOP {
            self.momentum_vx = 0.0;
            self.momentum_vy = 0.0;
            out.push(InputAction::Scroll {
                dx: 0.0,
                dy: 0.0,
                t_ms,
                phase: ScrollPhase::MomentumEnd,
            });
            return out;
        }
        let (vx, vy) = (self.momentum_vx, self.momentum_vy);
        self.emit_scroll_phased(
            vx * dt,
            vy * dt,
            t_ms,
            Some(ScrollPhase::Momentum),
            &mut out,
        );
        out
    }

    fn find(&mut self, id: u8) -> Option<usize> {
        self.pointers.iter().position(|p| p.id == id)
    }

    // ------------------------------------------------------------------ down

    fn on_down(&mut self, s: TouchSample) {
        // A new touch cancels any coasting scroll, like a real trackpad.
        self.momentum_vx = 0.0;
        self.momentum_vy = 0.0;
        self.last_down_t = s.t_ms;

        let (px, py) = self.to_px(s.x, s.y);
        self.pointers.push(Pointer {
            id: s.pointer_id,
            start_t: s.t_ms,
            start_x: px,
            start_y: py,
            x: px,
            y: py,
            dx: 0.0,
            dy: 0.0,
            path_px: 0.0,
            last_t: s.t_ms,
        });
        let n = self.pointers.len();
        self.max_fingers = self.max_fingers.max(n);

        if self.state == State::Idle {
            self.gesture_start_t = s.t_ms;
            self.state = State::Pending;
            // Tap-and-drag ("tap-and-a-half"): a tap, then a finger down again
            // within doubleTapMs, arms a drag on the next movement.
            self.armed_drag =
                (s.t_ms as i64 - self.tap_ended_t) <= self.cfg.tap.double_tap_ms as i64;
            self.armed_clicks = if self.armed_drag { self.click_state } else { 1 };
            return;
        }

        // A finger landing while the cursor is already moving is not a late
        // arrival to a gesture that has closed - it is the user changing their
        // mind, which is a thing a real trackpad lets them do. One finger
        // sliding and a second coming down is a scroll on a Mac every time, and
        // refusing it meant lifting the whole hand and starting again just to
        // scroll the page you had only now finished pointing at.
        //
        // Out of `Moving` only. `Drag` is holding a button down and must not be
        // taken apart in the middle of a selection, and `Scroll` and `Zoom`
        // already *are* the gesture this would promote to.
        if self.state == State::Moving && n >= 2 {
            self.regroup(s.t_ms);
            return;
        }

        // Additional fingers only change the intent while it is still forming.
        if matches!(
            self.state,
            State::Pending | State::TwoPending | State::MultiPending
        ) {
            // A finger may also join late as long as nothing has actually moved
            // yet: the hand is still settling, so this is one gesture forming
            // slowly rather than a second gesture starting.
            let settling = self
                .pointers
                .iter()
                .all(|p| (p.x - p.start_x).hypot(p.y - p.start_y) <= self.cfg.tap.tap_max_px);
            if settling || s.t_ms.saturating_sub(self.gesture_start_t) <= GROUP_WINDOW_MS {
                if n == 2 {
                    self.state = State::TwoPending;
                    self.zoom_ref_dist = self.pair_distance();
                    self.armed_drag = false;
                } else if n >= 3 {
                    self.state = State::MultiPending;
                    self.armed_drag = false;
                    // Remember how far apart they landed, so a pinch is measured
                    // against the start of the gesture.
                    self.multi_ref_spread = self.current_spread();
                }
            } else {
                // Landed too late to be part of this gesture; the intent is spent.
                self.state = State::Dead;
            }
        }
        // In a committed state (Moving/Drag/Scroll/Zoom) extra fingers are
        // ignored so the gesture cannot mutate mid-flight (section 9.1).
    }

    // ------------------------------------------------------------------ move

    fn on_move(&mut self, s: TouchSample, out: &mut Vec<InputAction>) {
        let Some(i) = self.find(s.pointer_id) else {
            return;
        };
        let (px, py) = self.to_px(s.x, s.y);

        let dt_ms;
        {
            let p = &mut self.pointers[i];
            let step = (px - p.x).hypot(py - p.y);
            p.dx += px - p.x;
            p.dy += py - p.y;
            p.x = px;
            p.y = py;
            p.path_px += step;
            dt_ms = (s.t_ms.saturating_sub(p.last_t)).max(1);
            p.last_t = s.t_ms;
        }

        match self.state {
            State::Pending => self.pending_move(s, i, dt_ms, out),
            State::Moving | State::Drag => self.cursor_move(i, dt_ms, s.t_ms, out),
            State::TwoPending => self.two_pending_move(s, out),
            State::Scroll => self.scroll_move(s, out),
            State::Zoom => self.zoom_move(s, out),
            State::MultiPending => self.multi_pending_move(s, out),
            _ => {}
        }
    }

    fn pending_move(&mut self, s: TouchSample, i: usize, dt_ms: u32, out: &mut Vec<InputAction>) {
        let p = self.pointers[i];
        let drift = (p.x - p.start_x).hypot(p.y - p.start_y);
        let held_ms = s.t_ms.saturating_sub(p.start_t);

        // Press-and-drag: stationary past pressMs, then moving. Checked before
        // the movement threshold so a long press that then moves becomes a drag,
        // not a cursor move.
        // Press-and-drag substitutes for holding a physical button, so it has to
        // be unmistakable. The hold does that: `pressMs` is deliberately long
        // (half a second), because a shorter one caught ordinary moves that
        // paused for a moment and turned them into selections.
        //
        // Deliberately measured by distance from the start, not by path length:
        // a resting finger jitters, and over half a second that noise adds up to
        // more travel than a real move, which would block every long press.
        if self.cfg.drag.press_and_drag
            && held_ms >= self.cfg.tap.press_ms
            && drift <= self.cfg.tap.tap_max_px
        {
            self.begin_drag(out, 1);
            self.cursor_move(i, dt_ms, s.t_ms, out);
            return;
        }

        if drift > self.cfg.tap.tap_max_px {
            // A drag armed by a DOUBLE tap always runs, whatever the host's
            // one-finger dragging style says. On a trackpad you would select
            // text by double-clicking and holding the physical button; a phone
            // has no button, so this gesture is the only way to do it at all.
            let double_tap_drag = self.armed_drag && self.armed_clicks >= 2;
            if self.armed_drag && (self.cfg.drag.tap_and_drag || double_tap_drag) {
                let clicks = self.armed_clicks.max(1);
                self.begin_drag(out, clicks);
            } else {
                self.state = State::Moving;
            }
            // Replay the whole drift so the cursor does not lag the finger, and
            // drop the unconsumed delta it already includes.
            self.pointers[i].take();
            let sens = self.cfg.sensitivity;
            self.acc_x += (p.x - p.start_x) * sens;
            self.acc_y += (p.y - p.start_y) * sens;
            self.flush_move(s.t_ms, out);
        }
    }

    fn begin_drag(&mut self, out: &mut Vec<InputAction>, clicks: u8) {
        self.state = State::Drag;
        self.held_button = Some(Button::Left);
        self.held_clicks = clicks;
        self.armed_drag = false;
        out.push(InputAction::ButtonDown {
            button: Button::Left,
            clicks,
        });
    }

    /// Re-open a committed gesture around the fingers that are down right now.
    ///
    /// Every finger is re-anchored where it currently sits, and that is the
    /// whole point of the function rather than a tidy-up afterwards: a finger
    /// that has been steering the cursor is a long way from where it landed,
    /// and both `two_pending_move` and `multi_pending_move` measure drift from
    /// `start_x`/`start_y`. Left alone, the promoted gesture would commit on its
    /// very first sample, in whatever direction that finger happened to be
    /// travelling - a sideways move would fire back/forward rather than ever
    /// becoming the scroll the second finger asked for.
    fn regroup(&mut self, t_ms: u32) {
        for p in self.pointers.iter_mut() {
            p.start_x = p.x;
            p.start_y = p.y;
            p.start_t = t_ms;
            p.path_px = 0.0;
            // Movement already spent on the cursor must not be spent again as
            // scroll: `take` is what keeps a delta to exactly one consumer.
            p.take();
        }
        self.gesture_start_t = t_ms;
        self.armed_drag = false;
        self.speed_ema = 0.0;
        self.acc_x = 0.0;
        self.acc_y = 0.0;
        // A gesture that has already moved the cursor is not a tap, whatever it
        // does next. Saying so here is not belt-and-braces: the re-anchoring
        // above has just made every finger look like it landed a moment ago and
        // has travelled nothing, which is precisely the test `on_up` applies -
        // so without this, an ordinary move-then-scroll would end in a stray
        // right-click.
        self.tap_ok = false;
        if self.pointers.len() == 2 {
            self.state = State::TwoPending;
            self.zoom_ref_dist = self.pair_distance();
        } else {
            self.state = State::MultiPending;
            self.multi_ref_spread = self.current_spread();
        }
    }

    fn cursor_move(&mut self, i: usize, dt_ms: u32, t_ms: u32, out: &mut Vec<InputAction>) {
        let (dx, dy) = self.pointers[i].take();
        if dx == 0.0 && dy == 0.0 {
            return;
        }
        let speed = dx.hypot(dy) / (dt_ms as f64 / 1000.0);
        // Smooth the speed estimate only - never the position - so acceleration
        // is stable without adding latency to the cursor itself (section 9.2).
        self.speed_ema = 0.5 * self.speed_ema + 0.5 * speed;
        let f = accel::factor(&self.cfg.accel, self.speed_ema) * self.cfg.sensitivity;
        self.acc_x += dx * f;
        self.acc_y += dy * f;
        self.flush_move(t_ms, out);
    }

    /// Emit whole pixels, carrying the remainder so slow movement is not lost.
    fn flush_move(&mut self, t_ms: u32, out: &mut Vec<InputAction>) {
        let ix = self.acc_x.trunc();
        let iy = self.acc_y.trunc();
        if ix == 0.0 && iy == 0.0 {
            return;
        }
        self.acc_x -= ix;
        self.acc_y -= iy;
        out.push(InputAction::Move {
            dx: ix,
            dy: iy,
            t_ms,
        });
    }

    // -------------------------------------------------- two-finger gestures

    fn two_pending_move(&mut self, s: TouchSample, out: &mut Vec<InputAction>) {
        if self.pointers.len() < 2 {
            return;
        }
        let dist = self.pair_distance();
        let ratio = if self.zoom_ref_dist > 1e-6 {
            dist / self.zoom_ref_dist
        } else {
            1.0
        };
        let n = self.pointers.len() as f64;
        let avg_drift = self
            .pointers
            .iter()
            .map(|p| (p.x - p.start_x).hypot(p.y - p.start_y))
            .sum::<f64>()
            / n;

        // A pinch is two fingers moving in OPPOSITE directions. Distance alone
        // is not enough: fingers report one at a time, so a plain sideways
        // scroll momentarily changes the spacing and used to be read as a
        // pinch - which is why horizontal two-finger scrolling misbehaved.
        let opposed = {
            let (a, b) = (self.pointers[0], self.pointers[1]);
            let (ax, ay) = (a.x - a.start_x, a.y - a.start_y);
            let (bx, by) = (b.x - b.start_x, b.y - b.start_y);
            ax * bx + ay * by < 0.0
        };

        if self.cfg.zoom.enabled && opposed && (ratio - 1.0).abs() > self.cfg.zoom.threshold {
            self.state = State::Zoom;
            self.zoom_ref_dist = dist;
            self.zoom_acc = 0.0;
        } else if avg_drift > self.cfg.tap.tap_max_px {
            // Two-finger sideways swipe navigates back/forward when the host
            // has that on; otherwise two fingers scroll.
            let (dx, dy) = self.average_drift();
            // Any action, not just back/forward: two fingers sideways is the
            // most reachable swipe there is, and restricting it to one action
            // was a leftover from when that was the only one it could carry.
            let direction = self.cfg.bindings.directional(2, true, dx > 0.0);
            // "Nothing" means this swipe is not bound, whether it is said by
            // the direction or by the paired setting it inherits from - and an
            // unbound two fingers sideways is a scroll. Reading a direction set
            // to "none" as a bound gesture took horizontal scrolling away in
            // that direction and gave nothing back for it.
            let bound = if direction == "inherit" {
                self.cfg.bindings.two_finger_swipe_navigate != "none"
            } else {
                direction != "none"
            };
            let navigating = bound && dx.abs() > dy.abs() * 1.5;
            if navigating {
                // A navigation swipe needs more travel than a scroll does, so
                // keep deciding rather than committing to a scroll we would
                // never be able to take back.
                if dx.abs() > self.cfg.swipe.min_px {
                    let shortcut = directional_shortcut(
                        direction,
                        &self.cfg.bindings.two_finger_swipe_navigate,
                        true,
                        dx > 0.0,
                        self.cfg.scroll.natural,
                    );
                    if let Some(shortcut) = shortcut {
                        out.push(InputAction::Shortcut {
                            shortcut,
                            t_ms: s.t_ms,
                        });
                    }
                    self.state = State::Dead;
                }
                return;
            }
            if !self.cfg.scroll.enabled {
                self.state = State::Dead;
                return;
            }
            self.state = State::Scroll;
            self.scroll_move(s, out);
        }
    }

    fn scroll_move(&mut self, s: TouchSample, out: &mut Vec<InputAction>) {
        if self.pointers.is_empty() {
            return;
        }
        let n = self.pointers.len() as f64;
        let mut sx = 0.0;
        let mut sy = 0.0;
        for p in self.pointers.iter_mut() {
            let (dx, dy) = p.take();
            sx += dx;
            sy += dy;
        }
        let (dx, dy) = (sx / n, sy / n);
        if dx == 0.0 && dy == 0.0 {
            return;
        }
        let dt = if self.momentum_t != 0 {
            (s.t_ms.saturating_sub(self.momentum_t)).max(1) as f64 / 1000.0
        } else {
            0.016
        };
        self.momentum_t = s.t_ms;

        // Scrolling accelerates on speed, exactly as cursor movement does. A
        // strictly one-to-one scroll is what makes a touch surface feel unlike a
        // trackpad: a quick flick should cover a page, not 200 pixels.
        let speed = dx.hypot(dy) / dt;
        self.scroll_speed_ema = 0.6 * self.scroll_speed_ema + 0.4 * speed;
        let gain = self.scroll_accel(self.scroll_speed_ema);

        // Velocity for the coast is taken AFTER the curve, so momentum carries
        // the speed the user actually saw.
        self.scroll_vx = dx * gain / dt;
        self.scroll_vy = dy * gain / dt;
        self.emit_scroll(dx * gain, dy * gain, s.t_ms, out);
    }

    fn emit_scroll(&mut self, dx: f64, dy: f64, t_ms: u32, out: &mut Vec<InputAction>) {
        self.emit_scroll_phased(dx, dy, t_ms, None, out);
    }

    /// How much further than the finger the content should travel.
    fn scroll_accel(&self, speed: f64) -> f64 {
        let s = speed / SCROLL_SPEED_REF;
        (SCROLL_BASE + self.cfg.scroll.accel * s * s * 2.5).clamp(0.2, SCROLL_MAX)
    }

    fn emit_scroll_phased(
        &mut self,
        dx: f64,
        dy: f64,
        t_ms: u32,
        phase: Option<ScrollPhase>,
        out: &mut Vec<InputAction>,
    ) {
        let k = self.cfg.scroll.speed;
        let sign = if self.cfg.scroll.natural { 1.0 } else { -1.0 };
        self.scroll_acc_x += dx * k * sign;
        self.scroll_acc_y += dy * k * sign;
        let ix = self.scroll_acc_x.trunc();
        let iy = self.scroll_acc_y.trunc();
        if ix == 0.0 && iy == 0.0 {
            return;
        }
        self.scroll_acc_x -= ix;
        self.scroll_acc_y -= iy;
        let phase = phase.unwrap_or_else(|| {
            let p = if self.scroll_started {
                ScrollPhase::Continue
            } else {
                ScrollPhase::Begin
            };
            self.scroll_started = true;
            p
        });
        out.push(InputAction::Scroll {
            dx: ix,
            dy: iy,
            t_ms,
            phase,
        });
    }

    fn zoom_move(&mut self, s: TouchSample, out: &mut Vec<InputAction>) {
        if self.pointers.len() < 2 {
            return;
        }
        // Zoom works off absolute distance; discard deltas so they cannot leak.
        for p in self.pointers.iter_mut() {
            p.take();
        }
        let dist = self.pair_distance();
        if self.zoom_ref_dist <= 1e-6 {
            self.zoom_ref_dist = dist;
            return;
        }
        self.zoom_acc += (dist.max(1e-6) / self.zoom_ref_dist).ln();
        self.zoom_ref_dist = dist;
        // One zoom step per ~12% distance change; matches an app zoom increment.
        let step_size = 1.12_f64.ln();
        while self.zoom_acc.abs() >= step_size {
            let direction: i32 = if self.zoom_acc > 0.0 { 1 } else { -1 };
            self.zoom_acc -= direction as f64 * step_size;
            out.push(InputAction::Zoom {
                steps: direction,
                t_ms: s.t_ms,
            });
        }
    }

    /// Three or more fingers, still deciding which swipe this is.
    fn multi_pending_move(&mut self, s: TouchSample, out: &mut Vec<InputAction>) {
        let n = self.pointers.len();
        if n < 3 {
            return;
        }

        // How far the hand as a whole has travelled, and how much the fingers
        // have closed or opened relative to each other.
        let (dx, dy) = self.average_drift();
        let travel = dx.hypot(dy);
        let spread_ratio = self.spread_ratio();
        let spread_px = (spread_ratio - 1.0).abs() * self.multi_ref_spread;

        // Four fingers pinching or five spreading are whole-desktop gestures.
        //
        // A pinch has to be dominated by the fingers moving relative to each
        // other, not by the hand moving as a unit. Spacing alone is not enough:
        // swiping four fingers up curls and splays them enough to change the
        // spread by well over the threshold, which fired Launchpad instead of
        // Mission Control and made vertical swipes feel random.
        if (spread_ratio - 1.0).abs() > 0.18 && spread_px > travel {
            let pinching = spread_ratio < 1.0;
            let binding = if self.max_fingers.max(n) >= 5 {
                self.cfg.bindings.five_finger_spread.as_str()
            } else {
                self.cfg.bindings.four_finger_pinch.as_str()
            };
            let shortcut = match (binding, pinching) {
                ("launchpad", true) => Some(Shortcut::Launchpad),
                ("showDesktop", false) => Some(Shortcut::ShowDesktop),
                // Reversing the gesture undoes it, as macOS does.
                ("launchpad", false) | ("showDesktop", true) => Some(Shortcut::ShowDesktop),
                _ => None,
            };
            if let Some(shortcut) = shortcut {
                out.push(InputAction::Shortcut {
                    shortcut,
                    t_ms: s.t_ms,
                });
                self.state = State::Dead;
                return;
            }
        }

        let min = self.cfg.swipe.min_px;
        // A swipe needs a clear winner, or a sloppy diagonal fires both axes.
        let horizontal = dx.abs() > min && dx.abs() > dy.abs() * 1.5;
        let vertical = dy.abs() > min && dy.abs() > dx.abs() * 1.5;
        if !horizontal && !vertical {
            return;
        }

        // Choose by the PEAK finger count, not by how many happen to be touching
        // at this instant. One finger lifting a moment early, or landing a moment
        // late, otherwise turns a four-finger swipe into a three-finger one -
        // which on a Mac that binds only the four-finger gestures means nothing
        // happens at all. Multi-finger taps already work this way.
        let fingers = self.max_fingers.max(n);
        let binding = match (fingers, horizontal) {
            (3, true) => self.cfg.bindings.three_finger_horiz_swipe.as_str(),
            (3, false) => self.cfg.bindings.three_finger_vert_swipe.as_str(),
            (_, true) => self.cfg.bindings.four_finger_horiz_swipe.as_str(),
            (_, false) => self.cfg.bindings.four_finger_vert_swipe.as_str(),
        };
        let positive = if horizontal { dx > 0.0 } else { dy > 0.0 };
        if let Some(shortcut) = directional_shortcut(
            self.cfg.bindings.directional(fingers, horizontal, positive),
            binding,
            horizontal,
            positive,
            self.cfg.scroll.natural,
        ) {
            out.push(InputAction::Shortcut {
                shortcut,
                t_ms: s.t_ms,
            });
        }
        // One swipe per gesture: hold until every finger lifts, so a long drag
        // does not fire the shortcut over and over.
        self.state = State::Dead;
    }

    /// How far apart the fingers are now, relative to when they landed.
    ///
    /// Generalises `pair_distance` to any number of fingers: the mean distance
    /// from their centre, which is what a four- or five-finger pinch changes.
    fn spread_ratio(&self) -> f64 {
        if self.multi_ref_spread <= 1e-6 {
            return 1.0;
        }
        self.current_spread() / self.multi_ref_spread
    }

    fn current_spread(&self) -> f64 {
        let n = self.pointers.len() as f64;
        if n < 2.0 {
            return 0.0;
        }
        let cx: f64 = self.pointers.iter().map(|p| p.x).sum::<f64>() / n;
        let cy: f64 = self.pointers.iter().map(|p| p.y).sum::<f64>() / n;
        self.pointers
            .iter()
            .map(|p| (p.x - cx).hypot(p.y - cy))
            .sum::<f64>()
            / n
    }

    /// How far the fingers have travelled from where they landed, averaged.
    fn average_drift(&self) -> (f64, f64) {
        let n = self.pointers.len() as f64;
        let sx: f64 = self.pointers.iter().map(|p| p.x - p.start_x).sum();
        let sy: f64 = self.pointers.iter().map(|p| p.y - p.start_y).sum();
        (sx / n, sy / n)
    }

    fn pair_distance(&self) -> f64 {
        if self.pointers.len() < 2 {
            return 0.0;
        }
        let (a, b) = (self.pointers[0], self.pointers[1]);
        (a.x - b.x).hypot(a.y - b.y)
    }

    // -------------------------------------------------------------------- up

    fn on_up(&mut self, s: TouchSample, out: &mut Vec<InputAction>) {
        let Some(i) = self.find(s.pointer_id) else {
            return;
        };
        let p = self.pointers.remove(i);

        // Timed from whichever came later: this finger landing, or the last of
        // the hand landing. Charging a finger for the time it spent waiting for
        // its neighbours is what made a four-finger tap a coin toss - fingers
        // never land together on a phone, the spread between index and little
        // finger routinely runs to 150 ms (the same fact `GROUP_WINDOW_MS`
        // exists for), and all of it came out of the first finger's `tapMaxMs`
        // budget. The hand is not making a tap until it is all down; that is the
        // moment worth timing from.
        //
        // For one finger the two are the same value, so nothing about a
        // single-finger tap or a double tap changes.
        let down_t = p.start_t.max(self.last_down_t);
        let quick = s.t_ms.saturating_sub(down_t) <= self.cfg.tap.tap_max_ms
            && p.path_px <= self.cfg.tap.tap_max_px;
        if !quick {
            self.tap_ok = false;
        }

        if !self.pointers.is_empty() {
            // Not the last finger. Stay in the committed intent: a two-finger tap
            // lifts one finger fractionally before the other, and a scroll must
            // not turn into a cursor jerk because one finger left early
            // (section 9.1). A lone remaining finger cannot move the cursor
            // because the two-finger handlers require two pointers.
            return;
        }

        if matches!(
            self.state,
            State::Pending | State::TwoPending | State::MultiPending
        ) && self.tap_ok
        {
            let binding = match self.max_fingers {
                1 => Some(self.cfg.bindings.one_tap.as_str()),
                2 => Some(self.cfg.bindings.two_finger_tap.as_str()),
                3 => Some(self.cfg.bindings.three_finger_tap.as_str()),
                4 => Some(self.cfg.bindings.four_finger_tap.as_str()),
                _ => None,
            };
            // Two fingers tapped twice in quick succession is smart zoom, not
            // two right-clicks.
            if self.max_fingers == 2
                && self.cfg.bindings.two_finger_double_tap == "smartZoom"
                && (s.t_ms as i64 - self.two_finger_tap_t) <= self.cfg.tap.double_tap_ms as i64
            {
                self.two_finger_tap_t = -10_000;
                out.push(InputAction::Shortcut {
                    shortcut: Shortcut::SmartZoom,
                    t_ms: s.t_ms,
                });
                self.reset(out);
                return;
            }
            if self.max_fingers == 2 {
                self.two_finger_tap_t = s.t_ms as i64;
            }
            if let Some(b) = binding {
                let fingers = self.max_fingers;
                // Resolved here, which ends the borrow of `self.cfg`.
                if let Some(button) = Button::from_binding(b) {
                    self.emit_click(button, s.t_ms, out);
                } else if let Some(shortcut) = Shortcut::from_binding(b) {
                    // A tap bound to anything but a click used to do nothing at
                    // all: the binding was read, matched no button, and was
                    // dropped without a word.
                    out.push(InputAction::Shortcut {
                        shortcut,
                        t_ms: s.t_ms,
                    });
                }
                if fingers == 1 {
                    // Opens the tap-and-drag window (section 9.5).
                    self.tap_ended_t = s.t_ms as i64;
                }
            }
        } else if self.state == State::Scroll {
            // Close the gesture before reset() clears the flag, so macOS knows
            // the fingers have left and can rubber-band.
            if self.scroll_started {
                out.push(InputAction::Scroll {
                    dx: 0.0,
                    dy: 0.0,
                    t_ms: s.t_ms,
                    phase: ScrollPhase::End,
                });
            }
            self.launch_momentum(s.t_ms);
        }

        self.reset(out);
    }

    fn launch_momentum(&mut self, t_ms: u32) {
        if !self.cfg.scroll.momentum {
            return;
        }
        if self.scroll_vx.hypot(self.scroll_vy) < MOMENTUM_MIN_LAUNCH {
            return;
        }
        self.momentum_vx = self.scroll_vx;
        self.momentum_vy = self.scroll_vy;
        self.momentum_t = t_ms;
    }

    /// Take a `Button` rather than the binding string it came from: the binding
    /// is borrowed out of `self.cfg`, and resolving it at the call site is what
    /// lets this take `&mut self` without cloning the string first.
    fn emit_click(&mut self, button: Button, t_ms: u32, out: &mut Vec<InputAction>) {
        if button == self.last_click_button
            && (t_ms as i64 - self.last_click_t) <= self.cfg.tap.double_tap_ms as i64
        {
            self.click_state = (self.click_state + 1).min(3);
        } else {
            self.click_state = 1;
        }
        self.last_click_t = t_ms as i64;
        self.last_click_button = button;
        out.push(InputAction::Click {
            button,
            count: self.click_state,
            t_ms,
        });
    }

    fn on_cancel(&mut self, s: TouchSample, out: &mut Vec<InputAction>) {
        if let Some(i) = self.find(s.pointer_id) {
            self.pointers.remove(i);
        }
        if self.pointers.is_empty() {
            self.reset(out);
        } else {
            self.state = State::Dead;
        }
    }
}

/// Which desktop action a swipe in this direction means.
///
/// Horizontal direction follows the scroll-direction setting, because that is
/// what decides whether content follows the fingers: with natural scrolling,
/// swiping right reveals the space to the left.
fn directional_shortcut(
    direction: &str,
    legacy: &str,
    horizontal: bool,
    positive: bool,
    natural: bool,
) -> Option<Shortcut> {
    if direction == "inherit" {
        swipe_shortcut(legacy, horizontal, positive, natural)
    } else {
        Shortcut::from_binding(direction)
    }
}

fn swipe_shortcut(
    binding: &str,
    horizontal: bool,
    positive: bool,
    natural_scroll: bool,
) -> Option<Shortcut> {
    match (binding, horizontal) {
        ("spaces", true) => {
            let left = if natural_scroll { positive } else { !positive };
            Some(if left {
                Shortcut::SpaceLeft
            } else {
                Shortcut::SpaceRight
            })
        }
        // Fingers up (negative, since surface y grows downward) opens Mission
        // Control; fingers down shows the current app's windows.
        ("missionControl", false) => Some(if positive {
            Shortcut::AppWindows
        } else {
            Shortcut::MissionControl
        }),
        ("appWindows", false) => Some(Shortcut::AppWindows),
        // Up is louder. The one mapping nobody has to be taught.
        ("volume", false) => Some(if positive {
            Shortcut::VolumeDown
        } else {
            Shortcut::VolumeUp
        }),
        // And up is brighter, for the same reason.
        ("brightness", false) => Some(if positive {
            Shortcut::BrightnessDown
        } else {
            Shortcut::BrightnessUp
        }),
        // Surface y grows downward, so `positive` is a downward swipe.
        ("zoom", false) => Some(if positive {
            Shortcut::ZoomOut
        } else {
            Shortcut::ZoomIn
        }),
        // Back/forward is not only a two-finger gesture any more: any sideways
        // swipe can carry it, and which side goes back follows the scrolling
        // direction, exactly as the two-finger path has always done.
        ("navigate", true) => {
            let back = if natural_scroll { positive } else { !positive };
            Some(if back {
                Shortcut::Back
            } else {
                Shortcut::Forward
            })
        }
        ("undoRedo", true) => Some(if positive {
            Shortcut::Redo
        } else {
            Shortcut::Undo
        }),
        ("tabs", true) => Some(if positive {
            Shortcut::TabNext
        } else {
            Shortcut::TabPrev
        }),
        _ => None,
    }
}

#[cfg(test)]
mod vocabulary_tests {
    use super::*;
    use crate::gesture::Config;

    /// Every action the settings page offers must actually reach the OS.
    ///
    /// `Config::vocabulary` exists so the page "never offers an action the
    /// engine would silently ignore", and for a while it did exactly that: both
    /// swipe axes were given the same four values, so a settings page could
    /// offer Mission Control on a sideways swipe and spaces on an upward one -
    /// neither of which `swipe_shortcut` has any case for. The control changed,
    /// the config saved, and nothing happened, with nothing to say why.
    #[test]
    fn every_offered_swipe_action_does_something() {
        let axes = [
            ("bindings.twoFingerSwipeNavigate", true),
            ("bindings.threeFingerHorizSwipe", true),
            ("bindings.threeFingerVertSwipe", false),
            ("bindings.fourFingerHorizSwipe", true),
            ("bindings.fourFingerVertSwipe", false),
        ];
        for (field, horizontal) in axes {
            let values = Config::vocabulary()
                .into_iter()
                .find(|(f, _)| *f == field)
                .unwrap_or_else(|| panic!("{field} has no vocabulary"))
                .1;
            for value in values.iter().filter(|v| **v != "none") {
                // Both directions along the axis, and both scroll directions:
                // an action offered here has to land whichever way it is done.
                for positive in [true, false] {
                    for natural in [true, false] {
                        assert!(
                            swipe_shortcut(value, horizontal, positive, natural).is_some(),
                            "{field} offers {value}, which does nothing when swiped \
                             {} (natural scrolling {natural})",
                            if positive { "one way" } else { "the other" },
                        );
                    }
                }
            }
        }
    }

    /// Every action offered for one direction has to reach the OS on its own.
    ///
    /// The per-direction lists are not the axis lists: an action bound to a
    /// single direction is sent as itself, so "Mission Control up, app windows
    /// down" has no meaning there and "Mission Control" does. Offering an axis
    /// action on a direction would save a config the engine then dropped.
    #[test]
    fn every_offered_direction_action_does_something() {
        let mut directions = 0;
        for (field, values) in Config::vocabulary() {
            if !field.contains("FingerSwipe") || field.ends_with("Navigate") {
                continue;
            }
            directions += 1;
            assert!(
                values.contains(&"inherit"),
                "{field} cannot follow the setting it came from",
            );
            for value in values.iter().filter(|v| !["none", "inherit"].contains(*v)) {
                assert!(
                    Shortcut::from_binding(value).is_some(),
                    "{field} offers {value}, which does nothing on its own",
                );
            }
        }
        assert_eq!(directions, 10, "a swipe direction lost its vocabulary");
    }

    /// A direction nobody has set follows its axis; one that is set does not.
    #[test]
    fn inherit_is_the_only_value_that_defers_to_the_axis() {
        // The axis says Mission Control, which means App Expose downward.
        assert_eq!(
            directional_shortcut("inherit", "missionControl", false, true, true),
            Some(Shortcut::AppWindows),
        );
        assert_eq!(
            directional_shortcut("volumeUp", "missionControl", false, true, true),
            Some(Shortcut::VolumeUp),
            "a direction with its own action still followed the axis",
        );
        // And an axis action is not a direction action: "missionControl" on a
        // sideways swipe means nothing, and must not be dressed up as an axis.
        assert_eq!(
            directional_shortcut("none", "missionControl", false, true, true),
            None,
        );
    }

    /// Every action a tap offers has to be a click or a shortcut.
    ///
    /// A tap binding that matched neither was read and dropped in silence -
    /// which is how a tap could be set to Mission Control and do nothing at all.
    #[test]
    fn every_offered_tap_action_does_something() {
        for (field, values) in Config::vocabulary() {
            if !field.ends_with("Tap") && !field.ends_with("Click") {
                continue;
            }
            for value in values.iter().filter(|v| **v != "none") {
                assert!(
                    Button::from_binding(value).is_some()
                        || Shortcut::from_binding(value).is_some(),
                    "{field} offers {value}, which is neither a click nor an action",
                );
            }
        }
    }

    /// Two names for one keystroke is not two actions.
    ///
    /// macOS has Mission Control *and* App Expose, and separate keystrokes for
    /// them. Windows and Linux have a single overview, and the injector sends
    /// the same chord for both - so offering both there listed one action twice
    /// under two names, which reads as a bug in the app rather than a fact
    /// about the system.
    #[test]
    fn a_system_with_one_overview_is_only_offered_one() {
        let offers = |field: &str| {
            Config::vocabulary()
                .into_iter()
                .find(|(f, _)| *f == field)
                .map(|(_, v)| v.contains(&"appWindows"))
                .unwrap_or(false)
        };
        let expected = cfg!(target_os = "macos");
        assert_eq!(offers("bindings.oneTap"), expected);
        assert_eq!(offers("bindings.threeFingerVertSwipe"), expected);
    }

    /// Anything the host's own trackpad settings produce must be offerable too.
    ///
    /// The mirror writes binding values straight into the config; a vocabulary
    /// that had narrowed past them would leave the page showing "(unknown)" for
    /// a value the computer itself chose.
    #[test]
    fn the_mirror_never_writes_a_value_the_page_cannot_offer() {
        for (field, expected) in [
            ("bindings.threeFingerHorizSwipe", "spaces"),
            ("bindings.threeFingerVertSwipe", "missionControl"),
            ("bindings.fourFingerHorizSwipe", "spaces"),
            ("bindings.fourFingerVertSwipe", "missionControl"),
        ] {
            let values = Config::vocabulary()
                .into_iter()
                .find(|(f, _)| *f == field)
                .unwrap()
                .1;
            assert!(
                values.contains(&expected),
                "the mirror writes {expected} into {field}, which cannot be chosen there",
            );
        }
    }
}
