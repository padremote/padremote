//! Entry point.
//!
//! Structure is dictated by macOS: the menu-bar event loop must own the **main**
//! thread, so the tokio runtime is started on a worker thread and the two sides
//! talk through `LinkStatus`. `--headless` skips the tray entirely, which is how
//! the app runs under a debugger or over SSH.
//!
//! Startup reads top to bottom as the decisions it actually makes: what was
//! asked for, whether this port is ours to take, what the config and the host's
//! trackpad say, whether we may inject input at all, and only then the server.
//! The port comes first so a duplicate copy exits before it has asked the user
//! for anything. The loops that run for the life of the process live in
//! [`padremote::app`].

mod cli;

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use padremote::app;
use padremote::auth::{Devices, Secret};
use padremote::gesture::Config;
use padremote::input::{Blocked, Injector, NullInjector};
use padremote::net::{self, LinkStatus, Shared};
use padremote::pairing::Pairing;
use padremote::sysprefs::HostTrackpad;

fn main() -> Result<()> {
    let args = match cli::parse(std::env::args().skip(1))? {
        cli::Parsed::Run(args) => *args,
        cli::Parsed::Print(text) => {
            print!("{text}");
            return Ok(());
        }
    };

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "padremote=info".into()),
        )
        .with_target(false)
        .init();

    let (cfg, config_path) = load_config();
    let (secret, secret_path) = load_secret(args.unpair)?;

    // The runtime lives on its own thread so the main thread is free for the
    // menu bar. Holding the guard keeps the runtime alive for the process.
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    let addr: SocketAddr = ([0, 0, 0, 0], args.port).into();
    // Bound before anything else, and in particular before asking macOS for
    // Accessibility: a second copy - the login item's and one the user opened
    // by hand - must die on "the port is taken" rather than first putting up
    // its own permission dialog. Two dialogs for one grant is how this looked
    // to the user, and only one of the two copies can ever serve a phone.
    // Bound here rather than inside the task, so that by the time the connect
    // page is opened a moment later there is something listening for it.
    let listener = runtime.block_on(tokio::net::TcpListener::bind(addr));
    let listener = match listener {
        Ok(listener) => listener,
        Err(e) => return Err(port_in_use(e, args.port)),
    };

    // A missing Accessibility grant must not be fatal. This is a menu-bar app:
    // launched from Spotlight it has no window and no terminal, so exiting with
    // an explanation on stderr is indistinguishable from "the app is broken".
    // Start deaf instead, keep the tray and the server up so the user can see
    // what is wrong, and swap the real backend in the moment permission lands.
    let mut awaiting_permission = false;
    let injector: Box<dyn Injector> = if args.dry_run {
        tracing::warn!("--dry-run: gestures are recognised but no input is injected");
        Box::new(NullInjector::new(Blocked::DryRun))
    } else {
        match make_injector() {
            Ok(inj) => inj,
            // Headless is a developer or CI run: there is nobody to notice a
            // tray icon explaining itself, so a missing grant is fatal there.
            Err(e) if args.headless => return Err(e),
            Err(_) => {
                eprint!("{}", padremote::input::permission_help());
                tracing::warn!(
                    "no Accessibility permission yet - running without input injection; \
                     grant it and PadRemote will start moving the cursor on its own"
                );
                awaiting_permission = true;
                Box::new(NullInjector::new(Blocked::Permission))
            }
        }
    };

    let status = LinkStatus::new();
    status.set_permission(!awaiting_permission);
    // Every device that connects gets its own recognizer, built from this
    // config: two phones driving one cursor must not share gesture state.
    let shared = Arc::new(Shared::new(
        cfg,
        injector,
        padremote::host_name(),
        status.clone(),
        secret.clone(),
        Devices::load(Devices::user_path()),
    ));

    // The settings page edits this file, so the server has to know where it is.
    shared.set_config_path(config_path.clone());
    // *Forget all devices* on the connect page rotates the secret, and a
    // rotation that is not written down is undone by the next launch.
    shared.set_secret_path(secret_path);

    // Always present: without a LAN address there is no QR, but Settings and
    // the diagnostics live on loopback and work regardless.
    // Seeded before the tray starts, so the menu never sees "no address yet"
    // and rebuilds the pairing links around it a moment after startup.
    let lan_ip = app::local_ip();
    shared.set_lan_ip(lan_ip.clone());
    let pairing = Some(
        lan_ip
            .as_deref()
            .map(|ip| {
                Pairing::new(
                    ip,
                    args.page_port(),
                    args.port,
                    shared.host_name.clone(),
                    &secret,
                )
            })
            .unwrap_or_else(|| {
                Pairing::local_only(
                    args.page_port(),
                    args.port,
                    shared.host_name.clone(),
                    &secret,
                )
            }),
    );
    // The console gets the whole URL - it is a terminal the user is looking at,
    // and a URL without its key pairs nothing. The menu bar derives its own
    // line from the pairing, without the key; see `tray::address_hint`.
    let hint = match &pairing {
        Some(p) if p.can_pair() => p.url.clone(),
        _ => "no LAN address found - is Wi-Fi on?".to_string(),
    };
    announce(&hint, pairing.as_ref());

    spawn_server(&runtime, listener, shared.clone());
    report_page_source(&args);

    #[cfg(target_os = "macos")]
    if awaiting_permission {
        app::spawn_permission_watch(&runtime, shared.clone());
    }
    app::spawn_maintenance(&runtime, shared.clone(), config_path.clone());

    // Open the connect page so the phone can just point its camera at the QR.
    // This is the whole first-run experience for a non-technical user: no URL
    // to type, and the devices already paired are on the same screen.
    if let Some(p) = &pairing {
        if !args.no_qr && p.can_pair() {
            if let Err(e) = p.show() {
                tracing::warn!("could not open the connect page ({e}); the URL above still works");
            }
        }
    }

    #[cfg(target_os = "macos")]
    if !args.headless {
        // Never returns; quitting from the menu exits the process.
        padremote::tray::run(status, shared.clone(), args.page_port(), args.port);
    }

    // Headless: hold the main thread until interrupted.
    runtime.block_on(async { tokio::signal::ctrl_c().await })?;
    tracing::info!("shutting down");
    // A phone that was mid-drag when we quit must not leave a button held down
    // on the desktop it was driving.
    shared.release_all();
    Ok(())
}

