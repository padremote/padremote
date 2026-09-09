//! The local WebSocket server (plan.md sections 7, 8 and 12).
//!
//! Two things stand between a socket and the cursor, and both happen here
//! before any other module sees the connection:
//!
//! 1. **The `Origin` check** ([`origin`]), decided during the HTTP handshake so
//!    a web page is refused with a 403 and never becomes a WebSocket at all.
//! 2. **The pairing challenge** ([`crate::auth`]), answered before the first
//!    touch frame is read. A connection that cannot answer is closed without
//!    ever being registered as a device.
//!
//! What is still missing is TLS: the link is plain `ws://`, so the challenge
//! proves *who is connecting* but does not hide *what they send* from someone
//! already positioned to read the Wi-Fi. That is milestone 3's other half; see
//! `docs/dev/threat-model.md` for what this does and does not defend against.
//!
//! Several phones may be connected at once. Each gets its **own** recognizer -
//! its own fingers, its own surface size, its own settings - and they take turns
//! driving the one cursor this computer has. Sharing a single recognizer was
//! what made two devices unusable: their finger ids interleaved into one state
//! machine, so a tap on the tablet could end the phone's drag.
//!
//! The module is split along the things a connection can be:
//!
//! - [`shared`]  what every connection shares - the devices, the arbiter that
//!   decides whose gestures reach the OS, the injector
//! - [`session`] one phone: touches in, state and control messages out
//! - [`observe`] one watcher on `/observe`: telemetry out, nothing in
//! - [`devices`]  the connect page on `/devices`: who is paired, and un-pairing
//! - [`settings`] one settings page on `/config`: the config in and out
//! - [`pages`]   an ordinary browser asking for the phone page itself
//! - [`origin`]  which callers may open a socket at all
//! - [`status`]  the two numbers the tray reads, and nothing else
//!
//! The first thing every connection meets is the split between the last two
//! groups: this port answers plain HTTP *and* WebSocket, so that the page and
//! the link the page opens are one process at one address. See [`pages`].

mod devices;
mod neighbor;
mod observe;
pub(crate) mod origin;
pub mod pages;
mod session;
mod settings;
mod shared;
mod status;

use std::net::SocketAddr;
use std::sync::Arc;

use anyhow::Result;
use futures_util::{SinkExt, StreamExt};
use tokio::net::{TcpListener, TcpStream};
use tokio_tungstenite::tungstenite::protocol::WebSocketConfig;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::WebSocketStream;

use tokio_tungstenite::tungstenite::handshake::server::{ErrorResponse, Request, Response};
use tokio_tungstenite::tungstenite::http::{Response as HttpResponse, StatusCode};

use crate::protocol::{ClientMessage, ServerMessage};

pub use shared::{ControlView, Device, DeviceRow, DriveSummary, Shared, MAX_DEVICES};
pub use status::{LinkStatus, LINK_ACTIVE, LINK_CONNECTED, LINK_WAITING};

type Tx = futures_util::stream::SplitSink<tokio_tungstenite::WebSocketStream<TcpStream>, Message>;
type Rx = futures_util::stream::SplitStream<tokio_tungstenite::WebSocketStream<TcpStream>>;

/// The largest message this server will assemble.
///
/// A touch batch is a couple of hundred bytes and the config is a few thousand;
/// nothing legitimate comes close. Without a cap, tungstenite will buffer a
/// fragmented message as large as the sender claims, and one unauthenticated
/// socket - the cap applies before the challenge is answered - could take the
/// process's memory with it.
const MAX_MESSAGE_BYTES: usize = 256 * 1024;

pub async fn serve(addr: SocketAddr, shared: Arc<Shared>) -> Result<()> {
    serve_on(TcpListener::bind(addr).await?, shared).await
}

/// Serve on a listener the caller already owns.
///
/// Split out for the tests, which bind port 0 and need to know which port they
/// got before anything can connect to it.
pub async fn serve_on(listener: TcpListener, shared: Arc<Shared>) -> Result<()> {
    tracing::info!("listening on ws://{}", listener.local_addr()?);
    loop {
        let (stream, peer) = listener.accept().await?;
        let shared = shared.clone();
        tokio::spawn(async move {
            if let Err(e) = handle(stream, peer, shared).await {
                tracing::debug!("session with {peer} ended: {e}");
            }
        });
    }
}

/// How long a connection has to say what it wants before it is dropped.
///
/// Generous by the standards of a LAN, because a phone waking its radio is
/// slow; short enough that a socket which opens and then says nothing does not
/// occupy a task for ever.
const HEAD_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

