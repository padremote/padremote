//! Reading Windows Precision Touchpad settings.
//!
//! Windows keeps them in the registry under
//! `HKCU\SOFTWARE\Microsoft\Windows\CurrentVersion\PrecisionTouchPad`, one
//! value per switch in Settings → Bluetooth & devices → Touchpad.
//!
//! The file is split so that the half that matters can be tested from any
//! machine: [`Raw`] is the numbers exactly as the registry holds them, [`map`]
//! turns them into a [`HostTrackpad`] and is pure, and only [`read`] touches
//! Windows. That is deliberate - this was written and tested on a Mac, and a
//! mapping nobody can exercise is a mapping nobody can trust.
//!
//! **Not yet confirmed on real hardware.** Every value below is read the way
//! Microsoft documents it, but only `ScrollDirection` is genuinely ambiguous in
//! the wild, and it is the one that matters most: get its polarity backwards
//! and scrolling runs the wrong way, which is exactly the bug the mirroring
//! exists to prevent. `padremote --headless` prints the mirror report, which
//! shows the raw value beside its meaning, so confirming it takes one run.

use super::HostTrackpad;

/// Where the touchpad's own settings live.
#[cfg(target_os = "windows")]
const TOUCHPAD_KEY: &str = r"SOFTWARE\Microsoft\Windows\CurrentVersion\PrecisionTouchPad";
/// Double-click speed is a mouse setting, not a touchpad one.
#[cfg(target_os = "windows")]
const MOUSE_KEY: &str = r"Control Panel\Mouse";

/// A multi-finger slide's assigned action. 0 means the gesture is off.
pub const SLIDE_NOTHING: u32 = 0;

/// The registry values, exactly as stored. `None` is "the user has never
/// touched this", which must leave PadRemote's own default alone.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Raw {
    /// `ScrollDirection`: 0 = down motion scrolls up (what macOS calls
    /// natural), 1 = down motion scrolls down.
    pub scroll_direction: Option<u32>,
    pub taps_enabled: Option<u32>,
    pub two_finger_tap_enabled: Option<u32>,
    pub three_finger_tap_enabled: Option<u32>,
    /// `PanEnabled`: drag two fingers to scroll.
    pub pan_enabled: Option<u32>,
    pub zoom_enabled: Option<u32>,
    /// `CursorSpeed`: 0-20, 10 being the middle of the slider.
    pub cursor_speed: Option<u32>,
    /// `ThreeFingerSlideEnabled` / `FourFingerSlideEnabled`: 0 off, otherwise
    /// the action Windows has assigned.
    pub three_finger_slide: Option<u32>,
    pub four_finger_slide: Option<u32>,
    /// `Control Panel\Mouse\DoubleClickSpeed`, in milliseconds, stored as text.
    pub double_click_ms: Option<u32>,
}

/// Turn the registry's numbers into the shape the mapping expects.
///
/// Pure on purpose: this is the part that can be wrong, so it is the part that
/// is tested.
pub fn map(raw: &Raw) -> HostTrackpad {
    let on = |v: Option<u32>| v.map(|n| n != 0);
    HostTrackpad {
        // 0 is "down motion scrolls up": the content follows the fingers, which
        // is what every other platform calls natural scrolling.
        natural_scroll: raw.scroll_direction.map(|v| v == 0),
        scrolling: on(raw.pan_enabled),
        // Windows has no separate switches for these two, and inventing an
        // answer would be worse than leaving PadRemote's own defaults alone.
        momentum_scroll: None,
        horizontal_scroll: None,

        tap_to_click: on(raw.taps_enabled),
        secondary_click: on(raw.two_finger_tap_enabled),
        // Corner secondary click is not a Precision Touchpad feature.
        corner_secondary_click: None,
        three_finger_tap: on(raw.three_finger_tap_enabled),
        // No smart zoom on Windows; the double tap is left to PadRemote.
        two_finger_double_tap: None,
        double_click_seconds: raw.double_click_ms.map(|ms| ms as f64 / 1000.0),

        // Windows has no three-finger drag; dragging is done by pressing and
        // moving, which needs no flag.
        three_finger_drag: None,
        dragging: None,

        pinch_zoom: on(raw.zoom_enabled),
        rotate: None,
        // One switch per finger count covers both directions: Windows assigns a
        // pair of actions to a three-finger slide, not one per axis.
        three_finger_horiz_swipe: raw.three_finger_slide.map(|v| v != SLIDE_NOTHING),
        three_finger_vert_swipe: raw.three_finger_slide.map(|v| v != SLIDE_NOTHING),
        four_finger_horiz_swipe: raw.four_finger_slide.map(|v| v != SLIDE_NOTHING),
        four_finger_vert_swipe: raw.four_finger_slide.map(|v| v != SLIDE_NOTHING),
        four_finger_pinch: None,
        five_finger_spread: None,
        swipe_navigate: None,

        force_click: None,
        springing: None,
        // The slider runs 0-20 with 10 in the middle, and PadRemote's own scale
        // has 1.0 in the middle, so the middle maps to the middle.
        tracking_speed: raw.cursor_speed.map(|v| v as f64 / 10.0),
    }
}

