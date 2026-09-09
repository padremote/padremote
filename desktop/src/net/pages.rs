//! Serving the phone page itself, on the same port as the link.
//!
//! PadRemote used to speak only WebSocket, and the page came from `npm run dev`
//! on port 5173. That made the product two processes: the menu-bar app survived
//! a reboot and the dev server did not, so the app sat there looking healthy
//! while every QR pointed at a dead port. The phone said "this site can't be
//! reached", which names neither half of the problem.
//!
//! Now the built page is compiled into the binary (see `build.rs`) and served
//! from here, on the port the phone already has to reach. One process, one
//! port, one address in the QR, one firewall prompt.
//!
//! Remote requests receive the public static trackpad bundle. Requests from
//! this computer to its own literal address (or localhost) receive the host
//! pairing screen, including the QR secret. Both the TCP peer and Host header
//! are checked before rendering that private screen.
//!
//! Static assets come from a compile-time table; the host screen is rendered
//! from current pairing state. Neither route reads request-selected files.

use std::io;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

include!(concat!(env!("OUT_DIR"), "/page_assets.rs"));

/// The largest request head this server will read before giving up.
///
/// A browser's `GET` with every header it has is a couple of kilobytes. The cap
/// is what stops a socket that never sends `\r\n\r\n` from growing a buffer for
/// as long as it likes.
pub(crate) const MAX_HEAD_BYTES: usize = 8 * 1024;

/// Was the page built into this binary?
///
/// False for a plain `cargo build` with no `web/dist` next to it, which is a
/// perfectly reasonable thing for someone working on the gesture engine to do.
/// Startup says so rather than letting the phone find out.
pub fn is_bundled() -> bool {
    !ASSETS.is_empty()
}

/// How many files the page is made of, for the startup log.
pub fn bundled_count() -> usize {
    ASSETS.len()
}

/// Answer one HTTP request and close.
///
/// `head` is the request head the caller peeked, and `head_len` its exact size
/// in bytes - those bytes are still sitting unread in the socket, and the first
/// thing here is to take them out. That is not tidiness. Closing a socket that
/// still holds unread data makes the kernel send an RST instead of a FIN, and
/// an RST throws away the response we just wrote: the browser reports a
/// connection error and never sees the page. It is intermittent, because it is
/// a race with how fast the client reads, which is the worst way to find out.
///
/// There is no keep-alive: the page is a handful of files fetched once, and
/// every browser opens parallel connections anyway.
pub async fn serve(
    mut stream: TcpStream,
    head: &str,
    head_len: usize,
    shared: &super::Shared,
) -> io::Result<()> {
    drain(&mut stream, head, head_len).await?;

    let (method, target) = match request_line(head) {
        Some(parts) => parts,
        // Not something we can answer, and not worth a reply that would only
        // teach a scanner what is here.
        None => return Ok(()),
    };

    if method != "GET" && method != "HEAD" {
        return respond(
            &mut stream,
            405,
            "Method Not Allowed",
            "text/plain; charset=utf-8",
            NO_CACHE,
            b"PadRemote serves the trackpad page. Only GET is supported.\n",
            method != "HEAD",
        )
        .await;
    }

    let path = normalize(target);
    if path == "/index.html" && is_host_request(head, stream.peer_addr()?, stream.local_addr()?) {
        let port = stream.local_addr()?.port();
        let secret = shared.secret();
        let pairing = match shared.lan_ip() {
            Some(ip) => {
                crate::pairing::Pairing::new(&ip, port, port, shared.host_name.clone(), &secret)
            }
            None => {
                crate::pairing::Pairing::local_only(port, port, shared.host_name.clone(), &secret)
            }
        };
        let html = pairing.render_page().map_err(io::Error::other)?;
        return respond(
            &mut stream,
            200,
            "OK",
            "text/html; charset=utf-8",
            NO_CACHE,
            html.as_bytes(),
            method == "GET",
        )
        .await;
    }
    let Some(&(_, mime, body)) = ASSETS.iter().find(|(p, _, _)| *p == path) else {
        return not_found(&mut stream, &path, method == "GET").await;
    };

    // Vite fingerprints everything under `assets/` with a content hash, so those
    // may be cached forever; the HTML that names them must never be, or a phone
    // keeps yesterday's page after an upgrade and nothing explains why.
    let cache = if path.starts_with("/assets/") {
        "public, max-age=31536000, immutable"
    } else {
        NO_CACHE
    };
    respond(&mut stream, 200, "OK", mime, cache, body, method == "GET").await
}

