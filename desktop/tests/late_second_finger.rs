//! A second finger that arrives after the cursor is already moving.
//!
//! On a MacBook you can change your mind mid-gesture. One finger slides, the
//! cursor follows, you lay a second finger down and the page scrolls - you do
//! not have to lift the hand and start again. PadRemote used to refuse that: a
//! finger that had drifted past `tapMaxPx` put the engine in `Moving`, and from
//! there `on_down` ignored every extra finger so the gesture "cannot mutate
//! mid-flight". The second finger did nothing at all and the cursor carried on.
//!
//! The rule now is: `Moving` alone re-opens. A `Drag` is holding a button down
//! and must not be taken apart in the middle of a selection, and `Scroll` and
//! `Zoom` already are the gesture the promotion would arrive at.
//!
//! Every finger is re-anchored where it sits when the promotion happens, which
//! is what these tests are mostly about - a finger that has travelled 120 px is
//! a long way from where it landed, and the two-finger handlers measure drift
//! from the landing point.

use padremote::gesture::{
    Button, Config, InputAction, Recognizer, ScrollPhase, Shortcut, TouchSample,
};

const W: f64 = 390.0;
const H: f64 = 716.0;

const DOWN: u8 = 0;
const MOVE: u8 = 1;
const UP: u8 = 2;

fn sample(t_ms: u32, id: u8, phase: u8, px: f64, py: f64) -> TouchSample {
    TouchSample {
        t_ms,
        pointer_id: id,
        phase,
        x: (px / W) as f32,
        y: (py / H) as f32,
    }
}

fn run(cfg: Config, samples: &[TouchSample]) -> Vec<InputAction> {
    let mut rec = Recognizer::new(cfg, W, H);
    let mut out = rec.feed(samples);
    out.extend(rec.release_all());
    out
}

fn scrolls(actions: &[InputAction]) -> Vec<(f64, f64)> {
    actions
        .iter()
        .filter_map(|a| match a {
            // The closing `end` and any coast carry no travel of their own.
            InputAction::Scroll {
                dx,
                dy,
                phase: ScrollPhase::Begin | ScrollPhase::Continue,
                ..
            } => Some((*dx, *dy)),
            _ => None,
        })
        .collect()
}

fn moves(actions: &[InputAction]) -> Vec<(f64, f64)> {
    actions
        .iter()
        .filter_map(|a| match a {
            InputAction::Move { dx, dy, .. } => Some((*dx, *dy)),
            _ => None,
        })
        .collect()
}

fn shortcuts(actions: &[InputAction]) -> Vec<Shortcut> {
    actions
        .iter()
        .filter_map(|a| match a {
            InputAction::Shortcut { shortcut, .. } => Some(*shortcut),
            _ => None,
        })
        .collect()
}

/// One finger travels far enough to be unmistakably a cursor move, then a
/// second lands and both drag down together.
fn move_then_second_finger(first: (f64, f64), together: (f64, f64)) -> Vec<TouchSample> {
    let (fx, fy) = first;
    let (tx, ty) = together;
    let mut s = vec![sample(0, 0, DOWN, 100.0, 300.0)];
    let mut t = 0;
    let (mut x, mut y) = (100.0, 300.0);

    // 15 steps of the first finger on its own: 120 px of travel, which no
    // threshold in the engine could mistake for a hand settling.
    for step in 1..=15 {
        t += 8;
        let f = step as f64 / 15.0;
        s.push(sample(t, 0, MOVE, 100.0 + fx * f, 300.0 + fy * f));
    }
    x += fx;
    y += fy;

    // The second finger lands beside it, well after the grouping window.
    t += 8;
    let (bx, by) = (x + 60.0, y);
    s.push(sample(t, 1, DOWN, bx, by));

    // Now both travel together.
    for step in 1..=15 {
        t += 8;
        let f = step as f64 / 15.0;
        s.push(sample(t, 0, MOVE, x + tx * f, y + ty * f));
        s.push(sample(t, 1, MOVE, bx + tx * f, by + ty * f));
    }
    s.push(sample(t + 8, 0, UP, x + tx, y + ty));
    s.push(sample(t + 8, 1, UP, bx + tx, by + ty));
    s
}

#[test]
fn a_finger_landing_mid_move_starts_a_scroll() {
    let actions = run(
        Config::default(),
        &move_then_second_finger((0.0, 120.0), (0.0, 150.0)),
    );
    let scrolled = scrolls(&actions);
    assert!(
        !scrolled.is_empty(),
        "a second finger during a move must scroll, it did nothing: {actions:?}"
    );
    assert!(
        scrolled.iter().all(|(dx, _)| dx.abs() <= 1.0),
        "a straight vertical scroll drifted sideways: {scrolled:?}"
    );
}

/// The cursor has to *stop*. Scrolling while the pointer still crawls along is
/// the same bug wearing a different hat.
#[test]
fn the_cursor_stops_once_the_second_finger_lands() {
    let samples = move_then_second_finger((0.0, 120.0), (0.0, 150.0));
    let mut rec = Recognizer::new(Config::default(), W, H);

    // Feed one sample at a time so "before" and "after" are separable.
    let landed = samples
        .iter()
        .position(|s| s.pointer_id == 1 && s.phase == DOWN)
        .expect("the second finger lands");
    let before: Vec<InputAction> = samples[..landed]
        .iter()
        .flat_map(|s| rec.feed(&[*s]))
        .collect();
    let after: Vec<InputAction> = samples[landed..]
        .iter()
        .flat_map(|s| rec.feed(&[*s]))
        .collect();

    assert!(
        !moves(&before).is_empty(),
        "the first finger should have been moving the cursor: {before:?}"
    );
    assert!(
        moves(&after).is_empty(),
        "the cursor kept moving after the second finger landed: {after:?}"
    );
}

