//! What `Recognizer::follow_up_ms` says, and why handover reads it.
//!
//! Idle is not the same as finished. The gap between the two halves of a
//! double-tap, and the pause between a tap and the drag it arms, are moments
//! with no finger down and a gesture very much in progress - so a second device
//! must not take the cursor there. Everything else is genuinely over the
//! instant the finger lifts.
//!
//! The desktop used to answer that with a flat 350 ms after *any* gesture,
//! which is right for a tap and pure delay for a scroll or a plain move. This
//! is the function that tells the two apart; `net/shared.rs::grace_for` is the
//! only caller, and `sessions.rs` covers what it does with the answer.

use padremote::gesture::{Config, Recognizer, TouchSample};

const W: f64 = 390.0;
const H: f64 = 716.0;

const DOWN: u8 = 0;
const MOVE: u8 = 1;
const UP: u8 = 2;

fn sample(t_ms: u32, phase: u8, px: f64, py: f64) -> TouchSample {
    TouchSample {
        t_ms,
        pointer_id: 0,
        phase,
        x: (px / W) as f32,
        y: (py / H) as f32,
    }
}

fn recognizer() -> Recognizer {
    Recognizer::new(Config::default(), W, H)
}

#[test]
fn a_fresh_recognizer_is_waiting_for_nothing() {
    assert_eq!(recognizer().follow_up_ms(), 0);
}

#[test]
fn a_tap_could_still_become_a_double_tap() {
    let mut rec = recognizer();
    rec.feed(&[sample(0, DOWN, 100.0, 100.0), sample(40, UP, 100.0, 100.0)]);
    assert!(
        rec.follow_up_ms() > 0,
        "a tap that has just landed can still gain a second half"
    );
    assert!(
        rec.is_idle(),
        "and it is idle while it waits - that is the trap"
    );
}

#[test]
fn the_window_runs_out() {
    let mut rec = recognizer();
    let window = Config::default().tap.double_tap_ms;
    rec.feed(&[sample(0, DOWN, 100.0, 100.0), sample(40, UP, 100.0, 100.0)]);

    // A later sample carries the phone's clock forward past the window. Feeding
    // a fresh touch is how the recognizer learns time has passed at all - it
    // has no clock of its own, deliberately.
    rec.feed(&[sample(40 + window + 1, DOWN, 100.0, 100.0)]);
    assert_eq!(
        rec.follow_up_ms(),
        0,
        "past the double-tap window there is nothing left to protect"
    );
}

#[test]
fn a_plain_move_arms_nothing() {
    let mut rec = recognizer();
    rec.feed(&[
        sample(0, DOWN, 100.0, 100.0),
        sample(16, MOVE, 160.0, 180.0),
        sample(32, MOVE, 220.0, 260.0),
        sample(48, UP, 220.0, 260.0),
    ]);
    assert_eq!(
        rec.follow_up_ms(),
        0,
        "a finger that travelled is not a tap, so no double-tap can follow it \
         and the cursor is free the moment it lifts"
    );
}

#[test]
fn a_two_finger_scroll_arms_nothing() {
    let mut rec = recognizer();
    rec.feed(&[
        sample(0, DOWN, 100.0, 300.0),
        TouchSample {
            t_ms: 0,
            pointer_id: 1,
            phase: DOWN,
            x: (160.0 / W) as f32,
            y: (300.0 / H) as f32,
        },
        sample(16, MOVE, 100.0, 200.0),
        TouchSample {
            t_ms: 16,
            pointer_id: 1,
            phase: MOVE,
            x: (160.0 / W) as f32,
            y: (200.0 / H) as f32,
        },
        sample(32, UP, 100.0, 200.0),
        TouchSample {
            t_ms: 32,
            pointer_id: 1,
            phase: UP,
            x: (160.0 / W) as f32,
            y: (200.0 / H) as f32,
        },
    ]);
    assert_eq!(
        rec.follow_up_ms(),
        0,
        "a scroll is over when the fingers leave"
    );
}

/// A phone that reconnects restarts `performance.now()`, so its timestamps can
/// land *before* the markers left by the previous session. That must read as
/// "expired", not as a full window - the alternative reserves the cursor for a
/// device that has just come back and touched nothing.
#[test]
fn a_clock_that_went_backwards_is_not_a_pending_gesture() {
    let mut rec = recognizer();
    rec.feed(&[
        sample(100_000, DOWN, 100.0, 100.0),
        sample(100_040, UP, 100.0, 100.0),
    ]);
    assert!(rec.follow_up_ms() > 0);

    rec.feed(&[sample(5, DOWN, 100.0, 100.0)]);
    assert_eq!(rec.follow_up_ms(), 0);
}
