//! Several devices, one cursor: how the desktop shares it out.
//!
//! A phone and a tablet connected at the same time used to be unusable. Both
//! halves of the bug are pinned here, because neither is visible from one side
//! alone:
//!
//! - **Shared gesture state.** One recognizer served the whole app, so two
//!   devices interleaved their finger ids into one state machine: the tablet's
//!   finger 1 and the phone's finger 1 were the same finger, and a tap on one
//!   ended a drag on the other.
//! - **Eviction.** The newest connection took the cursor and the loser was hung
//!   up on, so it reconnected on its backoff, evicted the device that had just
//!   taken over, and the two traded control about once a second.
//!
//! Now every connection gets its own engine and they take turns: the cursor
//! changes hands only at the start of a gesture, once whoever had it has been
//! quiet for a moment.

use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use padremote::auth::Secret;
use padremote::gesture::{Button, Config, ScrollPhase, Shortcut, TouchSample};
use padremote::input::{Blocked, Injector, NullInjector};
use padremote::net::{LinkStatus, Shared, LINK_CONNECTED, LINK_WAITING};
use padremote::protocol::encode_frame;
use tokio::net::TcpListener;
use tokio_tungstenite::tungstenite::Message;

type Socket =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

/// Longer than the desktop's handover grace, so "the other device has stopped"
/// is unambiguous.
const AFTER_GRACE: Duration = Duration::from_millis(450);

/// An injector that remembers what reached the OS.
///
/// Which device *held* the cursor is not observable from the wire - both are
/// still connected, both still get their state messages - so the only honest
/// assertion is about what was actually injected.
#[derive(Clone, Default)]
struct Recorder(Arc<Mutex<Vec<String>>>);

impl Recorder {
    fn taken(&self) -> Vec<String> {
        std::mem::take(&mut *self.0.lock().unwrap())
    }
    fn push(&mut self, what: impl Into<String>) {
        self.0.lock().unwrap().push(what.into());
    }
}

impl Injector for Recorder {
    fn move_by(&mut self, dx: f64, dy: f64) {
        self.push(format!("move {dx:.0},{dy:.0}"));
    }
    fn button_down(&mut self, button: Button, _clicks: u8) {
        self.push(format!("down {button:?}"));
    }
    fn button_up(&mut self, button: Button) {
        self.push(format!("up {button:?}"));
    }
    fn click(&mut self, button: Button, count: u8) {
        self.push(format!("click {button:?} x{count}"));
    }
    fn scroll_by(&mut self, _dx: f64, _dy: f64, phase: ScrollPhase) {
        self.push(format!("scroll {}", phase.name()));
    }
    fn zoom(&mut self, steps: i32) {
        self.push(format!("zoom {steps}"));
    }
    fn shortcut(&mut self, shortcut: Shortcut) {
        self.push(format!("shortcut {}", shortcut.name()));
    }
    fn release_all(&mut self) {
        self.push("release_all");
    }
    fn sync_cursor(&mut self) {
        self.push("sync");
    }
}

/// Bring up a server on a free port and hand back its addresses.
async fn server() -> (Addrs, Arc<Shared>, Recorder) {
    server_with(Config::default()).await
}

/// The secret every connection in this file pairs with.
///
/// Fixed rather than random so a failure is reproducible; the desktop treats it
/// like any other, and the nonce it challenges with is fresh either way.
fn test_secret() -> Secret {
    Secret::from_hex("0f1e2d3c4b5a69788796a5b4c3d2e1f0").expect("a valid test secret")
}

/// Where the test server can be reached - twice, because two addresses are how
/// a test gets to be two devices.
///
/// The desktop tells physical devices apart by the address they connect from,
/// and replaces a session when a second connection arrives from an address it
/// already has (`Shared::join`) - two tabs on one phone are one user with a
/// stale tab, not two people taking turns. Everything here is loopback, so
/// without two distinct addresses "the phone" and "the tablet" would be one
/// device replacing itself and none of the take-turns behaviour below could be
/// exercised at all.
///
/// macOS assigns lo0 exactly one IPv4 address, so `127.0.0.2` cannot be bound
/// without `ifconfig` and root. The second address is therefore the IPv6
/// loopback, and the server listens on both - which is a configuration the
/// shipped app can be asked for anyway.
#[derive(Clone, Copy)]
struct Addrs {
    v4: SocketAddr,
    v6: SocketAddr,
}

impl Addrs {
    /// Which address a named device dials from.
    ///
    /// Deterministic, so a test that reconnects "iPhone" comes back from the
    /// same address and is correctly recognised as that device returning.
    fn device(&self, name: &str) -> SocketAddr {
        match name {
            "iPhone" => self.v4,
            _ => self.v6,
        }
    }
}

/// The same, with a config standing in for what the host's trackpad says.
async fn server_with(cfg: Config) -> (Addrs, Arc<Shared>, Recorder) {
    let injected = Recorder::default();
    let (addrs, shared) = server_injecting(cfg, Box::new(injected.clone())).await;
    (addrs, shared, injected)
}

/// A server on a computer that cannot move its own cursor.
///
/// The state a Mac is in before Accessibility is granted: everything is
/// recognised, nothing is injected, and the whole app is otherwise healthy.
async fn deaf_server() -> (Addrs, Arc<Shared>) {
    server_injecting(
        Config::default(),
        Box::new(NullInjector::new(Blocked::Permission)),
    )
    .await
}

/// The listeners, around whichever backend the test wants behind them.
async fn server_injecting(cfg: Config, injector: Box<dyn Injector>) -> (Addrs, Arc<Shared>) {
    let shared = Arc::new(Shared::new(
        cfg,
        injector,
        "test-host".into(),
        LinkStatus::new(),
        test_secret(),
        padremote::auth::Devices::default(),
    ));
    let mut bound = Vec::new();
    for host in ["127.0.0.1:0", "[::1]:0"] {
        let listener = TcpListener::bind(host).await.unwrap();
        bound.push(listener.local_addr().unwrap());
        let server_shared = shared.clone();
        tokio::spawn(async move {
            let _ = padremote::net::serve_on(listener, server_shared).await;
        });
    }
    let addrs = Addrs {
        v4: bound[0],
        v6: bound[1],
    };
    (addrs, shared)
}

