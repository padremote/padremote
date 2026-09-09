//! Command-line arguments.
//!
//! Hand-rolled rather than pulled from a crate: five flags do not justify a
//! dependency, and the whole surface fits on one screen. What it does owe the
//! user is the two things a hand-rolled parser usually forgets - a real
//! `--help`, and an *error* for a flag it does not know. Silently ignoring
//! `--headles` and then behaving nothing like the command that was typed is the
//! kind of thing that costs an afternoon.

use anyhow::{bail, Result};

/// Default port for the phone's WebSocket link *and* for the page itself.
///
/// One port for both: the app serves the trackpad page and then accepts the
/// socket that page opens, so there is a single address in the QR, a single
/// firewall prompt, and nothing else to have running.
pub const DEFAULT_PORT: u16 = 8787;

const HELP: &str = "\
PadRemote - use your phone's touchscreen as a wireless trackpad.

USAGE:
  padremote [OPTIONS]

OPTIONS:
      --port <PORT>       Port for the phone page and the link it opens
                          (default 8787)
      --page-port <PORT>  Serve the page from somewhere else instead - the
                          address that goes in the QR. Only useful in
                          development, where `npm run dev` has the page on
                          5173 with hot reload.
      --dry-run           Recognise gestures but inject nothing. Safe to run
                          while you work; pair with the debug page to watch.
      --headless          No menu-bar icon. For running under a debugger, over
                          SSH, or in CI.
      --no-qr             Don't open the pairing page at startup.
      --unpair            Forget every paired device: a new pairing secret is
                          generated at startup, so every phone has to scan the
                          new QR again. Use it if a paired phone is lost.
  -h, --help              Print this help
  -V, --version           Print the version

The phone page and the pairing QR are described in docs/user/getting-started.md.
";

/// What the process was asked to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Args {
    pub port: u16,
    /// Where the QR should send the phone for the page.
    ///
    /// `None` - the default - means "this app, on [`Args::port`]", which is the
    /// shipped arrangement. `Some` is a development override pointing at Vite.
    pub page_port: Option<u16>,
    pub dry_run: bool,
    pub headless: bool,
    pub no_qr: bool,
    pub unpair: bool,
}

impl Default for Args {
    fn default() -> Self {
        Self {
            port: DEFAULT_PORT,
            page_port: None,
            dry_run: false,
            headless: false,
            no_qr: false,
            unpair: false,
        }
    }
}

impl Args {
    /// The port the QR should point the phone's browser at.
    ///
    /// Derived rather than stored, so that `--port 8788` moves the page with
    /// the link instead of quietly leaving the QR pointing at 8787.
    pub fn page_port(&self) -> u16 {
        self.page_port.unwrap_or(self.port)
    }
}

/// The outcome of parsing: either run, or print something and stop.
pub enum Parsed {
    Run(Box<Args>),
    /// `--help` or `--version`: print this and exit 0.
    Print(String),
}

/// Parse arguments, *excluding* the program name.
pub fn parse<I, S>(args: I) -> Result<Parsed>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut out = Args::default();
    let mut it = args.into_iter().peekable();
    while let Some(raw) = it.next() {
        let arg = raw.as_ref();
        match arg {
            "-h" | "--help" => return Ok(Parsed::Print(HELP.to_string())),
            "-V" | "--version" => {
                return Ok(Parsed::Print(format!(
                    "padremote {}\n",
                    env!("CARGO_PKG_VERSION")
                )))
            }
            "--dry-run" => out.dry_run = true,
            "--headless" => out.headless = true,
            "--no-qr" => out.no_qr = true,
            "--unpair" => out.unpair = true,
            "--port" | "--page-port" => {
                let Some(value) = it.next() else {
                    bail!("{arg} needs a port number");
                };
                let port: u16 = value.as_ref().parse().map_err(|_| {
                    anyhow::anyhow!("{arg}: '{}' is not a port number", value.as_ref())
                })?;
                if port == 0 {
                    bail!("{arg}: 0 is not a usable port");
                }
                if arg == "--port" {
                    out.port = port;
                } else {
                    out.page_port = Some(port);
                }
            }
            other => bail!("unknown option '{other}'\n\nTry --help."),
        }
    }
    Ok(Parsed::Run(Box::new(out)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(args: &[&str]) -> Args {
        match parse(args) {
            Ok(Parsed::Run(a)) => *a,
            Ok(Parsed::Print(_)) => panic!("expected Run"),
            Err(e) => panic!("expected Run, got error: {e}"),
        }
    }

    #[test]
    fn defaults_are_the_documented_ports() {
        let a = run(&[]);
        assert_eq!(a.port, DEFAULT_PORT);
        assert_eq!(a.page_port, None, "the app serves its own page");
        assert_eq!(a.page_port(), DEFAULT_PORT);
        assert!(!a.dry_run && !a.headless && !a.no_qr && !a.unpair);
    }

    /// The bug this exists to prevent: moving the link and leaving the QR
    /// pointing at the page on the old port, so the phone loads a trackpad
    /// that cannot reach anything.
    #[test]
    fn moving_the_port_moves_the_page_with_it() {
        assert_eq!(run(&["--port", "8788"]).page_port(), 8788);
    }

    /// Development still puts the page on Vite, for hot reload.
    #[test]
    fn the_page_can_be_pointed_somewhere_else() {
        // 5173 is Vite's port; see `npm run dev`.
        let a = run(&["--page-port", "5173"]);
        assert_eq!(a.port, DEFAULT_PORT);
        assert_eq!(a.page_port(), 5173);
    }

    #[test]
    fn flags_and_ports_parse() {
        let a = run(&[
            "--dry-run",
            "--port",
            "9000",
            "--page-port",
            "3000",
            "--headless",
        ]);
        assert_eq!(a.port, 9000);
        assert_eq!(a.page_port(), 3000);
        assert!(a.dry_run && a.headless);
    }

    /// The whole reason this is not three `iter().any()` calls: a typo has to
    /// stop the program rather than change its behaviour silently.
    #[test]
    fn an_unknown_flag_is_an_error() {
        assert!(parse(["--headles"]).is_err());
        assert!(
            parse(["--port"]).is_err(),
            "a missing value is an error too"
        );
        assert!(parse(["--port", "not-a-number"]).is_err());
        assert!(parse(["--port", "0"]).is_err());
    }

    /// Revoking every paired device is a thing you do once, in a hurry, from a
    /// terminal - it has to be spelled the way `--help` says it is.
    #[test]
    fn unpair_is_a_flag_of_its_own() {
        assert!(run(&["--unpair"]).unpair);
        assert!(!run(&[]).unpair);
        assert!(HELP.contains("--unpair"));
    }

    #[test]
    fn help_and_version_print_rather_than_run() {
        assert!(matches!(parse(["--help"]), Ok(Parsed::Print(_))));
        assert!(matches!(parse(["-V"]), Ok(Parsed::Print(_))));
    }
}
