//! One port, two protocols.
//!
//! The phone page used to come from `npm run dev` on a second port. That made
//! the product two processes, and the failure it produced was the worst kind:
//! the menu-bar app survived a reboot, the dev server did not, and every QR
//! went on pointing at a port with nothing behind it. The phone's message -
//! "this site can't be reached" - named neither half.
//!
//! So the app now serves the page itself, on the port the phone already has to
//! reach. The thing that can quietly break is the *dispatch*: a change that
//! sends browsers to the WebSocket code, or handshakes to the static server,
//! breaks one half while the other keeps working and keeps its test green. Both
//! halves are therefore checked on the same listener, in the same test run.
//!
//! Most of this holds whether or not `web/dist` was built - CI builds the Rust
//! without Node, and that has to stay possible. The assertions about actual page
//! content say so where they are skipped.

use std::sync::Arc;
use std::time::Duration;

use futures_util::StreamExt;
use padremote::auth::{Devices, Secret};
use padremote::gesture::Config;
use padremote::input::{Blocked, NullInjector};
use padremote::net::{pages, LinkStatus, Shared};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio_tungstenite::tungstenite::Message;

/// A server on a port the OS picked, with nothing that can touch the cursor.
async fn server() -> std::net::SocketAddr {
    let shared = Arc::new(Shared::new(
        Config::default(),
        Box::new(NullInjector::new(Blocked::DryRun)),
        "test-host".into(),
        LinkStatus::new(),
        Secret::from_hex("0f1e2d3c4b5a69788796a5b4c3d2e1f0").expect("a valid test secret"),
        Devices::default(),
    ));
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let _ = padremote::net::serve_on(listener, shared).await;
    });
    addr
}

/// One HTTP request, spoken by hand, as (status line + headers, body).
///
/// By hand because the whole point of the check is that an ordinary browser
/// request works; a WebSocket client library would prove nothing about it, and
/// the app has no HTTP client to reach for (`tools/check-no-phone-home.sh` sees
/// to that).
async fn http(addr: std::net::SocketAddr, request: &str) -> (String, String) {
    let mut stream = TcpStream::connect(addr).await.expect("connect");
    stream.write_all(request.as_bytes()).await.expect("write");
    let mut raw = Vec::new();
    tokio::time::timeout(Duration::from_secs(5), stream.read_to_end(&mut raw))
        .await
        .expect("the server answered and closed")
        .expect("read");
    let text = String::from_utf8_lossy(&raw).into_owned();
    match text.split_once("\r\n\r\n") {
        Some((head, body)) => (head.to_string(), body.to_string()),
        None => (text, String::new()),
    }
}

async fn get(addr: std::net::SocketAddr, path: &str) -> (String, String) {
    http(
        addr,
        &format!("GET {path} HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\n\r\n"),
    )
    .await
}

/// The regression that matters most: serving pages must not have cost us the
/// socket. A phone that loads the page and then cannot open the link is exactly
/// as broken as one that could not load the page.
#[tokio::test]
async fn the_socket_still_works_on_the_port_that_serves_the_page() {
    let addr = server().await;

    let (head, _) = get(addr, "/").await;
    assert!(
        head.starts_with("HTTP/1.1 "),
        "a browser must get an HTTP response, got: {head:?}"
    );

    let (mut ws, _) = tokio_tungstenite::connect_async(format!("ws://{addr}/"))
        .await
        .expect("the WebSocket handshake still works on the same port");
    let first = tokio::time::timeout(Duration::from_secs(2), ws.next())
        .await
        .expect("the challenge arrives")
        .expect("a message")
        .expect("not an error");
    let Message::Text(text) = first else {
        panic!("the first message should be the challenge, got {first:?}");
    };
    assert!(
        text.contains("\"challenge\""),
        "the socket is still the authenticated one, got {text}"
    );
}

/// Anything that is not a page and not a handshake gets a real HTTP answer
/// rather than a dropped connection, so that "is it running?" has an answer
/// from a browser, a `curl`, or a phone that typed the address wrong.
#[tokio::test]
async fn an_unknown_path_is_a_404_that_says_what_to_do() {
    let addr = server().await;
    let (head, body) = get(addr, "/definitely-not-a-page").await;
    assert!(head.starts_with("HTTP/1.1 404 "), "got: {head:?}");
    assert!(
        body.contains("PadRemote"),
        "the 404 should orient whoever reached it, got: {body:?}"
    );
}

