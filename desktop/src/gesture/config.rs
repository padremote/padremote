//! Tunables shared by every platform (plan.md section 9.9).
//!
//! The defaults are compiled in from `config.default.json`, seeded to the user
//! config directory on first run, and hot-reloaded when that file changes - so
//! behaviour can be tuned without a rebuild.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// The shipped defaults, embedded so the app always has a valid config.
pub const DEFAULT_JSON: &str = include_str!("../../config.default.json");

/// What `version` means today, and what `migrate` brings an older file up to.
///
/// 1: the original file.
/// 2: `drag.tapAndDrag` off. See `migrate`.
pub const CURRENT_VERSION: u32 = 2;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub version: u32,
    /// Mirror the host OS's own trackpad settings on top of this file.
    ///
    /// On by default: PadRemote should behave like the trackpad the user
    /// already has, not like whatever this file happened to ship with. Set it
    /// to `false` to pin behaviour to this file alone.
    #[serde(rename = "followSystem")]
    pub follow_system: bool,
    pub sensitivity: f64,
    pub accel: AccelCfg,
    pub tap: TapCfg,
    pub scroll: ScrollCfg,
    pub zoom: ZoomCfg,
    pub drag: DragCfg,
    pub swipe: SwipeCfg,
    pub bindings: Bindings,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AccelCfg {
    pub gain: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct TapCfg {
    #[serde(rename = "tapMaxMs")]
    pub tap_max_ms: u32,
    #[serde(rename = "tapMaxPx")]
    pub tap_max_px: f64,
    #[serde(rename = "doubleTapMs")]
    pub double_tap_ms: u32,
    #[serde(rename = "pressMs")]
    pub press_ms: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ScrollCfg {
    /// "Use trackpad for scrolling". Off means two fingers do nothing.
    pub enabled: bool,
    pub natural: bool,
    pub momentum: bool,
    pub speed: f64,
    /// How much scrolling accelerates with finger speed. 0 is one-to-one with
    /// the finger; higher makes a quick flick cover more ground.
    pub accel: f64,
    /// Side-to-side scrolling. Some users turn this off on the real trackpad.
    pub horizontal: bool,
}

/// Which ways of starting a drag are enabled. Both are one-finger styles that
/// substitute for holding a physical button, and macOS offers the same pair as
/// separate switches.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct DragCfg {
    #[serde(rename = "pressAndDrag")]
    pub press_and_drag: bool,
    #[serde(rename = "tapAndDrag")]
    pub tap_and_drag: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SwipeCfg {
    /// How far the fingers must travel, in surface pixels, before it counts.
    #[serde(rename = "minPx")]
    pub min_px: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ZoomCfg {
    pub enabled: bool,
    pub backend: String,
    pub threshold: f64,
}

/// Gesture -> action. The swipe keys map one-to-one onto the macOS trackpad
/// preferences so the host's configuration can be mirrored directly.
/// Values: "none" | "spaces" | "missionControl" | "appWindows"
/// (tap bindings: "none" | "leftClick" | "rightClick" | "middleClick").
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Bindings {
    #[serde(rename = "twoFingerSwipeLeft")]
    pub two_finger_swipe_left: String,
    #[serde(rename = "twoFingerSwipeRight")]
    pub two_finger_swipe_right: String,
    #[serde(rename = "threeFingerSwipeLeft")]
    pub three_finger_swipe_left: String,
    #[serde(rename = "threeFingerSwipeRight")]
    pub three_finger_swipe_right: String,
    #[serde(rename = "threeFingerSwipeUp")]
    pub three_finger_swipe_up: String,
    #[serde(rename = "threeFingerSwipeDown")]
    pub three_finger_swipe_down: String,
    #[serde(rename = "fourFingerSwipeLeft")]
    pub four_finger_swipe_left: String,
    #[serde(rename = "fourFingerSwipeRight")]
    pub four_finger_swipe_right: String,
    #[serde(rename = "fourFingerSwipeUp")]
    pub four_finger_swipe_up: String,
    #[serde(rename = "fourFingerSwipeDown")]
    pub four_finger_swipe_down: String,

    #[serde(rename = "oneTap")]
    pub one_tap: String,
    #[serde(rename = "twoFingerTap")]
    pub two_finger_tap: String,
    #[serde(rename = "threeFingerTap")]
    pub three_finger_tap: String,
    /// Four fingers tapped together.
    ///
    /// PadRemote's own, with no trackpad setting behind it: macOS has no
    /// four-finger tap to copy, so this one is never written by the mirror and
    /// starts out unbound. Whatever it does is the user's choice alone.
    #[serde(rename = "fourFingerTap")]
    pub four_finger_tap: String,
    /// Two-finger double tap. Value: "none" | "smartZoom".
    #[serde(rename = "twoFingerDoubleTap")]
    pub two_finger_double_tap: String,
    /// Secondary click by pressing a corner. Value: "none" | "rightClick".
    #[serde(rename = "cornerSecondaryClick")]
    pub corner_secondary_click: String,
    /// Two-finger sideways swipe. Value: "none" | "navigate" (back/forward).
    #[serde(rename = "twoFingerSwipeNavigate")]
    pub two_finger_swipe_navigate: String,
    #[serde(rename = "threeFingerHorizSwipe")]
    pub three_finger_horiz_swipe: String,
    #[serde(rename = "threeFingerVertSwipe")]
    pub three_finger_vert_swipe: String,
    #[serde(rename = "fourFingerHorizSwipe")]
    pub four_finger_horiz_swipe: String,
    #[serde(rename = "fourFingerVertSwipe")]
    pub four_finger_vert_swipe: String,
    /// Four fingers pinching together. Value: "none" | "launchpad".
    #[serde(rename = "fourFingerPinch")]
    pub four_finger_pinch: String,
    /// Five fingers spreading apart. Value: "none" | "showDesktop".
    #[serde(rename = "fiveFingerSpread")]
    pub five_finger_spread: String,
}

impl Default for Config {
    fn default() -> Self {
        // Written out rather than parsed from DEFAULT_JSON: `#[serde(default)]`
        // on this struct asks serde for `Config::default()` while deserializing,
        // so parsing here would recurse forever. `defaults_match_shipped_file`
        // below keeps the two in step.
        Self {
            version: CURRENT_VERSION,
            follow_system: true,
            sensitivity: 1.0,
            accel: AccelCfg::default(),
            tap: TapCfg::default(),
            scroll: ScrollCfg::default(),
            zoom: ZoomCfg::default(),
            drag: DragCfg::default(),
            swipe: SwipeCfg::default(),
            bindings: Bindings::default(),
        }
    }
}

impl Default for AccelCfg {
    fn default() -> Self {
        Self { gain: 1.0 }
    }
}

impl Default for TapCfg {
    fn default() -> Self {
        // 500 ms matches the macOS default double-click threshold; 300 ms was
        // stricter than the host and dropped double-clicks that a real trackpad
        // would have accepted.
        Self {
            tap_max_ms: 200,
            tap_max_px: 10.0,
            double_tap_ms: 500,
            press_ms: 500,
        }
    }
}

impl Default for ScrollCfg {
    fn default() -> Self {
        Self {
            enabled: true,
            natural: true,
            momentum: true,
            speed: 1.0,
            accel: 1.0,
            horizontal: true,
        }
    }
}

impl Default for DragCfg {
    fn default() -> Self {
        Self {
            press_and_drag: true,
            // Off, which is what macOS itself defaults to: "Enable dragging" is
            // an Accessibility option nobody has switched on until they switch
            // it on. Shipping it *on* meant a phone dragged where the Mac beside
            // it would not, and it did so invisibly - see `Config::migrate`.
            // A host that does have the setting turns this back on through
            // `HostTrackpad::apply_to`, which is the mirror working as intended.
            tap_and_drag: false,
        }
    }
}

impl Default for SwipeCfg {
    fn default() -> Self {
        Self { min_px: 50.0 }
    }
}

impl Default for ZoomCfg {
    fn default() -> Self {
        // Off, unlike a real trackpad. A pinch and a two-finger swipe are the
        // same two fingers moving, and on a surface the size of a palm the
        // recognizer has to guess between them from a few millimetres of
        // divergence - so an ordinary scroll or a sideways swipe would fire a
        // zoom often enough to be worse than not having it. The setting stays
        // for anyone who wants it back.
        Self {
            enabled: false,
            backend: "appZoom".into(),
            threshold: 0.05,
        }
    }
}

impl Default for Bindings {
    fn default() -> Self {
        Self {
            two_finger_swipe_left: "inherit".into(),
            two_finger_swipe_right: "inherit".into(),
            three_finger_swipe_left: "inherit".into(),
            three_finger_swipe_right: "inherit".into(),
            three_finger_swipe_up: "inherit".into(),
            three_finger_swipe_down: "inherit".into(),
            four_finger_swipe_left: "inherit".into(),
            four_finger_swipe_right: "inherit".into(),
            four_finger_swipe_up: "inherit".into(),
            four_finger_swipe_down: "inherit".into(),

            one_tap: "leftClick".into(),
            two_finger_tap: "rightClick".into(),
            three_finger_tap: "middleClick".into(),
            four_finger_tap: "none".into(),
            two_finger_double_tap: "none".into(),
            corner_secondary_click: "none".into(),
            two_finger_swipe_navigate: "none".into(),
            three_finger_horiz_swipe: "none".into(),
            three_finger_vert_swipe: "none".into(),
            four_finger_horiz_swipe: "none".into(),
            four_finger_vert_swipe: "none".into(),
            four_finger_pinch: "none".into(),
            five_finger_spread: "none".into(),
        }
    }
}

impl Config {
    /// `~/Library/Application Support/PadRemote/config.json` on macOS,
    /// the platform equivalent elsewhere.
    pub fn user_path() -> Option<PathBuf> {
        Some(dirs::config_dir()?.join("PadRemote").join("config.json"))
    }

    /// Write the shipped defaults to the user config path if it is not there yet.
    pub fn seed_user_config() -> Option<PathBuf> {
        let path = Self::user_path()?;
        if !path.exists() {
            std::fs::create_dir_all(path.parent()?).ok()?;
            std::fs::write(&path, DEFAULT_JSON).ok()?;
        }
        Some(path)
    }

    /// Bring a file written by an older version up to today's, in place.
    ///
    /// Returns whether anything changed, so the caller only writes when there
    /// is something to write.
    ///
    /// **v1 -> v2: `drag.tapAndDrag` off.** The original file shipped it on, and
    /// that turned the most ordinary sequence there is - tap a thing, then move
    /// the cursor off it - into a held drag, because any touch landing within
    /// `doubleTapMs` of a tap arms one. On a Mac that is a deliberate
    /// Accessibility setting which is off until you turn it on; on the phone it
    /// was on for everybody, and unlike the long press there is nothing drawn on
    /// the pad to say a drag has armed. So the symptom was text quietly
    /// selecting itself under a finger that was only moving.
    ///
    /// Corrected once rather than forced every launch: the version stamp is what
    /// says the correction has happened, so anyone who wants the gesture can turn
    /// it back on in Advanced settings and keep it. `followSystem` is unaffected -
    /// the host mirror runs afterwards and a Mac that has the dragging style
    /// switches it straight back on.
    pub fn migrate(&mut self) -> bool {
        if self.version >= CURRENT_VERSION {
            return false;
        }
        if self.version < 2 {
            self.drag.tap_and_drag = false;
        }
        self.version = CURRENT_VERSION;
        true
    }

    /// Write this config where the app reads it from.
    ///
    /// Pretty-printed, because a human still has to be able to open the file -
    /// the settings page is the front door, not the only one. Written whole
    /// rather than patched: the file is the serialised form of exactly this
    /// struct, and a partial write is how two writers corrupt one.
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let mut json = serde_json::to_string_pretty(self)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        json.push('\n');
        std::fs::write(path, json)
    }

    /// The values each binding will accept, for anything offering a choice.
    ///
    /// Lives next to the config rather than in the settings page, because the
    /// page is not what decides them: an action the engine does not implement
    /// is silently "none", and a menu offering it would be a lie.
    pub fn vocabulary() -> Vec<(&'static str, &'static [&'static str])> {
        // What a tap can do: the clicks, plus every action that means something
        // on its own. A swipe binds a *pair* - louder and quieter, back and
        // forward - and half of a pair is not something one tap can express.
        //
        // Per system, because the actions are not the same everywhere. macOS
        // has Mission Control *and* App Expose; Windows and Linux have one
        // overview, and offering both there listed the same keystroke twice
        // under two names. GNOME's overview is also its search, so Spotlight
        // has no separate meaning on Linux.
        #[cfg(target_os = "macos")]
        const CLICKS: &[&str] = &[
            "none",
            "leftClick",
            "rightClick",
            "middleClick",
            "missionControl",
            "appWindows",
            "showDesktop",
            "launchpad",
            "switchApps",
            "spotlight",
            "screenshot",
            "lockScreen",
            "mute",
            "smartZoom",
            "copy",
            "cut",
            "paste",
            "selectAll",
            "save",
            "find",
            "newTab",
            "closeWindow",
            "minimiseWindow",
            "quitApp",
            "fullScreen",
            "calculator",
        ];
        #[cfg(target_os = "windows")]
        const CLICKS: &[&str] = &[
            "none",
            "leftClick",
            "rightClick",
            "middleClick",
            "missionControl",
            "showDesktop",
            "launchpad",
            "switchApps",
            "spotlight",
            "screenshot",
            "lockScreen",
            "mute",
            "smartZoom",
            "copy",
            "cut",
            "paste",
            "selectAll",
            "save",
            "find",
            "newTab",
            "closeWindow",
            "minimiseWindow",
            "quitApp",
            "fullScreen",
            "calculator",
        ];
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        const CLICKS: &[&str] = &[
            "none",
            "leftClick",
            "rightClick",
            "middleClick",
            "missionControl",
            "showDesktop",
            "launchpad",
            "switchApps",
            "screenshot",
            "lockScreen",
            "mute",
            "smartZoom",
            "copy",
            "cut",
            "paste",
            "selectAll",
            "save",
            "find",
            "newTab",
            "closeWindow",
            "minimiseWindow",
            "quitApp",
            "fullScreen",
            "calculator",
        ];
        // Split by axis, because the axis is what the engine can deliver:
        // `swipe_shortcut` has no meaning for "spaces" travelling up, or for
        // Mission Control travelling sideways, and returns nothing for either.
        // Offering them anyway made the settings page promise an action that
        // silently did nothing - and made it offer combinations macOS itself
        // never offers, which is the more confusing half.
        const ACROSS: &[&str] = &["none", "spaces", "navigate", "tabs", "undoRedo"];
        #[cfg(target_os = "macos")]
        const UP_DOWN: &[&str] =
            &["none", "missionControl", "appWindows", "volume", "brightness", "zoom"];
        // One overview, so no separate App Expose to bind the other way.
        #[cfg(not(target_os = "macos"))]
        const UP_DOWN: &[&str] = &["none", "missionControl", "volume", "zoom"];
        const DIRECTIONAL: &[&str] = &[
            "inherit",
            "none",
            "desktopLeft",
            "desktopRight",
            "missionControl",
            "showDesktop",
            "switchApps",
            "launchpad",
            "back",
            "forward",
            "volumeUp",
            "volumeDown",
            "mute",
            // Brightness is a media key, and only macOS has one it will answer
            // - so only macOS is offered it. See `input/portable.rs`.
            #[cfg(target_os = "macos")]
            "brightnessUp",
            #[cfg(target_os = "macos")]
            "brightnessDown",
            "zoomIn",
            "zoomOut",
            "previousTab",
            "nextTab",
            "undo",
            "redo",
            "copy",
            "paste",
            "screenshot",
            "lockScreen",
            #[cfg(target_os = "macos")]
            "appWindows",
        ];
        vec![
            ("bindings.twoFingerSwipeLeft", DIRECTIONAL),
            ("bindings.twoFingerSwipeRight", DIRECTIONAL),
            ("bindings.threeFingerSwipeLeft", DIRECTIONAL),
            ("bindings.threeFingerSwipeRight", DIRECTIONAL),
            ("bindings.threeFingerSwipeUp", DIRECTIONAL),
            ("bindings.threeFingerSwipeDown", DIRECTIONAL),
            ("bindings.fourFingerSwipeLeft", DIRECTIONAL),
            ("bindings.fourFingerSwipeRight", DIRECTIONAL),
            ("bindings.fourFingerSwipeUp", DIRECTIONAL),
            ("bindings.fourFingerSwipeDown", DIRECTIONAL),
            ("zoom.backend", &["appZoom"]),
            ("bindings.oneTap", CLICKS),
            ("bindings.twoFingerTap", CLICKS),
            ("bindings.threeFingerTap", CLICKS),
            ("bindings.fourFingerTap", CLICKS),
            ("bindings.cornerSecondaryClick", CLICKS),
            ("bindings.twoFingerDoubleTap", &["none", "smartZoom"]),
            ("bindings.twoFingerSwipeNavigate", ACROSS),
            ("bindings.threeFingerHorizSwipe", ACROSS),
            ("bindings.threeFingerVertSwipe", UP_DOWN),
            ("bindings.fourFingerHorizSwipe", ACROSS),
            ("bindings.fourFingerVertSwipe", UP_DOWN),
            ("bindings.fourFingerPinch", &["none", "launchpad"]),
            ("bindings.fiveFingerSpread", &["none", "showDesktop"]),
        ]
    }

    /// Put any binding holding something the engine does not accept back to
    /// `none`.
    ///
    /// A value can become illegal without anybody doing anything wrong: the
    /// file is hand-editable, and the vocabularies have been narrowed as the
    /// engine learned which actions actually work on which gesture. A sideways
    /// action left on an up-or-down swipe does nothing, and the settings page
    /// then shows a control set to something the gesture cannot do.
    pub fn sanitise(&mut self) {
        let vocabulary = Self::vocabulary();
        let Ok(serde_json::Value::Object(mut map)) = serde_json::to_value(&*self) else {
            return;
        };
        let mut changed = false;
        for (path, allowed) in vocabulary {
            let Some((group, key)) = path.split_once('.') else {
                continue;
            };
            let Some(serde_json::Value::Object(fields)) = map.get_mut(group) else {
                continue;
            };
            let Some(serde_json::Value::String(value)) = fields.get(key) else {
                continue;
            };
            if !allowed.contains(&value.as_str()) {
                tracing::warn!("{path} was set to {value}, which it cannot do; clearing it");
                fields.insert(key.to_string(), serde_json::Value::String("none".into()));
                changed = true;
            }
        }
        if changed {
            if let Ok(fixed) = serde_json::from_value(serde_json::Value::Object(map)) {
                *self = fixed;
            }
        }
    }

    /// Load from disk, falling back to the embedded defaults on any problem.
    /// A broken config must never stop the trackpad from working.
    pub fn load(path: &Path) -> Self {
        match std::fs::read_to_string(path) {
            Ok(text) => match serde_json::from_str::<Self>(&text) {
                Ok(mut cfg) => {
                    cfg.sanitise();
                    cfg
                }
                Err(e) => {
                    tracing::warn!("config {} is invalid ({e}); using defaults", path.display());
                    Self::default()
                }
            },
            Err(_) => Self::default(),
        }
    }
}

/// The action bound to one swipe direction, or `"inherit"`.
///
/// `positive` is the direction the coordinate grows in: rightward for a
/// sideways swipe, and downward for a vertical one, since surface y grows
/// downward. Any hand this app does not offer a direction for - one finger,
/// five - inherits, which leaves the paired setting in charge of it.
impl Bindings {
    pub fn directional(&self, fingers: usize, horizontal: bool, positive: bool) -> &str {
        match (fingers, horizontal, positive) {
            (2, true, false) => &self.two_finger_swipe_left,
            (2, true, true) => &self.two_finger_swipe_right,
            (3, true, false) => &self.three_finger_swipe_left,
            (3, true, true) => &self.three_finger_swipe_right,
            (3, false, false) => &self.three_finger_swipe_up,
            (3, false, true) => &self.three_finger_swipe_down,
            (4, true, false) => &self.four_finger_swipe_left,
            (4, true, true) => &self.four_finger_swipe_right,
            (4, false, false) => &self.four_finger_swipe_up,
            (4, false, true) => &self.four_finger_swipe_down,
            _ => "inherit",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The compiled-in defaults and the file we ship to users must agree; they
    /// are two copies of the same tuning and would otherwise drift apart.
    #[test]
    fn defaults_match_shipped_file() {
        let from_file: Config =
            serde_json::from_str(DEFAULT_JSON).expect("config.default.json is malformed");
        let coded = Config::default();
        assert_eq!(
            serde_json::to_value(&from_file).unwrap(),
            serde_json::to_value(&coded).unwrap(),
            "config.default.json has drifted from Config::default()"
        );
    }

    /// A file written by v1 loses tap-and-drag, once.
    ///
    /// The "once" is the part worth testing: a migration that ran every launch
    /// would take the gesture away again from anyone who deliberately turned it
    /// back on, which is worse than the bug it fixes.
    #[test]
    fn v1_loses_tap_and_drag_and_is_not_migrated_twice() {
        let mut cfg = Config {
            version: 1,
            drag: DragCfg {
                press_and_drag: true,
                tap_and_drag: true,
            },
            ..Config::default()
        };
        assert!(cfg.migrate(), "a v1 file has something to migrate");
        assert!(!cfg.drag.tap_and_drag, "v1 -> v2 turns tap-and-drag off");
        assert_eq!(cfg.version, CURRENT_VERSION);

        cfg.drag.tap_and_drag = true;
        assert!(!cfg.migrate(), "a current file is left alone");
        assert!(
            cfg.drag.tap_and_drag,
            "a choice made after the migration has to survive the next launch"
        );
    }

    /// Nothing to do for the file we ship, which is already current.
    #[test]
    fn the_shipped_file_needs_no_migration() {
        let mut cfg: Config = serde_json::from_str(DEFAULT_JSON).unwrap();
        assert!(!cfg.migrate());
    }
}

#[cfg(test)]
mod sanitise_tests {
    use super::*;

    /// An action a gesture cannot perform is cleared, not kept.
    ///
    /// A sideways action on an up-or-down swipe does nothing at all, and the
    /// settings page showed the control set to it - a vertical gesture reading
    /// "Switch full-screen apps, left and right", which is not something it can
    /// do. The value could arrive from a hand-edited file or from a page built
    /// before the vocabularies were split by axis.
    #[test]
    fn a_binding_that_cannot_do_what_it_says_is_cleared() {
        let mut cfg = Config::default();
        cfg.bindings.three_finger_vert_swipe = "spaces".into();
        cfg.bindings.three_finger_horiz_swipe = "appWindows".into();
        cfg.sanitise();
        assert_eq!(cfg.bindings.three_finger_vert_swipe, "none");
        assert_eq!(cfg.bindings.three_finger_horiz_swipe, "none");
    }

    /// Everything legal is left exactly as it was.
    #[test]
    fn legal_bindings_survive_untouched() {
        let mut cfg = Config::default();
        cfg.bindings.three_finger_vert_swipe = "volume".into();
        cfg.bindings.three_finger_horiz_swipe = "spaces".into();
        cfg.bindings.one_tap = "spotlight".into();
        let before = cfg.clone();
        cfg.sanitise();
        assert_eq!(
            cfg.bindings.three_finger_vert_swipe,
            before.bindings.three_finger_vert_swipe
        );
        assert_eq!(
            cfg.bindings.three_finger_horiz_swipe,
            before.bindings.three_finger_horiz_swipe
        );
        assert_eq!(cfg.bindings.one_tap, before.bindings.one_tap);
    }
}