async fn connect(addr: SocketAddr) -> Socket {
    let mut ws = open(addr, "/").await.expect("connect");
    answer_challenge(&mut ws, &test_secret()).await;
    ws
}

/// Open a socket without answering the challenge.
///
/// Split out so the auth tests can look at what the desktop says to a caller
/// that never proves anything.
async fn open(addr: SocketAddr, path: &str) -> Result<Socket, String> {
    tokio_tungstenite::connect_async(format!("ws://{addr}{path}"))
        .await
        .map(|(ws, _)| ws)
        .map_err(|e| e.to_string())
}

/// Read the challenge and answer it, the way the phone page does.
/// A read-only watcher on `/observe`, past the challenge.
///
/// An observer never joins the device list, so which address it comes from
/// does not matter.
async fn observe(addrs: Addrs) -> Socket {
    let mut ws = open(addrs.v4, "/observe").await.expect("observe");
    answer_challenge(&mut ws, &test_secret()).await;
    ws
}

async fn answer_challenge(ws: &mut Socket, secret: &Secret) {
    let text = next_text(ws).await.expect("a challenge on connect");
    let msg: serde_json::Value = serde_json::from_str(&text).expect("challenge json");
    assert_eq!(msg["t"], "challenge", "the desktop must challenge first");
    let nonce = msg["nonce"].as_str().expect("a nonce").to_string();
    ws.send(Message::Text(
        serde_json::json!({ "t": "auth", "hmac": secret.sign(nonce.as_bytes()) }).to_string(),
    ))
    .await
    .unwrap();
}

/// Connect, name the device, and settle: the caller can then reason about who
/// holds the cursor without racing the greeting.
async fn connect_as(addrs: Addrs, name: &str) -> Socket {
    let mut ws = connect(addrs.device(name)).await;
    ws.send(Message::Text(
        serde_json::json!({
            "t": "welcome",
            "v": 1,
            "name": name,
            "surface": { "wpx": 390.0, "hpx": 669.0, "dpr": 3.0 },
        })
        .to_string(),
    ))
    .await
    .unwrap();
    settle().await;
    ws
}

/// Let the server finish what the last frame set in motion.
async fn settle() {
    tokio::time::sleep(Duration::from_millis(120)).await;
}

/// Read messages until a text one arrives, or time out.
///
/// The desktop volunteers `state`, `control` and `echo` frames unprompted, so a
/// test that wants a specific message has to skip past whatever else is in
/// flight.
async fn next_text(ws: &mut Socket) -> Option<String> {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
    loop {
        let msg = tokio::time::timeout_at(deadline, ws.next()).await.ok()??;
        match msg.ok()? {
            Message::Text(t) => return Some(t),
            Message::Close(_) => return None,
            _ => continue,
        }
    }
}

/// Everything the socket has to say right now, without waiting for more.
async fn drain(ws: &mut Socket) -> Vec<String> {
    let mut out = Vec::new();
    while let Ok(Some(Ok(msg))) = tokio::time::timeout(Duration::from_millis(120), ws.next()).await
    {
        if let Message::Text(t) = msg {
            out.push(t);
        }
    }
    out
}

fn sample(t_ms: u32, pointer_id: u8, phase: u8, x: f32, y: f32) -> TouchSample {
    TouchSample {
        t_ms,
        pointer_id,
        phase,
        x,
        y,
    }
}

/// A complete tap: down and straight back up in the same spot.
async fn tap(ws: &mut Socket, t_ms: u32, pointer_id: u8) {
    ws.send(Message::Binary(encode_frame(&[sample(
        t_ms, pointer_id, 0, 0.5, 0.5,
    )])))
    .await
    .unwrap();
    ws.send(Message::Binary(encode_frame(&[sample(
        t_ms + 40,
        pointer_id,
        2,
        0.5,
        0.5,
    )])))
    .await
    .unwrap();
    settle().await;
}

/// Both devices stay connected. Neither is evicted, and neither is told to
/// stand down - that eviction, and the reconnect it provoked, was the tug of
/// war that made two devices at once unusable.
#[tokio::test]
async fn a_second_device_does_not_evict_the_first() {
    let (addr, shared, _inj) = server().await;

    let mut phone = connect_as(addr, "iPhone").await;
    let _ = drain(&mut phone).await;
    let mut tablet = connect_as(addr, "Tablet").await;

    assert_eq!(
        shared.device_count(),
        2,
        "both devices are connected at once"
    );

    let from_phone = drain(&mut phone).await.join(" ");
    assert!(
        !from_phone.contains("superseded"),
        "the first device must not be evicted; it got {from_phone}"
    );

    // And it is still a live socket: a round trip still works.
    tap(&mut phone, 0, 1).await;
    assert!(
        next_text(&mut phone).await.is_some(),
        "the first device is still being served"
    );
    drop(tablet.close(None));
}

/// Two devices no longer share one gesture.
///
/// The tablet's three fingers must not appear in the phone's finger count.
/// When both drove one recognizer this is exactly what corrupted the state
/// machine: a touch anywhere promoted the other device's gesture to a
/// multi-finger one, so a drag became a swipe and a scroll became a zoom.
#[tokio::test]
async fn devices_do_not_share_fingers() {
    let (addr, shared, _inj) = server().await;

    let mut phone = connect_as(addr, "iPhone").await;
    let mut tablet = connect_as(addr, "Tablet").await;

    // The phone puts one finger down and keeps it there.
    phone
        .send(Message::Binary(encode_frame(&[sample(0, 1, 0, 0.5, 0.5)])))
        .await
        .unwrap();
    settle().await;

    // The tablet lands three fingers of its own, with ids that collide.
    for id in 1..4u8 {
        tablet
            .send(Message::Binary(encode_frame(&[sample(
                10, id, 0, 0.2, 0.2,
            )])))
            .await
            .unwrap();
    }
    settle().await;

    let devices = shared.devices();
    assert_eq!(devices.len(), 2);
    assert_eq!(
        devices[0].rec.lock().unwrap().finger_count(),
        1,
        "the phone's gesture must see only the phone's finger"
    );
    assert_eq!(
        devices[1].rec.lock().unwrap().finger_count(),
        3,
        "the tablet keeps its own fingers, and only its own"
    );
}