/// Say where the phone page is coming from, and complain if it is nowhere.
///
/// The page is compiled in, so normally there is nothing to say and the log
/// simply records how many files went in. The two cases worth a warning are the
/// ones that used to be discovered by a phone showing "this site can't be
/// reached": a binary built without `web/dist`, and a `--page-port` pointing at
/// a development server that may not be running.
fn report_page_source(args: &cli::Args) {
    use padremote::net::pages;
    match args.page_port {
        Some(port) => tracing::info!(
            "serving the phone page from port {port} instead of this app - \
             start it with `cd web && npm run dev`"
        ),
        None if pages::is_bundled() => {
            tracing::info!("phone page built in ({} files)", pages::bundled_count())
        }
        None => tracing::warn!(
            "this build has no phone page in it, so there is nothing for a phone \
             to load. Rebuild with ./install.sh, or run `cd web && npm run build` \
             and reinstall."
        ),
    }
}

/// The pairing secret, and the file it lives in.
///
/// `--unpair` replaces it, which is how a phone that has been lost, lent or
/// simply moved on is revoked: every device holding the old secret fails its
/// next handshake, and the QR that comes up carries the new one.
///
/// A machine with no config directory at all still runs - with a secret that
/// lasts only as long as the process, so pairing has to be redone on the next
/// start. That is worse than the file, and far better than starting with no
/// secret at all: a degraded install must not be an unauthenticated one.
fn load_secret(unpair: bool) -> Result<(Secret, Option<PathBuf>)> {
    let Some(path) = Secret::user_path() else {
        tracing::warn!(
            "no config directory: this session's pairing will be forgotten when PadRemote quits"
        );
        return Ok((Secret::generate(), None));
    };
    if unpair {
        let secret = Secret::generate();
        secret.save(&path)?;
        println!("Unpaired every device. Scan the new QR to pair again.");
        return Ok((secret, Some(path)));
    }
    match Secret::load_or_create(&path) {
        Ok(secret) => Ok((secret, Some(path))),
        Err(e) => {
            tracing::warn!(
                "could not save the pairing secret to {} ({e}); using one that lasts \
                 only for this session",
                path.display()
            );
            Ok((Secret::generate(), None))
        }
    }
}

