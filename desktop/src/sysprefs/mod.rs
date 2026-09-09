//! Mirroring the host's own trackpad settings.
//!
//! PadRemote should behave like the trackpad the user already has, on whatever
//! machine they run it on - not like whatever defaults happened to ship. This
//! module reads the host's real configuration and folds it onto [`Config`].
//!
//! Reading and mapping are kept apart on purpose: [`HostTrackpad`] is plain
//! data, so the mapping is unit-testable without touching the real system, and
//! a future Windows or Linux reader only has to produce the same struct.

#[cfg(target_os = "macos")]
pub mod macos;
// Compiled everywhere, not only on their own platform: each holds a *pure*
// mapping from that OS's raw settings onto `HostTrackpad`, and a mapping that
// can only be compiled on the machine it targets is one nobody can test. Only
// the `read()` inside each is platform-gated.
pub mod linux;
pub mod windows;

use crate::gesture::Config;

/// What a host trackpad reports about itself.
///
/// Every field is optional: a setting the OS does not expose, or that the user
/// has never touched, must leave PadRemote's own default alone rather than
/// silently reset it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct HostTrackpad {
    // --- scrolling ---
    /// "Natural" scrolling - content follows the fingers.
    pub natural_scroll: Option<bool>,
    /// "Use trackpad for scrolling" at all.
    pub scrolling: Option<bool>,
    /// "Use inertia when scrolling" - coasting after the fingers lift.
    pub momentum_scroll: Option<bool>,
    pub horizontal_scroll: Option<bool>,

    // --- clicking ---
    pub tap_to_click: Option<bool>,
    /// Two-finger tap or click gives the secondary (right) button.
    pub secondary_click: Option<bool>,
    /// Secondary click by pressing a corner of the pad instead.
    pub corner_secondary_click: Option<bool>,
    pub three_finger_tap: Option<bool>,
    /// Two-finger double tap - "smart zoom".
    pub two_finger_double_tap: Option<bool>,
    /// Seconds allowed between clicks of a double-click.
    pub double_click_seconds: Option<f64>,

    // --- dragging: two flags describing one choice ---
    /// Read, but never reproduced. PadRemote has no three-finger drag; this is
    /// here because selecting it on a Mac is what turns *one-finger* dragging
    /// off, which PadRemote does have to follow.
    pub three_finger_drag: Option<bool>,
    /// Tap-then-drag, macOS's "without drag lock".
    pub dragging: Option<bool>,

    // --- gestures ---
    pub pinch_zoom: Option<bool>,
    pub rotate: Option<bool>,
    pub three_finger_horiz_swipe: Option<bool>,
    pub three_finger_vert_swipe: Option<bool>,
    pub four_finger_horiz_swipe: Option<bool>,
    pub four_finger_vert_swipe: Option<bool>,
    /// Four fingers pinching together - Launchpad.
    pub four_finger_pinch: Option<bool>,
    /// Five fingers spreading apart - Show Desktop.
    pub five_finger_spread: Option<bool>,
    /// Two-finger sideways swipe to go back/forward between pages.
    pub swipe_navigate: Option<bool>,

    // --- read, but cannot be reproduced ---
    /// Force Touch. The phone has no pressure sensor.
    pub force_click: Option<bool>,
    /// Spring-loading. Handled by macOS itself once a real drag is underway.
    pub springing: Option<bool>,
    /// Tracking speed slider. Apple's curve is private, so this is approximate.
    pub tracking_speed: Option<f64>,
}

impl HostTrackpad {
    /// Read the host. Returns an empty reading on platforms with no support,
    /// which leaves every PadRemote default untouched.
    pub fn read() -> Self {
        #[cfg(target_os = "macos")]
        {
            macos::read()
        }
        #[cfg(target_os = "windows")]
        {
            windows::read()
        }
        #[cfg(all(unix, not(target_os = "macos")))]
        {
            linux::read()
        }
        // Anything else - and there is nothing else this app runs on today -
        // reports nothing, which leaves every PadRemote default untouched.
        #[cfg(not(any(target_os = "macos", target_os = "windows", unix)))]
        {
            Self::default()
        }
    }

