//! Reading the macOS trackpad preferences (plan.md section 11, "adapt to host").
//!
//! Uses CFPreferences rather than shelling out to `defaults`: this is polled
//! every few seconds, `cfprefsd` is the authority (files on disk can be stale),
//! and spawning processes on a timer is a waste.

use core_foundation::base::{CFType, TCFType};
use core_foundation::boolean::CFBoolean;
use core_foundation::number::CFNumber;
use core_foundation::string::{CFString, CFStringRef};
// The preferences bindings live in the -sys crate; core-foundation has no safe
// wrapper for them.
use core_foundation_sys::preferences::{kCFPreferencesAnyApplication, CFPreferencesCopyAppValue};

use super::HostTrackpad;

/// The built-in trackpad.
const DOMAIN_BUILTIN: &str = "com.apple.AppleMultitouchTrackpad";
/// An external Magic Trackpad.
const DOMAIN_BLUETOOTH: &str = "com.apple.driver.AppleBluetoothMultitouch.trackpad";
/// Read one preference and coerce it to on/off.
///
/// macOS is inconsistent about the type here, and getting this wrong reads as
/// "setting not configured": `Clicking` and `TrackpadRightClick` are stored as
/// booleans while `TrackpadPinch` and the swipe switches are integers. The
/// integers are tri-state - 0 is off, and both 1 and 2 mean on (2 is the
/// "with two or three fingers" variant in System Settings).
fn read_flag_in(domain: CFStringRef, key: &str) -> Option<bool> {
    let key = CFString::new(key);
    // Safety: `key` is a valid CFString and `domain` is either a CFString we
    // own or the framework's own constant. The result is NULL or a +1
    // reference, which wrap_under_create_rule takes ownership of.
    let value = unsafe { CFPreferencesCopyAppValue(key.as_concrete_TypeRef(), domain) };
    if value.is_null() {
        return None;
    }
    let value = unsafe { CFType::wrap_under_create_rule(value) };
    if let Some(b) = value.downcast::<CFBoolean>() {
        return Some(b.into());
    }
    value
        .downcast::<CFNumber>()
        .and_then(|n| n.to_i64())
        .map(|n| n != 0)
}

/// A trackpad switch, preferring the built-in trackpad over an external one.
fn read_flag(key: &str) -> Option<bool> {
    let builtin = CFString::new(DOMAIN_BUILTIN);
    let bluetooth = CFString::new(DOMAIN_BLUETOOTH);
    read_flag_in(builtin.as_concrete_TypeRef(), key)
        .or_else(|| read_flag_in(bluetooth.as_concrete_TypeRef(), key))
}

/// A preference stored as a number rather than a switch, such as the
/// double-click threshold (in seconds) or the tracking-speed slider.
fn read_float_in(domain: CFStringRef, key: &str) -> Option<f64> {
    let key = CFString::new(key);
    // Safety: as read_flag_in.
    let value = unsafe { CFPreferencesCopyAppValue(key.as_concrete_TypeRef(), domain) };
    if value.is_null() {
        return None;
    }
    let value = unsafe { CFType::wrap_under_create_rule(value) };
    value.downcast::<CFNumber>().and_then(|n| n.to_f64())
}

fn read_global_float(key: &str) -> Option<f64> {
    read_float_in(unsafe { kCFPreferencesAnyApplication }, key)
}

/// A system-wide setting, such as scroll direction. This must use the
/// framework's own "any application" constant - a CFString of the same name is
/// just an app id that does not exist.
fn read_global_flag(key: &str) -> Option<bool> {
    read_flag_in(unsafe { kCFPreferencesAnyApplication }, key)
}

