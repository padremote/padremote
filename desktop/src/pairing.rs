//! Pairing: show a QR the phone can simply point its camera at (plan.md section 7).
//!
//! The QR carries the desktop's own LAN address *and* the pairing secret, so the
//! phone connects straight back with nothing to type and can answer the
//! challenge on its first socket.
//!
//! The secret lives in the URL **fragment**, and that placement is the whole
//! security argument for putting it in a link at all: a fragment is never sent
//! in an HTTP request, so it does not reach the server that serves the page, its
//! access log, any proxy in between, or a `Referer` header on the way out. A
//! query string would reach all four. Everything below that builds a URL keeps
//! the address in the query if it likes, and the key strictly after the `#`.

use qrcode::render::svg;
use qrcode::{EcLevel, QrCode};

use crate::auth::Secret;

/// The connect screen, with `__PLACEHOLDER__` for everything this computer
/// has to fill in. Kept beside the code as a file so it stays editable as a
/// page rather than as an escaped string literal.
const PAGE: &str = include_str!("assets/connect.html");

/// Everything the phone needs to find this computer.
pub struct Pairing {
    /// The URL encoded in the QR.
    pub url: String,
    /// Name shown to the user, so they know which computer they are pairing.
    pub computer: String,
    page_port: u16,
    ws_port: u16,
    /// This computer's LAN address, so the page it renders can tell when the
    /// router has handed out a different one and the QR on screen is stale.
    ip: Option<String>,
    /// The pairing secret, hex, for the fragment of every URL built here.
    key: String,
}

impl Pairing {
    /// `http://<ip>:<page_port>/#h=<ip>:<ws_port>&k=<secret>` - the page, told
    /// where to call back and how to prove it was invited. Both are in the
    /// fragment, so neither reaches a server.
    pub fn new(ip: &str, page_port: u16, ws_port: u16, computer: String, secret: &Secret) -> Self {
        let key = secret.to_hex();
        Self {
            url: format!("http://{ip}:{page_port}/#h={ip}:{ws_port}&k={key}"),
            computer,
            page_port,
            ws_port,
            ip: Some(ip.to_string()),
            key,
        }
    }

    /// Everything except the QR, for a computer with no LAN address.
    ///
    /// The pages this machine opens for itself do not need a network, so they
    /// should not need one to be *reachable* either: with Wi-Fi off there was
    /// no `Pairing` at all, and Settings and the diagnostics were greyed out in
    /// the menu even though loopback would have served both perfectly.
    pub fn local_only(page_port: u16, ws_port: u16, computer: String, secret: &Secret) -> Self {
        Self {
            url: String::new(),
            computer,
            page_port,
            ws_port,
            ip: None,
            key: secret.to_hex(),
        }
    }

    /// Is there an address a phone could actually reach?
    pub fn can_pair(&self) -> bool {
        !self.url.is_empty()
    }

    /// The same pages, addressed the way this computer should reach itself.
    ///
    /// Everything below is opened *here* - from the menu bar, or from a link on
    /// the pairing page, which is also shown on this screen. Sending those
    /// through the LAN address made them depend on a network they do not need:
    /// the address changes when the router says so, it does not exist at all
    /// with Wi-Fi off, and a guest network with client isolation can refuse to
    /// route a machine back to itself. Loopback has none of those problems.
    ///
    /// The QR keeps the LAN address, because the phone genuinely is somewhere
    /// else and `localhost` would point it at itself.
    fn local(&self, page: &str) -> String {
        format!(
            "http://localhost:{}/{page}?h=localhost:{}#k={}",
            self.page_port, self.ws_port, self.key
        )
    }

    /// The live debug view, on the same host that serves the phone page.
    ///
    /// It attaches as a read-only observer, so opening it never takes control
    /// away from the phone the way loading the normal page would.
    pub fn debug_url(&self) -> String {
        self.local("debug.html")
    }

    /// The settings page, on the same host that serves the phone page.
    ///
    /// The config used to be reachable only as a JSON file on this computer,
    /// which is the wrong place: the device the user is holding is the phone.
    pub fn config_url(&self) -> String {
        self.local("config.html")
    }

    /// The connect page's own address, on this computer.
    ///
    /// The page used to be written to a file in `/tmp` and opened from there,
    /// which cost more than it looked like. A `file://` page has the opaque
    /// origin `null`, so it may not open a socket to this app at all - it had
    /// to reach the device list through an iframe served from loopback - and a
    /// copy of the pairing secret sat in a world-listable directory for as long
    /// as the file was there. Served over loopback instead, the page is
    /// same-origin with the server it talks to and outlives nothing.
    ///
    /// The app's own port, not `page_port`: in development the page port is
    /// Vite, which serves the *phone* page at `/`. This screen comes from the
    /// app itself (see `net::pages`), which only ever answers on its own port.
    pub fn host_url(&self) -> String {
        format!("http://localhost:{}/", self.ws_port)
    }