/// While one device is mid-gesture, the other's touches are read but not obeyed.
///
/// This is what stops the two from fighting over the cursor: a stray touch on
/// the tablet - or a hand resting on it - can no longer click in the middle of
/// the phone's drag.
#[tokio::test]
async fn the_holder_is_not_interrupted() {
    let (addr, _shared, injected) = server().await;

    let mut phone = connect_as(addr, "iPhone").await;
    let mut tablet = connect_as(addr, "Tablet").await;

    // The phone starts a gesture and holds a finger down.
    phone
        .send(Message::Binary(encode_frame(&[sample(0, 1, 0, 0.5, 0.5)])))
        .await
        .unwrap();
    settle().await;
    injected.taken();

    // The tablet taps, twice, while the phone's finger is still down.
    tap(&mut tablet, 100, 1).await;
    tap(&mut tablet, 300, 1).await;

    assert!(
        injected.taken().is_empty(),
        "nothing from the second device may reach the cursor mid-gesture"
    );
}

/// Put the other one down, pick this one up, and it just works.
///
/// No button to press and no page to reload: the cursor goes to whoever starts
/// a gesture once the previous device has been quiet for a beat.
#[tokio::test]
async fn control_passes_to_whoever_picks_up_next() {
    let (addr, _shared, injected) = server().await;

    let mut phone = connect_as(addr, "iPhone").await;
    let mut tablet = connect_as(addr, "Tablet").await;

    tap(&mut phone, 0, 1).await;
    assert!(
        injected.taken().iter().any(|a| a.starts_with("click")),
        "the first device to touch drives the cursor"
    );

    // The tablet, tapping immediately, is still inside the phone's grace period
    // - which is what keeps a double-tap from being stolen half way through.
    tap(&mut tablet, 100, 1).await;
    assert!(
        injected.taken().is_empty(),
        "the cursor stays with the device that just used it"
    );

    // Once the phone has been quiet, the tablet takes over by simply touching.
    tokio::time::sleep(AFTER_GRACE).await;
    tap(&mut tablet, 1000, 1).await;
    assert!(
        injected.taken().iter().any(|a| a.starts_with("click")),
        "the tablet takes the cursor once the phone is done with it"
    );

    // And the phone can take it straight back the same way.
    tokio::time::sleep(AFTER_GRACE).await;
    tap(&mut phone, 2000, 1).await;
    assert!(
        injected.taken().iter().any(|a| a.starts_with("click")),
        "and the phone takes it back"
    );
}

/// Finishing a move hands the cursor straight on - no waiting out a grace.
///
/// The grace exists to protect the gap inside a double-tap, and a finger that
/// travelled cannot be the first half of one. Charging it the full 350 ms
/// anyway is what made picking up the other device feel like the app had not
/// noticed: `Recognizer::follow_up_ms` is what tells the two apart, and
/// `tests/follow_up.rs` covers the answer it gives.
#[tokio::test]
async fn the_cursor_passes_on_at_once_after_a_plain_move() {
    let (addr, _shared, injected) = server().await;

    let mut phone = connect_as(addr, "iPhone").await;
    let mut tablet = connect_as(addr, "Tablet").await;

    // The phone drags the cursor across and lifts off. Nothing about that can
    // be continued.
    phone
        .send(Message::Binary(encode_frame(&[
            sample(0, 1, 0, 0.5, 0.5),
            sample(16, 1, 1, 0.7, 0.7),
            sample(32, 1, 1, 0.9, 0.9),
            sample(48, 1, 2, 0.9, 0.9),
        ])))
        .await
        .unwrap();
    settle().await;
    assert!(
        injected.taken().iter().any(|a| a.starts_with("move")),
        "the phone drove that move"
    );

    // No sleep: the tablet touches immediately, and must be obeyed.
    tap(&mut tablet, 100, 1).await;
    assert!(
        injected.taken().iter().any(|a| a.starts_with("click")),
        "a move is over when the finger lifts, so the next device in is not \
         made to wait out a grace that protects nothing"
    );
}

