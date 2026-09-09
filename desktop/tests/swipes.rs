//! Multi-finger swipes.
//!
//! These gestures arrived after the Python prototype was retired, so unlike the
//! original eleven they are driven by streams built here rather than by recorded
//! fixtures. The streams are still exact and reproducible.

use padremote::gesture::{Button, Config, InputAction, Recognizer, Shortcut, TouchSample};

const W: f64 = 390.0;
const H: f64 = 716.0;
const DT: u32 = 8;

const DOWN: u8 = 0;
const MOVE: u8 = 1;
const UP: u8 = 2;

/// Several fingers landing together and travelling in the same direction.
fn swipe(fingers: usize, dx_px: f64, dy_px: f64, steps: u32) -> Vec<TouchSample> {
    let mut out = Vec::new();
    let mut t = 0;
    // Start spread across the surface, as real fingers would be.
    let start: Vec<(f64, f64)> = (0..fingers)
        .map(|i| (80.0 + i as f64 * 60.0, 400.0))
        .collect();

    for (i, (x, y)) in start.iter().enumerate() {
        out.push(sample(t, i as u8, DOWN, *x, *y));
        t += 10; // inside the 60 ms grouping window
    }
    for step in 1..=steps {
        t += DT;
        let f = step as f64 / steps as f64;
        for (i, (x, y)) in start.iter().enumerate() {
            out.push(sample(t, i as u8, MOVE, x + dx_px * f, y + dy_px * f));
        }
    }
    for (i, (x, y)) in start.iter().enumerate() {
        out.push(sample(t, i as u8, UP, x + dx_px, y + dy_px));
    }
    out
}

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

