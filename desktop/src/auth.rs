//! The pairing secret, and the challenge every connection has to answer
//! (plan.md sections 7 and 12).
//!
//! Before this existed, anything that could open a socket to the port could
//! drive the cursor. Not only another device on the same Wi-Fi: a WebSocket is
//! not subject to the same-origin policy, so *any web page open in any browser
//! on this machine* could connect to `ws://127.0.0.1:8787` and click, drag and
//! scroll with no prompt anywhere. Two things close that, and both live here:
//!
//! - **A shared secret `P`**, 128 bits, generated once and kept in the config
//!   directory at mode 0600. It reaches the phone only through the QR's URL
//!   *fragment*, which no browser ever puts in a request.
//! - **A challenge**, sent by the desktop the moment a socket opens. The phone
//!   answers `HMAC-SHA256(P, nonce)`; a wrong answer, or none within a few
//!   seconds, and the connection is closed before a single touch is read.
//!
//! `P` itself never crosses the wire, so an eavesdropper on an unencrypted link
//! learns one nonce and one MAC and cannot reuse either: the next connection
//! gets a fresh nonce. That is deliberately *not* a substitute for TLS - a
//! man-in-the-middle can still read the touch stream of a session in flight -
//! but it is what stops an unauthorised device, or a hostile web page, from
//! opening a session of its own.

pub mod devices;

pub use devices::{Devices, Paired};

use std::io;
use std::path::{Path, PathBuf};

use hmac::{Hmac, Mac};
use sha2::Sha256;

type HmacSha256 = Hmac<Sha256>;

/// Bytes in the pairing secret and in each challenge nonce.
const SECRET_BYTES: usize = 16;
const NONCE_BYTES: usize = 16;

/// How long a fresh connection has to answer the challenge.
///
/// Long enough for a phone waking its radio on a bad Wi-Fi; short enough that a
/// socket which never intends to answer cannot sit on one of the device slots.
pub const AUTH_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

/// The shared secret both sides prove they know.
///
/// Deliberately not `Debug`, `Display` or `Serialize`: the one way to get the
/// bytes back out is [`Secret::to_hex`], which is called in exactly two places
/// (the QR and the file it is saved to) and is easy to grep for. A secret that
/// can be printed is a secret that ends up in a log.
#[derive(Clone, PartialEq, Eq)]
pub struct Secret([u8; SECRET_BYTES]);

impl Secret {
    /// A new random secret from the OS.
    pub fn generate() -> Self {
        let mut bytes = [0u8; SECRET_BYTES];
        // A failure here means the OS has no entropy source, which is not a
        // condition to paper over with a weaker secret: the whole pairing story
        // rests on this being unguessable.
        getrandom::getrandom(&mut bytes).expect("the OS random source is unavailable");
        Self(bytes)
    }

    /// Parse the hex form written by [`Secret::to_hex`] - the QR's `k=`.
    pub fn from_hex(text: &str) -> Option<Self> {
        let bytes = unhex(text.trim())?;
        Some(Self(bytes.try_into().ok()?))
    }

    /// The form that goes in the QR fragment and in the saved file.
    pub fn to_hex(&self) -> String {
        hex(&self.0)
    }

    /// The answer this secret gives to `nonce`.
    ///
    /// Used by the desktop to check the phone's reply, and by the replay tool
    /// and the tests to produce one.
    pub fn sign(&self, nonce: &[u8]) -> String {
        let mut mac = HmacSha256::new_from_slice(&self.0).expect("HMAC takes a key of any length");
        mac.update(nonce);
        hex(&mac.finalize().into_bytes())
    }

    /// Is `answer` the right reply to `nonce`?
    ///
    /// Compared through `verify_slice`, which is constant-time: a comparison
    /// that returns early on the first wrong byte leaks, one byte per attempt,
    /// how much of a guess was right.
    pub fn verify(&self, nonce: &[u8], answer: &str) -> bool {
        let Some(given) = unhex(answer.trim()) else {
            return false;
        };
        let mut mac = HmacSha256::new_from_slice(&self.0).expect("HMAC takes a key of any length");
        mac.update(nonce);
        mac.verify_slice(&given).is_ok()
    }

