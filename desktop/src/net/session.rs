//! One phone's connection, start to finish.
//!
//! Touch frames in, `state`, `control` and `echo` messages out. Everything
//! about *who is allowed to drive* lives in [`super::shared`]; this module's
//! job is the conversation.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::Result;
use futures_util::SinkExt;
use futures_util::StreamExt;
use tokio_tungstenite::tungstenite::Message;

use crate::protocol::{self, ClientMessage, ServerMessage};
use crate::sync::MutexExt;

use super::shared::{ControlView, Device, Shared};
use super::{json, Rx, Tx};

/// How often at most a session echoes a sample timestamp back to its phone.
///
/// The phone turns this into the latency figure on screen and nothing else
/// reads it, so the rate only has to beat the eye, not the frame clock.
const ECHO_EVERY: Duration = Duration::from_millis(100);

/// What this device is being driven with, and which of it is the computer's.
fn settings_msg(shared: &Arc<Shared>, dev: &Arc<Device>) -> ServerMessage {
    let (sensitivity, natural_scroll, following) = shared.effective(dev);
    ServerMessage::Settings {
        sensitivity,
        natural_scroll,
        following,
    }
}

fn control_msg(view: &ControlView, me: u64) -> ServerMessage {
    ServerMessage::Control {
        active: view.holder == me,
        holder: view
            .holder_label
            .clone()
            .filter(|_| view.holder != 0 && view.holder != me),
        devices: view.devices,
        blocked: view.blocked,
    }
}

