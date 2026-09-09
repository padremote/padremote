//! Which devices have been paired with this computer, and which have not.
//!
//! The pairing secret alone answers "were you shown the QR". That is enough to
//! keep strangers out, and not enough to do anything about a phone that has
//! been lost: one secret shared by every device means revoking any of them
//! revokes all of them, and everyone re-scans.
//!
//! So each device gets an id of its own and a key derived from it
//! ([`Secret::device_key`]), and this is the list of the ones that count.
//!
//! The rule that makes revocation real is which key a connection is checked
//! against:
//!
//! - **an id on the list** is checked against that device's own key. Forget it
//!   and the key it holds stops opening anything.
//! - **an id not on the list** is checked against the enrolment secret `P` -
//!   the QR itself - and joins the list if it passes.
//!
//! A phone that has been forgotten still holds its device key, and that is
//! precisely why enrolment is not allowed to accept one: it would let a revoked
//! phone walk straight back in under the same id. It has to be shown the code
//! again, and it no longer has `P` to fake that with, because it threw `P` away
//! the moment it enrolled.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

/// What is remembered about one paired device.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Paired {
    /// What it called itself when it enrolled. For the menu, so "forget the
    /// iPad" is a thing a person can actually pick out of a list.
    #[serde(default)]
    pub name: String,
    /// Seconds since the Unix epoch. Stored as a number rather than a
    /// formatted date so the file does not depend on a locale to be read.
    #[serde(default)]
    pub first_seen: u64,
    /// Observed LAN address, used only to group paired browsers in the UI.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mac: Option<String>,
}

/// The paired devices, and the file they are kept in.
pub struct Devices {
    /// `None` for the tests and for a machine with no config directory: the
    /// list still works, it just does not outlive the process.
    path: Option<PathBuf>,
    known: Mutex<BTreeMap<String, Paired>>,
}