    /// This device's own credential, derived from the enrolment secret.
    ///
    /// Two keys rather than one, because they answer different questions. The
    /// QR's secret `P` says *you were shown the code*; a device key says *you
    /// are this device*, and that is what makes "forget the iPad" mean
    /// anything. Derived rather than exchanged: a key sent over a link that is
    /// still plain `ws://` is a key an eavesdropper has, so both sides compute
    /// it from `P` and the device's own id, and nothing secret is ever sent.
    ///
    /// The phone keeps only this and throws `P` away, which is the point: a
    /// phone that is lost can be revoked, and cannot enrol itself again.
    pub fn device_key(&self, device_id: &str) -> Secret {
        let mut mac = HmacSha256::new_from_slice(&self.0).expect("HMAC takes a key of any length");
        mac.update(b"padremote:device:");
        mac.update(device_id.as_bytes());
        let full = mac.finalize().into_bytes();
        let mut key = [0u8; SECRET_BYTES];
        key.copy_from_slice(&full[..SECRET_BYTES]);
        Secret(key)
    }

    /// Read the secret from `path`, or make one and save it there.
    ///
    /// The same secret has to survive a restart, or every phone would need a
    /// fresh scan every morning - the point of pairing is that it is done once.
    pub fn load_or_create(path: &Path) -> io::Result<Self> {
        if let Ok(text) = std::fs::read_to_string(path) {
            if let Some(secret) = Self::from_hex(&text) {
                return Ok(secret);
            }
            tracing::warn!(
                "pairing secret at {} is unreadable; generating a new one \
                 (devices will need to scan the QR again)",
                path.display()
            );
        }
        let secret = Self::generate();
        secret.save(path)?;
        Ok(secret)
    }

    /// Write the secret with an owner-only mode.
    pub fn save(&self, path: &Path) -> io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let mut text = self.to_hex();
        text.push('\n');
        write_private(path, &text)
    }

    /// Where the secret lives, next to the config file.
    pub fn user_path() -> Option<PathBuf> {
        Some(dirs::config_dir()?.join("PadRemote").join("pairing-secret"))
    }
}

/// Write a file only its owner can read.
///
/// The mode is given at creation rather than fixed afterwards, so the contents
/// never exist at the default mode - not even for the instant between the two,
/// which is an instant another account on this machine can win a race for.
#[cfg(unix)]
pub(crate) fn write_private(path: &Path, contents: &str) -> io::Result<()> {
    use std::io::Write;
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)?;
    // `mode` applies only when the file is created, so one left over from an
    // earlier run - or planted by someone else - is tightened here as well.
    file.set_permissions(PermissionsExt::from_mode(0o600))?;
    file.write_all(contents.as_bytes())
}

#[cfg(not(unix))]
pub(crate) fn write_private(path: &Path, contents: &str) -> io::Result<()> {
    std::fs::write(path, contents)
}

/// Is this something a device may call itself?
///
/// The id is chosen by the phone and ends up in a file name's worth of places -
/// a log line, a JSON key, the menu bar. Hex of a fixed length keeps every one
/// of those uninteresting, and rejects the whole class of question about what a
/// hostile client might put here.
pub fn valid_device_id(id: &str) -> bool {
    id.len() == SECRET_BYTES * 2 && id.bytes().all(|b| b.is_ascii_hexdigit())
}

/// A fresh challenge for one connection.
pub fn nonce() -> String {
    let mut bytes = [0u8; NONCE_BYTES];
    getrandom::getrandom(&mut bytes).expect("the OS random source is unavailable");
    hex(&bytes)
}

fn hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        use std::fmt::Write;
        let _ = write!(out, "{b:02x}");
    }
    out
}

