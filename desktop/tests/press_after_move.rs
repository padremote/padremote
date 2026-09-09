//! A hold only counts from a finger that has not yet moved.
//!
//! Press-and-drag stands in for holding a physical trackpad button, so the
//! hold has to mean one specific thing: *this* touch, from the moment it
//! landed, was deliberate and still. A finger already steering the cursor that
//! happens to pause is not making that gesture - it is a hand resting. Letting
//! the pause count would put the button down in the middle of an ordinary move,
//! which is how a stray selection gets dragged across a document.
//!
//! The rule is therefore: move, and the hold is spent for that touch. Lift and
//! press again to drag.

use padremote::gesture::{Button, Config, InputAction, Recognizer, TouchSample};

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

fn run(samples: &[TouchSample]) -> Vec<InputAction> {
    let mut rec = Recognizer::new(Config::default(), W, H);
    let mut out = rec.feed(samples);
    out.extend(rec.release_all());
    out
}

fn pressed(actions: &[InputAction]) -> bool {
    actions.iter().any(|a| {
        matches!(
            a,
            InputAction::ButtonDown {
                button: Button::Left,
                ..
            }
        )
    })
}

/// One finger down, moved well past the tap threshold, then held perfectly
/// still for four times `pressMs`. The button must stay up.
#[test]
fn a_pause_while_moving_never_presses_the_button() {
    let mut s = vec![sample(0, DOWN, 100.0, 400.0)];
    let mut t = 0;
    // Travel 120 px - unambiguously a cursor move, not a jittering rest.
    for step in 1..=15 {
        t += 8;
        s.push(sample(t, MOVE, 100.0 + step as f64 * 8.0, 400.0));
    }
    // Now stop dead, and stay stopped for two full seconds.
    let (rest_x, rest_y) = (220.0, 400.0);
    for _ in 0..125 {
        t += 16;
        s.push(sample(t, MOVE, rest_x, rest_y));
    }
    s.push(sample(t + 16, UP, rest_x, rest_y));

    let actions = run(&s);
    assert!(
        !pressed(&actions),
        "a finger that has already moved must not arm a drag by pausing: {actions:?}"
    );
    assert!(
        actions
            .iter()
            .any(|a| matches!(a, InputAction::Move { .. })),
        "the move itself should still have driven the cursor"
    );
}

/// The same pause, but from a finger that never moved first, is the real
/// gesture and must still work. This is the control - without it the test
/// above would pass just as well if press-and-drag were broken outright.
#[test]
fn a_pause_from_a_still_finger_is_still_a_drag() {
    let mut s = vec![sample(0, DOWN, 200.0, 400.0)];
    let mut t = 0;
    // Rest for well over pressMs, with only the jitter a real hand has.
    for i in 0..60 {
        t += 16;
        let wobble = if i % 2 == 0 { 0.4 } else { -0.4 };
        s.push(sample(t, MOVE, 200.0 + wobble, 400.0));
    }
    // Then move, which is what a press-and-drag does next.
    for step in 1..=10 {
        t += 8;
        s.push(sample(t, MOVE, 200.0 + step as f64 * 6.0, 400.0));
    }
    s.push(sample(t + 8, UP, 260.0, 400.0));

    let actions = run(&s);
    assert!(
        pressed(&actions),
        "press-and-drag from a still finger must still arm: {actions:?}"
    );
}

/// Lifting clears the disqualification: move, let go, then press and hold.
#[test]
fn lifting_and_pressing_again_drags() {
    let mut s = vec![sample(0, DOWN, 100.0, 400.0)];
    let mut t = 0;
    for step in 1..=15 {
        t += 8;
        s.push(sample(t, MOVE, 100.0 + step as f64 * 8.0, 400.0));
    }
    s.push(sample(t + 8, UP, 220.0, 400.0));
    t += 8;

    // A clear gap, then a fresh press-and-hold that does not move first.
    t += 800;
    s.push(sample(t, DOWN, 220.0, 400.0));
    for i in 0..60 {
        t += 16;
        let wobble = if i % 2 == 0 { 0.4 } else { -0.4 };
        s.push(sample(t, MOVE, 220.0 + wobble, 400.0));
    }
    for step in 1..=10 {
        t += 8;
        s.push(sample(t, MOVE, 220.0 + step as f64 * 6.0, 400.0));
    }
    s.push(sample(t + 8, UP, 280.0, 400.0));

    let actions = run(&s);
    assert!(
        pressed(&actions),
        "lifting and pressing again must be able to drag: {actions:?}"
    );
}