/// Read the registry. Windows only; everywhere else this file is just the
/// mapping above and its tests.
#[cfg(target_os = "windows")]
pub fn read() -> HostTrackpad {
    use winreg::enums::HKEY_CURRENT_USER;
    use winreg::RegKey;

    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let pad = hkcu.open_subkey(TOUCHPAD_KEY).ok();
    let dword = |name: &str| pad.as_ref().and_then(|k| k.get_value::<u32, _>(name).ok());

    // Double-click speed is stored as a *string* of milliseconds, in a
    // different key, for reasons that predate all of this.
    let double_click_ms = hkcu
        .open_subkey(MOUSE_KEY)
        .ok()
        .and_then(|k| k.get_value::<String, _>("DoubleClickSpeed").ok())
        .and_then(|s| s.trim().parse::<u32>().ok());

    map(&Raw {
        scroll_direction: dword("ScrollDirection"),
        taps_enabled: dword("TapsEnabled"),
        two_finger_tap_enabled: dword("TwoFingerTapEnabled"),
        three_finger_tap_enabled: dword("ThreeFingerTapEnabled"),
        pan_enabled: dword("PanEnabled"),
        zoom_enabled: dword("ZoomEnabled"),
        cursor_speed: dword("CursorSpeed"),
        three_finger_slide: dword("ThreeFingerSlideEnabled"),
        four_finger_slide: dword("FourFingerSlideEnabled"),
        double_click_ms,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A machine with everything on and the sliders centred.
    fn typical() -> Raw {
        Raw {
            scroll_direction: Some(0),
            taps_enabled: Some(1),
            two_finger_tap_enabled: Some(1),
            three_finger_tap_enabled: Some(1),
            pan_enabled: Some(1),
            zoom_enabled: Some(1),
            cursor_speed: Some(10),
            three_finger_slide: Some(1),
            four_finger_slide: Some(1),
            double_click_ms: Some(500),
        }
    }

    #[test]
    fn a_typical_machine_maps_cleanly() {
        let host = map(&typical());
        assert_eq!(host.natural_scroll, Some(true));
        assert_eq!(host.tap_to_click, Some(true));
        assert_eq!(host.secondary_click, Some(true));
        assert_eq!(host.pinch_zoom, Some(true));
        assert_eq!(host.tracking_speed, Some(1.0), "centred slider is neutral");
        assert_eq!(host.double_click_seconds, Some(0.5));
        assert!(!host.is_empty());
    }

    /// The polarity that matters. Getting this backwards scrolls the wrong way,
    /// which is the exact failure the mirroring exists to prevent.
    #[test]
    fn scroll_direction_one_is_not_natural() {
        assert_eq!(
            map(&Raw {
                scroll_direction: Some(1),
                ..Raw::default()
            })
            .natural_scroll,
            Some(false),
            "1 is 'down motion scrolls down' - the traditional direction"
        );
        assert_eq!(
            map(&Raw {
                scroll_direction: Some(0),
                ..Raw::default()
            })
            .natural_scroll,
            Some(true)
        );
    }

    /// A switch the user has never touched must not be reported as "off".
    #[test]
    fn absent_values_stay_absent() {
        let host = map(&Raw::default());
        assert!(host.is_empty(), "nothing read means nothing mirrored");
        assert_eq!(host.natural_scroll, None);
        assert_eq!(host.tap_to_click, None);
    }

    /// A slide set to "nothing" is a gesture turned off, and must read that way
    /// rather than as an unread setting.
    #[test]
    fn a_disabled_slide_is_off_not_unknown() {
        let host = map(&Raw {
            three_finger_slide: Some(SLIDE_NOTHING),
            four_finger_slide: Some(2),
            ..Raw::default()
        });
        assert_eq!(host.three_finger_horiz_swipe, Some(false));
        assert_eq!(host.three_finger_vert_swipe, Some(false));
        assert_eq!(host.four_finger_horiz_swipe, Some(true));
    }

    /// The mapping has to survive the whole way into a `Config`, not just into
    /// a `HostTrackpad` - that is the part the engine actually reads.
    #[test]
    fn a_windows_reading_reaches_the_engine() {
        let cfg = map(&Raw {
            scroll_direction: Some(1),
            taps_enabled: Some(0),
            zoom_enabled: Some(0),
            ..typical()
        })
        .apply_to(&crate::gesture::Config::default());
        assert!(!cfg.scroll.natural, "the host said traditional scrolling");
        // Tap to click is mirrored on but never off, on every platform: the
        // phone has no button to fall back to. See `tap_to_click_tests`.
        assert_eq!(
            cfg.bindings.one_tap, "leftClick",
            "the pad must still click"
        );
        assert!(!cfg.zoom.enabled);
    }
}
