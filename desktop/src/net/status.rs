//! What the menu bar shows: how many phones are attached, and whether any of
//! them is doing something.
//!
//! Atomics rather than a channel, because the UI runs on the main thread and
//! the input path must never block on it - or on anything the UI is holding.

use std::sync::atomic::{AtomicBool, AtomicU8, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use super::shared::DeviceRow;

pub const LINK_WAITING: u8 = 0;
pub const LINK_CONNECTED: u8 = 1;
pub const LINK_ACTIVE: u8 = 2;

/// How many phones are connected, and whether any is doing anything.
///
/// Atomics rather than a channel so the UI can read them from its own thread
/// without ever blocking the input path.
#[derive(Clone, Default)]
pub struct LinkStatus {
    link: Arc<AtomicU8>,
    devices: Arc<AtomicUsize>,
    /// Who those devices are, for the menu bar's list.
    ///
    /// The one lock in here, and deliberately a trivial one: it is taken only
    /// to swap or clone an `Arc`, never by the input path, and never held
    /// across anything that can block. That is what keeps the rule above -
    /// the UI must not wait on the input path, or on anything holding it -
    /// true even though the tray now shows more than a number.
    devices_list: Arc<Mutex<Arc<Vec<DeviceRow>>>>,
    /// Set while macOS is refusing us input injection.
    ///
    /// Kept apart from the link state because the two are independent: a phone
    /// can be connected and sending perfect samples while every event we post
    /// is silently discarded, and that is precisely the case the user needs
    /// told. `false` is the right default - the common path is a granted app,
    /// and startup corrects it when it is not.
    blocked: Arc<AtomicBool>,
}

impl LinkStatus {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn get(&self) -> u8 {
        self.link.load(Ordering::Relaxed)
    }

    /// How many phones are connected right now.
    pub fn devices(&self) -> usize {
        self.devices.load(Ordering::Relaxed)
    }

    /// Which phones they are, oldest first.
    pub fn devices_list(&self) -> Arc<Vec<DeviceRow>> {
        match self.devices_list.lock() {
            Ok(list) => list.clone(),
            // A panic elsewhere must not take the menu bar down with it; an
            // empty list reads as "nothing connected", which is wrong but
            // harmless, and the next publish corrects it.
            Err(poisoned) => poisoned.into_inner().clone(),
        }
    }

    pub(super) fn set_devices_list(&self, list: Vec<DeviceRow>) {
        if let Ok(mut slot) = self.devices_list.lock() {
            *slot = Arc::new(list);
        }
    }

    /// True when input injection is not permitted, whatever the link is doing.
    pub fn needs_permission(&self) -> bool {
        self.blocked.load(Ordering::Relaxed)
    }

    pub fn set_permission(&self, granted: bool) {
        self.blocked.store(!granted, Ordering::Relaxed);
    }

    /// The whole link state in one write: nobody connected, connected and
    /// still, or connected and mid-gesture.
    ///
    /// One setter rather than three because with several devices the answer is
    /// a function of all of them at once - "is *anyone* touching" - and a
    /// per-connection setter would let the last one to speak overwrite the
    /// truth about the others.
    /// Returns whether anything actually changed.
    ///
    /// The session calls this after every message it handles, so with two
    /// phones touching it runs a couple of hundred times a second to write the
    /// same two values back. The answer lets the caller skip the rest of the
    /// work - the tray only needs telling when the tray would look different.
    pub fn set_link(&self, devices: usize, active: bool) -> bool {
        let link = if devices == 0 {
            LINK_WAITING
        } else if active {
            LINK_ACTIVE
        } else {
            LINK_CONNECTED
        };
        let was_link = self.link.swap(link, Ordering::Relaxed);
        let was_devices = self.devices.swap(devices, Ordering::Relaxed);
        was_link != link || was_devices != devices
    }
}