/// Take a reading of the host trackpad.
pub fn read() -> HostTrackpad {
    HostTrackpad {
        natural_scroll: read_global_flag("com.apple.swipescrolldirection"),
        scrolling: read_flag("TrackpadScroll"),
        momentum_scroll: read_flag("TrackpadMomentumScroll"),
        horizontal_scroll: read_flag("TrackpadHorizScroll"),
        tap_to_click: read_flag("Clicking"),
        secondary_click: read_flag("TrackpadRightClick"),
        corner_secondary_click: read_flag("TrackpadCornerSecondaryClick"),
        three_finger_tap: read_flag("TrackpadThreeFingerTapGesture"),
        two_finger_double_tap: read_flag("TrackpadTwoFingerDoubleTapGesture"),
        pinch_zoom: read_flag("TrackpadPinch"),
        rotate: read_flag("TrackpadRotate"),
        // The dragging style is three flags describing one choice.
        three_finger_drag: read_flag("TrackpadThreeFingerDrag"),
        dragging: read_flag("Dragging"),
        three_finger_horiz_swipe: read_flag("TrackpadThreeFingerHorizSwipeGesture"),
        three_finger_vert_swipe: read_flag("TrackpadThreeFingerVertSwipeGesture"),
        four_finger_horiz_swipe: read_flag("TrackpadFourFingerHorizSwipeGesture"),
        four_finger_vert_swipe: read_flag("TrackpadFourFingerVertSwipeGesture"),
        four_finger_pinch: read_flag("TrackpadFourFingerPinchGesture"),
        five_finger_spread: read_flag("TrackpadFiveFingerPinchGesture"),
        swipe_navigate: read_global_flag("AppleEnableSwipeNavigateWithScrolls"),
        force_click: read_global_flag("com.apple.trackpad.forceClick"),
        springing: read_global_flag("com.apple.springing.enabled"),
        // Seconds. Absent means the macOS default of 0.5 s.
        double_click_seconds: read_global_float("com.apple.mouse.doubleClickThreshold"),
        tracking_speed: read_global_float("com.apple.trackpad.scaling"),
    }
}

/// Whether the Mission Control keyboard shortcuts the swipe gestures are
/// synthesized from are still enabled.
///
/// Swipes are produced by sending Ctrl+arrow. If the user has switched those
/// shortcuts off, the keystroke goes nowhere - better to say so than to look
/// broken. Reads `AppleSymbolicHotKeys` 79/81 ("Move left/right a space").
pub fn space_shortcuts_enabled() -> bool {
    let key = CFString::new("AppleSymbolicHotKeys");
    let domain = CFString::new("com.apple.symbolichotkeys");
    let value = unsafe {
        CFPreferencesCopyAppValue(key.as_concrete_TypeRef(), domain.as_concrete_TypeRef())
    };
    if value.is_null() {
        // Never customised, so the macOS defaults are in force and enabled.
        return true;
    }
    let value = unsafe { CFType::wrap_under_create_rule(value) };
    let Some(dict) = value.downcast::<core_foundation::dictionary::CFDictionary>() else {
        return true;
    };
    // Absent entries mean "untouched", which means enabled.
    ["79", "81"].iter().all(|id| {
        let Some(entry) = dict.find(CFString::new(id).as_CFTypeRef()) else {
            return true;
        };
        let entry = unsafe { CFType::wrap_under_get_rule(*entry) };
        let Some(entry) = entry.downcast::<core_foundation::dictionary::CFDictionary>() else {
            return true;
        };
        match entry.find(CFString::new("enabled").as_CFTypeRef()) {
            Some(v) => {
                let v = unsafe { CFType::wrap_under_get_rule(*v) };
                v.downcast::<core_foundation::boolean::CFBoolean>()
                    .map(|b| b.into())
                    .unwrap_or(true)
            }
            None => true,
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Reads the machine this runs on. Asserts only shape, never specific
    /// values - the whole point is that they differ per machine.
    #[test]
    fn reading_the_host_does_not_panic() {
        let host = read();
        println!("host trackpad: {}", host.summary());
        // Every Mac has a trackpad domain or a scroll direction; if this is
        // ever empty on a Mac, the domains have moved and we want to know.
        assert!(!host.is_empty(), "read nothing at all from a macOS host");
    }

    /// Repeated reads must agree.
    ///
    /// The runtime treats "the reading changed" as a reason to rebuild the
    /// config, so a read that flaps between a value and `None` would silently
    /// switch gestures off and on again.
    #[test]
    fn reads_are_stable() {
        let first = read();
        for i in 0..50 {
            assert_eq!(read(), first, "reading changed on attempt {i}");
        }
    }

    #[test]
    fn shortcut_check_does_not_panic() {
        println!("space shortcuts enabled: {}", space_shortcuts_enabled());
    }
}