/// Hex back to bytes. `None` for anything that is not an even run of hex
/// digits, so a malformed reply is rejected rather than half-parsed.
fn unhex(text: &str) -> Option<Vec<u8>> {
    if text.is_empty() || text.len() % 2 != 0 {
        return None;
    }
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(text.get(i..i + 2)?, 16).ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_secret_survives_a_round_trip_through_hex() {
        let secret = Secret::generate();
        let back = Secret::from_hex(&secret.to_hex()).expect("its own hex must parse");
        assert!(secret == back);
    }

    #[test]
    fn two_secrets_differ() {
        // Not a test of the OS's randomness - a test that we asked for it. An
        // earlier draft that forgot the `getrandom` call would pass every other
        // test in this file and hand every install the same key.
        assert!(Secret::generate() != Secret::generate());
        assert_ne!(nonce(), nonce());
    }

    #[test]
    fn the_right_answer_verifies_and_nothing_else_does() {
        let secret = Secret::generate();
        let n = nonce();
        assert!(secret.verify(n.as_bytes(), &secret.sign(n.as_bytes())));

        // A different nonce: this is what makes a captured answer useless on
        // the next connection.
        let other = nonce();
        assert!(!secret.verify(other.as_bytes(), &secret.sign(n.as_bytes())));

        // A different secret, which is the attacker's position exactly.
        assert!(!secret.verify(n.as_bytes(), &Secret::generate().sign(n.as_bytes())));
    }

    #[test]
    fn junk_answers_are_rejected_rather_than_parsed() {
        let secret = Secret::generate();
        let n = nonce();
        for answer in ["", "not hex at all", "abc", &"0".repeat(64), "zz"] {
            assert!(
                !secret.verify(n.as_bytes(), answer),
                "{answer:?} must not verify"
            );
        }
    }

    /// Checked against an implementation that is not this one: the expected
    /// value below came out of Python's `hmac`, so a signer that agreed with
    /// its own verifier but with nothing else in the world would fail here.
    #[test]
    fn signing_matches_an_independent_implementation() {
        let secret = Secret::from_hex("000102030405060708090a0b0c0d0e0f").expect("hex");
        assert_eq!(
            secret.sign(b"padremote"),
            "3b5356ed1d3998637175426f8cef918c68292515b86c234d540dfc6a7e83b979"
        );
    }

    /// The phone derives the same key, in `web/src/hmac.ts`, from the same
    /// inputs - and if the two ever disagree, every device fails to pair with a
    /// `badAuth` and no test on either side alone would say why. The expected
    /// value came out of Python's `hmac`, so it is a third opinion rather than
    /// either implementation grading itself.
    #[test]
    fn a_device_key_matches_the_one_the_phone_derives() {
        let secret = Secret::from_hex("0f1e2d3c4b5a69788796a5b4c3d2e1f0").expect("hex");
        let key = secret.device_key("11111111111111111111111111111111");
        assert_eq!(key.to_hex(), "f49d3689374a991e2595220d938faa9e");

        // And two devices under one secret get different keys, which is the
        // property that lets one of them be revoked.
        assert!(secret.device_key("aa") != secret.device_key("bb"));
    }

    #[test]
    fn only_a_well_formed_device_id_is_accepted() {
        assert!(valid_device_id("0123456789abcdef0123456789abcdef"));
        for bad in [
            "",
            "not-hex",
            "../../etc/passwd",
            "0123456789abcdef0123456789abcde",   // one short
            "0123456789abcdef0123456789abcdef0", // one long
        ] {
            assert!(!valid_device_id(bad), "{bad:?} must be refused");
        }
    }

    #[test]
    fn hex_and_unhex_are_inverses() {
        assert_eq!(unhex("00ff10").unwrap(), vec![0x00, 0xff, 0x10]);
        assert_eq!(hex(&[0x00, 0xff, 0x10]), "00ff10");
        assert!(unhex("f").is_none(), "odd length");
        assert!(unhex("").is_none());
        assert!(unhex("gg").is_none());
    }

    #[test]
    fn a_saved_secret_comes_back_and_is_owner_only() {
        let dir = std::env::temp_dir().join(format!("padremote-auth-{}", std::process::id()));
        let path = dir.join("pairing-secret");
        let _ = std::fs::remove_file(&path);

        let made = Secret::load_or_create(&path).expect("create");
        let read = Secret::load_or_create(&path).expect("read back");
        assert!(made == read, "a restart must not re-pair every phone");

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode();
            assert_eq!(mode & 0o077, 0, "nobody else may read the pairing secret");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_corrupt_file_is_replaced_rather_than_fatal() {
        let dir = std::env::temp_dir().join(format!("padremote-auth-bad-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("pairing-secret");
        std::fs::write(&path, "this is not a secret").unwrap();

        let secret = Secret::load_or_create(&path).expect("must not fail on junk");
        assert!(
            Secret::from_hex(&std::fs::read_to_string(&path).unwrap()) == Some(secret),
            "the replacement is saved, so the next start agrees with this one"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