async fn handle(stream: TcpStream, peer: SocketAddr, shared: Arc<Shared>) -> Result<()> {
    // Nagle would add tens of milliseconds to a stream of tiny frames.
    stream.set_nodelay(true).ok();

    // One port serves both halves of the product, so the first question is
    // which one this is. The head is *peeked*, not read: tungstenite parses the
    // handshake itself and has to find the bytes still in the socket.
    let Some((head, head_len)) = peek_head(&stream).await else {
        return Ok(());
    };
    if !pages::is_websocket_upgrade(&head) {
        pages::serve(stream, &head, head_len, &shared).await?;
        return Ok(());
    }

    // The path decides the role. Routing on it rather than on a first message
    // means an observer never has to claim - and then release - control of the
    // cursor just to watch.
    let mut path = String::new();
    // The error type here is tungstenite's own `Response`, which clippy rightly
    // calls large. Boxing someone else's `Result` is not an option, so the lint
    // is silenced where it applies rather than repo-wide.
    #[allow(clippy::result_large_err)]
    fn inspect<'a>(
        path: &'a mut String,
    ) -> impl FnOnce(&Request, Response) -> Result<Response, ErrorResponse> + 'a {
        move |req, res| {
            // Refused during the handshake, so a hostile page gets an HTTP
            // error and no socket - not a socket it can then send frames on.
            let origin = req
                .headers()
                .get("origin")
                .and_then(|v| v.to_str().ok())
                .map(str::trim);
            if !origin::allowed(origin) {
                tracing::warn!(
                    "refusing a connection from origin {:?}: only the phone page may connect",
                    origin.unwrap_or("<none>")
                );
                return Err(HttpResponse::builder()
                    .status(StatusCode::FORBIDDEN)
                    .body(Some(
                        "PadRemote only accepts connections from its own page.\n".to_string(),
                    ))
                    .expect("a 403 with a string body is always well-formed"));
            }
            *path = req.uri().path().to_string();
            Ok(res)
        }
    }

    let config = WebSocketConfig {
        max_message_size: Some(MAX_MESSAGE_BYTES),
        max_frame_size: Some(MAX_MESSAGE_BYTES),
        ..Default::default()
    };
    let mut ws =
        tokio_tungstenite::accept_hdr_async_with_config(stream, inspect(&mut path), Some(config))
            .await?;

    // Nothing below this line runs for a caller that cannot prove it was paired
    // with this computer - including taking up one of the device slots.
    let who = authenticate(&mut ws, &shared, peer).await?;
    let device_id = match who {
        Outcome::Refused => return Ok(()),
        Outcome::Bare => None,
        Outcome::Device(id) => Some(id),
        Outcome::Enrolled(id) => {
            // First contact. Announce it: a device pairing that nobody was
            // expecting is exactly what a QR read over somebody's shoulder
            // looks like, and silence is what makes that worth stealing.
            shared.paired.enrol(&id, &peer.ip().to_string());
            tracing::info!("a new device paired from {peer}");
            shared.announce_new_device(&peer.ip().to_string());
            Some(id)
        }
    };

    if path == "/devices" {
        return devices::serve(ws, peer, shared).await;
    }
    if path.starts_with("/observe") {
        return observe::observe(ws, peer, shared).await;
    }
    if path.starts_with("/config") {
        return settings::serve(ws, peer, shared).await;
    }

    // Resolve only authenticated trackpads, before publishing their live row.
    if let Some(id) = &device_id {
        if let Some(mac) = neighbor::mac_for(peer.ip()).await {
            shared.paired.remember_mac(id, mac);
        }
    }

    let (mut tx, mut rx) = ws.split();

    // Only a phone moves the cursor, so only a phone changes the link light.
    // An observer on `/observe` used to flip it to "connected" with no phone
    // anywhere near, which made the tray lie.
    let Some(dev) = shared.join(peer.ip(), device_id) else {
        tracing::warn!("refusing {peer}: {MAX_DEVICES} devices are already connected");
        let _ = tx.send(json(&ServerMessage::Error { code: "busy" })).await;
        let _ = tx.send(Message::Close(None)).await;
        return Ok(());
    };
    tracing::info!(
        "device connected: {peer} ({} now connected)",
        shared.device_count()
    );

    let result = session::run(&mut tx, &mut rx, &shared, &dev, peer).await;

    shared.leave(&dev);
    tracing::info!(
        "device disconnected: {peer} ({} still connected)",
        shared.device_count()
    );
    result
}

/// The request head, read without consuming it.
///
/// `peek` rather than `read` is the point: whichever way the connection goes
/// next, the code that handles it wants the bytes from the beginning -
/// tungstenite parses the handshake itself, and it cannot be handed a socket
/// somebody has already read the first line out of.
///
/// Returns the head as text *and* its exact length in bytes. The length matters
/// because the page server has to consume precisely those bytes before it
/// answers, and a lossy UTF-8 conversion is not a byte count.
///
/// `None` means the caller hung up, sent nothing within [`HEAD_TIMEOUT`], or
/// sent a head larger than any browser produces. All three are "not a client we
/// can serve", and none is worth a log line on a shared network.
async fn peek_head(stream: &TcpStream) -> Option<(String, usize)> {
    let mut buf = vec![0u8; pages::MAX_HEAD_BYTES];
    let mut seen = 0usize;
    tokio::time::timeout(HEAD_TIMEOUT, async {
        loop {
            let n = stream.peek(&mut buf).await.ok()?;
            if n == 0 {
                return None; // closed before it said anything
            }
            if let Some(end) = find_head_end(&buf[..n]) {
                return Some((String::from_utf8_lossy(&buf[..end]).into_owned(), end));
            }
            if n >= buf.len() {
                return None; // no blank line in 8 KB: not an HTTP request
            }
            // A peek returns whatever is already buffered, so peeking again
            // straight away would spin at full speed until the rest arrives.
            // Wait for the socket to be readable *again* only helps if it went
            // quiet, which it has not - it still holds the partial head. So
            // yield for a moment instead. In practice this never runs: a
            // browser puts its whole head in the first segment.
            if n == seen {
                tokio::time::sleep(std::time::Duration::from_millis(2)).await;
            }
            seen = n;
        }
    })
    .await
    .ok()
    .flatten()
}

