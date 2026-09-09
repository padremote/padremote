//! Smoke test for the real macOS injection backend.
//!
//! Ignored by default: it moves the actual cursor and posts real scroll and
//! keyboard events, so it must never run as part of an unattended `cargo test`.
//! Run it deliberately:
//!
//! ```sh
//! cargo test --test injection -- --ignored --nocapture
//! ```
//!
//! Every effect it causes is undone before it returns.

#![cfg(target_os = "macos")]

use padremote::gesture::{Button, ScrollPhase, Shortcut};
use padremote::input::{accessibility_trusted, Injector, MacInjector};

#[test]
#[ignore = "moves the real cursor; run explicitly with --ignored"]
fn injection_backend_drives_the_real_cursor() {
    assert!(
        accessibility_trusted(),
        "Accessibility permission is required; see permission_help()"
    );

    let before = MacInjector::current_location().expect("cursor location");
    let mut inj = MacInjector::new().expect("injector");

    // Movement: the cursor must land where we asked, to the pixel.
    inj.move_by(60.0, 40.0);
    std::thread::sleep(std::time::Duration::from_millis(80));
    let after = MacInjector::current_location().expect("cursor location");
    let (dx, dy) = (after.0 - before.0, after.1 - before.1);
    assert!(
        (dx - 60.0).abs() < 2.0 && (dy - 40.0).abs() < 2.0,
        "expected the cursor to move by (60,40), it moved by ({dx:.1},{dy:.1})"
    );

    // Scroll and zoom: these have no observable return value, so this asserts
    // only that the CGEvent constructors accept our arguments and post without
    // panicking - the failure mode they previously had.
    // A complete, phased gesture - the shape a real trackpad sends.
    inj.scroll_by(0.0, 3.0, ScrollPhase::Begin);
    inj.scroll_by(0.0, -3.0, ScrollPhase::Continue);
    inj.scroll_by(0.0, 0.0, ScrollPhase::End);
    // Net-zero zoom, so a focused app is left exactly as it was found.
    inj.zoom(1);
    inj.zoom(-1);
    // Likewise net-zero: one space left, then straight back.
    inj.shortcut(Shortcut::SpaceLeft);
    inj.shortcut(Shortcut::SpaceRight);

    // A held button must survive release_all, not the test run.
    inj.button_down(Button::Left, 1);
    inj.release_all();

    // Put the cursor back where the user left it.
    inj.move_by(-dx, -dy);
    std::thread::sleep(std::time::Duration::from_millis(80));
    let restored = MacInjector::current_location().expect("cursor location");
    assert!(
        (restored.0 - before.0).abs() < 2.0 && (restored.1 - before.1).abs() < 2.0,
        "the test must leave the cursor where it found it"
    );
    println!("injection backend OK: move, scroll, zoom, button release all posted");
}

/// The cursor must carry on from where it is, not jump back to where we left it.
///
/// `MacInjector` posts mouse events at an absolute point, because that is what a
/// CGEvent mouse event carries - so it keeps its own idea of where the cursor
/// is, and nothing tells it when the user picks up the computer's own trackpad.
/// For a long time nothing re-read that idea either: `sync_from_system` existed,
/// said in its own comment that it was there "in case the user touched the real
/// trackpad", and was called from nowhere at all. Use the Mac's trackpad, then
/// touch the phone, and the first move teleported the cursor back to wherever
/// PadRemote had last driven it.
///
/// A second injector stands in for that hardware here: it seeds itself from the
/// real cursor, so moving it leaves the first one holding a stale position -
/// exactly the state a hand on the trackpad produces.
#[test]
#[ignore = "moves the real cursor; run explicitly with --ignored"]
fn a_synced_injector_moves_from_where_the_cursor_actually_is() {
    assert!(
        accessibility_trusted(),
        "Accessibility permission is required; see permission_help()"
    );

    let before = MacInjector::current_location().expect("cursor location");
    let mut phone = MacInjector::new().expect("injector");

    // The user picks up the real trackpad and moves the cursor 100 px right.
    // `phone` knows nothing about it and still believes `before`.
    MacInjector::new().expect("injector").move_by(100.0, 0.0);
    std::thread::sleep(std::time::Duration::from_millis(80));

    // Now they touch the phone, which is the moment `Shared::drive` syncs.
    phone.sync_cursor();
    phone.move_by(0.0, 30.0);
    std::thread::sleep(std::time::Duration::from_millis(80));

    let after = MacInjector::current_location().expect("cursor location");
    let (dx, dy) = (after.0 - before.0, after.1 - before.1);
    assert!(
        (dx - 100.0).abs() < 2.0 && (dy - 30.0).abs() < 2.0,
        "the cursor should have carried on from (+100,0) to (+100,+30); \
         it is at ({dx:.1},{dy:.1}) - a value near (0,30) is the jump back"
    );

    // Leave the cursor where the user left it.
    phone.move_by(-dx, -dy);
    std::thread::sleep(std::time::Duration::from_millis(80));
    let restored = MacInjector::current_location().expect("cursor location");
    assert!(
        (restored.0 - before.0).abs() < 2.0 && (restored.1 - before.1).abs() < 2.0,
        "the test must leave the cursor where it found it"
    );
    println!("sync OK: the cursor continued from where the trackpad left it");
}