/// A config shaped like a Mac with four-finger swipes turned on.
fn swipes_on() -> Config {
    let mut cfg = Config::default();
    cfg.bindings.four_finger_horiz_swipe = "spaces".into();
    cfg.bindings.four_finger_vert_swipe = "missionControl".into();
    cfg
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

#[test]
fn four_finger_swipe_switches_spaces() {
    // Natural scrolling: fingers travelling right reveal the space to the left.
    let left = run(swipes_on(), &swipe(4, 140.0, 0.0, 20));
    assert_eq!(shortcuts(&left), vec![Shortcut::SpaceLeft], "got {left:?}");

    let right = run(swipes_on(), &swipe(4, -140.0, 0.0, 20));
    assert_eq!(
        shortcuts(&right),
        vec![Shortcut::SpaceRight],
        "got {right:?}"
    );
}

#[test]
fn scroll_direction_flips_the_swipe() {
    let mut cfg = swipes_on();
    cfg.scroll.natural = false;
    let actions = run(cfg, &swipe(4, 140.0, 0.0, 20));
    assert_eq!(
        shortcuts(&actions),
        vec![Shortcut::SpaceRight],
        "with natural scrolling off the direction must invert"
    );
}

#[test]
fn four_finger_vertical_opens_mission_control() {
    // Surface y grows downward, so travelling up is a negative delta.
    let up = run(swipes_on(), &swipe(4, 0.0, -140.0, 20));
    assert_eq!(shortcuts(&up), vec![Shortcut::MissionControl], "got {up:?}");

    let down = run(swipes_on(), &swipe(4, 0.0, 140.0, 20));
    assert_eq!(shortcuts(&down), vec![Shortcut::AppWindows], "got {down:?}");
}

#[test]
fn a_swipe_fires_exactly_once() {
    // A long, continuing swipe must not repeat the shortcut every frame.
    let actions = run(swipes_on(), &swipe(4, 300.0, 0.0, 60));
    assert_eq!(
        shortcuts(&actions).len(),
        1,
        "one swipe, one action: {actions:?}"
    );
}

#[test]
fn a_short_swipe_is_ignored() {
    // Below the threshold this is just fingers resting, not a gesture.
    let actions = run(swipes_on(), &swipe(4, 20.0, 0.0, 20));
    assert!(shortcuts(&actions).is_empty(), "got {actions:?}");
}

#[test]
fn a_diagonal_smear_is_ignored() {
    // Neither axis clearly wins, so neither should fire.
    let actions = run(swipes_on(), &swipe(4, 120.0, 120.0, 20));
    assert!(
        shortcuts(&actions).is_empty(),
        "ambiguous direction must not fire: {actions:?}"
    );
}

#[test]
fn swipes_stay_off_when_unbound() {
    // The shipped defaults bind nothing, so nothing may happen.
    let actions = run(Config::default(), &swipe(4, 140.0, 0.0, 20));
    assert!(shortcuts(&actions).is_empty(), "got {actions:?}");
}

#[test]
fn three_fingers_swipe() {
    let mut cfg = Config::default();
    cfg.bindings.three_finger_horiz_swipe = "spaces".into();
    let actions = run(cfg, &swipe(3, 140.0, 0.0, 20));
    assert_eq!(
        shortcuts(&actions),
        vec![Shortcut::SpaceLeft],
        "got {actions:?}"
    );
}

/// Whatever happens, a swipe must never leave the mouse button down.
#[test]
fn no_gesture_leaves_a_button_held() {
    for fingers in 3..=4 {
        for cfg in [Config::default(), swipes_on()] {
            let mut rec = Recognizer::new(cfg, W, H);
            let mut actions = rec.feed(&swipe(fingers, 140.0, 0.0, 20));
            actions.extend(rec.release_all());
            let downs = actions
                .iter()
                .filter(|a| matches!(a, InputAction::ButtonDown { .. }))
                .count();
            let ups = actions
                .iter()
                .filter(|a| matches!(a, InputAction::ButtonUp(_)))
                .count();
            assert_eq!(
                downs, ups,
                "{fingers} fingers left a button held: {actions:?}"
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Gestures mirrored from the host's trackpad settings.
// ---------------------------------------------------------------------------

/// Several fingers landing together and moving toward or away from their centre.
fn pinch(fingers: usize, scale: f64, steps: u32) -> Vec<TouchSample> {
    let mut out = Vec::new();
    let mut t = 0;
    let cx = 195.0;
    let cy = 380.0;
    let radius = 120.0;
    let angle = |i: usize| i as f64 * std::f64::consts::TAU / fingers as f64;
    let at = |i: usize, r: f64| (cx + r * angle(i).cos(), cy + r * angle(i).sin());

    for i in 0..fingers {
        let (x, y) = at(i, radius);
        out.push(sample(t, i as u8, DOWN, x, y));
        t += 8;
    }
    for step in 1..=steps {
        t += DT;
        let f = step as f64 / steps as f64;
        let r = radius * (1.0 + (scale - 1.0) * f);
        for i in 0..fingers {
            let (x, y) = at(i, r);
            out.push(sample(t, i as u8, MOVE, x, y));
        }
    }
    for i in 0..fingers {
        let (x, y) = at(i, radius * scale);
        out.push(sample(t, i as u8, UP, x, y));
    }
    out
}

/// A tap with `fingers` fingers, landing and lifting together.
fn multi_tap(fingers: usize, start_t: u32) -> Vec<TouchSample> {
    let mut out = Vec::new();
    let mut t = start_t;
    for i in 0..fingers {
        out.push(sample(t, i as u8, DOWN, 150.0 + i as f64 * 60.0, 400.0));
        t += 8;
    }
    t += 60;
    for i in 0..fingers {
        out.push(sample(t, i as u8, UP, 150.0 + i as f64 * 60.0, 400.0));
    }
    out
}

fn mac_like() -> Config {
    // What the mirroring produces from a Mac with the usual gestures on.
    let mut cfg = Config::default();
    cfg.bindings.four_finger_pinch = "launchpad".into();
    cfg.bindings.five_finger_spread = "showDesktop".into();
    cfg.bindings.two_finger_double_tap = "smartZoom".into();
    cfg
}

#[test]
fn four_finger_pinch_opens_launchpad() {
    let actions = run(mac_like(), &pinch(4, 0.45, 20));
    assert_eq!(
        shortcuts(&actions),
        vec![Shortcut::Launchpad],
        "got {actions:?}"
    );
}

#[test]
fn five_finger_spread_shows_the_desktop() {
    let actions = run(mac_like(), &pinch(5, 1.9, 20));
    assert_eq!(
        shortcuts(&actions),
        vec![Shortcut::ShowDesktop],
        "got {actions:?}"
    );
}

#[test]
fn a_pinch_fires_exactly_once() {
    let actions = run(mac_like(), &pinch(4, 0.3, 60));
    assert_eq!(
        shortcuts(&actions).len(),
        1,
        "one pinch, one action: {actions:?}"
    );
}

#[test]
fn pinches_stay_off_when_unbound() {
    let actions = run(Config::default(), &pinch(4, 0.45, 20));
    assert!(shortcuts(&actions).is_empty(), "got {actions:?}");
}

/// Four fingers tapped together.
///
/// PadRemote's own gesture: macOS has no four-finger tap, so nothing is copied
/// and it ships unbound. It reaches the OS through the same path as the one-,
/// two- and three-finger taps, judged on the peak finger count once the last
/// finger lifts.
#[test]
fn four_fingers_tapped_fire_what_they_are_bound_to() {
    let mut cfg = mac_like();
    cfg.bindings.four_finger_tap = "missionControl".into();
    let actions = run(cfg, &multi_tap(4, 0));
    assert_eq!(shortcuts(&actions), vec![Shortcut::MissionControl]);
}

#[test]
fn four_fingers_tapped_do_nothing_until_they_are_bound() {
    // The shipped default, and the reason nobody's Mac behaves differently the
    // day this gesture arrives.
    let actions = run(mac_like(), &multi_tap(4, 0));
    assert!(
        actions.is_empty(),
        "an unbound four-finger tap did something: {actions:?}"
    );
}

/// The gesture is useless if it only works when four fingers land at once, and
/// four fingers never land at once on a phone.
///
/// The spread between index and little finger routinely runs to 150 ms - the
/// same fact `GROUP_WINDOW_MS` is generous for. Timing each finger's tap from
/// its own landing charged the first finger for that whole wait, so a hand that
/// touched down raggedly blew the 200 ms `tapMaxMs` budget before anyone had
/// lifted anything, and the tap silently did nothing. Timing from the moment
/// the hand is complete is what makes it land every time.
#[test]
fn a_ragged_four_finger_tap_still_counts() {
    let mut cfg = mac_like();
    cfg.bindings.four_finger_tap = "missionControl".into();

    for spread in [0, 60, 120, 150] {
        let mut s = Vec::new();
        let step = spread / 3;
        for i in 0..4u8 {
            s.push(sample(
                i as u32 * step,
                i,
                DOWN,
                80.0 + i as f64 * 60.0,
                400.0,
            ));
        }
        // A tap that dwells 120 ms, which is an ordinary one, not a fast one.
        let up = spread + 120;
        for i in 0..4u8 {
            s.push(sample(
                up + i as u32 * 5,
                i,
                UP,
                80.0 + i as f64 * 60.0,
                400.0,
            ));
        }
        assert_eq!(
            shortcuts(&run(cfg.clone(), &s)),
            vec![Shortcut::MissionControl],
            "four fingers landing {spread} ms apart did not tap",
        );
    }
}

/// Forgiving the landing spread must not forgive an actual hold.
#[test]
fn four_fingers_held_down_are_not_a_tap() {
    let mut cfg = mac_like();
    cfg.bindings.four_finger_tap = "missionControl".into();

    let mut s = Vec::new();
    for i in 0..4u8 {
        s.push(sample(
            i as u32 * 20,
            i,
            DOWN,
            80.0 + i as f64 * 60.0,
            400.0,
        ));
    }
    // Every finger down, and the hand stays there for most of a second.
    for i in 0..4u8 {
        s.push(sample(
            900 + i as u32 * 5,
            i,
            UP,
            80.0 + i as f64 * 60.0,
            400.0,
        ));
    }
    assert!(
        shortcuts(&run(cfg, &s)).is_empty(),
        "a deliberate four-finger hold was read as a tap",
    );
}

/// A four-finger tap must not be mistaken for the swipe or the pinch that share
/// its finger count.
#[test]
fn four_fingers_that_travel_swipe_rather_than_tap() {
    let mut cfg = swipes_on();
    cfg.bindings.four_finger_tap = "missionControl".into();

    let fired = shortcuts(&run(cfg, &swipe(4, 0.0, -140.0, 10)));
    assert_eq!(
        fired,
        vec![Shortcut::MissionControl],
        "a four-finger swipe up should be Mission Control from the swipe, once",
    );
}

#[test]
fn two_finger_double_tap_is_smart_zoom() {
    let mut samples = multi_tap(2, 0);
    samples.extend(multi_tap(2, 200)); // inside doubleTapMs
    let actions = run(mac_like(), &samples);
    assert_eq!(
        shortcuts(&actions),
        vec![Shortcut::SmartZoom],
        "the second two-finger tap must zoom, not right-click again: {actions:?}"
    );
}

#[test]
fn a_single_two_finger_tap_is_still_a_right_click() {
    let actions = run(mac_like(), &multi_tap(2, 0));
    assert!(
        shortcuts(&actions).is_empty(),
        "one tap is not a zoom: {actions:?}"
    );
    assert!(
        matches!(
            actions[..],
            [InputAction::Click {
                button: Button::Right,
                ..
            }]
        ),
        "got {actions:?}"
    );
}

#[test]
fn two_finger_swipe_navigates_when_the_host_enables_it() {
    let mut cfg = Config::default();
    cfg.bindings.two_finger_swipe_navigate = "navigate".into();
    // Two fingers travelling right, holding their spacing so it is not a pinch.
    let mut samples = Vec::new();
    let mut t = 0;
    samples.push(sample(t, 0, DOWN, 120.0, 400.0));
    t += 10;
    samples.push(sample(t, 1, DOWN, 200.0, 400.0));
    for step in 1..=20 {
        t += DT;
        let dx = 140.0 * step as f64 / 20.0;
        samples.push(sample(t, 0, MOVE, 120.0 + dx, 400.0));
        samples.push(sample(t, 1, MOVE, 200.0 + dx, 400.0));
    }
    samples.push(sample(t, 0, UP, 260.0, 400.0));
    samples.push(sample(t, 1, UP, 340.0, 400.0));

    let actions = run(cfg, &samples);
    assert_eq!(shortcuts(&actions), vec![Shortcut::Back], "got {actions:?}");
    assert!(
        !actions.iter().any(|a| a.kind() == "scroll"),
        "a navigation swipe must not also scroll: {actions:?}"
    );
}

#[test]
fn scrolling_can_be_switched_off_by_the_host() {
    let mut cfg = Config::default();
    cfg.scroll.enabled = false;
    let mut samples = Vec::new();
    let mut t = 0;
    samples.push(sample(t, 0, DOWN, 150.0, 500.0));
    t += 10;
    samples.push(sample(t, 1, DOWN, 230.0, 500.0));
    for step in 1..=20 {
        t += DT;
        let dy = -120.0 * step as f64 / 20.0;
        samples.push(sample(t, 0, MOVE, 150.0, 500.0 + dy));
        samples.push(sample(t, 1, MOVE, 230.0, 500.0 + dy));
    }
    samples.push(sample(t, 0, UP, 150.0, 380.0));
    samples.push(sample(t, 1, UP, 230.0, 380.0));

    let actions = run(cfg, &samples);
    assert!(
        !actions.iter().any(|a| a.kind() == "scroll"),
        "the host turned scrolling off: {actions:?}"
    );
}

/// Sideways two-finger scrolling must not be mistaken for a pinch.
///
/// Fingers report one at a time, so mid-gesture the spacing between them
/// changes even though neither is pinching. Judging a pinch on distance alone
/// turned every horizontal scroll into a zoom; a pinch also requires the fingers
/// to be moving in opposing directions.
#[test]
fn horizontal_two_finger_scroll_is_not_a_zoom() {
    let mut samples = Vec::new();
    let mut t = 0;
    samples.push(sample(t, 0, DOWN, 120.0, 400.0));
    t += 10;
    samples.push(sample(t, 1, DOWN, 200.0, 400.0));
    for step in 1..=20 {
        t += DT;
        let dx = 140.0 * step as f64 / 20.0;
        // One finger at a time, exactly as a real digitiser reports.
        samples.push(sample(t, 0, MOVE, 120.0 + dx, 400.0));
        samples.push(sample(t, 1, MOVE, 200.0 + dx, 400.0));
    }
    samples.push(sample(t, 0, UP, 260.0, 400.0));
    samples.push(sample(t, 1, UP, 340.0, 400.0));

    let actions = run(Config::default(), &samples);
    let kinds: Vec<&str> = actions.iter().map(|a| a.kind()).collect();
    assert!(
        !kinds.contains(&"zoom"),
        "a sideways scroll must not zoom: {actions:?}"
    );
    assert!(kinds.contains(&"scroll"), "it should scroll: {actions:?}");
}

/// And a real pinch still zooms.
#[test]
fn opposing_fingers_still_zoom() {
    // Zoom ships off, because a pinch and a two-finger swipe are the same two
    // fingers moving on a surface this size. The recognizer still implements
    // it, and this is what keeps that true for anyone who turns it back on.
    let mut cfg = Config::default();
    cfg.zoom.enabled = true;
    let actions = run(cfg, &pinch(2, 2.2, 20));
    let kinds: Vec<&str> = actions.iter().map(|a| a.kind()).collect();
    assert!(
        kinds.contains(&"zoom"),
        "fingers moving apart must zoom: {actions:?}"
    );
}

/// With the host's dragging style set to Three-Finger Drag, a one-finger move
/// must move the cursor — never hold the button down.
///
/// This is the failure a user actually feels: the finger pauses for a moment
/// (easy on a phone), press-and-drag fires, and from then on the "cursor" is
/// dragging a selection instead of moving.
#[test]
fn one_finger_never_drags_when_the_host_uses_three_finger_drag() {
    let mut cfg = Config::default();
    // What the mirroring produces from a Mac with Three-Finger Drag selected:
    // the Mac has no one-finger dragging, so neither has this.
    cfg.drag.tap_and_drag = false;
    cfg.drag.press_and_drag = false;

    // A finger that rests a while, then travels.
    let mut samples = vec![sample(0, 0, DOWN, 150.0, 400.0)];
    let mut t = 0;
    for _ in 0..80 {
        t += DT; // well past pressMs
        samples.push(sample(t, 0, MOVE, 150.2, 400.1));
    }
    for step in 1..=20 {
        t += DT;
        samples.push(sample(t, 0, MOVE, 150.0 + step as f64 * 6.0, 400.0));
    }
    samples.push(sample(t, 0, UP, 270.0, 400.0));

    let actions = run(cfg, &samples);
    let kinds: Vec<&str> = actions.iter().map(|a| a.kind()).collect();
    assert!(kinds.contains(&"move"), "the cursor must move: {actions:?}");
    assert!(
        !kinds.contains(&"button_down"),
        "one finger must not start a drag when the host drags with three: {actions:?}"
    );
}

/// And a tap immediately followed by a move is still just a move.
#[test]
fn a_move_right_after_a_tap_is_not_a_drag() {
    let mut cfg = Config::default();
    cfg.drag.tap_and_drag = false;
    cfg.drag.press_and_drag = false;

    let mut samples = vec![
        sample(0, 0, DOWN, 150.0, 400.0),
        sample(70, 0, UP, 150.0, 400.0),
    ];
    // Down again well inside doubleTapMs, then travel.
    let mut t = 160;
    samples.push(sample(t, 0, DOWN, 150.0, 400.0));
    for step in 1..=20 {
        t += DT;
        samples.push(sample(t, 0, MOVE, 150.0 + step as f64 * 6.0, 400.0));
    }
    samples.push(sample(t, 0, UP, 270.0, 400.0));

    let actions = run(cfg, &samples);
    let kinds: Vec<&str> = actions.iter().map(|a| a.kind()).collect();
    assert!(kinds.contains(&"move"), "{actions:?}");
    assert!(
        !kinds.contains(&"button_down"),
        "tap-then-move must not drag: {actions:?}"
    );
}

/// When the host *does* use a one-finger style, dragging still works.
#[test]
fn one_finger_drag_still_works_when_the_host_enables_it() {
    let mut cfg = Config::default();
    cfg.drag.press_and_drag = true;

    let mut samples = vec![sample(0, 0, DOWN, 150.0, 400.0)];
    let mut t = 0;
    // pressMs is 500 ms: the hold has to outlast it.
    for _ in 0..80 {
        t += DT;
        samples.push(sample(t, 0, MOVE, 150.2, 400.1));
    }
    for step in 1..=12 {
        t += DT;
        samples.push(sample(t, 0, MOVE, 150.0 + step as f64 * 6.0, 400.0));
    }
    samples.push(sample(t, 0, UP, 222.0, 400.0));

    let actions = run(cfg, &samples);
    let kinds: Vec<&str> = actions.iter().map(|a| a.kind()).collect();
    assert!(
        kinds.contains(&"button_down"),
        "press-and-drag was enabled: {actions:?}"
    );
    assert_eq!(
        kinds.last(),
        Some(&"button_up"),
        "and must release: {kinds:?}"
    );
}

/// Four fingers that land raggedly must still be a four-finger swipe.
///
/// On a phone the little finger routinely lands 80-150 ms after the index. A
/// tight grouping window demoted those gestures to three fingers, and on a Mac
/// that binds only the four-finger swipes, three fingers mean nothing - so the
/// gesture silently did nothing.
fn ragged_swipe(fingers: usize, spread_ms: u32, dx: f64, dy: f64) -> Vec<TouchSample> {
    let mut out = Vec::new();
    let start: Vec<(f64, f64)> = (0..fingers)
        .map(|i| (70.0 + i as f64 * 62.0, 400.0))
        .collect();
    let step_ms = spread_ms / fingers.max(1) as u32;

    let mut t = 0;
    for (i, (x, y)) in start.iter().enumerate() {
        out.push(sample(t, i as u8, DOWN, *x, *y));
        t += step_ms;
    }
    for step in 1..=20 {
        t += DT;
        let f = step as f64 / 20.0;
        for (i, (x, y)) in start.iter().enumerate() {
            out.push(sample(t, i as u8, MOVE, x + dx * f, y + dy * f));
        }
    }
    for (i, (x, y)) in start.iter().enumerate() {
        out.push(sample(t, i as u8, UP, x + dx, y + dy));
    }
    out
}

#[test]
fn four_fingers_landing_raggedly_still_open_mission_control() {
    // 150 ms between the first finger and the last - an ordinary human hand.
    let actions = run(swipes_on(), &ragged_swipe(4, 150, 0.0, -140.0));
    assert_eq!(
        shortcuts(&actions),
        vec![Shortcut::MissionControl],
        "a slowly-landing four-finger swipe must not be demoted: {actions:?}"
    );
}

#[test]
fn a_very_slow_hand_is_still_one_gesture() {
    // Even beyond the window: nothing has moved yet, so the hand is still
    // settling and these are one gesture, not two.
    let actions = run(swipes_on(), &ragged_swipe(4, 400, 140.0, 0.0));
    assert_eq!(
        shortcuts(&actions),
        vec![Shortcut::SpaceLeft],
        "got {actions:?}"
    );
}

/// A finger lifting early must not demote the gesture either.
#[test]
fn a_finger_lifting_early_does_not_demote_the_swipe() {
    let mut samples = Vec::new();
    let start: Vec<(f64, f64)> = (0..4).map(|i| (70.0 + i as f64 * 62.0, 400.0)).collect();
    let mut t = 0;
    for (i, (x, y)) in start.iter().enumerate() {
        samples.push(sample(t, i as u8, DOWN, *x, *y));
        t += 12;
    }
    for step in 1..=20 {
        t += DT;
        let f = step as f64 / 20.0;
        // The little finger leaves a third of the way through.
        let live = if step > 7 { 3 } else { 4 };
        for (i, (x, y)) in start.iter().take(live).enumerate() {
            samples.push(sample(t, i as u8, MOVE, x + 140.0 * f, *y));
        }
        if step == 7 {
            samples.push(sample(t, 3, UP, start[3].0, start[3].1));
        }
    }
    for (i, (x, y)) in start.iter().take(3).enumerate() {
        samples.push(sample(t, i as u8, UP, x + 140.0, *y));
    }

    let actions = run(swipes_on(), &samples);
    assert_eq!(
        shortcuts(&actions),
        vec![Shortcut::SpaceLeft],
        "the peak finger count decides, not whoever is still touching: {actions:?}"
    );
}

/// Three fingers must still be three fingers.
#[test]
fn three_fingers_are_not_promoted_to_four() {
    let mut cfg = swipes_on();
    cfg.bindings.three_finger_horiz_swipe = "spaces".into();
    cfg.bindings.four_finger_horiz_swipe = "none".into();
    let actions = run(cfg, &ragged_swipe(3, 100, 140.0, 0.0));
    assert_eq!(
        shortcuts(&actions),
        vec![Shortcut::SpaceLeft],
        "got {actions:?}"
    );
}

/// A four-finger swipe up, made by a real hand rather than a rigid template.
///
/// Fingers do not travel identically: the index leads, the little finger lags,
/// and the hand curls, so the spacing between fingers changes a lot during an
/// ordinary vertical swipe. Judging a pinch on spacing alone read that as a
/// pinch and fired Launchpad — which is what made vertical swipes feel random
/// while horizontal ones, along the axis the fingers are arranged on, were fine.
fn splayed_swipe(fingers: usize, dx: f64, dy: f64, splay: f64) -> Vec<TouchSample> {
    let mut out = Vec::new();
    let start: Vec<(f64, f64)> = (0..fingers)
        .map(|i| (70.0 + i as f64 * 62.0, 470.0))
        .collect();
    let mut t = 0;
    for (i, (x, y)) in start.iter().enumerate() {
        out.push(sample(t, i as u8, DOWN, *x, *y));
        t += 14;
    }
    for step in 1..=20 {
        t += DT;
        let f = step as f64 / 20.0;
        for (i, (x, y)) in start.iter().enumerate() {
            // Each finger travels a slightly different distance, and they drift
            // together — the hand closing as it moves.
            let lag = 1.0 - (i as f64 * 0.10);
            let pull = (i as f64 - (fingers as f64 - 1.0) / 2.0) * -splay * f;
            out.push(sample(
                t,
                i as u8,
                MOVE,
                x + dx * f * lag + pull,
                y + dy * f * lag,
            ));
        }
    }
    for (i, (x, y)) in start.iter().enumerate() {
        out.push(sample(t, i as u8, UP, x + dx, y + dy));
    }
    out
}

#[test]
fn a_splayed_four_finger_swipe_up_is_mission_control() {
    let mut cfg = swipes_on();
    // Launchpad bound too, exactly as the mirroring sets it from a real Mac —
    // this is the competition the swipe used to lose.
    cfg.bindings.four_finger_pinch = "launchpad".into();
    cfg.bindings.five_finger_spread = "showDesktop".into();

    let actions = run(cfg, &splayed_swipe(4, 0.0, -170.0, 14.0));
    assert_eq!(
        shortcuts(&actions),
        vec![Shortcut::MissionControl],
        "the hand travelled far and merely curled; that is a swipe: {actions:?}"
    );
}

#[test]
fn a_splayed_four_finger_swipe_down_shows_app_windows() {
    let mut cfg = swipes_on();
    cfg.bindings.four_finger_pinch = "launchpad".into();
    let actions = run(cfg, &splayed_swipe(4, 0.0, 170.0, 14.0));
    assert_eq!(
        shortcuts(&actions),
        vec![Shortcut::AppWindows],
        "got {actions:?}"
    );
}

/// And a genuine pinch — fingers closing while the hand stays put — still wins.
#[test]
fn a_stationary_pinch_is_still_launchpad() {
    let mut cfg = swipes_on();
    cfg.bindings.four_finger_pinch = "launchpad".into();
    let actions = run(cfg, &pinch(4, 0.4, 20));
    assert_eq!(
        shortcuts(&actions),
        vec![Shortcut::Launchpad],
        "got {actions:?}"
    );
}

// ---------------------------------------------------------- one direction at a time
//
// A swipe used to bind a whole axis: one setting for "three fingers, up and
// down", and the engine decided which end did what. Each direction now carries
// its own action, and the axis setting survives as what a direction inherits
// when nobody has given it one - which is what keeps an existing config, and
// everything the host's trackpad settings are mirrored into, working untouched.

fn scrolled(actions: &[InputAction]) -> bool {
    actions
        .iter()
        .any(|a| matches!(a, InputAction::Scroll { .. }))
}

#[test]
fn a_direction_can_be_given_its_own_action() {
    let mut cfg = swipes_on();
    cfg.bindings.four_finger_swipe_up = "showDesktop".into();

    let up = run(cfg.clone(), &swipe(4, 0.0, -140.0, 20));
    assert_eq!(shortcuts(&up), vec![Shortcut::ShowDesktop], "got {up:?}");

    // And the direction nobody touched still follows the axis it inherits.
    let down = run(cfg, &swipe(4, 0.0, 140.0, 20));
    assert_eq!(shortcuts(&down), vec![Shortcut::AppWindows], "got {down:?}");
}

#[test]
fn a_direction_set_to_nothing_fires_nothing() {
    let mut cfg = swipes_on();
    cfg.bindings.four_finger_swipe_up = "none".into();

    let up = run(cfg.clone(), &swipe(4, 0.0, -140.0, 20));
    assert!(shortcuts(&up).is_empty(), "got {up:?}");

    let down = run(cfg, &swipe(4, 0.0, 140.0, 20));
    assert_eq!(shortcuts(&down), vec![Shortcut::AppWindows], "got {down:?}");
}

#[test]
fn a_direction_outranks_the_axis_it_inherits_from() {
    // The axis says spaces; this one direction says otherwise, and the other
    // end of the same axis is unaffected.
    let mut cfg = swipes_on();
    cfg.bindings.four_finger_swipe_right = "screenshot".into();

    let right = run(cfg.clone(), &swipe(4, 140.0, 0.0, 20));
    assert_eq!(
        shortcuts(&right),
        vec![Shortcut::Screenshot],
        "got {right:?}"
    );

    let left = run(cfg, &swipe(4, -140.0, 0.0, 20));
    assert_eq!(shortcuts(&left), vec![Shortcut::SpaceRight], "got {left:?}");
}

#[test]
fn two_fingers_sideways_can_carry_one_action_and_still_scroll_the_other_way() {
    // Two fingers sideways is the most reachable swipe there is, and it is also
    // how a page is scrolled sideways. A direction that is bound navigates; the
    // one left alone keeps scrolling.
    let mut cfg = Config::default();
    cfg.bindings.two_finger_swipe_right = "copy".into();

    let right = run(cfg.clone(), &swipe(2, 140.0, 0.0, 20));
    assert_eq!(shortcuts(&right), vec![Shortcut::Copy], "got {right:?}");
    assert!(
        !scrolled(&right),
        "a bound swipe scrolled as well: {right:?}"
    );

    let left = run(cfg, &swipe(2, -140.0, 0.0, 20));
    assert!(shortcuts(&left).is_empty(), "got {left:?}");
    assert!(scrolled(&left), "the unbound direction stopped scrolling");
}

#[test]
fn a_two_finger_direction_set_to_nothing_still_scrolls() {
    // "Nothing" means the swipe is not bound, and an unbound two fingers
    // sideways is a scroll. Reading it as a bound gesture took horizontal
    // scrolling away in that direction and gave nothing back for it.
    let mut cfg = Config::default();
    cfg.bindings.two_finger_swipe_navigate = "navigate".into();
    cfg.bindings.two_finger_swipe_right = "none".into();

    let right = run(cfg.clone(), &swipe(2, 140.0, 0.0, 20));
    assert!(shortcuts(&right).is_empty(), "got {right:?}");
    assert!(
        scrolled(&right),
        "nothing bound, and nothing scrolled either"
    );

    // The other direction still inherits back/forward from the axis.
    let left = run(cfg, &swipe(2, -140.0, 0.0, 20));
    assert_eq!(shortcuts(&left), vec![Shortcut::Forward], "got {left:?}");
}