/// A hand-written static server's classic hole. There is no filesystem behind
/// this one - responses come from a table fixed at compile time - so the test
/// is here to keep it that way rather than to prove a filter works.
#[tokio::test]
async fn nothing_outside_the_page_can_be_asked_for() {
    let addr = server().await;
    for target in [
        "/../../../../etc/passwd",
        "/..%2f..%2f..%2fetc%2fpasswd",
        "/assets/../../../../etc/passwd",
        "/./../Cargo.toml",
    ] {
        let (head, body) = get(addr, target).await;
        assert!(
            head.starts_with("HTTP/1.1 404 "),
            "{target} should be a 404, got: {head:?}"
        );
        assert!(
            !body.contains("root:") && !body.contains("[package]"),
            "{target} served something real"
        );
    }
}

/// Only reading. A `POST` to this port is either a mistake or a probe, and
/// either way it must not be quietly accepted.
#[tokio::test]
async fn the_page_server_only_reads() {
    let addr = server().await;
    let (head, _) = http(
        addr,
        &format!(
            "POST / HTTP/1.1\r\nHost: {addr}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
        ),
    )
    .await;
    assert!(head.starts_with("HTTP/1.1 405 "), "got: {head:?}");
}

/// The page itself, when this build has one.
///
/// Skipped rather than failed without `web/dist`: `cargo test` has to keep
/// working for someone who has never run `npm`, and CI's Rust job does exactly
/// that.
#[tokio::test]
async fn the_trackpad_page_is_served_from_the_app() {
    if !pages::is_bundled() {
        eprintln!("skipped: this build has no web/dist in it (run `npm run build` in web/)");
        return;
    }
    let addr = server().await;

    let (head, body) = get(addr, "/").await;
    assert!(head.starts_with("HTTP/1.1 200 "), "got: {head:?}");
    assert!(head.contains("text/html"), "got: {head:?}");
    assert!(
        body.contains("<title>") && body.contains("Connect a device"),
        "the local root should be the connect page"
    );

    // The pages the *computer* opens, which the menu bar links to by name.
    for page in ["/config.html", "/debug.html"] {
        let (head, _) = get(addr, page).await;
        assert!(head.starts_with("HTTP/1.1 200 "), "{page} got: {head:?}");
    }

    // A query and a fragment are the phone's business, not the server's: the
    // pairing links carry both, and an exact-match lookup that forgot to strip
    // them would 404 every link the app itself hands out.
    let (head, _) = get(addr, "/config.html?h=localhost:8787").await;
    assert!(head.starts_with("HTTP/1.1 200 "), "got: {head:?}");
}

/// An upgraded page must not be met by a cached old one. The HTML names
/// content-hashed assets, so the hashes may be cached forever and the HTML
/// never - getting that backwards leaves a phone on yesterday's build with no
/// way to tell.
#[tokio::test]
async fn the_html_is_never_cached_and_the_hashed_assets_always_are() {
    if !pages::is_bundled() {
        eprintln!("skipped: this build has no web/dist in it");
        return;
    }
    let addr = server().await;

    let (head, body) = http(
        addr,
        &format!(
            "GET /index.html HTTP/1.1\r\nHost: 192.168.1.5:{}\r\nConnection: close\r\n\r\n",
            addr.port()
        ),
    )
    .await;
    assert!(
        head.contains("Cache-Control: no-store"),
        "the HTML must not be cached, got: {head:?}"
    );

    let asset = body
        .split('"')
        .find(|s| s.starts_with("/assets/") && s.ends_with(".js"))
        .expect("the page loads at least one hashed module")
        .to_string();
    let (head, _) = get(addr, &asset).await;
    assert!(head.starts_with("HTTP/1.1 200 "), "{asset} got: {head:?}");
    assert!(
        head.contains("immutable"),
        "{asset} is content-hashed and should be cacheable, got: {head:?}"
    );
    assert!(
        head.contains("text/javascript"),
        "a module served as the wrong type does not run, got: {head:?}"
    );
}

#[tokio::test]
async fn the_host_gets_pairing_but_untrusted_hosts_get_no_secret() {
    let addr = server().await;
    let (head, body) = get(addr, "/").await;
    assert!(head.starts_with("HTTP/1.1 200 "));
    assert!(body.contains("Connect a device"));
    assert!(body.contains("0f1e2d3c4b5a69788796a5b4c3d2e1f0"));
    for host in ["evil.example", "localhost.evil.example", "192.168.1.5"] {
        let (_, body) = http(
            addr,
            &format!("GET / HTTP/1.1\r\nHost: {host}:{}\r\n\r\n", addr.port()),
        )
        .await;
        assert!(!body.contains("0f1e2d3c4b5a69788796a5b4c3d2e1f0"));
        if pages::is_bundled() {
            assert!(body.contains("surface"));
        }
    }
}
