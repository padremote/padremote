//! Reading Linux touchpad settings.
//!
//! There is no single place to look. The kernel and libinput know nothing about
//! user preference - they are told what to do by whatever desktop is running -
//! so this asks the desktop:
//!
//! - **GNOME** (and Cinnamon, Budgie, anything on GSettings) answers through
//!   `gsettings get org.gnome.desktop.peripherals.touchpad …`.
//! - **KDE** keeps the same choices in `~/.config/kcminputrc`, as INI.
//!
//! Anything else reports nothing, and PadRemote's own defaults stand - which is
//! the right outcome, not a failure. A desktop that cannot be read is not a
//! desktop whose settings can be safely guessed.
//!
//! As with the Windows reader, the parsing and the mapping are pure and tested
//! from any machine; only [`read`] shells out.

use super::HostTrackpad;

/// The subset of a desktop's touchpad settings PadRemote can use.
///
/// One shape for both sources, so the mapping is written once.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Raw {
    pub natural_scroll: Option<bool>,
    pub tap_to_click: Option<bool>,
    pub two_finger_scroll: Option<bool>,
    /// libinput's click method: two-finger tap gives the secondary button only
    /// when it is set to `fingers`.
    pub click_method_fingers: Option<bool>,
    /// GNOME's pointer speed, -1.0 (slowest) to 1.0 (fastest), 0 being default.
    pub speed: Option<f64>,
    pub double_click_ms: Option<u32>,
}

/// GNOME's `gsettings get` output, one value.
///
/// Values come back quoted, typed and occasionally as `uint32 400`, so this
/// takes the last word and strips the quotes rather than trusting a format.
pub fn parse_gsettings_value(out: &str) -> Option<&str> {
    let v = out.trim();
    if v.is_empty() || v == "nothing" {
        return None;
    }
    let v = v.rsplit(' ').next().unwrap_or(v);
    Some(v.trim_matches('\'').trim_matches('"'))
}

/// GSettings' own spelling of a boolean. Used by the reader and by its tests,
/// so it exists on every platform even where the reader does not.
pub fn as_bool(v: &str) -> Option<bool> {
    match v {
        "true" => Some(true),
        "false" => Some(false),
        _ => None,
    }
}

/// KDE's `kcminputrc`.
///
/// Every touchpad gets its own `[Libinput][vendor][product][name]` section, so
/// there is no fixed section name to look for - this takes the first value it
/// finds for each key. A machine with two touchpads is not a case worth being
/// clever about; a laptop with a dock is far more likely to have a *mouse*
/// section, and mouse sections carry different keys.
pub fn parse_kcminputrc(text: &str) -> Raw {
    let mut raw = Raw::default();
    let mut in_libinput = false;
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_libinput = line.starts_with("[Libinput]");
            continue;
        }
        if !in_libinput {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let (key, value) = (key.trim(), value.trim());
        let yes = value.eq_ignore_ascii_case("true") || value == "1";
        match key {
            "NaturalScroll" if raw.natural_scroll.is_none() => raw.natural_scroll = Some(yes),
            "TapToClick" if raw.tap_to_click.is_none() => raw.tap_to_click = Some(yes),
            "ScrollTwoFinger" if raw.two_finger_scroll.is_none() => {
                raw.two_finger_scroll = Some(yes)
            }
            "ClickMethodClickfinger" if raw.click_method_fingers.is_none() => {
                raw.click_method_fingers = Some(yes)
            }
            "PointerAcceleration" if raw.speed.is_none() => {
                raw.speed = value.parse::<f64>().ok();
            }
            _ => {}
        }
    }
    raw
}

/// The desktop's settings, in PadRemote's own shape.
pub fn map(raw: &Raw) -> HostTrackpad {
    HostTrackpad {
        natural_scroll: raw.natural_scroll,
        scrolling: raw.two_finger_scroll,
        // libinput always coasts, and no desktop exposes a switch for it.
        momentum_scroll: None,
        horizontal_scroll: None,

        tap_to_click: raw.tap_to_click,
        secondary_click: raw.click_method_fingers,
        // "Areas" click method is the alternative, and it is a *bottom corner*
        // click rather than a two-finger one - so the two are opposites.
        corner_secondary_click: raw.click_method_fingers.map(|f| !f),
        three_finger_tap: None,
        two_finger_double_tap: None,
        double_click_seconds: raw.double_click_ms.map(|ms| ms as f64 / 1000.0),

        three_finger_drag: None,
        dragging: None,

        pinch_zoom: None,
        rotate: None,
        // Multi-finger swipes are handled by the compositor and are not
        // exposed as switches. Leaving PadRemote's defaults alone is right:
        // they map to the shortcuts the desktop itself listens for.
        three_finger_horiz_swipe: None,
        three_finger_vert_swipe: None,
        four_finger_horiz_swipe: None,
        four_finger_vert_swipe: None,
        four_finger_pinch: None,
        five_finger_spread: None,
        swipe_navigate: None,

        force_click: None,
        springing: None,
        // GNOME's -1..1 slider, centred at 0, onto PadRemote's multiplier,
        // centred at 1. KDE's `PointerAcceleration` uses the same range.
        tracking_speed: raw.speed.map(|s| (1.0 + s).clamp(0.25, 3.0)),
    }
}

