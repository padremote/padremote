//! Who is allowed to *open* a socket, decided from the handshake alone.
//!
//! A WebSocket is not subject to the same-origin policy. There is no preflight,
//! no CORS, nothing to opt into: a page on any website can open a socket to any
//! address its browser can reach, `127.0.0.1` very much included, and read every
//! byte the server sends back. For a server that moves the cursor and clicks
//! things, that is as serious as it sounds - a tab left open in the background
//! could drive this machine.
//!
//! The browser will not let that page *lie* about one thing, though: the
//! `Origin` header. So the rule is a short one.
//!
//! - **No `Origin` at all** - a native client: the replay tool, the tests,
//!   `websocat`. Allowed here and stopped by the pairing challenge instead,
//!   which is the check that actually holds against a program (a program can
//!   send whatever header it likes, so treating a missing `Origin` as hostile
//!   would buy nothing and break every local tool).
//! - **An origin whose host is a bare IP address, or `localhost`** - the phone
//!   page, however it is being served: from Vite in development, from this app
//!   later, from `file://`-less loopback in the tests.
//! - **Anything else is refused**, which is every website there is: a page at
//!   `https://example.com` keeps that origin even if its DNS name resolves
//!   straight to this LAN, so DNS rebinding does not get around it either.
//!
//! The check costs nothing and is the difference between "an attacker needs to
//! be on your Wi-Fi" and "an attacker needs you to have a tab open".

/// Is this `Origin` header one we are willing to talk to?
pub fn allowed(origin: Option<&str>) -> bool {
    let Some(origin) = origin else {
        // A native client. The challenge in `auth` is what gates it.
        return true;
    };
    // `null` is what a sandboxed iframe or a `file://` page sends. It is not an
    // address, it is the absence of one, and it is the origin an attacker gets
    // for free from a sandboxed frame - so it is not "no origin", it is "no".
    let Some(host) = host_of(origin) else {
        return false;
    };
    host == "localhost" || is_ip_literal(&host)
}

/// The host part of an origin, lowercased, without scheme, port or brackets.
///
/// `None` for anything that is not `scheme://host[:port]` - including `null`.
fn host_of(origin: &str) -> Option<String> {
    let rest = origin.split_once("://")?.1;
    // Anything after the authority is not part of an origin, but a header we
    // did not write is not a header we get to assume is well-formed.
    let authority = rest.split(['/', '?', '#']).next()?;
    if authority.is_empty() {
        return None;
    }
    // An IPv6 origin is `http://[::1]:5173`; the colons inside the brackets are
    // part of the address, not a port separator.
    let host = if let Some(inner) = authority.strip_prefix('[') {
        inner.split_once(']')?.0
    } else {
        authority.split(':').next()?
    };
    if host.is_empty() {
        return None;
    }
    Some(host.to_ascii_lowercase())
}

/// Is this a literal address rather than a name someone can register?
fn is_ip_literal(host: &str) -> bool {
    host.parse::<std::net::IpAddr>().is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_clients_have_no_origin_and_are_left_to_the_challenge() {
        assert!(allowed(None));
    }

    #[test]
    fn the_phone_page_is_allowed_however_it_is_served() {
        for origin in [
            "http://192.168.1.24:5173", // Vite on the LAN
            "http://192.168.1.24:8787", // served by this app
            "http://localhost:5173",
            "http://127.0.0.1:5173",
            "https://10.0.0.5",
            "http://[::1]:5173",
            "http://[fe80::1]:5173",
            "HTTP://LOCALHOST:5173", // headers are not case-normalised for us
        ] {
            assert!(allowed(Some(origin)), "{origin} is the phone page");
        }
    }

    /// The whole point of the module: a page on the web must not be able to
    /// open a session, whatever it resolves to.
    #[test]
    fn websites_are_refused() {
        for origin in [
            "https://example.com",
            "http://evil.test:5173",
            // DNS rebinding: the name resolves to the LAN, the origin does not
            // change with it, and that is exactly what saves us.
            "http://rebind.evil.test",
            "https://padremote.com",
            "null",    // a sandboxed iframe
            "",        // a header with nothing in it
            "http://", // an authority that is not there
            "not an origin at all",
        ] {
            assert!(!allowed(Some(origin)), "{origin} must be refused");
        }
    }

    #[test]
    fn a_name_that_merely_looks_numeric_is_still_a_name() {
        // `1.2.3.4.evil.test` resolves to whatever its owner says. Only the
        // parse decides, never the shape.
        assert!(!allowed(Some("http://192.168.1.24.evil.test")));
        assert!(!allowed(Some("http://localhost.evil.test")));
    }
}