/// Where the blank line that ends an HTTP head is, if it has arrived.
fn find_head_end(buf: &[u8]) -> Option<usize> {
    buf.windows(4).position(|w| w == b"\r\n\r\n").map(|i| i + 4)
}

/// Challenge the caller, and hang up unless it answers correctly.
///
/// Returns the id of the device that answered - `None` for a native client that
/// has only the QR secret and never enrols - or `Ok(None)` for a refusal that
/// has already been closed, where the caller's only job is to stop. A refusal
/// is not an error: being connected to by something that does not know the
/// secret is expected on a shared network, not exceptional, and burying it in
/// the error path would make the log of an ordinary machine noisy.
///
/// **Which key the answer is checked against decides whether revoking a phone
/// means anything**, so it is worth stating plainly:
///
/// - a device id **on the paired list** is checked against that device's own
///   key. Forgetting it takes the id off the list, and the key the phone still
///   holds stops opening anything.
/// - a device id **not on the list** is checked against the enrolment secret -
///   the QR itself - and joins the list if it passes. A forgotten phone cannot
///   come back this way: it has its device key and not the QR secret, having
///   thrown that away the moment it enrolled.
async fn authenticate(
    ws: &mut WebSocketStream<TcpStream>,
    shared: &Arc<Shared>,
    peer: SocketAddr,
) -> Result<Outcome> {
    let nonce = crate::auth::nonce();
    ws.send(json(&ServerMessage::Challenge {
        nonce: nonce.clone(),
    }))
    .await?;

    // The *first* message has to be the answer. Accepting touches from a socket
    // that has not answered yet - even briefly, even "we will check in a moment"
    // - is the whole hole this closes.
    let outcome = tokio::time::timeout(crate::auth::AUTH_TIMEOUT, async {
        while let Some(msg) = ws.next().await {
            match msg {
                Ok(Message::Text(text)) => {
                    return match serde_json::from_str::<ClientMessage>(&text) {
                        Ok(ClientMessage::Auth { hmac, device }) => {
                            check(shared, &nonce, &hmac, device)
                        }
                        // Any other message before the answer is a client that
                        // is not following the protocol, which is what an
                        // attacker's first frame looks like.
                        _ => Outcome::Refused,
                    };
                }
                // A binary frame here is a touch batch from something that has
                // not authenticated. Refuse it rather than skipping past it.
                Ok(Message::Binary(_)) | Ok(Message::Close(_)) | Err(_) => return Outcome::Refused,
                // Keepalives are answered by tungstenite and mean nothing here.
                Ok(_) => continue,
            }
        }
        Outcome::Refused
    })
    .await
    .unwrap_or(Outcome::Refused);

    if !matches!(outcome, Outcome::Refused) {
        return Ok(outcome);
    }
    tracing::warn!("refusing {peer}: it did not answer the pairing challenge");
    let _ = ws
        .send(json(&ServerMessage::Error { code: "badAuth" }))
        .await;
    let _ = ws.send(Message::Close(None)).await;
    let _ = ws.flush().await;
    Ok(Outcome::Refused)
}

/// What the challenge decided.
enum Outcome {
    Refused,
    /// A paired device, signing with its own key.
    Device(String),
    /// A device that has just enrolled with the QR secret, and is new here.
    Enrolled(String),
    /// The QR secret alone, with no device id: the replay tool and the tests.
    /// Authenticated, but never added to the paired list.
    Bare,
}

fn check(shared: &Arc<Shared>, nonce: &str, hmac: &str, device: Option<String>) -> Outcome {
    let secret = shared.secret();
    let Some(id) = device else {
        return if secret.verify(nonce.as_bytes(), hmac) {
            Outcome::Bare
        } else {
            Outcome::Refused
        };
    };
    // The id is chosen by the other end and reaches a log line, a JSON key and
    // the menu bar, so it is checked for shape before any of that.
    if !crate::auth::valid_device_id(&id) {
        return Outcome::Refused;
    }
    if shared.paired.is_paired(&id) {
        if secret.device_key(&id).verify(nonce.as_bytes(), hmac) {
            return Outcome::Device(id);
        }
        return Outcome::Refused;
    }
    // Not paired: this has to be a fresh scan, so only the QR secret will do.
    if secret.verify(nonce.as_bytes(), hmac) {
        return Outcome::Enrolled(id);
    }
    Outcome::Refused
}

fn json(msg: &ServerMessage) -> Message {
    Message::Text(serde_json::to_string(msg).unwrap_or_default())
}