/// The debug feed still gets everything, now that it is built only on demand.
///
/// `session.rs` skips assembling the telemetry document unless somebody is on
/// `/observe` - it was serializing one per frame per device into a channel with
/// no receivers. The saving is only correct if a real observer still sees every
/// batch, which is what this asserts.
#[tokio::test]
async fn an_observer_still_receives_every_batch() {
    let (addr, _shared, _injected) = server().await;

    let mut watcher = observe(addr).await;
    // Past the `hello` the observer is greeted with.
    let _ = drain(&mut watcher).await;

    let mut phone = connect_as(addr, "iPhone").await;
    tap(&mut phone, 0, 1).await;

    let lines = drain(&mut watcher).await;
    let batches: Vec<&String> = lines
        .iter()
        .filter(|l| l.contains(r#""t":"batch""#))
        .collect();
    assert!(
        !batches.is_empty(),
        "the debug feed must still see touch batches, got: {lines:?}"
    );
    assert!(
        batches.iter().any(|b| b.contains(r#""points""#)),
        "including the per-sample points it is there to show"
    );
}

/// Every gesture opens by asking the backend where the cursor really is.
///
/// A backend that posts absolute positions - which the macOS one must, because
/// a CGEvent mouse event carries a point and not a delta - keeps its own idea
/// of where the cursor sits. Nothing tells it when the user picks up the
/// computer's own trackpad, so between gestures that idea quietly goes stale,
/// and the first move of the next gesture snaps the cursor back to wherever
/// PadRemote last left it.
///
/// `MacInjector::sync_from_system` was written for exactly this and then never
/// called from anywhere - the fix was one line of wiring, which is precisely
/// the kind that rots back out again unnoticed. So the assertion is about the
/// order: the sync has to reach the backend before anything it would correct.
#[tokio::test]
async fn a_fresh_gesture_re_reads_the_real_cursor_first() {
    let (addr, _shared, injected) = server().await;
    let mut phone = connect_as(addr, "iPhone").await;

    // Down, a move, and up: a whole gesture, from a device that has just taken
    // the cursor.
    phone
        .send(Message::Binary(encode_frame(&[sample(0, 1, 0, 0.5, 0.5)])))
        .await
        .unwrap();
    phone
        .send(Message::Binary(encode_frame(&[sample(16, 1, 1, 0.7, 0.7)])))
        .await
        .unwrap();
    phone
        .send(Message::Binary(encode_frame(&[sample(32, 1, 2, 0.7, 0.7)])))
        .await
        .unwrap();
    settle().await;

    let done = injected.taken();
    let synced = done.iter().position(|a| a == "sync");
    let moved = done.iter().position(|a| a.starts_with("move"));
    assert!(
        synced.is_some(),
        "a gesture that starts must re-read the cursor: {done:?}"
    );
    assert!(
        moved.is_some(),
        "the gesture should have moved the cursor: {done:?}"
    );
    assert!(
        synced < moved,
        "the sync arrived after the move it exists to correct: {done:?}"
    );

    // And it is once per gesture, not once per batch: re-reading mid-gesture
    // would fight whatever the gesture is already doing.
    assert_eq!(
        done.iter().filter(|a| *a == "sync").count(),
        1,
        "one sync per gesture, not one per batch: {done:?}"
    );
}

/// A device that is not driving is told so, and by whom.
///
/// Without this the hand-over is indistinguishable from a dead link: the page
/// looks connected, the finger moves, and nothing happens.
#[tokio::test]
async fn a_waiting_device_is_told_who_has_the_cursor() {
    let (addr, _shared, _inj) = server().await;

    let mut phone = connect_as(addr, "iPhone").await;
    let mut tablet = connect_as(addr, "Tablet").await;
    let _ = drain(&mut tablet).await;

    // The phone takes the cursor.
    tap(&mut phone, 0, 1).await;

    let seen = drain(&mut tablet).await.join(" ");
    assert!(
        seen.contains("\"t\":\"control\""),
        "the tablet must hear about the hand-over; it got {seen}"
    );
    assert!(
        seen.contains("\"active\":false") && seen.contains("iPhone"),
        "and be told who holds it; it got {seen}"
    );

    // The holder's own view says the opposite.
    let mine = drain(&mut phone).await.join(" ");
    assert!(
        mine.contains("\"active\":true"),
        "the driving device is told it is driving; it got {mine}"
    );
}

/// A computer that cannot move its own cursor says so, unprompted.
///
/// This is the failure that is invisible from the phone. Without Accessibility
/// macOS takes every event the app posts and discards it, so the page sees a
/// healthy socket, a gesture readout that follows every finger, a live latency
/// figure - and a cursor that never moves. The user's conclusion is that the
/// app is broken, and the actual fix is a checkbox they were never told about.
#[tokio::test]
async fn a_computer_that_cannot_inject_tells_the_phone_why() {
    let (addr, _shared) = deaf_server().await;

    let mut phone = connect_as(addr, "iPhone").await;
    let seen = drain(&mut phone).await.join(" ");

    assert!(
        seen.contains("\"blocked\":\"permission\""),
        "a phone driving a computer with no Accessibility grant must be told; it got {seen}"
    );
}

/// Granting permission reaches the phone already in someone's hand.
///
/// The app swaps its backend in without a restart, so the phone is never going
/// to reconnect and ask again. A page that had been apologising since it
/// connected would go on apologising over a trackpad that now works.
#[tokio::test]
async fn granting_permission_reaches_a_phone_that_is_already_connected() {
    let (addr, shared) = deaf_server().await;

    let mut phone = connect_as(addr, "iPhone").await;
    let _ = drain(&mut phone).await;

    shared.set_injector(Box::new(Recorder::default()));
    settle().await;

    let seen = drain(&mut phone).await.join(" ");
    assert!(
        seen.contains("\"t\":\"control\"") && !seen.contains("blocked"),
        "the phone must hear that the cursor is live, without reconnecting; it got {seen}"
    );
}

/// And the ordinary case stays quiet: nothing to explain, nothing said.
#[tokio::test]
async fn a_working_computer_says_nothing_about_being_blocked() {
    let (addr, _shared, _inj) = server().await;

    let mut phone = connect_as(addr, "iPhone").await;
    let seen = drain(&mut phone).await.join(" ");

    assert!(
        !seen.contains("blocked"),
        "a computer that injects must not apologise for anything; it got {seen}"
    );
}

/// One device leaving must not report the other as gone.
#[tokio::test]
async fn one_device_leaving_leaves_the_other_connected() {
    let (addr, shared, _inj) = server().await;

    let phone = connect_as(addr, "iPhone").await;
    let tablet = connect_as(addr, "Tablet").await;

    drop(phone);
    settle().await;

    assert_eq!(shared.device_count(), 1, "the tablet is still connected");
    assert_eq!(shared.status.get(), LINK_CONNECTED);
    assert_eq!(shared.status.devices(), 1);

    // And when the last one leaves, the link really does go quiet.
    drop(tablet);
    settle().await;
    assert_eq!(shared.status.get(), LINK_WAITING);
    assert_eq!(shared.status.devices(), 0);
}

/// A device that drops mid-drag never leaves the button down - and its exit
/// hands the cursor straight to whoever else is connected.
#[tokio::test]
async fn a_device_that_vanishes_mid_drag_releases_the_button() {
    let (addr, _shared, injected) = server().await;

    let mut phone = connect_as(addr, "iPhone").await;
    let mut tablet = connect_as(addr, "Tablet").await;

    // Hold long enough for the press to arm a drag, then die.
    phone
        .send(Message::Binary(encode_frame(&[sample(0, 1, 0, 0.5, 0.5)])))
        .await
        .unwrap();
    settle().await;
    let press_ms = Config::default().tap.press_ms;
    phone
        .send(Message::Binary(encode_frame(&[sample(
            press_ms + 50,
            1,
            1,
            0.5,
            0.5,
        )])))
        .await
        .unwrap();
    settle().await;
    let during = injected.taken();
    assert!(
        during.iter().any(|a| a == "down Left"),
        "the hold should have started a drag; got {during:?}"
    );

    drop(phone);
    settle().await;
    let after = injected.taken();
    assert!(
        after.iter().any(|a| a == "up Left"),
        "a dropped connection must not leave the button down; got {after:?}"
    );

    // The cursor is free again the moment the holder is gone.
    tap(&mut tablet, 1000, 1).await;
    assert!(
        injected.taken().iter().any(|a| a.starts_with("click")),
        "the surviving device can drive immediately"
    );
}

/// Each device keeps its own settings.
///
/// A tablet and a phone are different sizes, so the sensitivity that suits one
/// is wrong for the other; they used to overwrite each other's.
#[tokio::test]
async fn settings_are_per_device() {
    let (addr, shared, _inj) = server().await;

    let mut phone = connect_as(addr, "iPhone").await;
    let mut tablet = connect_as(addr, "Tablet").await;

    phone
        .send(Message::Text(
            r#"{"t":"settings","sensitivity":0.5}"#.into(),
        ))
        .await
        .unwrap();
    tablet
        .send(Message::Text(
            r#"{"t":"settings","sensitivity":2.0}"#.into(),
        ))
        .await
        .unwrap();
    settle().await;

    let devices = shared.devices();
    assert_eq!(devices[0].rec.lock().unwrap().cfg.sensitivity, 0.5);
    assert_eq!(devices[1].rec.lock().unwrap().cfg.sensitivity, 2.0);
}

/// Watching on `/observe` is not holding the cursor.
///
/// An observer used to flip the link light to "connected", so the tray claimed
/// a phone was attached whenever the debug page was open.
#[tokio::test]
async fn an_observer_is_not_a_phone() {
    let (addr, shared, _inj) = server().await;

    let mut obs = observe(addr).await;
    // The observer's own hello proves the connection is fully up.
    let mut obs_text = String::new();
    if let Some(t) = next_text(&mut obs).await {
        obs_text = t;
    }
    assert!(
        obs_text.contains("\"hello\""),
        "observer hello, got {obs_text}"
    );

    assert_eq!(
        shared.status.get(),
        LINK_WAITING,
        "an observer must not look like a connected phone"
    );
    assert_eq!(shared.device_count(), 0);
}

/// The telemetry stream says which device each batch came from, and whether it
/// was driving - the debug page shows two hands at once, so it has to be able
/// to tell them apart.
#[tokio::test]
async fn telemetry_names_the_device() {
    let (addr, _shared, _inj) = server().await;

    let mut obs = observe(addr).await;
    next_text(&mut obs).await;

    let mut phone = connect_as(addr, "iPhone").await;
    tap(&mut phone, 0, 1).await;

    let batches = drain(&mut obs).await.join(" ");
    assert!(
        batches.contains("\"device\":\"iPhone\"") && batches.contains("\"held\":true"),
        "a batch should carry its device and whether it drove; got {batches}"
    );
}

// ---------------------------------------------------------- mirrored settings
//
// PadRemote's whole promise is that it behaves like the trackpad the user
// already has. That promise was quietly broken from the *phone* side: the page
// stored its own copy of every setting and sent it the moment it connected, so
// whatever the settings sheet happened to be left on overrode what the computer
// actually said. A Mac with natural scrolling off scrolled the wrong way, and
// nothing in the desktop's own mirroring was wrong.

/// A phone that has expressed no opinion is driven by the computer's settings.
#[tokio::test]
async fn a_silent_phone_follows_the_computer() {
    // Stand in for a Mac with natural scrolling switched off.
    let mut cfg = Config::default();
    cfg.scroll.natural = false;
    let (addr, shared, _inj) = server_with(cfg).await;

    let mut phone = connect_as(addr, "iPhone").await;

    assert!(
        !shared.devices()[0].rec.lock().unwrap().cfg.scroll.natural,
        "the computer said off, so the phone scrolls off"
    );

    // And it is *told*, so its settings sheet can show the computer's value
    // rather than a guess.
    let seen = drain(&mut phone).await.join(" ");
    assert!(
        seen.contains("\"t\":\"settings\"") && seen.contains("\"naturalScroll\":false"),
        "the phone must be told what it is being driven with; it got {seen}"
    );
    assert!(
        seen.contains("\"naturalScroll\":true"),
        "...and that this value is the computer's own; it got {seen}"
    );
}

/// A setting the user changed on the phone survives the computer changing its
/// own - which happens twice a second, every time the host is re-read.
#[tokio::test]
async fn an_override_survives_a_config_reload() {
    let mut off = Config::default();
    off.scroll.natural = false;
    let (addr, shared, _inj) = server_with(off.clone()).await;

    let mut phone = connect_as(addr, "iPhone").await;
    phone
        .send(Message::Text(
            r#"{"t":"settings","naturalScroll":true}"#.into(),
        ))
        .await
        .unwrap();
    settle().await;
    assert!(shared.devices()[0].rec.lock().unwrap().cfg.scroll.natural);

    // The host is re-read on a timer and the config rebuilt from it. This used
    // to overwrite the recognizer wholesale, undoing the user's choice about
    // twice a second - a setting that would not stay set, with nothing in the
    // log to say why.
    shared.set_config(off);
    settle().await;
    assert!(
        shared.devices()[0].rec.lock().unwrap().cfg.scroll.natural,
        "the phone's own choice must outlive a reload"
    );

    // ...and the phone is told again, because the values behind the sheet moved.
    let seen = drain(&mut phone).await.join(" ");
    assert!(
        seen.contains("\"t\":\"settings\""),
        "a reload must re-state the settings; got {seen}"
    );
}

/// The user can hand a setting back to the computer.
#[tokio::test]
async fn a_phone_can_follow_the_computer_again() {
    let mut off = Config::default();
    off.scroll.natural = false;
    let (addr, shared, _inj) = server_with(off).await;

    let mut phone = connect_as(addr, "iPhone").await;
    phone
        .send(Message::Text(
            r#"{"t":"settings","naturalScroll":true}"#.into(),
        ))
        .await
        .unwrap();
    settle().await;
    assert!(shared.devices()[0].rec.lock().unwrap().cfg.scroll.natural);

    // Handing it back is not the same as saying nothing about it, which is why
    // it is a list of names and not an absent field.
    phone
        .send(Message::Text(
            r#"{"t":"settings","follow":["naturalScroll"]}"#.into(),
        ))
        .await
        .unwrap();
    settle().await;
    assert!(
        !shared.devices()[0].rec.lock().unwrap().cfg.scroll.natural,
        "the computer's value must come back"
    );

    let seen = drain(&mut phone).await.join(" ");
    assert!(
        seen.contains("\"naturalScroll\":true"),
        "and the phone must be told it is following again; got {seen}"
    );
}

/// Nonsense from the network never reaches the engine.
#[tokio::test]
async fn an_absurd_sensitivity_is_ignored() {
    let (addr, shared, _inj) = server().await;
    let mut phone = connect_as(addr, "iPhone").await;

    for bad in ["0", "-3", "1e9", "null"] {
        phone
            .send(Message::Text(format!(
                r#"{{"t":"settings","sensitivity":{bad}}}"#
            )))
            .await
            .unwrap();
    }
    settle().await;

    let s = shared.devices()[0].rec.lock().unwrap().cfg.sensitivity;
    assert_eq!(
        s,
        Config::default().sensitivity,
        "a bad value changes nothing"
    );
}

// ------------------------------------------------- one session per device

/// The rule the take-turns arbiter above is deliberately *not* applied to.
///
/// Two tabs on one phone are one user with a stale tab, not two people sharing
/// a cursor. The older one is hung up on, and - this is the half that matters -
/// it is told why, so it stays down instead of reconnecting and taking the link
/// straight back off the tab the user is actually looking at.
#[tokio::test]
async fn a_second_tab_on_the_same_device_replaces_the_first() {
    let (addrs, shared, injected) = server().await;
    let mut first = connect_as(addrs, "iPhone").await;
    assert_eq!(shared.device_count(), 1);

    // The same phone again: same address, new socket.
    let mut second = connect(addrs.v4).await;
    settle().await;

    assert_eq!(
        shared.device_count(),
        1,
        "one phone must not appear twice however many tabs it has open"
    );

    // The old tab is told which of its problems this is. "replaced" and not
    // "offline": the advice is to close the other tab, not to check the Wi-Fi.
    let said = drain(&mut first).await;
    assert!(
        said.iter().any(|t| t.contains("replaced")),
        "the first tab was dropped without being told why: {said:?}"
    );

    // And the surviving tab drives.
    let _ = injected.taken();
    tap(&mut second, 1_000, 1).await;
    assert!(
        !injected.taken().is_empty(),
        "the tab that replaced the other one cannot drive the cursor"
    );
}

/// The distinction the whole rule turns on. Evicting the newest connection
/// *globally* is what this server used to do, and it cost a tug of war: the
/// loser reconnected on its backoff, evicted whoever had taken over, and the
/// two traded the cursor about once a second. Only a device replacing itself
/// may ever be hung up on.
#[tokio::test]
async fn a_different_device_is_never_replaced() {
    let (addrs, shared, _injected) = server().await;
    let mut phone = connect_as(addrs, "iPhone").await;
    let mut tablet = connect_as(addrs, "Tablet").await;
    settle().await;

    assert_eq!(shared.device_count(), 2, "both devices must stay connected");
    for (who, ws) in [("iPhone", &mut phone), ("Tablet", &mut tablet)] {
        let said = drain(ws).await;
        assert!(
            !said.iter().any(|t| t.contains("replaced")),
            "{who} was hung up on by the other device: {said:?}"
        );
    }
}

/// A phone that was mid-drag when its own reload replaced it must not leave the
/// button down - the same guarantee a disconnect gives, on a path that does not
/// go through `leave`.
#[tokio::test]
async fn being_replaced_mid_drag_releases_the_button() {
    let (addrs, _shared, injected) = server().await;
    let mut first = connect_as(addrs, "iPhone").await;

    // Hold still, long enough for the press to arm a drag - the same shape as
    // `a_device_that_vanishes_mid_drag_releases_the_button`, because it is the
    // same guarantee arriving down a different path.
    first
        .send(Message::Binary(encode_frame(&[sample(0, 1, 0, 0.5, 0.5)])))
        .await
        .unwrap();
    settle().await;
    let press_ms = Config::default().tap.press_ms;
    first
        .send(Message::Binary(encode_frame(&[sample(
            press_ms + 50,
            1,
            1,
            0.5,
            0.5,
        )])))
        .await
        .unwrap();
    settle().await;
    let during = injected.taken();
    assert!(
        during.iter().any(|a| a == "down Left"),
        "the hold should have started a drag; got {during:?}"
    );

    // The user reloads the page. The old session goes away mid-gesture, and the
    // replacement path has to release what it was holding just as a disconnect
    // does - it does not go through `leave`.
    let _second = connect(addrs.v4).await;
    settle().await;
    let after = injected.taken();
    assert!(
        after.iter().any(|a| a == "up Left"),
        "the replaced tab left the button held down; got {after:?}"
    );
}

/// Hanging up from the menu bar is the same mechanism, and has to reach a
/// device that is connected right now.
#[tokio::test]
async fn disconnecting_from_the_menu_bar_drops_one_device_only() {
    let (addrs, shared, _injected) = server().await;
    let mut phone = connect_as(addrs, "iPhone").await;
    let mut tablet = connect_as(addrs, "Tablet").await;
    settle().await;

    let view = shared.control_view();
    assert_eq!(view.list.len(), 2, "the menu bar sees both devices");
    let phone_id = view
        .list
        .iter()
        .find(|d| d.label == "iPhone")
        .expect("the phone is listed by name")
        .id;

    assert!(shared.disconnect(phone_id), "disconnect reports success");
    settle().await;

    assert_eq!(shared.device_count(), 1);
    let said = drain(&mut phone).await;
    assert!(
        said.iter().any(|t| t.contains("replaced")),
        "the disconnected phone was not told: {said:?}"
    );
    let tablet_said = drain(&mut tablet).await;
    assert!(
        !tablet_said.iter().any(|t| t.contains("replaced")),
        "disconnecting the phone also dropped the tablet: {tablet_said:?}"
    );

    assert!(
        !shared.disconnect(phone_id),
        "disconnecting a device that has already gone reports nothing to do"
    );
}

/// What the menu bar reads. A bare count cannot answer the question people
/// actually have - *which* device is that second one?
#[tokio::test]
async fn the_device_list_names_who_is_connected_and_who_is_driving() {
    let (addrs, shared, _injected) = server().await;
    let mut phone = connect_as(addrs, "iPhone").await;
    let _tablet = connect_as(addrs, "Tablet").await;
    settle().await;

    let listed = shared.status.devices_list();
    let names: Vec<&str> = listed.iter().map(|d| d.label.as_str()).collect();
    assert_eq!(names, vec!["iPhone", "Tablet"], "oldest first, by name");
    assert!(
        listed.iter().all(|d| !d.driving),
        "nobody is driving before anyone touches anything"
    );

    tap(&mut phone, 1_000, 1).await;
    settle().await;
    let listed = shared.status.devices_list();
    let driving: Vec<&str> = listed
        .iter()
        .filter(|d| d.driving)
        .map(|d| d.label.as_str())
        .collect();
    assert_eq!(driving, vec!["iPhone"], "the device holding the cursor");
}

// ------------------------------------------------------------- the address

/// What the menu bar watches so its QR does not go stale.
///
/// The failure this guards against is quiet and complete: the router hands out
/// a new address, every QR and printed URL from before points at nothing, and
/// the phone reports only "this site can't be reached" - which says nothing
/// about the address being what moved. Before this, the pairing links were
/// built once at startup and never looked again.
#[tokio::test]
async fn the_lan_address_is_published_and_only_when_it_changes() {
    let (_addrs, shared, _injected) = server().await;
    assert_eq!(shared.lan_ip(), None, "nothing has looked yet");

    // A phone that has enrolled must stay enrolled when the router renumbers
    // the network under it, so the paired list is what this watches.
    let device = "55555555555555555555555555555555";
    shared.paired.enrol(device, "iPhone");

    shared.set_lan_ip(Some("192.168.1.117".into()));
    assert_eq!(shared.lan_ip().as_deref(), Some("192.168.1.117"));

    // Re-publishing the same address must be a no-op, or the menu would rebuild
    // its pairing links - and reopen nothing - every five seconds forever.
    shared.set_lan_ip(Some("192.168.1.117".into()));
    assert_eq!(shared.lan_ip().as_deref(), Some("192.168.1.117"));

    // A move, and Wi-Fi going away, are both just changes.
    shared.set_lan_ip(Some("192.168.1.165".into()));
    assert_eq!(shared.lan_ip().as_deref(), Some("192.168.1.165"));
    shared.set_lan_ip(None);
    assert_eq!(shared.lan_ip(), None);

    assert!(
        shared.paired.is_paired(device),
        "moving address un-paired a device that had enrolled"
    );
}

/// The connect page never counts itself, or settings, as a connected phone.
#[tokio::test]
async fn the_devices_channel_reports_real_devices_without_joining() {
    let (addrs, shared, _) = server().await;
    let mut monitor = open(addrs.v4, "/devices").await.unwrap();
    answer_challenge(&mut monitor, &test_secret()).await;
    let initial: serde_json::Value =
        serde_json::from_str(&next_text(&mut monitor).await.unwrap()).unwrap();
    assert_eq!(initial["t"], "devices");
    assert_eq!(initial["connected"], 0);
    assert_eq!(initial["devices"], serde_json::json!([]));
    assert_eq!(shared.device_count(), 0);
    let _observer = observe(addrs).await;
    let mut settings = open(addrs.v4, "/config").await.unwrap();
    answer_challenge(&mut settings, &test_secret()).await;
    assert!(next_text(&mut settings)
        .await
        .unwrap()
        .contains("\"t\":\"config\""));
    assert_eq!(shared.device_count(), 0);
    let mut phone = connect_as(addrs, "iPhone").await;
    assert_eq!(devices_reaching(&mut monitor, 1).await["connected"], 1);
    phone.close(None).await.unwrap();
    assert_eq!(devices_reaching(&mut monitor, 0).await["connected"], 0);
    // Rotating the secret hangs up on everything, this page included: the key
    // it holds no longer opens anything.
    shared.unpair_all();
    assert!(next_text(&mut monitor).await.is_none());
}

/// Read from the devices channel until it reports `count` connected.
///
/// A device that joins and then names itself changes the list twice, so a
/// single read is a race with the label - which is exactly the sort of flake
/// that gets a test quarantined rather than believed.
async fn devices_reaching(ws: &mut Socket, count: u64) -> serde_json::Value {
    loop {
        let text = next_text(ws)
            .await
            .expect("the devices channel stayed open");
        let msg: serde_json::Value = serde_json::from_str(&text).unwrap();
        if msg["connected"].as_u64() == Some(count) {
            return msg;
        }
    }
}

/// Everything the channel says before it hangs up, and `None` if it never does.
async fn read_until_closed(ws: &mut Socket) -> Option<serde_json::Value> {
    let mut last = None;
    while let Some(text) = next_text(ws).await {
        last = serde_json::from_str(&text).ok();
    }
    last
}

/// The list is what the menu bar's submenu used to be, so it has to answer the
/// question that submenu answered: *which* devices, and can this one be
/// revoked? A phone in a bag downstairs is paired and not connected, and it is
/// exactly the one somebody wants to forget.
#[tokio::test]
async fn the_devices_channel_lists_paired_devices_and_forgets_them() {
    let (addrs, shared, _) = server().await;
    let absent = "44444444444444444444444444444444";
    shared.paired.enrol(absent, "Old iPad");

    let mut page = open(addrs.v4, "/devices").await.unwrap();
    answer_challenge(&mut page, &test_secret()).await;
    let listed: serde_json::Value =
        serde_json::from_str(&next_text(&mut page).await.unwrap()).unwrap();
    assert!(
        listed["manage"].as_bool().unwrap(),
        "loopback is this computer"
    );
    let rows = listed["devices"].as_array().unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["id"], absent);
    assert_eq!(rows[0]["name"], "Old iPad");
    assert_eq!(rows[0]["connected"], false);

    page.send(Message::Text(
        serde_json::json!({ "t": "forget", "id": absent }).to_string(),
    ))
    .await
    .unwrap();
    let after: serde_json::Value =
        serde_json::from_str(&next_text(&mut page).await.unwrap()).unwrap();
    assert_eq!(after["devices"], serde_json::json!([]));
    assert!(
        !shared.paired.is_paired(absent),
        "forgetting a device that was not connected did nothing"
    );
}

/// Forgetting everything rotates the secret, which drops every socket - this
/// one included, so the page knows to reload and show the new code.
#[tokio::test]
async fn forgetting_everything_from_the_page_rotates_the_secret() {
    let (addrs, shared, _) = server().await;
    shared
        .paired
        .enrol("55555555555555555555555555555555", "iPhone");
    let before = shared.secret().to_hex();

    let mut page = open(addrs.v4, "/devices").await.unwrap();
    answer_challenge(&mut page, &test_secret()).await;
    next_text(&mut page).await.unwrap();
    page.send(Message::Text(
        serde_json::json!({ "t": "forgetAll" }).to_string(),
    ))
    .await
    .unwrap();

    // The last thing it hears is the emptied list; then the socket closes,
    // because the key the page holds no longer opens anything.
    let final_list = read_until_closed(&mut page)
        .await
        .expect("the emptied list, before the socket closed");
    assert_eq!(final_list["devices"], serde_json::json!([]));
    assert_ne!(shared.secret().to_hex(), before);
    assert!(shared.paired.list().is_empty());
}

/// Browser IDs remain separate credentials, but a MAC represents one visible device.
#[tokio::test]
async fn mac_groups_browser_pairings_and_forget_revokes_the_whole_device() {
    let (addrs, shared, _) = server().await;
    let a = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    let b = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
    let c = "cccccccccccccccccccccccccccccccc";
    for id in [a, b, c] {
        shared.paired.enrol(id, "Phone");
    }
    for id in [a, b] {
        shared.paired.remember_mac(id, "02:11:22:33:44:55".into());
    }
    shared.paired.remember_mac(c, "02:11:22:33:44:66".into());
    let old = shared
        .join("192.168.1.10".parse().unwrap(), Some(a.into()))
        .unwrap();
    // A DHCP change and another browser must still replace this physical device.
    let phone = shared
        .join("192.168.1.20".parse().unwrap(), Some(b.into()))
        .unwrap();
    shared.set_label(&phone, "My iPhone".into());
    assert_eq!(shared.device_count(), 1);
    assert!(!shared.devices().iter().any(|d| d.id == old.id));
    let other = shared
        .join("192.168.1.20".parse().unwrap(), Some(c.into()))
        .unwrap();
    shared.set_label(&other, "Other phone".into());
    let mut page = open(addrs.v4, "/devices").await.unwrap();
    answer_challenge(&mut page, &test_secret()).await;
    let listed: serde_json::Value =
        serde_json::from_str(&next_text(&mut page).await.unwrap()).unwrap();
    assert_eq!(listed["connected"], 2);
    let rows = listed["devices"].as_array().unwrap();
    assert_eq!(rows.len(), 2);
    let phone_row = rows
        .iter()
        .find(|r| r["mac"] == "02:11:22:33:44:55")
        .unwrap();
    assert_eq!(phone_row["name"], "My iPhone");
    assert_eq!(phone_row["connected"], true);
    page.send(Message::Text(
        serde_json::json!({ "t": "forget", "id": phone_row["id"] }).to_string(),
    ))
    .await
    .unwrap();
    let after = devices_reaching(&mut page, 1).await;
    assert_eq!(after["devices"].as_array().unwrap().len(), 1);
    assert!(!shared.paired.is_paired(a));
    assert!(!shared.paired.is_paired(b));
    assert!(shared.paired.is_paired(c));
}

#[tokio::test]
async fn unknown_macs_do_not_merge_same_named_pairings() {
    let (addrs, shared, _) = server().await;
    for id in [
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
    ] {
        shared.paired.enrol(id, "iPhone");
    }
    let mut page = open(addrs.v4, "/devices").await.unwrap();
    answer_challenge(&mut page, &test_secret()).await;
    let listed: serde_json::Value =
        serde_json::from_str(&next_text(&mut page).await.unwrap()).unwrap();
    assert_eq!(listed["devices"].as_array().unwrap().len(), 2);
    assert_eq!(listed["connected"], 0);
}
