//! The background work that runs for the life of the process.
//!
//! Three loops that have nothing to do with any one phone: the clock that keeps
//! momentum scrolling alive, the watcher that notices the config or the host's
//! trackpad settings changing, and - on macOS - the one waiting for the
//! Accessibility grant to arrive. They live here rather than in `main` so that
//! startup reads as a sequence of decisions rather than as three nested loops.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

use tokio::runtime::Runtime;

use crate::gesture::Config;
use crate::net::Shared;
use crate::sysprefs::HostTrackpad;

/// The engine's heartbeat. One display frame: momentum scroll is integrated on
/// it, so anything slower is visibly steppy.
const TICK: Duration = Duration::from_millis(16);
/// How often the config file and the host's trackpad settings are re-read -
/// about twice a second (plan.md 9.9). Frequent enough to feel live while
/// editing, rare enough to cost nothing.
const CHECK_EVERY: Duration = Duration::from_millis(500);
/// How many config checks between address checks - about every five seconds.
///
/// Rarer than the rest because [`local_ip`] spawns a process to find out, and
/// because a router reassigning an address is a thing that happens on the order
/// of hours. Five seconds is well inside the time it takes anyone to notice the
/// QR is stale and reach for the menu.
const ADDRESS_EVERY: u32 = 10;

/// Momentum scroll, config hot-reload and trackpad mirroring.
///
/// Two tasks, not one, and that is the whole point of the split. Momentum is
/// integrated on a 16 ms clock; the config and the host's trackpad settings are
/// re-read twice a second, and reading them is not cheap - `HostTrackpad::read`
/// makes about twenty-five round trips to `cfprefsd`, `Config::load` touches
/// the disk, and [`local_ip`] spawns a process. Run on the same task, every one
/// of those pushed the next tick late, and a scroll coasting at the time
/// visibly stepped. Momentum now has a clock nobody else can hold up.
///
/// Both run whether or not anything is connected: a phone that drops mid-flick
/// must still have its momentum wound down, and a config edited while nobody is
/// connected must be live by the time somebody is.
pub fn spawn_maintenance(runtime: &Runtime, shared: Arc<Shared>, config_path: Option<PathBuf>) {
    let start = Instant::now();

    // The heartbeat. Nothing in here may block or await anything slow.
    let ticking = shared.clone();
    runtime.spawn(async move {
        let mut ticker = tokio::time::interval(TICK);
        loop {
            ticker.tick().await;
            ticking.tick(crate::now_ms(start));
        }
    });

    // The housekeeping, on its own slower clock.
    runtime.spawn(async move {
        let mut ticker = tokio::time::interval(CHECK_EVERY);
        let path = config_path.clone();
        let mut last_mtime = blocking(move || config_path_mtime(&path)).await;
        let mut last_host = blocking(HostTrackpad::read).await;
        let mut since_address = ADDRESS_EVERY; // look once, immediately.
        loop {
            ticker.tick().await;

            // The address this computer is reachable at can change under us -
            // a DHCP lease renewing, a laptop moving between networks - and
            // when it does, every QR and printed URL from before is wrong.
            // Nothing notices on its own, so it is checked here and published
            // for the menu bar to rebuild the pairing links from.
            since_address += 1;
            if since_address >= ADDRESS_EVERY {
                since_address = 0;
                shared.set_lan_ip(blocking(local_ip).await);
            }

            // Both reads go to a blocking thread. Neither is slow on a healthy
            // Mac, but `cfprefsd` and a process spawn are other people's code,
            // and a runtime worker stuck in one is a worker not reading
            // anybody's touches.
            let path = config_path.clone();
            let (m, host) = blocking(move || (config_path_mtime(&path), HostTrackpad::read())).await;

            let file_changed = m.is_some() && m != last_mtime;
            if file_changed {
                last_mtime = m;
            }
            let host_changed = host != last_host;
            if !file_changed && !host_changed {
                continue;
            }

            let path = config_path.clone();
            let file_cfg = blocking(move || match path.as_deref() {
                Some(path) => Config::load(path),
                None => Config::default(),
            })
            .await;
            let cfg = host.apply_to(&file_cfg);
            if host_changed {
                tracing::info!("system trackpad changed: {}", host.summary());
            } else {
                tracing::info!(
                    "config reloaded: sensitivity={} natural={} accel_gain={}",
                    cfg.sensitivity,
                    cfg.scroll.natural,
                    cfg.accel.gain
                );
            }
            last_host = host;
            // Every connected device is re-tuned, not just the next one to
            // arrive: an edit is meant to be felt on the phone in your hand.
            shared.set_config(cfg);
        }
    });
}

/// Run something blocking off the runtime's worker threads.
///
/// A thin wrapper so the call sites above read as the sequence of decisions
/// they are. If the pool cannot run it - only at shutdown - fall back to
/// running it here rather than losing the reading entirely.
async fn blocking<T, F>(f: F) -> T
where
    F: FnOnce() -> T + Send + 'static,
    T: Send + 'static,
{
    match tokio::task::spawn_blocking(f).await {
        Ok(v) => v,
        Err(e) => match e.try_into_panic() {
            Ok(p) => std::panic::resume_unwind(p),
            Err(_) => unreachable!("spawn_blocking is only cancelled at shutdown"),
        },
    }
}

fn config_path_mtime(path: &Option<PathBuf>) -> Option<SystemTime> {
    path.as_deref().and_then(mtime)
}

/// Watch for the Accessibility grant arriving, and go live the moment it does.
///
/// Ticking the box in System Settings does not restart us, and telling the user
/// to quit and relaunch for a permission they just granted is a poor way to be
/// met. Stops as soon as the backend is swapped in.
#[cfg(target_os = "macos")]
pub fn spawn_permission_watch(runtime: &Runtime, shared: Arc<Shared>) {
    runtime.spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(1)).await;
            if !crate::input::accessibility_trusted() {
                continue;
            }
            match crate::input::PlatformInjector::new() {
                Ok(inj) => {
                    shared.set_injector(Box::new(inj));
                    tracing::info!("Accessibility granted - input injection is live");
                }
                Err(e) => tracing::error!("Accessibility granted but the backend failed: {e}"),
            }
            return;
        }
    });
}

/// The LAN address the phone should use, for the console hint and the QR.
///
/// `ipconfig getifaddr` rather than enumerating interfaces ourselves: it
/// answers with the address macOS would actually route from, which is the one
/// the phone has to reach. `en0` is Wi-Fi on every Mac; `en1` covers the wired
/// adapter on the machines where it is not.
pub fn local_ip() -> Option<String> {
    for iface in ["en0", "en1"] {
        // `continue`, not `?`: a failure to run `ipconfig` for one interface
        // says nothing about the next one, and returning here meant a machine
        // where the first probe failed reported "no network" while sitting on
        // a working one.
        let Ok(out) = std::process::Command::new("ipconfig")
            .arg("getifaddr")
            .arg(iface)
            .output()
        else {
            continue;
        };
        let ip = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if !ip.is_empty() {
            return Some(ip);
        }
    }
    None
}

fn mtime(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).ok()?.modified().ok()
}