impl Devices {
    /// Load the list, or start an empty one.
    ///
    /// An unreadable or corrupt file is not fatal. Refusing to start would turn
    /// one bad write into a trackpad that never comes back; starting empty
    /// costs one re-scan per device and says so in the log.
    pub fn load(path: Option<PathBuf>) -> Self {
        let known = path
            .as_ref()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|text| match serde_json::from_str(&text) {
                Ok(map) => Some(map),
                Err(e) => {
                    tracing::warn!(
                        "the paired-device list is unreadable ({e}); starting empty, \
                         so every device has to scan the QR once more"
                    );
                    None
                }
            })
            .unwrap_or_default();
        Self {
            path,
            known: Mutex::new(known),
        }
    }

    /// Where the list lives, beside the secret it is derived from.
    pub fn user_path() -> Option<PathBuf> {
        Some(dirs::config_dir()?.join("PadRemote").join("devices.json"))
    }

    pub fn is_paired(&self, id: &str) -> bool {
        self.lock().contains_key(id)
    }

    /// Add a device that has just proved it holds the enrolment secret.
    ///
    /// Returns true when this is the first time this computer has seen it -
    /// the moment worth telling the user about, since a device pairing that
    /// nobody was expecting is the one thing a stolen QR looks like.
    pub fn enrol(&self, id: &str, name: &str) -> bool {
        let mut known = self.lock();
        if known.contains_key(id) {
            return false;
        }
        known.insert(
            id.to_string(),
            Paired {
                name: name.to_string(),
                first_seen: now_secs(),
                mac: None,
            },
        );
        let snapshot = known.clone();
        drop(known);
        self.save(&snapshot);
        true
    }

    pub fn mac(&self, id: &str) -> Option<String> {
        self.lock().get(id).and_then(|entry| entry.mac.clone())
    }

    pub fn remember_mac(&self, id: &str, mac: String) {
        let mut known = self.lock();
        let Some(entry) = known.get_mut(id) else {
            return;
        };
        if entry.mac.as_ref() == Some(&mac) {
            return;
        }
        entry.mac = Some(mac);
        let snapshot = known.clone();
        drop(known);
        self.save(&snapshot);
    }

    /// All browser credentials represented by one displayed device.
    pub fn group_ids(&self, id: &str) -> Vec<String> {
        let known = self.lock();
        let Some(entry) = known.get(id) else {
            return Vec::new();
        };
        known
            .iter()
            .filter(|(key, other)| {
                key.as_str() == id || (entry.mac.is_some() && entry.mac == other.mac)
            })
            .map(|(key, _)| key.clone())
            .collect()
    }

    /// Rename a device that has since told us what it prefers to be called.
    pub fn rename(&self, id: &str, name: &str) {
        let mut known = self.lock();
        match known.get_mut(id) {
            Some(entry) if entry.name != name => entry.name = name.to_string(),
            _ => return,
        }
        let snapshot = known.clone();
        drop(known);
        self.save(&snapshot);
    }

    /// Revoke one device. Its key stops working, here and after a restart.
    pub fn forget(&self, id: &str) -> bool {
        let mut known = self.lock();
        if known.remove(id).is_none() {
            return false;
        }
        let snapshot = known.clone();
        drop(known);
        self.save(&snapshot);
        true
    }

    /// Revoke everything. Pairs with rotating the secret.
    pub fn forget_all(&self) {
        let mut known = self.lock();
        if known.is_empty() {
            return;
        }
        known.clear();
        drop(known);
        self.save(&BTreeMap::new());
    }

    /// Every paired device, by id.
    pub fn list(&self) -> Vec<(String, Paired)> {
        self.lock()
            .iter()
            .map(|(id, p)| (id.clone(), p.clone()))
            .collect()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, BTreeMap<String, Paired>> {
        // A panic while holding this must not take pairing down with it: the
        // list is a plain map and whatever was half-written to it is still a
        // valid map.
        self.known.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Written owner-only, like the secret: the ids in here are not usable on
    /// their own, but they are the list of what may drive this computer.
    fn save(&self, known: &BTreeMap<String, Paired>) {
        let Some(path) = &self.path else { return };
        match serde_json::to_string_pretty(known) {
            Ok(mut json) => {
                json.push('\n');
                if let Err(e) = super::write_private(path, &json) {
                    tracing::warn!(
                        "could not save the paired-device list to {} ({e}); \
                         it will be forgotten when PadRemote restarts",
                        path.display()
                    );
                }
            }
            Err(e) => tracing::warn!("could not serialise the paired-device list: {e}"),
        }
    }
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Load a list that is not backed by a file. Tests, and the replay tool.
impl Default for Devices {
    fn default() -> Self {
        Self::load(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("padremote-devices-{}-{name}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("devices.json")
    }

    #[test]
    fn old_records_load_and_mac_groups_survive_serialization() {
        let old: Paired = serde_json::from_str(r#"{"name":"Phone","first_seen":123}"#).unwrap();
        assert_eq!(old.mac, None);
        let devices = Devices::default();
        devices.enrol("a", "Safari");
        devices.enrol("b", "Chrome");
        devices.remember_mac("a", "02:11:22:33:44:55".into());
        devices.remember_mac("b", "02:11:22:33:44:55".into());
        assert_eq!(devices.group_ids("a"), vec!["a", "b"]);
        let json = serde_json::to_string(&*devices.lock()).unwrap();
        let restored: BTreeMap<String, Paired> = serde_json::from_str(&json).unwrap();
        assert_eq!(restored["a"].mac, restored["b"].mac);
        assert_eq!(restored["a"].mac.as_deref(), Some("02:11:22:33:44:55"));
    }

    #[test]
    fn a_device_is_enrolled_once_and_survives_a_restart() {
        let path = temp("restart");
        let _ = std::fs::remove_file(&path);

        let devices = Devices::load(Some(path.clone()));
        assert!(devices.enrol("aa", "iPhone"), "the first sight of a device");
        assert!(!devices.enrol("aa", "iPhone"), "and it is only new once");
        assert!(devices.is_paired("aa"));

        // The point of the file: pairing is done once, not once per launch.
        let reopened = Devices::load(Some(path.clone()));
        assert!(reopened.is_paired("aa"));
        assert_eq!(reopened.list()[0].1.name, "iPhone");

        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn forgetting_a_device_outlives_the_process() {
        let path = temp("forget");
        let _ = std::fs::remove_file(&path);

        let devices = Devices::load(Some(path.clone()));
        devices.enrol("aa", "iPhone");
        devices.enrol("bb", "iPad");
        assert!(devices.forget("aa"));
        assert!(!devices.forget("aa"), "forgetting twice does nothing");

        // A revocation that came back on the next launch would be worse than
        // no revocation at all - the user would believe the phone was locked
        // out, and it would not be.
        let reopened = Devices::load(Some(path.clone()));
        assert!(!reopened.is_paired("aa"));
        assert!(reopened.is_paired("bb"), "and only that one");

        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn forget_all_empties_the_list() {
        let devices = Devices::default();
        devices.enrol("aa", "iPhone");
        devices.enrol("bb", "iPad");
        devices.forget_all();
        assert!(devices.list().is_empty());
    }

    #[test]
    fn a_corrupt_list_starts_empty_rather_than_refusing_to_start() {
        let path = temp("corrupt");
        std::fs::write(&path, "{ not json").unwrap();
        let devices = Devices::load(Some(path.clone()));
        assert!(devices.list().is_empty());
        // And is usable, so the next enrolment repairs the file.
        assert!(devices.enrol("aa", "iPhone"));
        assert!(Devices::load(Some(path.clone())).is_paired("aa"));
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    #[cfg(unix)]
    fn the_list_is_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let path = temp("mode");
        let _ = std::fs::remove_file(&path);
        let devices = Devices::load(Some(path.clone()));
        devices.enrol("aa", "iPhone");
        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o077, 0);
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn renaming_keeps_the_pairing() {
        let devices = Devices::default();
        devices.enrol("aa", "iPhone");
        devices.rename("aa", "Work phone");
        assert!(devices.is_paired("aa"));
        assert_eq!(devices.list()[0].1.name, "Work phone");
    }
}
