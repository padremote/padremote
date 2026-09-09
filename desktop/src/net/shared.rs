//! State every connection shares, and the rule that decides who drives.
//!
//! One computer, one cursor, any number of phones. [`Shared`] owns the device
//! list, the injector and the arbiter in [`Shared::claim`]; each [`Device`] owns
//! the recognizer for exactly one phone.

use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::auth::{Devices, Secret};
use crate::gesture::{Config, InputAction, Recognizer, TouchSample};
use crate::input::{Blocked, Injector};
use crate::sync::MutexExt;

use super::status::LinkStatus;

/// How long the cursor stays reserved for a device after it goes quiet.
///
/// The gap between the two halves of a double-tap, or between a tap and the
/// drag it arms, is a moment where no finger is down and the gesture is very
/// much not over. Handing the cursor to another device inside that gap would
/// break sequences that every trackpad supports, so a device that has just
/// stopped touching keeps its claim for a beat. Long enough to protect a
/// double-tap, short enough that picking up the other device feels immediate.
const HANDOVER_GRACE: Duration = Duration::from_millis(350);

/// Connections allowed at once. Generous - the point of the limit is to stop a
/// runaway reconnect loop from allocating recognizers forever, not to ration.
pub const MAX_DEVICES: usize = 8;

/// How long `dev` keeps its claim after going quiet.
///
/// [`HANDOVER_GRACE`] is the ceiling, not the answer. The full wait is only
/// owed to a device that could still be *extending* what it just did - the gap
/// inside a double-tap, the pause in a tap-and-drag - and its recognizer knows
/// exactly how much of that window is left. Finishing a scroll or a plain move
/// arms nothing, so the cursor is free the moment the finger lifts.
///
/// Charging every gesture the flat 350 ms is what made picking up the other
/// device feel like the app had not noticed.
fn grace_for(dev: &Device) -> Duration {
    Duration::from_millis(dev.rec.locked().follow_up_ms() as u64).min(HANDOVER_GRACE)
}

/// Fallback surface size until a device sends its real geometry in `welcome`.
const DEFAULT_SURFACE: (f64, f64) = (390.0, 716.0);

/// One connected phone or tablet.
///
/// The recognizer lives here, not in [`Shared`], because gesture state is per
/// device: two phones each have their own finger 1, their own surface, and
/// their own idea of how far a scroll has travelled.
/// What one phone has chosen for itself, in place of the computer's own value.
///
/// `None` means "whatever the computer says", which is the default and the
/// point: PadRemote mirrors the host's trackpad, so a phone that has expressed
/// no opinion must not have one invented for it. Kept apart from the
/// recognizer's `Config` because that is rebuilt wholesale every time the host
/// or the config file changes - overrides stored in it were silently lost on
/// the next reload.
#[derive(Debug, Clone, Copy, Default)]
pub struct Overrides {
    pub sensitivity: Option<f64>,
    pub natural_scroll: Option<bool>,
}

pub struct Device {
    pub id: u64,
    /// Where this connection came from.
    ///
    /// A fallback for same-address session replacement. Paired credentials and
    /// their observed LAN MAC additionally identify reconnects after an IP change.
    pub addr: IpAddr,
    /// The paired-device id this connection authenticated with, when it has
    /// one. `None` for a native client holding only the QR secret - the replay
    /// tool, the tests - which authenticates but never enrols.
    pub device_id: Option<String>,
    /// What to call it in logs, the tray and the other phones' screens.
    label: Mutex<String>,
    pub rec: Mutex<Recognizer>,
    /// This phone's own settings, if it has asked for any.
    overrides: Mutex<Overrides>,
    /// Mid-gesture: fingers down, or a button held. Read by the arbiter without
    /// taking the recognizer lock, which the input path is usually holding.
    busy: AtomicBool,
    /// Flipped when a newer session from the same device takes this one's
    /// place. A `watch` rather than a `Notify` because a session that is busy
    /// injecting when it is replaced must still see the change when it next
    /// comes round the loop - a missed wake-up would leave the old tab
    /// connected, which is the whole thing this is here to prevent.
    evict_tx: tokio::sync::watch::Sender<bool>,
}

impl Device {
    pub fn label(&self) -> String {
        self.label.locked().clone()
    }

    /// Fires when a newer session from the same device has replaced this one.
    pub(super) fn evicted(&self) -> tokio::sync::watch::Receiver<bool> {
        self.evict_tx.subscribe()
    }
}

