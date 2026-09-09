//! What happens to a connection that should not be allowed to move the cursor.
//!
//! Every test here is an attack the server used to lose. Before the pairing
//! challenge and the `Origin` check existed, all of them worked: anything that
//! could open a TCP socket to the port could click, drag and scroll, and that
//! included a page on any website the user happened to have open, because a
//! WebSocket is not subject to the same-origin policy and needs no permission.
//!
//! So these are written from the attacker's side. Each one does the least an
//! attacker would have to do, and asserts that **nothing reached the OS** -
//! the injector records every event, and an empty recording is the only
//! evidence that counts. Asserting on the socket alone would pass against a
//! server that refuses politely and injects anyway.

use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use padremote::auth::Secret;
use padremote::gesture::{Button, Config, ScrollPhase, Shortcut, TouchSample};
use padremote::input::Injector;
use padremote::net::{LinkStatus, Shared};
use padremote::protocol::encode_frame;
use tokio::net::TcpListener;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::Message;

type Socket =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

/// Records what actually reached the OS. The only assertion that matters.
#[derive(Clone, Default)]
struct Recorder(Arc<Mutex<Vec<String>>>);

impl Recorder {
    fn events(&self) -> Vec<String> {
        self.0.lock().unwrap().clone()
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
    fn scroll_by(&mut self, dx: f64, dy: f64, _phase: ScrollPhase) {
        self.push(format!("scroll {dx:.0},{dy:.0}"));
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
}

/// Answer a challenge the way a phone does: with a device id, and with the key
/// that id implies.
async fn auth_as(ws: &mut Socket, device_id: &str, key: &Secret) {
    let nonce = challenge(ws).await;
    ws.send(Message::Text(
        serde_json::json!({
            "t": "auth",
            "device": device_id,
            "hmac": key.sign(nonce.as_bytes()),
        })
        .to_string(),
    ))
    .await
    .unwrap();
}

fn test_secret() -> Secret {
    Secret::from_hex("0f1e2d3c4b5a69788796a5b4c3d2e1f0").expect("a valid test secret")
}

async fn server() -> (SocketAddr, Arc<Shared>, Recorder) {
    let injected = Recorder::default();
    let shared = Arc::new(Shared::new(
        Config::default(),
        Box::new(injected.clone()),
        "test-host".into(),
        LinkStatus::new(),
        test_secret(),
        padremote::auth::Devices::default(),
    ));
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server_shared = shared.clone();
    tokio::spawn(async move {
        let _ = padremote::net::serve_on(listener, server_shared).await;
    });
    (addr, shared, injected)
}

async fn open(addr: SocketAddr, path: &str) -> Result<Socket, String> {
    tokio_tungstenite::connect_async(format!("ws://{addr}{path}"))
        .await
        .map(|(ws, _)| ws)
        .map_err(|e| e.to_string())
}

/// Open a socket claiming to be a page served from `origin`.
async fn open_from(addr: SocketAddr, origin: &str) -> Result<Socket, String> {
    let mut req = format!("ws://{addr}/")
        .into_client_request()
        .expect("a valid request");
    req.headers_mut()
        .insert("Origin", origin.parse().expect("a header value"));
    tokio_tungstenite::connect_async(req)
        .await
        .map(|(ws, _)| ws)
        .map_err(|e| e.to_string())
}

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

/// The nonce from the challenge the desktop opens with.
async fn challenge(ws: &mut Socket) -> String {
    let text = next_text(ws).await.expect("a challenge");
    let msg: serde_json::Value = serde_json::from_str(&text).expect("challenge json");
    assert_eq!(msg["t"], "challenge");
    msg["nonce"].as_str().expect("a nonce").to_string()
}

/// A full press-move-release, which is a click and a drag if it is obeyed.
async fn try_to_drive(ws: &mut Socket) {
    let frames = [
        (0u32, 0u8, 0.5f32, 0.5f32),
        (16, 1, 0.7, 0.5),
        (32, 1, 0.9, 0.5),
        (48, 2, 0.9, 0.5),
    ];
    for (t_ms, phase, x, y) in frames {
        let _ = ws
            .send(Message::Binary(encode_frame(&[TouchSample {
                t_ms,
                pointer_id: 1,
                phase,
                x,
                y,
            }])))
            .await;
    }
    // Long enough for anything that was going to be injected to have been.
    tokio::time::sleep(Duration::from_millis(200)).await;
}

// ---------------------------------------------------------------- the secret

#[tokio::test]
async fn the_desktop_challenges_before_it_reads_anything() {
    let (addr, _shared, injected) = server().await;
    let mut ws = open(addr, "/").await.expect("the socket itself opens");

    // The first thing on the wire is the challenge, not a greeting.
    let nonce = challenge(&mut ws).await;
    assert_eq!(nonce.len(), 32, "a 128-bit nonce, hex");

    // And a fresh one each time: an answer captured off an unencrypted link
    // must be worthless on the next connection.
    let mut second = open(addr, "/").await.expect("connect");
    assert_ne!(nonce, challenge(&mut second).await);

    assert!(injected.events().is_empty());
}

#[tokio::test]
async fn touches_before_the_answer_are_refused() {
    let (addr, shared, injected) = server().await;
    let mut ws = open(addr, "/").await.expect("connect");
    challenge(&mut ws).await;

    // The attacker's shortest path: skip the handshake, send touches.
    try_to_drive(&mut ws).await;

    assert!(
        injected.events().is_empty(),
        "an unauthenticated socket moved the cursor: {:?}",
        injected.events()
    );
    assert_eq!(
        shared.device_count(),
        0,
        "it should not even hold a device slot"
    );
}

#[tokio::test]
async fn a_wrong_answer_is_refused_and_told_so() {
    let (addr, _shared, injected) = server().await;
    let mut ws = open(addr, "/").await.expect("connect");
    let nonce = challenge(&mut ws).await;

    // Someone who guessed. The signature is well-formed and simply wrong,
    // which is the only case a constant-time comparison exists for.
    let wrong = Secret::from_hex("ffffffffffffffffffffffffffffffff").unwrap();
    ws.send(Message::Text(
        serde_json::json!({ "t": "auth", "hmac": wrong.sign(nonce.as_bytes()) }).to_string(),
    ))
    .await
    .unwrap();

    let reply = next_text(&mut ws).await.expect("a refusal");
    assert!(
        reply.contains("badAuth"),
        "the phone has to be told why, or it retries forever: {reply}"
    );

    try_to_drive(&mut ws).await;
    assert!(injected.events().is_empty());
}

#[tokio::test]
async fn an_answer_to_a_different_nonce_is_refused() {
    let (addr, _shared, injected) = server().await;
    let mut ws = open(addr, "/").await.expect("connect");
    challenge(&mut ws).await;

    // A replay: the right secret, the right *shape*, but signed over a nonce
    // from some other connection. This is what an eavesdropper on the plain
    // `ws://` link actually has.
    ws.send(Message::Text(
        serde_json::json!({
            "t": "auth",
            "hmac": test_secret().sign(b"a nonce from another connection"),
        })
        .to_string(),
    ))
    .await
    .unwrap();

    try_to_drive(&mut ws).await;
    assert!(
        injected.events().is_empty(),
        "a replayed answer drove the cursor: {:?}",
        injected.events()
    );
}

#[tokio::test]
async fn junk_in_place_of_an_answer_is_refused() {
    let (addr, _shared, injected) = server().await;
    for answer in ["", "not hex", "0", &"ab".repeat(64)] {
        let mut ws = open(addr, "/").await.expect("connect");
        challenge(&mut ws).await;
        ws.send(Message::Text(
            serde_json::json!({ "t": "auth", "hmac": answer }).to_string(),
        ))
        .await
        .unwrap();
        try_to_drive(&mut ws).await;
        assert!(
            injected.events().is_empty(),
            "{answer:?} was accepted as an answer"
        );
    }
}

#[tokio::test]
async fn a_welcome_cannot_stand_in_for_an_answer() {
    let (addr, _shared, injected) = server().await;
    let mut ws = open(addr, "/").await.expect("connect");
    challenge(&mut ws).await;

    // Exactly what the old client sent first. It has to be refused, or a build
    // from before the challenge is an unauthenticated client that still works.
    ws.send(Message::Text(
        serde_json::json!({
            "t": "welcome",
            "v": 1,
            "surface": { "wpx": 390.0, "hpx": 669.0, "dpr": 3.0 },
        })
        .to_string(),
    ))
    .await
    .unwrap();

    try_to_drive(&mut ws).await;
    assert!(injected.events().is_empty());
}

#[tokio::test]
async fn the_right_answer_gets_a_working_trackpad() {
    let (addr, shared, injected) = server().await;
    let mut ws = open(addr, "/").await.expect("connect");
    let nonce = challenge(&mut ws).await;
    ws.send(Message::Text(
        serde_json::json!({ "t": "auth", "hmac": test_secret().sign(nonce.as_bytes()) })
            .to_string(),
    ))
    .await
    .unwrap();

    try_to_drive(&mut ws).await;
    assert!(
        !injected.events().is_empty(),
        "a paired phone must still be able to drive the cursor"
    );
    assert_eq!(shared.device_count(), 1);
}

/// The other two paths are not lesser connections: `/observe` mirrors every
/// touch, and `/config` rewrites how the whole app behaves.
#[tokio::test]
async fn the_observer_and_settings_paths_are_challenged_too() {
    let (addr, _shared, _injected) = server().await;
    for path in ["/observe", "/config"] {
        let mut ws = open(addr, path).await.expect("connect");
        let text = next_text(&mut ws).await.expect("a challenge");
        assert!(
            text.contains("challenge"),
            "{path} said {text} before challenging"
        );

        // Without an answer, nothing follows - no telemetry, no config.
        ws.send(Message::Text(
            serde_json::json!({ "t": "auth", "hmac": "00" }).to_string(),
        ))
        .await
        .unwrap();
        let after = next_text(&mut ws).await.unwrap_or_default();
        assert!(
            after.contains("badAuth") || after.is_empty(),
            "{path} leaked {after} to a caller that failed the challenge"
        );
    }
}

// ---------------------------------------------------------------- the origin

/// The one that used to be wide open: a page on the internet, in a tab the user
/// has forgotten about, opening a socket to `127.0.0.1` and driving the cursor.
/// No prompt, no CORS, no way for the user to know.
#[tokio::test]
async fn a_web_page_cannot_open_a_socket_at_all() {
    let (addr, _shared, injected) = server().await;
    for origin in [
        "https://example.com",
        "http://evil.test:5173",
        // DNS rebinding: the name points at this machine, the origin does not.
        "http://rebind.evil.test",
        "null",
    ] {
        let err = open_from(addr, origin)
            .await
            .err()
            .unwrap_or_else(|| panic!("{origin} was allowed to open a socket"));
        assert!(
            err.contains("403") || err.to_lowercase().contains("forbidden"),
            "{origin} was refused, but not with a 403: {err}"
        );
    }
    assert!(injected.events().is_empty());
}

#[tokio::test]
async fn the_phone_page_is_still_allowed() {
    let (addr, _shared, injected) = server().await;
    // Vite on the LAN, and the app serving the page itself.
    for origin in ["http://127.0.0.1:5173", "http://localhost:5173"] {
        let mut ws = open_from(addr, origin)
            .await
            .unwrap_or_else(|e| panic!("{origin} is the phone page and was refused: {e}"));
        let nonce = challenge(&mut ws).await;
        ws.send(Message::Text(
            serde_json::json!({ "t": "auth", "hmac": test_secret().sign(nonce.as_bytes()) })
                .to_string(),
        ))
        .await
        .unwrap();
        try_to_drive(&mut ws).await;
        assert!(
            !injected.events().is_empty(),
            "{origin} could not drive the cursor"
        );
    }
}

// -------------------------------------------------------------- unpairing

/// Revoking has to reach the phone that is holding the cursor *now*, not only
/// the next one to connect.
#[tokio::test]
async fn unpairing_drops_the_devices_that_are_already_connected() {
    let (addr, shared, injected) = server().await;
    let mut ws = open(addr, "/").await.expect("connect");
    let nonce = challenge(&mut ws).await;
    ws.send(Message::Text(
        serde_json::json!({ "t": "auth", "hmac": test_secret().sign(nonce.as_bytes()) })
            .to_string(),
    ))
    .await
    .unwrap();
    try_to_drive(&mut ws).await;
    assert_eq!(shared.device_count(), 1);

    let fresh = shared.unpair_all();
    assert!(fresh != test_secret(), "unpairing must change the secret");

    // The live session is told and closed.
    let mut said = Vec::new();
    while let Ok(Some(Ok(msg))) = tokio::time::timeout(Duration::from_millis(500), ws.next()).await
    {
        if let Message::Text(t) = msg {
            said.push(t);
        }
    }
    assert!(
        said.iter().any(|t| t.contains("unpaired")),
        "the phone was dropped without being told why: {said:?}"
    );
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(shared.device_count(), 0, "the session is gone");

    // And the old secret no longer opens a new one.
    let before = injected.events().len();
    let mut stale = open(addr, "/").await.expect("connect");
    let nonce = challenge(&mut stale).await;
    stale
        .send(Message::Text(
            serde_json::json!({ "t": "auth", "hmac": test_secret().sign(nonce.as_bytes()) })
                .to_string(),
        ))
        .await
        .unwrap();
    try_to_drive(&mut stale).await;
    assert_eq!(
        injected.events().len(),
        before,
        "a revoked phone could still drive the cursor"
    );

    // While the new one does.
    let mut paired = open(addr, "/").await.expect("connect");
    let nonce = challenge(&mut paired).await;
    paired
        .send(Message::Text(
            serde_json::json!({ "t": "auth", "hmac": fresh.sign(nonce.as_bytes()) }).to_string(),
        ))
        .await
        .unwrap();
    try_to_drive(&mut paired).await;
    assert!(injected.events().len() > before, "re-pairing did not work");
}

// ------------------------------------------------------- per-device pairing

/// The point of giving each device its own key: one of them can be revoked.
#[tokio::test]
async fn forgetting_one_device_leaves_the_others_working() {
    let (addr, shared, injected) = server().await;
    let secret = test_secret();

    // Two devices enrol, each signing with the QR secret the first time.
    let phone = "11111111111111111111111111111111";
    let tablet = "22222222222222222222222222222222";
    for id in [phone, tablet] {
        let mut ws = open(addr, "/").await.expect("connect");
        auth_as(&mut ws, id, &secret).await;
        try_to_drive(&mut ws).await;
    }
    assert!(shared.paired.is_paired(phone) && shared.paired.is_paired(tablet));

    // From here they use their own keys, which is what makes them separable.
    let mut ws = open(addr, "/").await.expect("connect");
    auth_as(&mut ws, phone, &secret.device_key(phone)).await;
    let before = injected.events().len();
    try_to_drive(&mut ws).await;
    assert!(
        injected.events().len() > before,
        "a paired device must drive with its own key"
    );

    assert!(shared.forget_device(phone));
    assert!(!shared.paired.is_paired(phone));
    assert!(shared.paired.is_paired(tablet), "and only that one");

    // The revoked phone still holds its key. It must get nowhere with it.
    let before = injected.events().len();
    let mut revoked = open(addr, "/").await.expect("connect");
    auth_as(&mut revoked, phone, &secret.device_key(phone)).await;
    try_to_drive(&mut revoked).await;
    assert_eq!(
        injected.events().len(),
        before,
        "a forgotten device drove the cursor with the key it kept"
    );

    // The tablet is untouched - nobody else has to re-scan.
    let mut still_paired = open(addr, "/").await.expect("connect");
    auth_as(&mut still_paired, tablet, &secret.device_key(tablet)).await;
    try_to_drive(&mut still_paired).await;
    assert!(
        injected.events().len() > before,
        "forgetting one device locked out another"
    );
}

/// The hole this design exists to close. A revoked phone keeps its device key,
/// so enrolment must not accept one - or "forget this device" would be undone
/// by the phone simply reconnecting.
#[tokio::test]
async fn a_forgotten_device_cannot_re_enrol_with_the_key_it_kept() {
    let (addr, shared, injected) = server().await;
    let secret = test_secret();
    let phone = "33333333333333333333333333333333";

    let mut ws = open(addr, "/").await.expect("connect");
    auth_as(&mut ws, phone, &secret).await;
    try_to_drive(&mut ws).await;
    assert!(shared.paired.is_paired(phone));
    assert!(shared.forget_device(phone));

    let before = injected.events().len();
    // Its device key is no longer checked against anything, because the id is
    // not on the list - so the enrolment path is what it hits, and that wants
    // the QR secret it threw away.
    for key in [secret.device_key(phone), Secret::generate()] {
        let mut ws = open(addr, "/").await.expect("connect");
        auth_as(&mut ws, phone, &key).await;
        try_to_drive(&mut ws).await;
    }
    assert_eq!(
        injected.events().len(),
        before,
        "a forgotten device enrolled itself again"
    );

    // Being shown the QR again is what brings it back, and that is the point.
    let mut rescanned = open(addr, "/").await.expect("connect");
    auth_as(&mut rescanned, phone, &secret).await;
    try_to_drive(&mut rescanned).await;
    assert!(
        injected.events().len() > before,
        "scanning the QR again must re-pair the device"
    );
}

/// A device id is chosen by the other end and reaches a log line, a JSON key
/// and the menu bar.
#[tokio::test]
async fn a_malformed_device_id_is_refused() {
    let (addr, shared, injected) = server().await;
    for id in [
        "",
        "not-hex",
        "../../etc/passwd",
        "11111111111111111111111111111111111111",
        "1111111111111111111111111111111",
    ] {
        let mut ws = open(addr, "/").await.expect("connect");
        auth_as(&mut ws, id, &test_secret()).await;
        try_to_drive(&mut ws).await;
        assert!(
            injected.events().is_empty(),
            "{id:?} was accepted as a device id"
        );
        assert!(
            !shared.paired.is_paired(id),
            "{id:?} was written to the list"
        );
    }
}

/// Unpairing everything has to do both halves. A new secret with the old list
/// still in place would leave every phone signing with a key derived from a
/// secret that no longer exists; an emptied list with the old secret would let
/// them all enrol straight back in.
#[tokio::test]
async fn unpairing_everything_clears_the_paired_list_too() {
    let (addr, shared, _injected) = server().await;
    let phone = "44444444444444444444444444444444";
    let mut ws = open(addr, "/").await.expect("connect");
    auth_as(&mut ws, phone, &test_secret()).await;
    try_to_drive(&mut ws).await;
    assert!(shared.paired.is_paired(phone));

    shared.unpair_all();
    assert!(
        !shared.paired.is_paired(phone),
        "unpairing left the device list behind"
    );
}

#[tokio::test]
async fn qr_enrolment_publishes_the_named_device_without_a_second_row_on_reconnect() {
    let (addr, shared, _) = server().await;
    let secret = test_secret();
    let id = "abababababababababababababababab";
    let mut page = open(addr, "/devices").await.unwrap();
    let nonce = challenge(&mut page).await;
    page.send(Message::Text(
        serde_json::json!({"t":"auth", "hmac":secret.sign(nonce.as_bytes())}).to_string(),
    ))
    .await
    .unwrap();
    next_text(&mut page).await.unwrap();
    let mut first = open(addr, "/").await.unwrap();
    auth_as(&mut first, id, &secret).await;
    first
        .send(Message::Text(
            serde_json::json!({"t":"welcome", "v":1,
        "name":"Kitchen iPad", "surface":{"wpx":390,"hpx":669,"dpr":3}})
            .to_string(),
        ))
        .await
        .unwrap();
    // Wait for the name update, not merely the connection-count update.
    let named = tokio::time::timeout(std::time::Duration::from_secs(2), async {
        loop {
            let msg: serde_json::Value =
                serde_json::from_str(&next_text(&mut page).await.unwrap()).unwrap();
            if msg["devices"][0]["name"] == "Kitchen iPad" {
                break msg;
            }
        }
    })
    .await
    .unwrap();
    assert_eq!(named["connected"], 1);
    assert_eq!(named["devices"].as_array().unwrap().len(), 1);
    let mut second = open(addr, "/").await.unwrap();
    auth_as(&mut second, id, &secret.device_key(id)).await;
    // Receiving the initial control state proves this authenticated session joined.
    next_text(&mut second).await.unwrap();
    assert_eq!(shared.device_count(), 1);
    assert_eq!(shared.paired.list().len(), 1);
}
