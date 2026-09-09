//! Menu-bar presence (plan.md section 11).
//!
//! The app is meant to sit quietly in the tray, so the menu answers exactly one
//! question - *is anything connected?* - and offers the three things there is no
//! other way to reach: the connect page, the settings page, and quitting.
//!
//! Everything else that used to live here has moved to the connect page. A menu
//! is the wrong shape for managing devices: it has to be held open, it cannot
//! show a list that changes underneath the pointer, and "forget the iPad" is a
//! decision people want to read before they make. See `net::devices`.
//!
//! Threading note, and it is the whole reason this module is shaped like this:
//! on macOS the menu-bar event loop must own the **main** thread. So the async
//! runtime runs on a worker thread and communicates through `Status`, and this
//! function never returns - it is the last thing `main` calls.

use std::sync::Arc;

use tao::event_loop::{ControlFlow, EventLoopBuilder};
use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, TrayIconBuilder};

use crate::net::Shared;
use crate::pairing::Pairing;

/// What the tray shows, read straight from the network layer.
pub use crate::net::LinkStatus as Status;

fn label(status: &Status) -> String {
    // A missing grant outranks the count: a connected phone that cannot move
    // the cursor looks like a broken app until this says otherwise.
    if status.needs_permission() {
        return "Needs Accessibility permission".into();
    }
    match status.devices() {
        0 => "No devices connected".into(),
        1 => "1 device connected".into(),
        n => format!("{n} devices connected"),
    }
}

/// A plain menu-bar glyph: a rounded trackpad outline with two contact dots.
/// Drawn in code so the binary stays self-contained.
fn icon() -> Option<Icon> {
    const N: u32 = 32;
    let mut rgba = vec![0u8; (N * N * 4) as usize];
    let put = |rgba: &mut Vec<u8>, x: u32, y: u32, a: u8| {
        let i = ((y * N + x) * 4) as usize;
        // macOS renders template-style icons; white with alpha reads correctly
        // in both light and dark menu bars.
        rgba[i] = 255;
        rgba[i + 1] = 255;
        rgba[i + 2] = 255;
        rgba[i + 3] = a;
    };
    // Outline of the pad.
    for y in 4..28u32 {
        for x in 7..25u32 {
            let edge = x == 7 || x == 24 || y == 4 || y == 27;
            // Knock the corners off so it reads as rounded.
            let corner = !(9..=22).contains(&x) && !(6..=25).contains(&y);
            if edge && !corner {
                put(&mut rgba, x, y, 235);
            }
        }
    }
    // Two contact dots, as if two fingers were resting on it.
    for (cx, cy, a) in [(13u32, 14u32, 235u8), (19, 19, 140u8)] {
        for dy in 0..3u32 {
            for dx in 0..3u32 {
                put(&mut rgba, cx + dx, cy + dy, a);
            }
        }
    }
    Icon::from_rgba(rgba, N, N).ok()
}

/// Run the menu bar. Never returns.
///
/// Both links are built when they are clicked rather than held here, because
/// both carry the pairing secret and that can be replaced while the app runs -
/// a settings link cached at startup would stop opening the moment somebody
/// pressed *Forget all devices* on the connect page.
pub fn run(status: Status, shared: Arc<Shared>, page_port: u16, port: u16) -> ! {
    let menu = Menu::new();
    let status_item = MenuItem::new(label(&status), false, None);
    let connect = MenuItem::new("Connect a device…", true, None);
    let settings = MenuItem::new("Settings…", true, None);
    let quit = MenuItem::new("Quit PadRemote", true, None);
    let _ = menu.append_items(&[
        &status_item,
        &PredefinedMenuItem::separator(),
        &connect,
        &settings,
        &PredefinedMenuItem::separator(),
        &quit,
    ]);

    let event_loop = EventLoopBuilder::new().build();

    // The tray must be built after the event loop exists, or macOS has no
    // application object to attach it to.
    let mut tray = None;
    let connect_id = connect.id().clone();
    let settings_id = settings.id().clone();
    let quit_id = quit.id().clone();
    let menu_rx = MenuEvent::receiver();

    event_loop.run(move |_event, _target, control_flow| {
        // Wake regularly so the status line stays current without a channel.
        *control_flow = ControlFlow::WaitUntil(
            std::time::Instant::now() + std::time::Duration::from_millis(500),
        );

        if tray.is_none() {
            tray = TrayIconBuilder::new()
                .with_menu(Box::new(menu.clone()))
                .with_tooltip("PadRemote")
                .with_icon_as_template(true)
                .with_icon(icon().expect("built-in tray icon"))
                .build()
                .ok();
        }

        status_item.set_text(label(&status));
        if let Some(t) = &tray {
            let _ = t.set_tooltip(Some(format!("PadRemote - {}", label(&status))));
        }

        while let Ok(event) = menu_rx.try_recv() {
            if event.id == quit_id {
                std::process::exit(0);
            } else if event.id == connect_id {
                // Served by this app on loopback, and rendered from the address
                // and the secret it holds right now - so it is correct even if
                // the router renumbered the network a minute ago.
                open(&format!("http://localhost:{port}/"));
            } else if event.id == settings_id {
                let links = Pairing::local_only(
                    page_port,
                    port,
                    shared.host_name.clone(),
                    &shared.secret(),
                );
                open(&links.config_url());
            }
        }
    })
}

/// Hand a URL to the browser. A failure here is not worth interrupting anyone
/// over: the same pages are reachable from the console output at startup.
fn open(url: &str) {
    if let Err(e) = std::process::Command::new("open").arg(url).status() {
        tracing::warn!("could not open {url}: {e}");
    }
}