/// Only the host may receive the QR secret. Check the TCP peer as well as
/// Host: a remote peer can forge headers, and DNS rebinding can reach loopback.
fn is_host_request(head: &str, peer: std::net::SocketAddr, local: std::net::SocketAddr) -> bool {
    if !peer.ip().is_loopback() && peer.ip() != local.ip() {
        return false;
    }
    let hosts: Vec<_> = head
        .lines()
        .skip(1)
        .filter_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.eq_ignore_ascii_case("host").then_some(value.trim())
        })
        .collect();
    if hosts.len() != 1 {
        return false;
    }
    let host = hosts[0];
    host.eq_ignore_ascii_case(&format!("localhost:{}", local.port()))
        || host.parse::<std::net::SocketAddr>().is_ok_and(|addr| {
            addr.port() == local.port() && (addr.ip().is_loopback() || addr.ip() == local.ip())
        })
}

const NO_CACHE: &str = "no-store";

/// The 404, written for the person most likely to see it.
///
/// Which is not an attacker scanning ports - it is someone who built the app
/// without building the page, or who typed the address by hand. Both are one
/// sentence away from fixed, so the page says which one happened.
async fn not_found(stream: &mut TcpStream, path: &str, with_body: bool) -> io::Result<()> {
    let body = if is_bundled() {
        format!(
            "PadRemote has no page at {path}.\n\n\
             The trackpad is at / - open this address on your phone, or scan the \
             QR from the menu bar (Connect your phone…).\n"
        )
    } else {
        "PadRemote was built without its phone page.\n\n\
         Build it and reinstall:\n\
           ./install.sh\n\n\
         or, by hand:\n\
           cd web && npm ci && npm run build\n\
           ./desktop/packaging/make-app.sh\n"
            .to_string()
    };
    respond(
        stream,
        404,
        "Not Found",
        "text/plain; charset=utf-8",
        NO_CACHE,
        body.as_bytes(),
        with_body,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn respond(
    stream: &mut TcpStream,
    code: u16,
    reason: &str,
    mime: &str,
    cache: &str,
    body: &[u8],
    with_body: bool,
) -> io::Result<()> {
    // `X-Content-Type-Options` because the page is served over plain http on a
    // LAN: a browser that sniffs a type it was not given is a browser that can
    // be talked into running something as script.
    let head = format!(
        "HTTP/1.1 {code} {reason}\r\n\
         Content-Type: {mime}\r\n\
         Content-Length: {len}\r\n\
         Cache-Control: {cache}\r\n\
         X-Content-Type-Options: nosniff\r\n\
         Referrer-Policy: no-referrer\r\n\
         Connection: close\r\n\
         \r\n",
        len = body.len(),
    );
    stream.write_all(head.as_bytes()).await?;
    if with_body {
        stream.write_all(body).await?;
    }
    stream.flush().await?;
    // A browser that is still writing when we close gets an RST and reports a
    // network error instead of the response we just sent. Shutting down the
    // write half says "that is all" without discarding what is in flight.
    let _ = stream.shutdown().await;
    Ok(())
}

/// Take the request out of the socket, so closing it does not send an RST.
///
/// The head is a known length. A body is not something any request here has,
/// but a client that sent one anyway has to be read past all the same, and a
/// `Content-Length` is an untrusted number - so it is capped rather than
/// believed. Past the cap we simply stop: the response still goes out, and the
/// reset that may follow is the sender's doing.
async fn drain(stream: &mut TcpStream, head: &str, head_len: usize) -> io::Result<()> {
    let mut scratch = vec![0u8; head_len];
    stream.read_exact(&mut scratch).await?;

    let body = content_length(head).unwrap_or(0).min(MAX_DRAIN_BYTES);
    if body > 0 {
        let mut rest = vec![0u8; body];
        // Best effort: a client that announced a body and then did not send it
        // must not hold the task open, and the response is unaffected.
        let _ = tokio::time::timeout(DRAIN_TIMEOUT, stream.read_exact(&mut rest)).await;
    }
    Ok(())
}

/// The most of a request body worth reading before answering.
const MAX_DRAIN_BYTES: usize = 64 * 1024;
/// How long to spend on that. A body is already unexpected here.
const DRAIN_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(2);

/// The declared body size, if the request declared one.
fn content_length(head: &str) -> Option<usize> {
    head.lines().skip(1).find_map(|line| {
        let (name, value) = line.split_once(':')?;
        name.trim()
            .eq_ignore_ascii_case("content-length")
            .then(|| value.trim().parse().ok())
            .flatten()
    })
}

/// `("GET", "/config.html?h=…")` from a request head.
fn request_line(head: &str) -> Option<(&str, &str)> {
    let line = head.lines().next()?;
    let mut parts = line.split_whitespace();
    let method = parts.next()?;
    let target = parts.next()?;
    // The version is not checked: a request that got this far is a browser, and
    // refusing HTTP/1.0 would buy nothing.
    Some((method, target))
}

/// The asset path a request target names.
///
/// Query and fragment are dropped - the phone page carries `?h=` and `#k=`, and
/// both are for the JavaScript, not for us. `/` means the trackpad, and a bare
/// name gets `.html` appended so `/config` reaches the settings page too.
fn normalize(target: &str) -> String {
    let path = target
        .split(['?', '#'])
        .next()
        .unwrap_or("/")
        .trim_end_matches('/');
    if path.is_empty() {
        return "/index.html".to_string();
    }
    if !path.contains('.') && ASSETS.iter().any(|(p, _, _)| *p == format!("{path}.html")) {
        return format!("{path}.html");
    }
    path.to_string()
}

/// Does this request head ask to become a WebSocket?
///
/// Header names are case-insensitive and `Connection` is a comma-separated list
/// (`keep-alive, Upgrade` is what several browsers actually send), so neither
/// can be compared literally.
pub(crate) fn is_websocket_upgrade(head: &str) -> bool {
    let mut upgrade_websocket = false;
    let mut connection_upgrade = false;
    for line in head.lines().skip(1) {
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        let name = name.trim().to_ascii_lowercase();
        let value = value.trim().to_ascii_lowercase();
        match name.as_str() {
            "upgrade" => upgrade_websocket |= value.split(',').any(|v| v.trim() == "websocket"),
            "connection" => connection_upgrade |= value.split(',').any(|v| v.trim() == "upgrade"),
            _ => {}
        }
    }
    upgrade_websocket && connection_upgrade
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_detection_checks_peer_and_authority() {
        let local = "192.168.1.5:8787".parse().unwrap();
        for (peer, host, expected) in [
            ("192.168.1.5:50000", "192.168.1.5:8787", true),
            ("127.0.0.1:50000", "localhost:8787", true),
            ("192.168.1.6:50000", "localhost:8787", false),
            ("192.168.1.6:50000", "192.168.1.5:8787", false),
            ("127.0.0.1:50000", "evil.example:8787", false),
            ("127.0.0.1:50000", "localhost:9999", false),
        ] {
            assert_eq!(
                is_host_request(
                    &format!("GET / HTTP/1.1\r\nHost: {host}\r\n\r\n"),
                    peer.parse().unwrap(),
                    local
                ),
                expected
            );
        }
    }

    #[test]
    fn a_browser_asking_for_the_page_is_not_an_upgrade() {
        let head = "GET / HTTP/1.1\r\nHost: 192.168.1.5:8787\r\nAccept: text/html\r\n\r\n";
        assert!(!is_websocket_upgrade(head));
        assert_eq!(request_line(head), Some(("GET", "/")));
    }

    #[test]
    fn a_websocket_handshake_is_recognised_however_it_is_cased() {
        for head in [
            "GET / HTTP/1.1\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n\r\n",
            "GET / HTTP/1.1\r\nupgrade: WebSocket\r\nconnection: keep-alive, Upgrade\r\n\r\n",
            "GET /observe HTTP/1.1\r\nConnection: Upgrade\r\nUpgrade: websocket\r\n\r\n",
        ] {
            assert!(is_websocket_upgrade(head), "{head:?}");
        }
    }

    /// Half an upgrade is not one. Safari sends `Connection: keep-alive` on
    /// ordinary requests, and treating that as a handshake would hand every
    /// page load to tungstenite.
    #[test]
    fn half_an_upgrade_is_not_one() {
        for head in [
            "GET / HTTP/1.1\r\nConnection: keep-alive\r\n\r\n",
            "GET / HTTP/1.1\r\nUpgrade: websocket\r\n\r\n",
            "GET / HTTP/1.1\r\nConnection: Upgrade\r\n\r\n",
            // The request line itself must never be read as a header.
            "GET /upgrade: websocket HTTP/1.1\r\nConnection: Upgrade\r\n\r\n",
        ] {
            assert!(!is_websocket_upgrade(head), "{head:?}");
        }
    }

    #[test]
    fn the_root_is_the_trackpad_and_the_query_is_not_ours() {
        assert_eq!(normalize("/"), "/index.html");
        assert_eq!(normalize(""), "/index.html");
        assert_eq!(normalize("/?h=localhost:8787"), "/index.html");
        assert_eq!(normalize("/#k=deadbeef"), "/index.html");
        assert_eq!(
            normalize("/config.html?h=localhost:8787#k=deadbeef"),
            "/config.html"
        );
    }

    /// Nothing outside the table can be named, whatever the request says. The
    /// lookup is an equality test against compiled-in strings, so `..` is not a
    /// traversal - it is simply a path that is not there.
    #[test]
    fn there_is_nothing_to_traverse_to() {
        for target in [
            "/../../etc/passwd",
            "/..%2f..%2fetc%2fpasswd",
            "//etc/passwd",
            "/assets/../../../../etc/passwd",
        ] {
            let path = normalize(target);
            assert!(
                !ASSETS.iter().any(|(p, _, _)| *p == path),
                "{target} resolved to something servable: {path}"
            );
        }
    }

    #[test]
    fn a_request_that_is_not_a_request_is_answered_with_nothing() {
        assert_eq!(request_line(""), None);
        assert_eq!(request_line("GET"), None);
    }
}