pub(super) async fn run(
    tx: &mut Tx,
    rx: &mut Rx,
    shared: &Arc<Shared>,
    dev: &Arc<Device>,
    peer: SocketAddr,
) -> Result<()> {
    // Greet on connect rather than after the first touch. The phone shows this
    // computer's name in place of a bare IP, and it needs it before anything is
    // touched.
    //
    // Read the config out into a local first: a `MutexGuard` held across the
    // await below would make this whole future non-`Send`, and it is spawned.
    let (press_ms, tap_max_px) = {
        let rec = dev.rec.locked();
        (rec.cfg.tap.press_ms, rec.cfg.tap.tap_max_px)
    };
    tx.send(json(&ServerMessage::State {
        gesture: "idle",
        fingers: 0,
        name: Some(shared.host_name.clone()),
        press_ms: Some(press_ms),
        tap_max_px: Some(tap_max_px),
    }))
    .await?;

    // Who has the cursor, from the first frame. A phone that connects while
    // another one is driving needs to say so rather than look broken.
    let mut control_rx = shared.subscribe_control();
    control_rx.mark_changed();

    // What this device is actually being driven with. The desktop mirrors the
    // computer's own trackpad, so the phone cannot be allowed to assume any of
    // it - the sheet is filled in from here.
    tx.send(json(&settings_msg(shared, dev))).await?;
    let mut config_rx = shared.subscribe_config();

    // Unpairing has to reach the phones that are *already* connected, or the
    // device you just revoked keeps the cursor until it happens to disconnect.
    let mut pairing_rx = shared.subscribe_pairing();

    // And this device on its own, when a newer tab on the same phone takes its
    // place, or somebody hangs up on it from the menu bar.
    let mut evict_rx = dev.evicted();

    let mut last_state = "idle";
    let mut last_batch: Option<Instant> = None;
    // Were fingers still down when the previous batch arrived?
    let mut touching_before = false;
    // When this session last echoed a timestamp back. See the send below: the
    // echo is throttled, and the phone is the only thing that reads it.
    let mut last_echo: Option<Instant> = None;
    // What the tray was last told about this device, so the common case - a
    // finger that keeps moving - does not re-derive it from every frame.
    let mut last_busy = false;

    loop {
        let msg = tokio::select! {
            msg = rx.next() => match msg {
                Some(m) => m?,
                None => break,
            },
            // The cursor changed hands. Tell this phone either way: the one
            // that lost it must be able to explain why it stopped moving
            // anything, and the one that gained it should stop apologising.
            Ok(()) = control_rx.changed() => {
                let view = control_rx.borrow_and_update().clone();
                tx.send(json(&control_msg(&view, dev.id))).await?;
                continue;
            }
            // The computer's trackpad settings changed under us, or its config
            // file was edited. Whatever this phone is following now moved.
            Ok(()) = config_rx.changed() => {
                config_rx.borrow_and_update();
                tx.send(json(&settings_msg(shared, dev))).await?;
                continue;
            }
            // The computer has forgotten every pairing. Say so rather than
            // dropping the socket silently: the phone's own reconnect would
            // otherwise retry forever against a secret it can no longer answer.
            Ok(()) = pairing_rx.changed() => {
                tracing::info!("{peer}: unpaired, closing");
                let _ = tx.send(json(&ServerMessage::Error { code: "unpaired" })).await;
                let _ = tx.send(Message::Close(None)).await;
                break;
            }
            // This page has been superseded by a newer one on the same device -
            // another tab, or another browser. It has to be told which, because
            // the answer is "close the other tab", not "check your Wi-Fi", and
            // it must not reconnect: two tabs racing to replace each other is
            // the tug of war this whole rule exists to avoid.
            Ok(()) = evict_rx.changed() => {
                tracing::debug!("{peer}: replaced, closing");
                let _ = tx.send(json(&ServerMessage::Error { code: "replaced" })).await;
                let _ = tx.send(Message::Close(None)).await;
                break;
            }
        };

        // The recognizer state after this message, when the message could have
        // changed it. `None` for control frames, which never touch a gesture.
        let mut state_after: Option<(&'static str, usize)> = None;

        match msg {
            Message::Binary(data) => {
                let Some(samples) = protocol::decode_frame(&data) else {
                    tracing::debug!("dropped a malformed touch frame ({} bytes)", data.len());
                    continue;
                };
                let last_t = samples.last().map(|s| s.t_ms);

                // Gap since the previous batch. This is the number that explains
                // stutter: the phone sends once per animation frame, so a steady
                // ~8-16 ms here means smooth, and a wildly varying gap means the
                // network - not the recognizer - is the problem.
                //
                // Only gaps *within* a continuous touch count. The pause between
                // one gesture and the next is the user thinking, not the link
                // stalling, and letting it in made the jitter figure meaningless.
                let now = Instant::now();
                let gap_ms = last_batch.replace(now).and_then(|prev| {
                    let ms = now.duration_since(prev).as_secs_f64() * 1000.0;
                    (touching_before && ms < 250.0).then_some(ms)
                });

                let summary = shared.drive(dev, &samples);
                touching_before = summary.fingers > 0;
                state_after = Some((summary.gesture, summary.fingers));

                // Only built when the debug page is actually open. This is a
                // nested document with an array per touch sample and a label
                // clone taken under a mutex, and it was being assembled and
                // serialized on every frame from every device purely to be
                // dropped by a broadcast channel with no receivers.
                if shared.observed() {
                    shared.publish(
                        serde_json::json!({
                            "t": "batch",
                            "device": dev.label(),
                            "deviceId": dev.id,
                            // False when another device holds the cursor: the
                            // gesture was read but deliberately not injected.
                            "held": summary.held,
                            "samples": samples.len(),
                            "gapMs": gap_ms,
                            "gesture": summary.gesture,
                            "fingers": summary.fingers,
                            // The count the gesture is judged on: how many
                            // fingers were down at the busiest moment, not now.
                            "peakFingers": summary.peak_fingers,
                            "actions": summary.actions,
                            "movePx": summary.move_px,
                            "points": samples
                                .iter()
                                .map(|s| serde_json::json!([s.pointer_id, s.phase, s.x, s.y]))
                                .collect::<Vec<_>>(),
                        })
                        .to_string(),
                    );
                }
                // Echo the newest timestamp so the phone can show touch-to-cursor
                // latency (acceptance criterion: under 50 ms on the LAN).
                //
                // Throttled, because this is the only thing that reads it: a
                // number a person glances at, redrawn on a phone. Answering
                // every batch put a frame back on the air for every frame that
                // arrived, and on Wi-Fi a small frame costs nearly the airtime
                // of a large one - so with two phones touching at once, half of
                // everything on the channel was this readout. Ten a second
                // still updates faster than anyone can read it.
                if let Some(t_ms) = last_t {
                    let due = last_echo.map_or(true, |at| at.elapsed() >= ECHO_EVERY);
                    if due {
                        last_echo = Some(now);
                        tx.send(json(&ServerMessage::Echo { t_ms })).await?;
                    }
                }
            }
            Message::Text(text) => match serde_json::from_str::<ClientMessage>(&text) {
                Ok(ClientMessage::Welcome { v, surface, name }) => {
                    if v != protocol::VERSION as u32 {
                        tx.send(json(&ServerMessage::Error { code: "version" }))
                            .await?;
                        break;
                    }
                    dev.rec.locked().set_surface(surface.wpx, surface.hpx);
                    if let Some(name) = name.filter(|n| !n.trim().is_empty()) {
                        // Names come from the other end of the network: keep it
                        // short and printable before it reaches a log or another
                        // phone's screen.
                        let clean: String = name
                            .trim()
                            .chars()
                            .filter(|c| !c.is_control())
                            .take(32)
                            .collect();
                        if !clean.is_empty() {
                            shared.set_label(dev, clean);
                        }
                    }
                    tracing::info!(
                        "{}: surface {}x{} @{}x",
                        dev.label(),
                        surface.wpx,
                        surface.hpx,
                        surface.dpr
                    );
                }
                Ok(ClientMessage::Settings {
                    sensitivity,
                    natural_scroll,
                    follow,
                }) => {
                    // Per device: the tablet and the phone are different sizes,
                    // so they are allowed different sensitivities - and each
                    // keeps its choice when the computer's own settings change.
                    shared.set_overrides(dev, sensitivity, natural_scroll, &follow);
                    tx.send(json(&settings_msg(shared, dev))).await?;
                }
                // Already answered before this session started; a second one
                // proves nothing and changes nothing.
                Ok(ClientMessage::Auth { .. }) => {}
                Err(e) => tracing::debug!("ignoring unparseable control message: {e}"),
            },
            Message::Close(_) => break,
            _ => {}
        }

        // The gesture readout, from the lock `drive` already held. A control
        // message cannot change it, so there is nothing to say after one.
        let Some((gesture, fingers)) = state_after else {
            continue;
        };
        // The tray shows a count and whether anyone is mid-gesture. Neither can
        // change while a finger simply keeps moving, and deriving them means
        // locking the map every other device is also reading, so ask only when
        // this device's own busy flag has flipped.
        let busy = fingers > 0;
        if busy != last_busy {
            last_busy = busy;
            shared.refresh_status();
        }
        if gesture != last_state {
            last_state = gesture;
            tx.send(json(&ServerMessage::State {
                gesture,
                fingers,
                name: None,
                press_ms: None,
                tap_max_px: None,
            }))
            .await?;
        }
    }
    tracing::debug!("session with {peer} closing");
    Ok(())
}