    /// True when the host told us nothing at all.
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }

    /// Fold this reading onto a config, returning the adjusted copy.
    ///
    /// Only fields the host actually reported are touched, so a setting the OS
    /// does not expose leaves PadRemote's own default alone.
    pub fn apply_to(&self, cfg: &Config) -> Config {
        let mut c = cfg.clone();
        if !c.follow_system {
            return c;
        }

        // Scrolling.
        if let Some(v) = self.natural_scroll {
            c.scroll.natural = v;
        }
        if let Some(v) = self.scrolling {
            c.scroll.enabled = v;
        }
        if let Some(v) = self.momentum_scroll {
            c.scroll.momentum = v;
        }
        if let Some(v) = self.horizontal_scroll {
            c.scroll.horizontal = v;
        }

        // Clicking.
        // Tap to click is mirrored on, never off.
        //
        // A Mac with it off still clicks - you press the trackpad down. A phone
        // screen has no button to press, so following that setting left the pad
        // with no way to click at all, and macOS ships with it off. It is the
        // same reasoning as the long press: this substitutes for hardware the
        // phone does not have, so no trackpad setting can take it away.
        if self.tap_to_click == Some(true) {
            mirror_binding(&mut c.bindings.one_tap, true, "leftClick");
        }
        if let Some(v) = self.secondary_click {
            c.bindings.two_finger_tap = binding(v, "rightClick");
        }
        if let Some(v) = self.corner_secondary_click {
            c.bindings.corner_secondary_click = binding(v, "rightClick");
        }
        if let Some(v) = self.three_finger_tap {
            c.bindings.three_finger_tap = binding(v, "middleClick");
        }
        if let Some(v) = self.two_finger_double_tap {
            c.bindings.two_finger_double_tap = binding(v, "smartZoom");
        }
        if let Some(sec) = self.double_click_seconds {
            // Guard against a nonsensical stored value locking out double-click.
            c.tap.double_tap_ms = (sec * 1000.0).clamp(100.0, 2000.0) as u32;
        }

        // Dragging: macOS's "Dragging style" is a single choice spread over two
        // flags, and they are mutually exclusive - either three-finger drag or
        // a one-finger style, never both. PadRemote only has the one-finger
        // style, so the three-finger flag is read purely to know when to switch
        // tap-and-drag OFF.
        //
        // Getting that wrong is worse than it sounds: with three-finger drag
        // selected the Mac has no one-finger dragging at all, so leaving
        // tap-and-drag on turns a plain move that follows a tap into a
        // button-held drag instead of moving the cursor.
        let three = self.three_finger_drag.unwrap_or(false);
        let dragging = self.dragging.unwrap_or(false);
        if self.three_finger_drag.is_some() || self.dragging.is_some() {
            c.drag.tap_and_drag = dragging && !three;
            // `press_and_drag` is deliberately NOT mirrored. It substitutes for
            // hardware the phone does not have: on a trackpad you hold the
            // button - or Force Click - to drag out a selection, and a
            // touchscreen has neither. Long-pressing is the only equivalent, so
            // it stays available whatever the host's dragging style says - and
            // on a Mac set to Three-Finger Drag it is the only drag left.
        }

        // Gestures.
        // Pinch zoom is deliberately not mirrored. Whether it belongs on a
        // trackpad is a question about the trackpad's size: a Mac's is large
        // enough to tell a pinch from a two-finger swipe, and a phone's is not.
        // Following the host here turned the gesture back on for anybody whose
        // Mac has it, which is nearly everybody.
        if let Some(v) = self.swipe_navigate {
            mirror_binding(&mut c.bindings.two_finger_swipe_navigate, v, "navigate");
        }
        if let Some(v) = self.three_finger_horiz_swipe {
            mirror_binding(&mut c.bindings.three_finger_horiz_swipe, v, "spaces");
        }
        if let Some(v) = self.three_finger_vert_swipe {
            mirror_binding(&mut c.bindings.three_finger_vert_swipe, v, "missionControl");
        }
        if let Some(v) = self.four_finger_horiz_swipe {
            mirror_binding(&mut c.bindings.four_finger_horiz_swipe, v, "spaces");
        }
        if let Some(v) = self.four_finger_vert_swipe {
            mirror_binding(&mut c.bindings.four_finger_vert_swipe, v, "missionControl");
        }
        if let Some(v) = self.four_finger_pinch {
            mirror_binding(&mut c.bindings.four_finger_pinch, v, "launchpad");
        }
        if let Some(v) = self.five_finger_spread {
            mirror_binding(&mut c.bindings.five_finger_spread, v, "showDesktop");
        }
        if let Some(speed) = self.tracking_speed {
            // The slider runs 0..3; Apple's curve is private, so this is only a
            // rough alignment of the overall feel.
            c.sensitivity = (speed / 1.0).clamp(0.25, 3.0);
        }

        c
    }

    /// Which config field each mirrored setting decides.
    ///
    /// The pairs are `(config path, the setting's name in the report)`, and
    /// they exist so the settings page can say *why* a control is not editable
    /// instead of leaving one that quietly does nothing. The page used to keep
    /// its own copy of this table, which drifted within a day - a renamed row
    /// simply stopped locking its control, silently.
    ///
    /// **Edit this beside `apply_to`.** It is the same relationship written
    /// twice, once for the engine and once for the person looking at it, and
    /// the test below keeps the two honest about the *names*.
    /// The value the mirror writes into each binding it owns.
    ///
    /// Sent to the settings page so it can tell a control the host is deciding
    /// from one the user has taken *outside* the host's vocabulary - a volume
    /// swipe is not something a Mac trackpad has an opinion about, so showing
    /// it as "your computer decides this" would be false and would grey out a
    /// control that now works perfectly well.
    pub fn mirror_actions() -> Vec<(&'static str, &'static str)> {
        vec![
            ("bindings.twoFingerSwipeNavigate", "navigate"),
            ("bindings.threeFingerHorizSwipe", "spaces"),
            ("bindings.threeFingerVertSwipe", "missionControl"),
            ("bindings.fourFingerHorizSwipe", "spaces"),
            ("bindings.fourFingerVertSwipe", "missionControl"),
            ("bindings.fourFingerPinch", "launchpad"),
            ("bindings.fiveFingerSpread", "showDesktop"),
        ]
    }

    pub fn controls() -> Vec<(&'static str, &'static str)> {
        vec![
            ("scroll.natural", "Scrolling direction: Natural"),
            ("scroll.enabled", "Use trackpad for scrolling"),
            ("scroll.momentum", "Use inertia when scrolling"),
            ("scroll.horizontal", "Scroll horizontally"),
            ("bindings.twoFingerTap", "Secondary click (two fingers)"),
            ("bindings.cornerSecondaryClick", "Secondary click (corner)"),
            ("bindings.threeFingerTap", "Three finger tap"),
            (
                "bindings.twoFingerDoubleTap",
                "Smart zoom (two-finger double tap)",
            ),
            (
                "bindings.twoFingerSwipeNavigate",
                "Swipe between pages (two fingers)",
            ),
            (
                "bindings.threeFingerHorizSwipe",
                "Swipe between pages (three fingers)",
            ),
            (
                "bindings.threeFingerVertSwipe",
                "Mission Control (three fingers)",
            ),
            (
                "bindings.fourFingerHorizSwipe",
                "Swipe between full-screen apps",
            ),
            (
                "bindings.fourFingerVertSwipe",
                "Mission Control (four fingers)",
            ),
            ("bindings.fourFingerPinch", "Launchpad (four-finger pinch)"),
            (
                "bindings.fiveFingerSpread",
                "Show Desktop (five-finger spread)",
            ),
            ("drag.tapAndDrag", "Dragging style: without drag lock"),
            ("tap.doubleTapMs", "Double-click speed"),
            ("sensitivity", "Tracking speed"),
        ]
    }

    /// One line for the startup log.
    pub fn summary(&self) -> String {
        let rows = self.report();
        let on = rows.iter().filter(|r| r.status == Status::Mirrored).count();
        format!("{on}/{} settings mirrored", rows.len())
    }

    /// Every setting, its value here, and what PadRemote does with it.
    ///
    /// This exists because "does it mirror my trackpad?" was unanswerable from
    /// the outside, which cost several rounds of guessing. Every field of this
    /// struct must appear, so nothing can be silently ignored again.
    pub fn report(&self) -> Vec<Row> {
        let mut r = Vec::new();
        let mut flag = |setting: &str, name: &str, v: Option<bool>, status: Status| {
            r.push(Row {
                setting: setting.to_string(),
                name: name.to_string(),
                value: match v {
                    Some(true) => "on".into(),
                    Some(false) => "off".into(),
                    None => "not set".into(),
                },
                status,
            });
        };

        use Status::*;
        // The report answers "does this match my trackpad?", and a macOS
        // preference key is no answer at all on Windows. `cfg!` rather than a
        // runtime flag: the report is always about the machine it runs on.
        let key = |macos: &str, windows: &str, linux: &str| -> String {
            if cfg!(target_os = "macos") {
                macos
            } else if cfg!(target_os = "windows") {
                windows
            } else {
                linux
            }
            .to_string()
        };
        flag(
            "Scrolling direction: Natural",
            &key(
                "com.apple.swipescrolldirection",
                "ScrollDirection",
                "natural-scroll",
            ),
            self.natural_scroll,
            Mirrored,
        );
        flag(
            "Use trackpad for scrolling",
            &key(
                "TrackpadScroll",
                "PanEnabled",
                "two-finger-scrolling-enabled",
            ),
            self.scrolling,
            Mirrored,
        );
        flag(
            "Use inertia when scrolling",
            &key("TrackpadMomentumScroll", "-", "-"),
            self.momentum_scroll,
            Mirrored,
        );
        flag(
            "Scroll horizontally",
            &key("TrackpadHorizScroll", "-", "-"),
            self.horizontal_scroll,
            Mirrored,
        );
        flag(
            "Tap to click",
            &key("Clicking", "TapsEnabled", "tap-to-click"),
            self.tap_to_click,
            Mirrored,
        );
        flag(
            "Secondary click (two fingers)",
            &key(
                "TrackpadRightClick",
                "TwoFingerTapEnabled",
                "click-method=fingers",
            ),
            self.secondary_click,
            Mirrored,
        );
        flag(
            "Secondary click (corner)",
            &key("TrackpadCornerSecondaryClick", "-", "click-method=areas"),
            self.corner_secondary_click,
            Mirrored,
        );
        flag(
            "Three finger tap",
            &key(
                "TrackpadThreeFingerTapGesture",
                "ThreeFingerTapEnabled",
                "-",
            ),
            self.three_finger_tap,
            Mirrored,
        );
        flag(
            "Smart zoom (two-finger double tap)",
            &key("TrackpadTwoFingerDoubleTapGesture", "-", "-"),
            self.two_finger_double_tap,
            Approximated("macOS has no public smart-zoom event; sent as app zoom"),
        );
        flag(
            "Three finger drag",
            &key("TrackpadThreeFingerDrag", "-", "-"),
            self.three_finger_drag,
            NotPossible("three fingers are kept for the swipes; press and hold to drag"),
        );
        flag(
            "Dragging style: without drag lock",
            &key("Dragging", "-", "-"),
            self.dragging,
            Mirrored,
        );
        flag(
            "Zoom in or out (pinch)",
            &key("TrackpadPinch", "ZoomEnabled", "-"),
            self.pinch_zoom,
            Approximated("no public magnify event; sent as Cmd +/-"),
        );
        flag(
            "Rotate",
            &key("TrackpadRotate", "-", "-"),
            self.rotate,
            NotPossible("macOS exposes no way to synthesize a rotation gesture"),
        );
        flag(
            "Swipe between pages (two fingers)",
            &key("AppleEnableSwipeNavigateWithScrolls", "-", "-"),
            self.swipe_navigate,
            Mirrored,
        );
        flag(
            "Swipe between pages (three fingers)",
            &key(
                "TrackpadThreeFingerHorizSwipeGesture",
                "ThreeFingerSlideEnabled",
                "-",
            ),
            self.three_finger_horiz_swipe,
            Mirrored,
        );
        flag(
            "Mission Control (three fingers)",
            &key(
                "TrackpadThreeFingerVertSwipeGesture",
                "ThreeFingerSlideEnabled",
                "-",
            ),
            self.three_finger_vert_swipe,
            Mirrored,
        );
        flag(
            "Swipe between full-screen apps",
            &key(
                "TrackpadFourFingerHorizSwipeGesture",
                "FourFingerSlideEnabled",
                "-",
            ),
            self.four_finger_horiz_swipe,
            Mirrored,
        );
        flag(
            "Mission Control (four fingers)",
            &key(
                "TrackpadFourFingerVertSwipeGesture",
                "FourFingerSlideEnabled",
                "-",
            ),
            self.four_finger_vert_swipe,
            Mirrored,
        );
        flag(
            "Launchpad (four-finger pinch)",
            &key("TrackpadFourFingerPinchGesture", "-", "-"),
            self.four_finger_pinch,
            Mirrored,
        );
        flag(
            "Show Desktop (five-finger spread)",
            &key("TrackpadFiveFingerPinchGesture", "-", "-"),
            self.five_finger_spread,
            Mirrored,
        );
        flag(
            "Force Click",
            &key("com.apple.trackpad.forceClick", "-", "-"),
            self.force_click,
            NotPossible("a phone screen has no pressure sensor"),
        );
        flag(
            "Spring-loading",
            &key("com.apple.springing.enabled", "-", "-"),
            self.springing,
            Automatic("the OS springs folders open from the real drag events we send"),
        );

        let num = |v: Option<f64>, unit: &str| match v {
            Some(n) => format!("{n}{unit}"),
            None => "not set (system default)".to_string(),
        };
        r.push(Row {
            setting: "Double-click speed".into(),
            name: "com.apple.mouse.doubleClickThreshold".into(),
            value: num(self.double_click_seconds, " s"),
            status: Mirrored,
        });
        r.push(Row {
            setting: "Tracking speed".into(),
            name: "com.apple.trackpad.scaling".into(),
            value: num(self.tracking_speed, ""),
            status: Approximated("Apple's acceleration curve is private"),
        });
        r
    }
}

