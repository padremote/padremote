//! Pointer acceleration (plan.md section 9.2).
//!
//! A trackpad feels right when a slow finger gives fine control and a fast
//! finger crosses the screen. That is a curve on *speed*, not a constant gain.
//!
//! The shape of that curve is fixed: quadratic, which is the closest of the
//! ones tried to how macOS feels. It used to be a setting, and the only honest
//! description of the alternative was "not quite as good" - a question nobody
//! could answer from the settings page, offered next to a slider (`gain`) that
//! answers the thing they actually came to change.

use super::config::AccelCfg;

/// Speed at which the curve reaches its knee, in surface px/s.
pub const SPEED_REF: f64 = 1000.0;
/// Gain floor: below this, slow movement would feel dead.
pub const BASE: f64 = 0.55;
/// Gain ceiling: above this, the cursor becomes impossible to aim.
pub const MAX: f64 = 3.5;

/// Multiplier to apply to a finger delta moving at `speed` surface px/s.
pub fn factor(cfg: &AccelCfg, speed: f64) -> f64 {
    let s = speed / SPEED_REF;
    // Gentle at low speed, steep once moving.
    let f = BASE + cfg.gain * s * s * 2.2;
    f.clamp(0.15, MAX)
}