/// One connected device, as the menu bar and the phones see it.
///
/// A count alone cannot answer the question people actually have - *which*
/// device is that third one? - so the list carries the names, and the tray
/// reads a snapshot of it rather than locking the device map on its own thread.
#[derive(Clone, PartialEq, Eq)]
pub struct DeviceRow {
    pub id: u64,
    pub label: String,
    /// True for the one device currently allowed to move the cursor.
    pub driving: bool,
    /// Its paired-device id, when it has one. What "forget this device" needs.
    pub device_id: Option<String>,
}

/// Quote a string for AppleScript.
///
/// The text includes a device's self-chosen name, which came off the network.
/// Pasting that into a script unescaped is how a device name becomes a command
/// on this machine, so the quoting is not cosmetic.
#[cfg(target_os = "macos")]
fn applescript_string(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for c in text.chars() {
        match c {
            '"' | '\\' => {
                out.push('\\');
                out.push(c);
            }
            // Control characters have no business in a notification, and a
            // newline would end the statement.
            c if c.is_control() => out.push(' '),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Lay a phone's own settings over the config it was built from.
fn apply(cfg: &mut Config, over: Overrides) {
    if let Some(s) = over.sensitivity {
        cfg.sensitivity = s;
    }
    if let Some(n) = over.natural_scroll {
        cfg.scroll.natural = n;
    }
}

/// Who currently owns the cursor.
struct Control {
    /// Device id, or 0 for nobody.
    holder: u64,
    /// When the holder last drove. Starts the handover grace period.
    last_active: Instant,
}

/// What every connected phone is told about the shared cursor.
#[derive(Clone, PartialEq, Eq)]
pub struct ControlView {
    pub holder: u64,
    pub holder_label: Option<String>,
    pub devices: usize,
    /// Every connected device, oldest first.
    pub list: Vec<DeviceRow>,
    /// Why this computer will move nothing for *anybody*, when that is the
    /// case: no Accessibility grant, or `--dry-run`.
    ///
    /// It rides on the control view because it answers the same question the
    /// rest of it answers - "will my touches move anything?" - and it changes
    /// while a phone is connected, which is exactly what the Accessibility
    /// watcher in `app::spawn_permission_watch` does to it.
    pub blocked: Option<&'static str>,
}

/// Everything a session needs, shared across connections.
pub struct Shared {
    injector: Mutex<Box<dyn Injector>>,
    /// The config new devices start from, and the one hot-reload writes to.
    cfg: Mutex<Config>,
    devices: Mutex<HashMap<u64, Arc<Device>>>,
    control: Mutex<Control>,
    next_id: AtomicU64,
    config_generation: AtomicU64,
    config_tx: tokio::sync::watch::Sender<u64>,
    /// Where the config file lives, once the app has told us.
    ///
    /// Optional because the tests and the replay tool have no file, and a
    /// settings page that cannot save is better than a server that will not
    /// start without one.
    config_path: Mutex<Option<std::path::PathBuf>>,
    control_tx: tokio::sync::watch::Sender<ControlView>,
    /// Bumped when the pairing secret is replaced. Every live session watches
    /// it and hangs up, because a new secret that only applied to the *next*
    /// handshake would leave the phone you just unpaired still holding the
    /// cursor.
    pairing_tx: tokio::sync::watch::Sender<u64>,
    /// This computer's LAN address, as of the last time anyone looked.
    ///
    /// Kept here so the menu bar can notice the router handing out a new one
    /// without shelling out to `ipconfig` on the UI thread twice a second.
    /// Reading it is a `watch` borrow; the subprocess that finds it out lives
    /// in the maintenance loop.
    lan_tx: tokio::sync::watch::Sender<Option<String>>,
    pub host_name: String,
    pub status: LinkStatus,
    /// What every connection has to prove it knows before it is let in.
    ///
    /// Behind a lock because it can be replaced while the app runs: unpairing
    /// swaps it, and every device that knew the old one is locked out of its
    /// next handshake. Read per connection rather than cached anywhere, so that
    /// takes effect immediately.
    secret: Mutex<Secret>,
    /// Where the secret is kept, so a rotation survives a restart.
    ///
    /// Optional for the same reason as `config_path`: the tests and the replay
    /// tool have no file, and unpairing that only lasts until the next launch
    /// is still better than an app that will not start without one.
    secret_path: Mutex<Option<std::path::PathBuf>>,
    /// The devices this computer has paired with, and the file they live in.
    ///
    /// Deliberately not the same thing as `devices` above. That one is who is
    /// *connected* right now and dies with the process; this is who is
    /// *allowed*, and outlives both the connection and the restart.
    pub paired: Devices,
    /// Live telemetry for anyone watching on `/observe`.
    ///
    /// Visible to the `observe` module so a watcher can subscribe; nothing
    /// outside `net` can reach it.
    ///
    /// Debugging "it feels wrong" needs both people looking at the same
    /// numbers; this is that shared view. Observers never control the cursor.
    pub(super) telemetry: tokio::sync::broadcast::Sender<String>,
}

/// What one batch of touch samples did, for the telemetry stream.
///
/// The gesture readout rides along rather than being read back out of the
/// recognizer afterwards. `drive` already holds that lock, and the session used
/// to take it twice more per frame - once for the telemetry line and once to
/// decide whether the gesture name had changed - on the hot path of every
/// device at once.
pub struct DriveSummary {
    pub actions: Vec<&'static str>,
    /// Total cursor travel this batch produced, in pixels.
    pub move_px: f64,
    /// False when another device holds the cursor and this batch was watched
    /// rather than obeyed.
    pub held: bool,
    /// The recognizer's state after this batch, as the phone and the debug
    /// page name it.
    pub gesture: &'static str,
    pub fingers: usize,
    /// Most fingers seen at once during the gesture in progress.
    pub peak_fingers: usize,
}

impl Shared {
    /// Build the shared state. Use this rather than the struct literal so the
    /// device and control bookkeeping stays private.
    pub fn new(
        cfg: Config,
        injector: Box<dyn Injector>,
        host_name: String,
        status: LinkStatus,
        secret: Secret,
        devices: Devices,
    ) -> Self {
        // A small buffer: an observer that falls behind should drop old frames
        // rather than slow the input path down.
        let (telemetry, _) = tokio::sync::broadcast::channel(64);
        let (control_tx, _) = tokio::sync::watch::channel(ControlView {
            holder: 0,
            holder_label: None,
            devices: 0,
            list: Vec::new(),
            blocked: injector.blocked().map(Blocked::code),
        });
        Self {
            injector: Mutex::new(injector),
            cfg: Mutex::new(cfg),
            devices: Mutex::new(HashMap::new()),
            control: Mutex::new(Control {
                holder: 0,
                last_active: Instant::now(),
            }),
            next_id: AtomicU64::new(0),
            config_generation: AtomicU64::new(0),
            config_tx: tokio::sync::watch::channel(0).0,
            config_path: Mutex::new(None),
            control_tx,
            pairing_tx: tokio::sync::watch::channel(0).0,
            lan_tx: tokio::sync::watch::channel(None).0,
            host_name,
            status,
            secret: Mutex::new(secret),
            secret_path: Mutex::new(None),
            paired: devices,
            telemetry,
        }
    }

    // -------------------------------------------------------------- pairing

    /// The secret a handshake must currently answer with.
    pub fn secret(&self) -> Secret {
        self.secret.locked().clone()
    }

    /// Tell the server where the pairing secret is kept, so rotating it lasts.
    pub fn set_secret_path(&self, path: Option<std::path::PathBuf>) {
        *self.secret_path.locked() = path;
    }

    /// Forget every pairing: new secret, and everyone connected is dropped.
    ///
    /// Half of this would be useless. A new secret alone locks out the *next*
    /// handshake but leaves the sessions already open running, so a phone that
    /// has been un-paired keeps the cursor until someone notices; dropping the
    /// sessions alone changes nothing, because the old secret still opens a new
    /// one.
    ///
    /// Saving is part of the same act rather than the caller's job to remember:
    /// an unpairing that quietly reverts on the next launch is worse than one
    /// that never happened, because the user has been told it worked.
    pub fn unpair_all(&self) -> Secret {
        let fresh = Secret::generate();
        *self.secret.locked() = fresh.clone();
        if let Some(path) = self.secret_path.locked().as_ref() {
            if let Err(e) = fresh.save(path) {
                tracing::warn!(
                    "unpaired, but could not save the new secret to {} ({e}); \
                     pairing will revert when PadRemote restarts",
                    path.display()
                );
            }
        }
        // Both halves, or neither counts. A new secret with the old device list
        // still in place would leave every previously paired phone able to sign
        // with a key derived from a secret that no longer exists - and worse,
        // an emptied list with the old secret would let them all simply enrol
        // again.
        self.paired.forget_all();
        self.pairing_tx.send_modify(|n| *n += 1);
        fresh
    }

    /// Say out loud that a device paired for the first time.
    ///
    /// The one thing a stolen QR looks like is a device pairing that nobody was
    /// expecting, so it must not be silent. A notification rather than a modal:
    /// the user is usually holding the phone that just paired, and a dialog on
    /// the computer they are not looking at would be dismissed unread by the
    /// next person to touch the keyboard.
    pub fn announce_new_device(&self, what: &str) {
        #[cfg(target_os = "macos")]
        {
            // Through `osascript` rather than a notification framework: the app
            // needs no new dependency, no entitlement and no bundle identity to
            // do this, and a failure here must never affect the pairing that
            // just succeeded.
            let script = format!(
                "display notification {} with title \"PadRemote\" subtitle {}",
                applescript_string("It can now control this computer."),
                applescript_string(&format!("New device paired: {what}")),
            );
            // `spawn` rather than `status`: waiting for osascript to exit
            // blocks whatever tokio worker thread is running the connection
            // that just paired, for as long as the notification takes to be
            // drawn. Nothing here reads the exit code anyway - the comment
            // above already says a failure must not affect the pairing.
            match std::process::Command::new("osascript")
                .arg("-e")
                .arg(script)
                .spawn()
            {
                Ok(mut child) => {
                    // Reaped on a thread of its own so the process does not
                    // linger as a zombie for the life of the app.
                    std::thread::spawn(move || {
                        let _ = child.wait();
                    });
                }
                Err(e) => tracing::debug!("could not announce the new device: {e}"),
            }
        }
        #[cfg(not(target_os = "macos"))]
        let _ = what;
    }

    /// Revoke one device for good, and drop it if it is connected.
    ///
    /// The difference from a disconnect: this survives a restart, and the phone
    /// cannot come back, because the id it signs with is no longer on the list
    /// and it has no QR secret left to enrol with again.
    pub fn forget_device(&self, device_id: &str) -> bool {
        let ids = self.paired.group_ids(device_id);
        if ids.is_empty() {
            return false;
        }
        for id in &ids {
            self.paired.forget(id);
        }
        // Revoking a credential while the session it opened keeps running would
        // leave the phone you just revoked holding the cursor until it happened
        // to disconnect.
        let connected: Vec<u64> = self
            .devices
            .locked()
            .values()
            .filter(|d| d.device_id.as_ref().is_some_and(|id| ids.contains(id)))
            .map(|d| d.id)
            .collect();
        for id in connected {
            self.disconnect(id);
        }
        true
    }

    // ------------------------------------------------------------- address

    /// This computer's LAN address, or `None` with the network down.
    pub fn lan_ip(&self) -> Option<String> {
        self.lan_tx.borrow().clone()
    }

    /// Fires when this computer's address changes.
    ///
    /// A router handing out a new address invalidates the QR on any connect
    /// page that is already open, and nothing else notices: the phone just
    /// gets "this site can't be reached", with nothing on screen to suggest
    /// the address is what changed.
    pub(super) fn subscribe_lan(&self) -> tokio::sync::watch::Receiver<Option<String>> {
        self.lan_tx.subscribe()
    }

    /// Record the address the maintenance loop just found.
    pub fn set_lan_ip(&self, ip: Option<String>) {
        self.lan_tx.send_if_modified(|cur| {
            if *cur == ip {
                false
            } else {
                if let Some(new) = &ip {
                    tracing::info!("this computer's address is now {new}");
                } else {
                    tracing::warn!("this computer has no LAN address - is Wi-Fi off?");
                }
                *cur = ip;
                true
            }
        });
    }

    /// Fires when the pairing secret is replaced; a session that sees it stops.
    pub(super) fn subscribe_pairing(&self) -> tokio::sync::watch::Receiver<u64> {
        self.pairing_tx.subscribe()
    }

    // ------------------------------------------------------------- devices

    /// Register a connection from `addr`. `None` once [`MAX_DEVICES`] are
    /// already attached.
    ///
    /// A second connection from an address that is already here **replaces** the
    /// first. Two tabs on one phone are one user with a stale tab, not two
    /// people taking turns, and leaving both attached means every touch races a
    /// page the user has forgotten about.
    ///
    /// Note how narrow that is. Evicting the *newest* connection globally is
    /// what this server used to do, and `tests/sessions.rs` records what it
    /// cost: the loser reconnected on its backoff, evicted whoever had taken
    /// over, and the two traded the cursor about once a second. A phone and a
    /// tablet still take turns exactly as before - only a device replacing
    /// *itself* is ever hung up on, and that one is told to stay down.
    pub fn join(&self, addr: IpAddr, device_id: Option<String>) -> Option<Arc<Device>> {
        let mac = device_id.as_deref().and_then(|id| self.paired.mac(id));
        let stale: Vec<Arc<Device>> = self
            .devices
            .locked()
            .values()
            .filter(|d| {
                let old_mac = d.device_id.as_deref().and_then(|id| self.paired.mac(id));
                (device_id.is_some() && d.device_id == device_id)
                    || (mac.is_some() && old_mac == mac)
                    || (d.addr == addr && (mac.is_none() || old_mac.is_none()))
            })
            .cloned()
            .collect();
        for old in &stale {
            tracing::info!(
                "{}: replaced by a newer connection from {addr}",
                old.label()
            );
            // Retired here rather than left to the old session's own `leave`,
            // so the device count is right the moment the new one joins and a
            // replacement can never be refused for being one over the limit.
            self.retire(old);
            let _ = old.evict_tx.send(true);
        }

        let cfg = self.cfg.locked().clone();
        let mut devices = self.devices.locked();
        if devices.len() >= MAX_DEVICES {
            return None;
        }
        let id = self.next_id.fetch_add(1, Ordering::SeqCst) + 1;
        let dev = Arc::new(Device {
            id,
            addr,
            device_id,
            label: Mutex::new(addr.to_string()),
            rec: Mutex::new(Recognizer::new(cfg, DEFAULT_SURFACE.0, DEFAULT_SURFACE.1)),
            overrides: Mutex::new(Overrides::default()),
            busy: AtomicBool::new(false),
            evict_tx: tokio::sync::watch::channel(false).0,
        });
        devices.insert(id, dev.clone());
        drop(devices);
        self.refresh_status();
        self.publish_control();
        Some(dev)
    }

    /// Take a device off the list and give up anything it was holding.
    ///
    /// Shared by [`Shared::leave`] and by the replacement path in
    /// [`Shared::join`], and safe to run twice for exactly that reason: the
    /// second call finds no claim, nothing pressed and nothing to remove.
    fn retire(&self, dev: &Device) {
        // Whatever it was in the middle of, a phone that is gone must never
        // leave a button down - and only the holder's actions may be injected.
        let held = self.holds(dev.id);
        let actions = dev.rec.locked().release_all();
        if held {
            self.inject(actions);
        }
        {
            let mut control = self.control.locked();
            if control.holder == dev.id {
                control.holder = 0;
            }
        }
        self.devices.locked().remove(&dev.id);
        dev.busy.store(false, Ordering::Relaxed);
    }

    /// Unregister a connection, releasing anything it was still holding.
    pub fn leave(&self, dev: &Device) {
        self.retire(dev);
        self.refresh_status();
        self.publish_control();
    }

    /// Hang up on one device, by id. The menu bar's per-device Disconnect.
    ///
    /// The session is told and closed; the device is free to reconnect, which
    /// is the honest behaviour while every device shares one secret. Revoking
    /// a device for good is [`Shared::unpair_all`].
    pub fn disconnect(&self, id: u64) -> bool {
        let Some(dev) = self.devices.locked().get(&id).cloned() else {
            return false;
        };
        tracing::info!("{}: disconnected from the menu bar", dev.label());
        self.retire(&dev);
        let _ = dev.evict_tx.send(true);
        self.refresh_status();
        self.publish_control();
        true
    }

    /// Swap the input backend under the running server.
    ///
    /// The one caller that matters is the Accessibility watcher: the app starts
    /// deaf when permission has not been granted, and this is how it goes live
    /// without a restart. Setting the permission flag here rather than at the
    /// call site keeps the two from ever disagreeing.
    pub fn set_injector(&self, injector: Box<dyn Injector>) {
        *self.injector.locked() = injector;
        self.status.set_permission(true);
        // Tell the phones already in someone's hand. Going live has to reach
        // them here: they were told the cursor was frozen, and nothing else
        // will ever contradict it - a phone that has been apologising since it
        // connected would go on apologising over a working trackpad.
        self.publish_control();
    }

    pub fn device_count(&self) -> usize {
        self.devices.locked().len()
    }

    /// Every connected device, oldest first.
    pub fn devices(&self) -> Vec<Arc<Device>> {
        let mut devices: Vec<Arc<Device>> = self.devices.locked().values().cloned().collect();
        devices.sort_by_key(|d| d.id);
        devices
    }

    pub fn set_label(&self, dev: &Device, label: String) {
        // Remembered as well as shown. A device enrols before it has said what
        // it is called, so the paired list starts with the address it came
        // from - which is what somebody deciding whether to forget the spare
        // iPad would have had to work with, now that the list outlives the
        // connection.
        if let Some(id) = &dev.device_id {
            self.paired.rename(id, &label);
        }
        *dev.label.locked() = label;
        self.publish_control();
    }

    /// The config all devices start from. Replacing it re-tunes the ones
    /// already connected, which is what config hot-reload expects.
    ///
    /// A device that has overridden something keeps its choice: the new config
    /// is the *base*, and the override goes back on top. Without that, flipping
    /// any switch in System Settings - or saving `config.json` - quietly undid
    /// every phone's own settings twice a second.
    pub fn set_config(&self, cfg: Config) {
        *self.cfg.locked() = cfg.clone();
        for dev in self.devices.locked().values() {
            let mut rec = dev.rec.locked();
            rec.cfg = cfg.clone();
            apply(&mut rec.cfg, *dev.overrides.locked());
        }
        // Every connected phone is showing these values in its settings sheet.
        let _ = self
            .config_tx
            .send(self.config_generation.fetch_add(1, Ordering::SeqCst) + 1);
    }

    /// Apply this phone's own settings, and remember them.
    ///
    /// `follow` names settings it is handing *back* to the computer - which is
    /// not the same as saying nothing about them, and is why it is a list of
    /// names rather than a `None`.
    pub fn set_overrides(
        &self,
        dev: &Device,
        sensitivity: Option<f64>,
        natural_scroll: Option<bool>,
        follow: &[String],
    ) {
        {
            let mut over = dev.overrides.locked();
            if let Some(s) = sensitivity.filter(|s| s.is_finite() && *s > 0.0 && *s <= 10.0) {
                over.sensitivity = Some(s);
            }
            if let Some(n) = natural_scroll {
                over.natural_scroll = Some(n);
            }
            for name in follow {
                match name.as_str() {
                    "sensitivity" => over.sensitivity = None,
                    "naturalScroll" => over.natural_scroll = None,
                    _ => {}
                }
            }
        }
        // Rebuild from the shared config so that dropping an override really
        // does hand the setting back, rather than leaving the last value set.
        let base = self.cfg.locked().clone();
        let over = *dev.overrides.locked();
        let mut rec = dev.rec.locked();
        rec.cfg = base;
        apply(&mut rec.cfg, over);
    }

    /// What this device is actually being driven with, and what it is following.
    pub fn effective(&self, dev: &Device) -> (f64, bool, crate::protocol::Following) {
        let over = *dev.overrides.locked();
        let rec = dev.rec.locked();
        (
            rec.cfg.sensitivity,
            rec.cfg.scroll.natural,
            crate::protocol::Following {
                sensitivity: over.sensitivity.is_none(),
                natural_scroll: over.natural_scroll.is_none(),
            },
        )
    }

    /// Bumped whenever the computer's own settings change, so every session can
    /// tell its phone what it is now being driven with.
    pub fn subscribe_config(&self) -> tokio::sync::watch::Receiver<u64> {
        self.config_tx.subscribe()
    }

    pub fn config(&self) -> Config {
        self.cfg.locked().clone()
    }

    /// Tell the server where the config file is, so the settings page can save.
    pub fn set_config_path(&self, path: Option<std::path::PathBuf>) {
        *self.config_path.locked() = path;
    }

    pub fn config_path(&self) -> Option<std::path::PathBuf> {
        self.config_path.locked().clone()
    }

    /// The config as it is *on disk* - what the user has chosen, before the
    /// host's own trackpad settings are folded on top.
    ///
    /// Read fresh rather than cached: the file can be edited by hand at any
    /// moment, and a settings page showing a stale copy would overwrite an edit
    /// it never saw.
    pub fn file_config(&self) -> Config {
        match self.config_path() {
            Some(path) => Config::load(&path),
            None => self.config(),
        }
    }

    /// Save a config the user has chosen, and put it straight into effect.
    ///
    /// The file is the source of truth - the app watches it, and anything that
    /// changed only memory would be undone by the next reload - but waiting up
    /// to half a second for that watch to notice makes the settings page feel
    /// broken, so the same config is applied here too.
    pub fn write_config(&self, mut file_cfg: Config) -> std::io::Result<()> {
        // A page from before a vocabulary narrowed, or an older config sent
        // back wholesale, can carry a binding the engine no longer accepts.
        // Cleared on the way in rather than written to disk and puzzled over
        // later.
        file_cfg.sanitise();
        if let Some(path) = self.config_path() {
            file_cfg.save(&path)?;
        }
        let effective = crate::sysprefs::HostTrackpad::read().apply_to(&file_cfg);
        self.set_config(effective);
        Ok(())
    }

    // ------------------------------------------------------------- control

    pub fn subscribe_control(&self) -> tokio::sync::watch::Receiver<ControlView> {
        self.control_tx.subscribe()
    }

    pub fn control_view(&self) -> ControlView {
        let holder = self.control.locked().holder;
        // Before the device map, not inside it: every other path takes the
        // injector last, and one that took it while holding the devices would
        // be the only place the two could be ordered the other way round.
        let blocked = self.injector.locked().blocked().map(Blocked::code);
        let devices = self.devices.locked();
        let mut list: Vec<DeviceRow> = devices
            .values()
            .map(|d| DeviceRow {
                id: d.id,
                label: d.label(),
                driving: d.id == holder,
                device_id: d.device_id.clone(),
            })
            .collect();
        // Oldest first, so the menu bar does not reshuffle itself under the
        // pointer every time somebody picks up a different phone.
        list.sort_by_key(|d| d.id);
        ControlView {
            holder,
            holder_label: devices.get(&holder).map(|d| d.label()),
            devices: devices.len(),
            list,
            blocked,
        }
    }

    fn publish_control(&self) {
        let view = self.control_view();
        // The menu bar reads the list from here rather than locking the device
        // map on the main thread - see the note at the top of `status.rs`.
        self.status.set_devices_list(view.list.clone());
        self.control_tx.send_if_modified(|cur| {
            if *cur == view {
                false
            } else {
                *cur = view;
                true
            }
        });
    }

    pub fn holds(&self, id: u64) -> bool {
        self.control.locked().holder == id
    }

    /// Decide whether `dev` may drive the cursor for this batch.
    ///
    /// The rule, in one sentence: the cursor belongs to whoever is using it,
    /// and changes hands only at the start of a gesture, once the previous
    /// device has been quiet for [`HANDOVER_GRACE`]. That is what makes two
    /// devices at once feel like taking turns rather than like a fight - a
    /// second phone's stray touch cannot interrupt a drag in progress, and
    /// picking one up when the other is idle just works, with nothing to press.
    ///
    /// `starting` says this batch opens a fresh gesture, which is the only
    /// moment a takeover is safe: mid-gesture, the new device's recognizer is
    /// already part-way through a state machine whose first half was never
    /// injected.
    fn claim(&self, dev: &Device, starting: bool) -> bool {
        let mut control = self.control.locked();
        if control.holder == dev.id {
            control.last_active = Instant::now();
            return true;
        }
        if !starting {
            return false;
        }
        let previous = if control.holder == 0 {
            None
        } else {
            let devices = self.devices.locked();
            match devices.get(&control.holder) {
                // Still mid-gesture, or only just stopped with a follow-up
                // still possible: leave it alone.
                Some(prev)
                    if prev.busy.load(Ordering::Relaxed)
                        || control.last_active.elapsed() < grace_for(prev) =>
                {
                    return false;
                }
                // A holder that has vanished from the map cannot object.
                other => other.cloned(),
            }
        };
        control.holder = dev.id;
        control.last_active = Instant::now();
        drop(control);

        // Wipe the loser's leftovers - a coasting momentum scroll, most of all,
        // which would otherwise keep decaying against a cursor it no longer
        // owns and resume the moment it got control back.
        if let Some(prev) = previous {
            let actions = prev.rec.locked().release_all();
            self.inject(actions);
            prev.busy.store(false, Ordering::Relaxed);
            tracing::info!("cursor: {} takes over from {}", dev.label(), prev.label());
        } else {
            tracing::debug!("cursor: {} takes control", dev.label());
        }
        self.publish_control();
        true
    }

    // --------------------------------------------------------------- input

    /// Feed samples through one device's engine and inject whatever comes out -
    /// if that device currently holds the cursor.
    pub fn drive(&self, dev: &Device, samples: &[TouchSample]) -> DriveSummary {
        let (actions, idle_before, idle_after, gesture, fingers, peak_fingers) = {
            let mut rec = dev.rec.locked();
            let idle_before = rec.is_idle();
            let actions = rec.feed(samples);
            let idle_after = rec.is_idle();
            (
                actions,
                idle_before,
                idle_after,
                rec.state().gesture_name(),
                rec.finger_count(),
                rec.peak_fingers(),
            )
        };
        dev.busy.store(!idle_after, Ordering::Relaxed);

        // A gesture that began in this batch is one we can honour from its very
        // first event, so it is the one kind of batch allowed to take over.
        let starting = idle_before && samples.iter().any(|s| s.phase == crate::gesture::DOWN);
        let held = self.claim(dev, starting);

        // The one moment worth asking the OS where the cursor is. This batch
        // opens a gesture, so nothing is in flight to disturb - and whatever the
        // user did with the computer's own trackpad in between is about to
        // matter, because a backend that tracks an absolute position is still
        // holding the one it last drove to.
        //
        // Ahead of the empty-actions return below on purpose: the batch that
        // opens a gesture is usually a lone `DOWN`, which recognises nothing.
        if held && starting {
            self.injector.locked().sync_cursor();
        }

        let mut summary = DriveSummary {
            actions: Vec::new(),
            move_px: 0.0,
            held,
            gesture,
            fingers,
            peak_fingers,
        };
        if actions.is_empty() {
            return summary;
        }
        for a in &actions {
            // Swipes are rare, deliberate and easy to get wrong, so say when one
            // fires - otherwise a mis-tuned threshold looks like a dead feature.
            if let InputAction::Shortcut { shortcut, .. } = a {
                if held {
                    tracing::info!("swipe -> {}", shortcut.name());
                }
                summary.actions.push(shortcut.name());
            } else {
                summary.actions.push(a.kind());
            }
            if let InputAction::Move { dx, dy, .. } = a {
                summary.move_px += dx.hypot(*dy);
            }
        }
        // Recognised either way - the telemetry above is the same whoever is
        // driving - but only the holder's intent reaches the OS.
        if held {
            self.inject(actions);
        }
        summary
    }

    fn inject(&self, actions: Vec<InputAction>) {
        if actions.is_empty() {
            return;
        }
        let mut inj = self.injector.locked();
        for a in actions {
            inj.apply(a);
        }
    }

    /// Whether anyone is watching `/observe` right now.
    ///
    /// Ask this *before* building a telemetry line. `publish` below discards
    /// one that nobody wants, but the discard happens after the whole nested
    /// JSON document - an array per touch sample, and a `label()` clone taken
    /// under a mutex - has already been built and serialized, which the session
    /// was doing sixty times a second per device with the debug page closed.
    pub fn observed(&self) -> bool {
        self.telemetry.receiver_count() > 0
    }

    /// Publish a telemetry line. Silently does nothing when nobody is watching.
    pub fn publish(&self, json: String) {
        let _ = self.telemetry.send(json);
    }

    /// Momentum scroll continuation, driven by the app's timer.
    pub fn tick(&self, t_ms: u32) {
        let devices: Vec<Arc<Device>> = self.devices.locked().values().cloned().collect();
        if devices.is_empty() {
            return;
        }
        for dev in devices {
            let actions = dev.rec.locked().tick(t_ms);
            // Every device's engine keeps ticking so its own state stays
            // coherent, but a coast only reaches the cursor if it owns it.
            if !actions.is_empty() && self.holds(dev.id) {
                self.inject(actions);
            }
        }
    }

    /// Release every held button, everywhere. Called on shutdown.
    pub fn release_all(&self) {
        let devices: Vec<Arc<Device>> = self.devices.locked().values().cloned().collect();
        for dev in devices {
            let actions = dev.rec.locked().release_all();
            self.inject(actions);
            dev.busy.store(false, Ordering::Relaxed);
        }
        self.injector.locked().release_all();
        self.refresh_status();
    }

    /// Recompute the tray light from every device at once.
    pub(super) fn refresh_status(&self) {
        let devices = self.devices.locked();
        let count = devices.len();
        let active = devices.values().any(|d| d.busy.load(Ordering::Relaxed));
        drop(devices);
        self.status.set_link(count, active);
    }
}