/// What PadRemote does with one host setting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    /// Reproduced exactly.
    Mirrored,
    /// Reproduced, but not faithfully - the reason says why.
    Approximated(&'static str),
    /// Cannot be reproduced at all - the reason says why.
    NotPossible(&'static str),
    /// Nothing to do; the OS already handles it.
    Automatic(&'static str),
}

impl Status {
    pub fn label(self) -> &'static str {
        match self {
            Self::Mirrored => "mirrored",
            Self::Approximated(_) => "approximated",
            Self::NotPossible(_) => "not possible",
            Self::Automatic(_) => "handled by the OS",
        }
    }

    pub fn detail(self) -> &'static str {
        match self {
            Self::Mirrored => "",
            Self::Approximated(w) | Self::NotPossible(w) | Self::Automatic(w) => w,
        }
    }
}

/// One line of the mirror report.
#[derive(Debug, Clone)]
pub struct Row {
    /// What System Settings calls it.
    pub setting: String,
    /// The underlying preference key.
    pub name: String,
    /// Its value on this machine.
    pub value: String,
    pub status: Status,
}

fn binding(enabled: bool, action: &str) -> String {
    if enabled {
        action.into()
    } else {
        "none".into()
    }
}

/// Let the host decide one binding - unless the user has put something there
/// that the host has no concept of.
///
/// Mirroring exists to reproduce the trackpad someone already has, and for
/// every action the host knows about that is exactly right. But PadRemote can
/// bind gestures the host cannot: a three-finger vertical swipe that changes the
/// volume is not a macOS trackpad function, so the host's "Mission Control:
/// three fingers" switch says nothing about it. Overwriting it anyway meant a
/// deliberate choice was quietly undone twice a second, with the settings page
/// showing the value the user picked and the engine running another.
///
/// So the mirror only ever overwrites a value it could have written itself.
fn mirror_binding(current: &mut String, enabled: bool, action: &str) {
    if current.is_empty() || current == "none" || current == action {
        *current = binding(enabled, action);
    }
}

