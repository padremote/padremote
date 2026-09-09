//! PadRemote desktop app: turn a phone's touchscreen into a wireless trackpad.
//!
//! Layered so that porting to another OS touches exactly one module:
//!
//! - [`auth`]     the pairing secret, and the challenge every connection
//!   answers before it is allowed to touch the cursor.
//! - [`app`]      the loops that run for the life of the process: the engine's
//!   clock, config hot-reload, and the wait for permission.
//! - [`gesture`]  touch samples -> intent. Pure, deterministic, OS-agnostic.
//! - [`input`]    intent -> real OS input events. The only per-platform code.
//! - [`protocol`] the wire format shared with the phone page.
//! - [`net`]      the local WebSocket server that joins them together.
//! - [`pairing`]  the QR that points a phone's camera at this computer.
//! - [`sync`]     locking that survives a panic elsewhere in the process.
//! - [`sysprefs`] reads the host's own trackpad settings, so PadRemote
//!   behaves like the trackpad the user already has.
//! - [`tray`]     the menu-bar presence (macOS).

pub mod app;
pub mod auth;
pub mod gesture;
pub mod input;
pub mod net;
pub mod pairing;
pub mod protocol;
pub mod sync;
pub mod sysprefs;
#[cfg(target_os = "macos")]
pub mod tray;

/// Milliseconds since the process started, the clock the engine runs on.
pub fn now_ms(start: std::time::Instant) -> u32 {
    start.elapsed().as_millis() as u32
}

/// The name shown on the phone while connected.
pub fn host_name() -> String {
    std::process::Command::new("scutil")
        .arg("--get")
        .arg("ComputerName")
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "This computer".to_string())
}