/// Ask whichever desktop is running. Unix only, and never fatal.
#[cfg(all(unix, not(target_os = "macos")))]
pub fn read() -> HostTrackpad {
    let raw = read_gsettings().unwrap_or_else(|| {
        std::fs::read_to_string(kde_config_path())
            .map(|text| parse_kcminputrc(&text))
            .unwrap_or_default()
    });
    map(&raw)
}

#[cfg(all(unix, not(target_os = "macos")))]
fn kde_config_path() -> std::path::PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("kcminputrc")
}

/// `gsettings`, if this is a GSettings desktop with the touchpad schema.
///
/// `None` when the tool or the schema is missing - which is how a KDE or
/// Sway machine falls through to the file above rather than reporting a
/// touchpad with every setting switched off.
#[cfg(all(unix, not(target_os = "macos")))]
fn read_gsettings() -> Option<Raw> {
    const PAD: &str = "org.gnome.desktop.peripherals.touchpad";
    let get = |schema: &str, key: &str| -> Option<String> {
        let out = std::process::Command::new("gsettings")
            .args(["get", schema, key])
            .output()
            .ok()?;
        if !out.status.success() {
            return None;
        }
        let text = String::from_utf8_lossy(&out.stdout).to_string();
        parse_gsettings_value(&text).map(|s| s.to_string())
    };

    // One probe first: if the schema is not installed, `gsettings` fails and
    // there is nothing here to read.
    let natural = get(PAD, "natural-scroll")?;
    Some(Raw {
        natural_scroll: as_bool(&natural),
        tap_to_click: get(PAD, "tap-to-click").as_deref().and_then(as_bool),
        two_finger_scroll: get(PAD, "two-finger-scrolling-enabled")
            .as_deref()
            .and_then(as_bool),
        click_method_fingers: get(PAD, "click-method").map(|m| m == "fingers"),
        speed: get(PAD, "speed").and_then(|s| s.parse().ok()),
        double_click_ms: get("org.gnome.desktop.peripherals.mouse", "double-click")
            .and_then(|s| s.parse().ok()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn booleans_are_gsettings_spelling() {
        assert_eq!(as_bool("true"), Some(true));
        assert_eq!(as_bool("false"), Some(false));
        assert_eq!(as_bool("yes"), None, "anything else is not an answer");
    }

    #[test]
    fn gsettings_values_come_back_typed_and_quoted() {
        assert_eq!(parse_gsettings_value("true\n"), Some("true"));
        assert_eq!(parse_gsettings_value("'fingers'\n"), Some("fingers"));
        assert_eq!(parse_gsettings_value("uint32 400\n"), Some("400"));
        assert_eq!(parse_gsettings_value("-0.35\n"), Some("-0.35"));
        assert_eq!(parse_gsettings_value("  \n"), None);
    }

    /// A real KDE file: several sections, one of which is a mouse.
    #[test]
    fn kde_reads_the_touchpad_section() {
        let raw = parse_kcminputrc(
            "[Libinput][1739][52781][SYNA8004:00 06CB:CD8B Touchpad]\n\
             NaturalScroll=true\n\
             TapToClick=true\n\
             ClickMethodClickfinger=true\n\
             PointerAcceleration=0.400\n\
             \n\
             [Libinput][1133][16500][Logitech USB Receiver]\n\
             NaturalScroll=false\n\
             PointerAcceleration=-0.100\n\
             \n\
             [Keyboard]\n\
             NumLock=0\n",
        );
        assert_eq!(raw.natural_scroll, Some(true));
        assert_eq!(raw.tap_to_click, Some(true));
        assert_eq!(raw.click_method_fingers, Some(true));
        assert_eq!(raw.speed, Some(0.4));
    }

    /// Sections that are not libinput must not contribute values.
    #[test]
    fn other_sections_are_ignored() {
        let raw = parse_kcminputrc("[Keyboard]\nNaturalScroll=true\n");
        assert_eq!(raw.natural_scroll, None);
    }

    #[test]
    fn an_unreadable_desktop_changes_nothing() {
        let host = map(&Raw::default());
        assert!(host.is_empty());
        let cfg = host.apply_to(&crate::gesture::Config::default());
        assert_eq!(
            cfg.scroll.natural,
            crate::gesture::Config::default().scroll.natural
        );
    }

    #[test]
    fn a_gnome_reading_reaches_the_engine() {
        let cfg = map(&Raw {
            natural_scroll: Some(false),
            tap_to_click: Some(true),
            two_finger_scroll: Some(true),
            click_method_fingers: Some(true),
            speed: Some(0.5),
            double_click_ms: Some(400),
        })
        .apply_to(&crate::gesture::Config::default());
        assert!(!cfg.scroll.natural);
        assert!(cfg.scroll.enabled);
        assert_eq!(cfg.bindings.one_tap, "leftClick");
        assert_eq!(cfg.bindings.two_finger_tap, "rightClick");
        assert_eq!(cfg.tap.double_tap_ms, 400);
        assert!((cfg.sensitivity - 1.5).abs() < 1e-9, "0.5 above centre");
    }
}