/// The promoted gesture follows the fingers, not the momentum of the one that
/// was already travelling.
///
/// This is what the re-anchoring buys. Measured from where the first finger
/// *landed*, the pair is already 120 px to the right the instant the second one
/// arrives: a horizontal swipe, long past `swipe.minPx`, which on a host with
/// back/forward bound would fire before a single scroll ever came out.
#[test]
fn the_first_fingers_travel_does_not_decide_the_gesture() {
    let mut cfg = Config::default();
    cfg.bindings.two_finger_swipe_navigate = "navigate".into();

    let actions = run(cfg, &move_then_second_finger((120.0, 0.0), (0.0, 150.0)));
    assert_eq!(
        shortcuts(&actions),
        Vec::<Shortcut>::new(),
        "the first finger's sideways travel fired a navigation swipe: {actions:?}"
    );
    let scrolled = scrolls(&actions);
    assert!(
        !scrolled.is_empty() && scrolled.iter().all(|(dx, _)| dx.abs() <= 1.0),
        "the pair moved straight down; the scroll should too: {scrolled:?}"
    );
}

/// Re-anchoring resets `start_t` and `path_px`, which is exactly what `on_up`
/// reads to decide a tap. Both fingers therefore *look* tap-like when they
/// lift, and a move that ended in a scroll must not sign off with a right-click.
#[test]
fn a_move_then_a_quick_second_finger_is_not_a_right_click() {
    let mut s = vec![sample(0, 0, DOWN, 100.0, 300.0)];
    let mut t = 0;
    for step in 1..=15 {
        t += 8;
        s.push(sample(t, 0, MOVE, 100.0 + step as f64 * 8.0, 300.0));
    }
    // Second finger down and both away again inside `tapMaxMs`, having gone
    // nowhere since the promotion.
    t += 8;
    s.push(sample(t, 1, DOWN, 280.0, 300.0));
    t += 40;
    s.push(sample(t, 1, UP, 280.0, 300.0));
    s.push(sample(t, 0, UP, 220.0, 300.0));

    let actions = run(Config::default(), &s);
    assert!(
        !actions.iter().any(|a| matches!(
            a,
            InputAction::Click {
                button: Button::Right,
                ..
            }
        )),
        "an ordinary move ended in a stray right-click: {actions:?}"
    );
}

/// A drag is holding a physical button as far as every application is
/// concerned. A finger brushing the surface mid-selection must not let go of it.
#[test]
fn a_second_finger_never_interrupts_a_drag() {
    let mut cfg = Config::default();
    cfg.drag.press_and_drag = true;

    let mut s = vec![sample(0, 0, DOWN, 100.0, 300.0)];
    let mut t = 0;
    // Hold still past `pressMs`, which is what arms press-and-drag.
    for _ in 0..40 {
        t += 16;
        s.push(sample(t, 0, MOVE, 100.0, 300.0));
    }
    // Then drag.
    for step in 1..=10 {
        t += 8;
        s.push(sample(t, 0, MOVE, 100.0 + step as f64 * 8.0, 300.0));
    }
    // A second finger arrives half way through the selection.
    t += 8;
    s.push(sample(t, 1, DOWN, 250.0, 300.0));
    for step in 1..=10 {
        t += 8;
        s.push(sample(t, 0, MOVE, 180.0 + step as f64 * 8.0, 300.0));
        s.push(sample(t, 1, MOVE, 250.0 + step as f64 * 8.0, 300.0));
    }

    let mut rec = Recognizer::new(cfg, W, H);
    let during = rec.feed(&s);
    assert!(
        during.iter().any(|a| matches!(
            a,
            InputAction::ButtonDown {
                button: Button::Left,
                ..
            }
        )),
        "the press should have started a drag: {during:?}"
    );
    assert!(
        !during.iter().any(|a| matches!(a, InputAction::ButtonUp(_))),
        "a stray second finger released the button mid-drag: {during:?}"
    );
    // And the drag is still live: the button only comes up when asked.
    assert!(
        rec.release_all()
            .iter()
            .any(|a| matches!(a, InputAction::ButtonUp(Button::Left))),
        "the drag should still have been holding the button"
    );
}

/// Two fingers landing together is the path that always worked. It has to keep
/// working, and by the same route - the promotion must not have become the only
/// way to reach a scroll.
#[test]
fn fingers_landing_together_still_scroll() {
    let mut s = vec![
        sample(0, 0, DOWN, 100.0, 300.0),
        sample(4, 1, DOWN, 160.0, 300.0),
    ];
    let mut t = 4;
    for step in 1..=15 {
        t += 8;
        let d = step as f64 * 10.0;
        s.push(sample(t, 0, MOVE, 100.0, 300.0 + d));
        s.push(sample(t, 1, MOVE, 160.0, 300.0 + d));
    }
    let actions = run(Config::default(), &s);
    assert!(
        !scrolls(&actions).is_empty(),
        "two fingers landing together stopped scrolling: {actions:?}"
    );
}
