//! Wire format, mirroring `protocol/v1.schema.json`.

use serde::{Deserialize, Serialize};

use crate::gesture::TouchSample;

pub const VERSION: u8 = 1;
/// u32 t_ms + u8 pointerId + u8 phase + f32 x + f32 y
pub const SAMPLE_BYTES: usize = 14;

/// Decode a binary touch frame. Returns `None` if the frame is malformed, which
/// is treated as "drop it" rather than "kill the connection" - a phone on a
/// flaky link should not lose its session over one bad frame.
pub fn decode_frame(data: &[u8]) -> Option<Vec<TouchSample>> {
    if data.len() < 2 || data[0] != VERSION {
        return None;
    }
    let count = data[1] as usize;
    if data.len() != 2 + SAMPLE_BYTES * count {
        return None;
    }
    let mut out = Vec::with_capacity(count);
    for i in 0..count {
        let o = 2 + i * SAMPLE_BYTES;
        out.push(TouchSample {
            t_ms: u32::from_le_bytes(data[o..o + 4].try_into().ok()?),
            pointer_id: data[o + 4],
            phase: data[o + 5],
            x: f32::from_le_bytes(data[o + 6..o + 10].try_into().ok()?),
            y: f32::from_le_bytes(data[o + 10..o + 14].try_into().ok()?),
        });
    }
    Some(out)
}

/// Encode a touch frame. Used by the tests and the replay client.
pub fn encode_frame(samples: &[TouchSample]) -> Vec<u8> {
    let mut buf = Vec::with_capacity(2 + SAMPLE_BYTES * samples.len());
    buf.push(VERSION);
    buf.push(samples.len() as u8);
    for s in samples {
        buf.extend_from_slice(&s.t_ms.to_le_bytes());
        buf.push(s.pointer_id);
        buf.push(s.phase);
        buf.extend_from_slice(&s.x.to_le_bytes());
        buf.extend_from_slice(&s.y.to_le_bytes());
    }
    buf
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "t")]
pub enum ClientMessage {
    #[serde(rename = "welcome")]
    Welcome {
        v: u32,
        surface: Surface,
        /// What to call this device on the other phones' screens and in the
        /// log. Optional: an older page sends none and gets its IP address.
        #[serde(default)]
        name: Option<String>,
    },
    #[serde(rename = "settings")]
    Settings {
        sensitivity: Option<f64>,
        #[serde(rename = "naturalScroll")]
        natural_scroll: Option<bool>,
        /// Settings to hand back to the computer, by name - "sensitivity",
        /// "naturalScroll".
        ///
        /// A separate field because absence and "match my computer again" are
        /// different requests and `Option` cannot tell them apart: a phone that
        /// simply is not overriding a setting omits it, and one that is giving
        /// an override *up* has to say so.
        #[serde(default)]
        follow: Vec<String>,
    },
    /// The answer to a [`ServerMessage::Challenge`]: hex
    /// `HMAC-SHA256(key, nonce)`. Must be the *first* message on the socket.
    ///
    /// Which key depends on `device`, and that is what makes revoking one phone
    /// possible - see [`crate::auth::devices`]. A device the computer already
    /// knows signs with its own key; one it does not signs with the QR's
    /// enrolment secret and joins the list.
    #[serde(rename = "auth")]
    Auth {
        hmac: String,
        /// This device's id, as it chose it: 32 hex characters. Absent from a
        /// native client that has only the QR secret - the replay tool, the
        /// tests - which authenticates but never enrols.
        #[serde(default)]
        device: Option<String>,
    },
}

#[derive(Debug, Clone, Copy, Deserialize)]
pub struct Surface {
    pub wpx: f64,
    pub hpx: f64,
    #[serde(default = "one")]
    pub dpr: f64,
}

fn one() -> f64 {
    1.0
}

/// Which settings this device is taking from the computer.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct Following {
    pub sensitivity: bool,
    #[serde(rename = "naturalScroll")]
    pub natural_scroll: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "t")]
pub enum ServerMessage {
    #[serde(rename = "state")]
    State {
        gesture: &'static str,
        fingers: usize,
        #[serde(skip_serializing_if = "Option::is_none")]
        name: Option<String>,
        /// The hold that starts a drag, so the phone's long-press ring finishes
        /// exactly when the drag really begins. Sent once, with the name.
        #[serde(rename = "pressMs", skip_serializing_if = "Option::is_none")]
        press_ms: Option<u32>,
        /// The distance that separates a press from a move. The ring dies when
        /// the finger passes it, because the engine has by then committed to a
        /// cursor move and will not press the button however long you rest.
        /// Sent once, with `pressMs`.
        #[serde(rename = "tapMaxPx", skip_serializing_if = "Option::is_none")]
        tap_max_px: Option<f64>,
    },
    /// The settings this device is actually being driven with.
    ///
    /// Sent on connect and whenever the computer's own trackpad settings or
    /// config file change. The desktop mirrors the host's trackpad, so the
    /// phone must not assume it knows any of these: it used to send its stored
    /// values the moment it connected, which silently overrode the mirroring
    /// with whatever the sheet happened to be left on.
    #[serde(rename = "settings")]
    Settings {
        sensitivity: f64,
        #[serde(rename = "naturalScroll")]
        natural_scroll: bool,
        /// Which of these are the computer's own values rather than this
        /// phone's overrides, so the sheet can say which is which.
        following: Following,
    },
    #[serde(rename = "echo")]
    Echo {
        #[serde(rename = "tMs")]
        t_ms: u32,
    },
    /// Who holds the cursor, sent whenever that changes.
    ///
    /// Several phones can be connected at once but only one drives the cursor
    /// at a time, so a phone whose touches are being read and not obeyed has to
    /// be able to say so - otherwise the hand-over reads as lag.
    #[serde(rename = "control")]
    Control {
        /// True when *this* connection is the one driving.
        active: bool,
        /// The device that holds it instead, when that is somebody else.
        #[serde(skip_serializing_if = "Option::is_none")]
        holder: Option<String>,
        /// How many devices are connected in total, this one included.
        devices: usize,
        /// Why this computer will move nothing at all, for anybody: absent in
        /// the normal case, `"permission"` when macOS has not granted
        /// Accessibility, `"dryRun"` when the app was started with `--dry-run`.
        ///
        /// Without this the phone cannot tell a working trackpad from a frozen
        /// one. Everything else it can see says the link is healthy - the
        /// socket is up, the gesture readout follows every finger, the latency
        /// figure is live - and the cursor never moves. That is the one failure
        /// this app can have that leaves the user nothing to try.
        #[serde(skip_serializing_if = "Option::is_none")]
        blocked: Option<&'static str>,
    },
    /// The first thing any connection is sent: prove you know the pairing
    /// secret before you are allowed to move anything.
    ///
    /// A fresh nonce per connection, so an answer captured off an unencrypted
    /// link cannot be replayed into the next one.
    #[serde(rename = "challenge")]
    Challenge { nonce: String },
    #[serde(rename = "error")]
    Error { code: &'static str },
}