/// The config, and where it came from: file, plus the host's own trackpad.
fn load_config() -> (Config, Option<PathBuf>) {
    let config_path = Config::seed_user_config();
    let mut file_cfg = match &config_path {
        Some(p) => Config::load(p),
        None => Config::default(),
    };
    if let Some(p) = &config_path {
        tracing::info!("config: {}", p.display());
        // Written back straight away rather than carried in memory: the
        // correction has to survive the next launch, and the settings page
        // writes the whole file on any edit, so a version stamp that only lived
        // here would be lost the moment somebody moved a slider.
        if file_cfg.migrate() {
            tracing::info!("config migrated to version {}", file_cfg.version);
            if let Err(e) = file_cfg.save(p) {
                tracing::warn!("could not write the migrated config: {e}");
            }
        }
    }

    // Behave like the trackpad the user already has, rather than like whatever
    // this file happened to ship with.
    let host = HostTrackpad::read();
    let cfg = host.apply_to(&file_cfg);
    if file_cfg.follow_system {
        tracing::info!("mirroring system trackpad: {}", host.summary());
        // The full table, so "does it mirror my trackpad?" is answerable without
        // having to ask anyone.
        print!("{}", padremote::sysprefs::text_report(&host.report()));
        #[cfg(target_os = "macos")]
        if !padremote::sysprefs::macos::space_shortcuts_enabled()
            && cfg.bindings.four_finger_horiz_swipe != "none"
        {
            tracing::warn!(
                "swipe-to-switch-spaces is enabled, but the Mission Control keyboard shortcuts \
                 it is sent as are disabled - re-enable them in System Settings > Keyboard > \
                 Keyboard Shortcuts > Mission Control"
            );
        }
    } else {
        tracing::info!("followSystem is off: using config.json alone");
    }
    (cfg, config_path)
}

/// What to type on the phone, for whoever started this from a terminal.
fn announce(hint: &str, pairing: Option<&Pairing>) {
    println!("PadRemote is listening.");
    println!("  phone -> {hint}");
    if let Some(p) = pairing {
        println!("  settings -> {}", p.config_url());
        println!("  debug -> {}", p.debug_url());
    }
    println!("  (Ctrl-C to stop)");
}

fn spawn_server(
    runtime: &tokio::runtime::Runtime,
    listener: tokio::net::TcpListener,
    shared: Arc<Shared>,
) {
    runtime.spawn(async move {
        if let Err(e) = net::serve_on(listener, shared).await {
            tracing::error!("server stopped: {e}");
            std::process::exit(1);
        }
    });
}

/// The most common reason the port will not bind is a second copy already
/// running, and "Address already in use" sends people hunting in the wrong
/// place entirely.
fn port_in_use(e: std::io::Error, port: u16) -> anyhow::Error {
    if e.kind() == std::io::ErrorKind::AddrInUse {
        eprintln!(
            "\n  Port {port} is already taken - PadRemote is probably already running.\n\
             \x20 Quit the other copy (menu-bar icon > Quit PadRemote), or start this\n\
             \x20 one on another port:  --port 8788\n"
        );
    }
    e.into()
}

#[cfg(target_os = "macos")]
fn make_injector() -> Result<Box<dyn Injector>> {
    use padremote::input::{permission_help, request_accessibility, PlatformInjector};
    // Asking with the prompt shows macOS's own dialog, which has a button
    // straight to the right settings pane - far better than a URL in a log.
    if !request_accessibility() {
        // Never inject silently before permission exists; guide instead
        // (plan.md section 11.1).
        eprintln!("{}", permission_help());
        anyhow::bail!("Accessibility permission is not granted");
    }
    Ok(Box::new(PlatformInjector::new()?))
}

#[cfg(not(target_os = "macos"))]
fn make_injector() -> Result<Box<dyn Injector>> {
    // Windows (SendInput) and Linux (uinput/XTEST) backends land with the
    // cross-platform work in plan.md section 16.
    anyhow::bail!("no input backend for this platform yet; run with --dry-run")
}