/// Render the mirror report as an aligned text table, for the startup log.
pub fn text_report(rows: &[Row]) -> String {
    let w = rows.iter().map(|r| r.setting.len()).max().unwrap_or(0);
    let v = rows.iter().map(|r| r.value.len()).max().unwrap_or(0).max(5);
    let mut out = String::from("\n  Your trackpad, and what PadRemote does with it:\n\n");
    for r in rows {
        let detail = if r.status.detail().is_empty() {
            String::new()
        } else {
            format!("  ({})", r.status.detail())
        };
        out.push_str(&format!(
            "    {:<w$}  {:<v$}  {}{}\n",
            r.setting,
            r.value,
            r.status.label(),
            detail,
            w = w,
            v = v
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The exact reading taken from a real Mac. Three-finger tap is off there
    /// while PadRemote's own default maps it to middle-click, and four gestures
    /// are on that PadRemote shipped without - this case is the whole reason
    /// the mirroring exists.
    fn real_mac() -> HostTrackpad {
        HostTrackpad {
            natural_scroll: Some(true),
            scrolling: Some(true),
            momentum_scroll: Some(true),
            horizontal_scroll: Some(true),
            tap_to_click: Some(true),
            secondary_click: Some(true),
            corner_secondary_click: Some(false),
            three_finger_tap: Some(false),
            two_finger_double_tap: Some(true),
            double_click_seconds: None,
            three_finger_drag: Some(true),
            dragging: Some(false),
            pinch_zoom: Some(true),
            rotate: Some(true),
            three_finger_horiz_swipe: Some(false),
            three_finger_vert_swipe: Some(false),
            four_finger_horiz_swipe: Some(true),
            four_finger_vert_swipe: Some(true),
            four_finger_pinch: Some(true),
            five_finger_spread: Some(true),
            swipe_navigate: Some(false),
            force_click: Some(true),
            springing: Some(true),
            tracking_speed: None,
        }
    }

    #[test]
    fn mirrors_the_host_configuration() {
        let cfg = real_mac().apply_to(&Config::default());

        // The mismatch this feature was built to fix.
        assert_eq!(cfg.bindings.three_finger_tap, "none");
        // The host's Three-Finger Drag is not reproduced. Its three-finger
        // swipes read as off because macOS greys them out while it is on, and
        // that reading is mirrored as it stands - PadRemote no longer clears
        // them itself.
        assert_eq!(cfg.bindings.three_finger_horiz_swipe, "none");
        // Gestures the host uses that PadRemote defaulted to off.
        assert_eq!(cfg.bindings.four_finger_horiz_swipe, "spaces");
        assert_eq!(cfg.bindings.four_finger_vert_swipe, "missionControl");
        assert_eq!(cfg.bindings.four_finger_pinch, "launchpad");
        assert_eq!(cfg.bindings.five_finger_spread, "showDesktop");
        assert_eq!(cfg.bindings.two_finger_double_tap, "smartZoom");
        // Settings that already agreed must survive unchanged.
        assert!(cfg.scroll.natural && cfg.scroll.enabled && cfg.scroll.momentum);
        // Pinch zoom is no longer mirrored: it is a question about how big the
        // trackpad is, and the phone's answer differs from the Mac's.
        assert!(!cfg.zoom.enabled, "the host turned pinch zoom back on");
        assert_eq!(cfg.bindings.one_tap, "leftClick");
        assert_eq!(cfg.bindings.two_finger_tap, "rightClick");
        // Off on the host means off here.
        assert_eq!(cfg.bindings.corner_secondary_click, "none");
        assert_eq!(cfg.bindings.two_finger_swipe_navigate, "none");
    }

    #[test]
    fn inertia_is_taken_from_the_host() {
        let host = HostTrackpad {
            momentum_scroll: Some(false),
            ..Default::default()
        };
        assert!(
            !host.apply_to(&Config::default()).scroll.momentum,
            "momentum was hardcoded before"
        );
    }

    /// The three dragging-style flags describe one choice.
    /// Long-pressing substitutes for hardware the phone lacks, so no host
    /// setting may take it away - without it there is no way to drag out a text
    /// selection with one finger at all.
    #[test]
    fn long_press_drag_is_never_mirrored_away() {
        for host in [
            HostTrackpad {
                three_finger_drag: Some(true),
                dragging: Some(false),
                ..Default::default()
            },
            HostTrackpad {
                dragging: Some(false),
                ..Default::default()
            },
            real_mac(),
        ] {
            assert!(host.apply_to(&Config::default()).drag.press_and_drag);
        }
    }

    #[test]
    fn dragging_styles() {
        let style = |three, dragging| {
            let h = HostTrackpad {
                three_finger_drag: Some(three),
                dragging: Some(dragging),
                ..Default::default()
            };
            let c = h.apply_to(&Config::default());
            // press_and_drag is the phone's stand-in for holding a physical
            // button, so it stays on regardless of the host's dragging style.
            assert!(
                c.drag.press_and_drag,
                "long-press must survive any host style"
            );
            c.drag.tap_and_drag
        };
        // Three-Finger Drag means the Mac has no one-finger dragging, so
        // tap-and-drag has to be off here too - otherwise an ordinary
        // one-finger move turns into a drag the moment it follows a tap.
        assert!(!style(true, false), "Three-Finger Drag");
        // Both drag-lock styles are the same one-finger dragging to PadRemote,
        // which has no lock of its own.
        assert!(style(false, true), "one-finger dragging");
        assert!(!style(false, false), "dragging off");
    }

    #[test]
    fn double_click_threshold_follows_the_host() {
        // Unset means the macOS default, which the shipped config now matches.
        assert_eq!(
            HostTrackpad::default()
                .apply_to(&Config::default())
                .tap
                .double_tap_ms,
            500
        );

        let host = HostTrackpad {
            double_click_seconds: Some(0.8),
            ..Default::default()
        };
        assert_eq!(host.apply_to(&Config::default()).tap.double_tap_ms, 800);

        // A nonsensical stored value must not lock double-click out entirely.
        let silly = HostTrackpad {
            double_click_seconds: Some(0.0),
            ..Default::default()
        };
        assert_eq!(silly.apply_to(&Config::default()).tap.double_tap_ms, 100);
    }

    /// The Mac's three-finger drag no longer takes the three-finger swipes.
    ///
    /// It used to, because the fingers could not mean both. PadRemote has no
    /// three-finger drag any more, so there is nothing for them to lose to.
    #[test]
    fn three_finger_drag_leaves_the_three_finger_swipes_alone() {
        let host = HostTrackpad {
            three_finger_drag: Some(true),
            three_finger_horiz_swipe: Some(true),
            ..Default::default()
        };
        let cfg = host.apply_to(&Config::default());
        assert_eq!(cfg.bindings.three_finger_horiz_swipe, "spaces");
    }

    #[test]
    fn an_unreadable_setting_leaves_the_default_alone() {
        let cfg = HostTrackpad::default().apply_to(&Config::default());
        let default = Config::default();
        assert_eq!(
            cfg.bindings.three_finger_tap,
            default.bindings.three_finger_tap
        );
        assert_eq!(cfg.scroll.natural, default.scroll.natural);
        assert!(HostTrackpad::default().is_empty());
    }

    /// Every name in `controls()` must be a row the report actually produces.
    ///
    /// This is the drift the settings page kept hitting: a row renamed here,
    /// and a control on the page silently stopped explaining itself. A name
    /// that matches nothing is a lie the user cannot see.
    #[test]
    fn every_controlled_setting_names_a_real_row() {
        let rows = real_mac().report();
        for (field, setting) in HostTrackpad::controls() {
            assert!(
                rows.iter().any(|r| r.setting == setting),
                "controls() says {field} comes from \"{setting}\", which the report does not have"
            );
        }
    }

    #[test]
    fn follow_system_false_pins_the_file() {
        let mut cfg = Config {
            follow_system: false,
            ..Config::default()
        };
        cfg.bindings.three_finger_tap = "middleClick".into();
        let out = real_mac().apply_to(&cfg);
        assert_eq!(
            out.bindings.three_finger_tap, "middleClick",
            "the file must win"
        );
    }

    #[test]
    fn disabled_gestures_map_to_none() {
        let host = HostTrackpad {
            tap_to_click: Some(false),
            pinch_zoom: Some(false),
            ..Default::default()
        };
        let cfg = host.apply_to(&Config::default());
        // Tap to click is the exception: see `tap_to_click_is_never_mirrored_away`.
        assert_eq!(cfg.bindings.one_tap, "leftClick");
        assert!(!cfg.zoom.enabled);
    }

    /// Every setting must appear in the report.
    ///
    /// A summary that showed 8 of 11 fields once made a real change look like a
    /// no-op; the report is the user's only window onto this, so nothing may go
    /// missing from it.
    #[test]
    fn the_report_hides_nothing() {
        let rows = real_mac().report();
        let fields = 24; // every field of HostTrackpad
        assert_eq!(
            rows.len(),
            fields,
            "report covers {} of {fields} settings",
            rows.len()
        );

        // Every row says something useful about what PadRemote does.
        for row in &rows {
            assert!(!row.setting.is_empty() && !row.name.is_empty(), "{row:?}");
            assert!(!row.value.is_empty(), "{row:?}");
            if row.status != Status::Mirrored {
                assert!(
                    !row.status.detail().is_empty(),
                    "{row:?} must explain itself"
                );
            }
        }

        // The limits stay stated, never quietly dropped.
        let unsupported: Vec<&str> = rows
            .iter()
            .filter(|r| matches!(r.status, Status::NotPossible(_)))
            .map(|r| r.setting.as_str())
            .collect();
        assert!(unsupported.contains(&"Rotate"), "{unsupported:?}");
        assert!(unsupported.contains(&"Force Click"), "{unsupported:?}");
    }
}

#[cfg(test)]
mod mirror_scope_tests {
    use super::*;

    /// A gesture bound to something the host has never heard of stays put.
    ///
    /// `followSystem` is on by default, and the mirror runs about twice a
    /// second. Before this, assigning the volume to a three-finger vertical
    /// swipe was overwritten by the host's Mission Control switch within half a
    /// second - the settings page showed the choice, the engine ran the other
    /// thing, and nothing anywhere said why.
    #[test]
    fn the_mirror_leaves_choices_it_cannot_express() {
        let host = HostTrackpad {
            three_finger_vert_swipe: Some(true),
            four_finger_vert_swipe: Some(true),
            ..Default::default()
        };

        let mut cfg = Config::default();
        cfg.bindings.three_finger_vert_swipe = "volume".into();
        let out = host.apply_to(&cfg);

        assert_eq!(
            out.bindings.three_finger_vert_swipe, "volume",
            "the host overwrote a gesture it has no concept of"
        );
        // The one it does know about is still mirrored as before.
        assert_eq!(out.bindings.four_finger_vert_swipe, "missionControl");
    }

    /// And a gesture the host *does* own is still followed, on and off.
    #[test]
    fn the_mirror_still_owns_what_it_understands() {
        let mut host = HostTrackpad {
            three_finger_vert_swipe: Some(false),
            ..Default::default()
        };
        let mut cfg = Config::default();
        cfg.bindings.three_finger_vert_swipe = "missionControl".into();
        assert_eq!(host.apply_to(&cfg).bindings.three_finger_vert_swipe, "none");

        host.three_finger_vert_swipe = Some(true);
        cfg.bindings.three_finger_vert_swipe = "none".into();
        assert_eq!(
            host.apply_to(&cfg).bindings.three_finger_vert_swipe,
            "missionControl"
        );
    }
}

#[cfg(test)]
mod mirror_actions_tests {
    use super::*;

    /// `mirror_actions` has to describe what `apply_to` actually writes.
    ///
    /// Two lists of the same facts, and the settings page believes the one that
    /// is not the code doing the work - so a renamed action would leave a
    /// control greyed out for a reason that had stopped being true. Checked by
    /// running the mirror and reading the result back by config path.
    #[test]
    fn every_declared_mirror_action_is_the_one_written() {
        for (path, action) in HostTrackpad::mirror_actions() {
            let mut host = HostTrackpad::default();
            // Turn on whichever host switch feeds this field.
            for flag in [
                &mut host.swipe_navigate,
                &mut host.three_finger_horiz_swipe,
                &mut host.three_finger_vert_swipe,
                &mut host.four_finger_horiz_swipe,
                &mut host.four_finger_vert_swipe,
                &mut host.four_finger_pinch,
                &mut host.five_finger_spread,
            ] {
                *flag = Some(true);
            }
            let out = host.apply_to(&Config::default());
            let json = serde_json::to_value(&out).expect("config serialises");
            let mut node = &json;
            for part in path.split('.') {
                node = node
                    .get(part)
                    .unwrap_or_else(|| panic!("{path} is not a config field"));
            }
            assert_eq!(
                node.as_str(),
                Some(action),
                "{path} is declared as {action} but the mirror writes something else",
            );
        }
    }
}

#[cfg(test)]
mod tap_to_click_tests {
    use super::*;

    /// A phone must always be able to click.
    ///
    /// macOS ships with "Tap to click" off, and a Mac trackpad does not need
    /// it, because you press the trackpad down instead. A phone screen has no
    /// button, so mirroring that setting left a fresh install unable to click
    /// at all: the single most common gesture, doing nothing, on first run.
    #[test]
    fn tap_to_click_is_never_mirrored_away() {
        for host in [
            HostTrackpad {
                tap_to_click: Some(false),
                ..Default::default()
            },
            HostTrackpad::default(),
        ] {
            let cfg = host.apply_to(&Config::default());
            assert_eq!(
                cfg.bindings.one_tap, "leftClick",
                "the pad was left with no way to click"
            );
        }
    }

    /// And a deliberate choice still survives the host having it on.
    #[test]
    fn a_chosen_tap_action_is_not_overwritten() {
        let host = HostTrackpad {
            tap_to_click: Some(true),
            ..Default::default()
        };
        let mut cfg = Config::default();
        cfg.bindings.one_tap = "spotlight".into();
        assert_eq!(host.apply_to(&cfg).bindings.one_tap, "spotlight");
    }
}