    /// Render the connect screen: the QR, and the devices already paired.
    ///
    /// Server-rendered because the two things on it that cannot be fetched are
    /// the QR - drawn here from the address and the secret - and the secret the
    /// page needs to open its own socket. Everything that changes while it is
    /// open arrives over that socket instead; see `net::devices`.
    pub fn render_page(&self) -> anyhow::Result<String> {
        let code = QrCode::with_error_correction_level(&self.url, EcLevel::M)?;
        // Quiet zone and high contrast: phone cameras are unforgiving.
        let qr_svg = code
            .render::<svg::Color>()
            .min_dimensions(320, 320)
            .quiet_zone(true)
            .dark_color(svg::Color("#000000"))
            .light_color(svg::Color("#ffffff"))
            .build();

        let qr_block = if self.can_pair() {
            format!(
                "<div class=\"qr\" role=\"img\" aria-label=\"QR code to connect a device to this \
                 computer\">{qr_svg}</div>\n    <p>Scan it with the phone’s camera.</p>\n    \
                 <p class=\"hint\">Both devices on the same Wi-Fi.</p>"
            )
        } else {
            "<p class=\"no-wifi\">This computer has no network address, so there is nothing for a \
             phone to reach.</p>\n    <p class=\"hint\">Connect it to Wi-Fi, then reload this \
             page.</p>"
                .to_string()
        };

        // Substituted rather than formatted, so the page below can be read as
        // the HTML and JavaScript it is, without every brace doubled.
        //
        // The computer's name is the one value here that a user chose, so it
        // goes in last: no placeholder that follows it can be one it contains.
        let html = PAGE
            .replace("__QR__", &qr_block)
            .replace("__URL__", &html_escape(&self.url))
            .replace("__CONFIG_URL__", &html_escape(&self.config_url()))
            .replace("__DEBUG_URL__", &html_escape(&self.debug_url()))
            .replace("__PORT__", &self.ws_port.to_string())
            .replace("__IP__", self.ip.as_deref().unwrap_or_default())
            .replace("__KEY__", &self.key)
            .replace("__COMPUTER__", &html_escape(&self.computer));

        Ok(html)
    }

    /// Open the connect page in the default browser.
    pub fn show(&self) -> anyhow::Result<()> {
        std::process::Command::new("open")
            .arg(self.host_url())
            .status()?;
        Ok(())
    }

    /// A compact QR for the terminal, for anyone running this over SSH.
    pub fn terminal_qr(&self) -> anyhow::Result<String> {
        let code = QrCode::with_error_correction_level(&self.url, EcLevel::M)?;
        Ok(code
            .render::<char>()
            .quiet_zone(true)
            .module_dimensions(2, 1)
            .dark_color('█')
            .light_color(' ')
            .build())
    }
}

/// The computer name is user-controlled text going into a page we open.
fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn secret() -> Secret {
        Secret::from_hex("0123456789abcdef0123456789abcdef").expect("a valid test secret")
    }

    /// The QR is for the phone; every other link is for this machine.
    #[test]
    fn the_desktops_own_pages_do_not_go_through_the_network() {
        let p = Pairing::new("192.168.1.42", 5173, 8787, "Studio Mac".into(), &secret());

        // Opened here, from the menu bar and from the pairing page's own links.
        for url in [p.config_url(), p.debug_url()] {
            assert!(
                url.starts_with("http://localhost:5173/"),
                "{url} goes out over the LAN to reach this same computer"
            );
            assert!(
                url.contains("?h=localhost:8787"),
                "{url} points the socket at the LAN address"
            );
            assert!(
                !url.contains("192.168.1.42"),
                "{url} still carries the LAN address"
            );
        }

        // Scanned by a phone, which is genuinely somewhere else.
        assert_eq!(
            p.url,
            "http://192.168.1.42:5173/#h=192.168.1.42:8787&k=0123456789abcdef0123456789abcdef"
        );
    }

    /// The one placement rule the whole scheme rests on: a secret in a query
    /// string is a secret in the page server's access log.
    #[test]
    fn the_secret_is_only_ever_in_the_fragment() {
        let p = Pairing::new("192.168.1.42", 5173, 8787, "Studio Mac".into(), &secret());
        let key = secret().to_hex();
        for url in [p.url.clone(), p.config_url(), p.debug_url()] {
            let (before_hash, after_hash) = url.split_once('#').unwrap_or((&url, ""));
            assert!(
                !before_hash.contains(&key),
                "{url} puts the pairing secret where a server would see it"
            );
            assert!(after_hash.contains(&key), "{url} carries no secret at all");
        }
    }

    /// With no LAN address there is no QR - and everything else still works.
    #[test]
    fn settings_and_diagnostics_survive_the_network_being_off() {
        let p = Pairing::local_only(5173, 8787, "Laptop".into(), &secret());
        assert!(
            !p.can_pair(),
            "a phone cannot reach a computer with no address"
        );
        assert_eq!(
            p.config_url(),
            "http://localhost:5173/config.html?h=localhost:8787#k=0123456789abcdef0123456789abcdef"
        );
        assert_eq!(
            p.debug_url(),
            "http://localhost:5173/debug.html?h=localhost:8787#k=0123456789abcdef0123456789abcdef"
        );
    }

    /// A non-default port has to survive into the local links.
    #[test]
    fn the_local_links_keep_whatever_ports_are_in_use() {
        let p = Pairing::new("10.0.0.9", 3000, 9100, "Laptop".into(), &secret());
        assert!(p
            .config_url()
            .starts_with("http://localhost:3000/config.html?h=localhost:9100#"));
    }
}
