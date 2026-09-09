# The gesture engine

`desktop/src/gesture/` — touch samples in, `InputAction`s out.

Pure and deterministic: no I/O, no platform calls, no clock of its own. Feed it
the same samples and it produces the same actions, which is what makes it
testable and portable.

## The state machine

```
IDLE ─1 down─▶ PENDING ─move─▶ MOVING ────────────────────┐
                      └─hold > pressMs─▶ DRAG             │
     ─2 down─▶ TWO_PENDING ─parallel move─▶ SCROLL        │
                          ├─opposing move─▶ ZOOM          │ all
                          ├─sideways─▶ back/forward       │ fingers
                          └─quick up─▶ right click        │ up
     ─3+ down─▶ MULTI_PENDING ─move─▶ MULTI_DRAG          │
                             ├─spread change─▶ Launchpad  │
                             └─travel─▶ swipe shortcut    │
IDLE ─1 down/up quick─▶ tap → click                       │
MOVING ─another finger down─▶ TWO_PENDING / MULTI_PENDING │
       (re-anchored: see "changing your mind" below)      │
any ◀─────────────────────────────────────────────────────┘
     (release held buttons → IDLE)
```

`DEAD` is the tenth state: the intent is spent, wait for every finger to lift. A
swipe enters it after firing so one gesture cannot fire twice.

## Rules that are easy to break

These each exist because of a specific bug. Changing them will reintroduce it.

**Commit to an intent and hold it.** Once a gesture is committed, extra fingers
are ignored. A finger lifting early must not change what is happening — a
two-finger tap lifts one finger fractionally before the other, and a scroll must
not become a cursor jerk.

**Except out of `MOVING`, where the user is allowed to change their mind.** A
MacBook lets you slide one finger, watch the cursor go, then lay a second finger
down and scroll — without lifting the hand and starting again. Holding the
commit here meant the second finger did nothing at all, and moving the cursor
before scrolling is about the most ordinary thing anyone does with a trackpad.
`Recognizer::regroup` re-opens the gesture and **re-anchors every finger where it
now sits**, which is the part that matters: the finger that has been steering is
a long way from where it landed, and the two-finger handlers measure drift from
the landing point — left alone, the promoted gesture commits on its first sample
in whatever direction that finger was already travelling, firing back/forward
instead of ever scrolling. It also clears `tap_ok`, because re-anchoring resets
exactly the `start_t` and `path_px` that `on_up` reads to decide a tap, and an
ordinary move must not sign off with a stray right click.

`DRAG` keeps the strict rule. It is holding a button as far as every application
is concerned, and a finger brushing the surface must not let go of it mid
selection. `SCROLL` and `ZOOM` keep it too — they already *are* what the
promotion would arrive at.

What a finger *lifting* does is asymmetric, and worth knowing before you read a
freeze as a bug. Once `SCROLL` has committed, `scroll_move` needs only one
pointer, so lifting one of the two leaves the other scrolling. In `TWO_PENDING`
the intent is undecided and `two_pending_move` returns early below two pointers,
so the remaining finger does nothing at all until it lifts. That predates the
promotion above — landing two fingers and lifting one has always done it — but
the promotion is a second route in, reached by a stray touch during a move.

**Time a tap from when the hand is complete, not from each finger's own
landing.** `quick` measures `t - max(finger's own start, the last finger's
start)`. Four fingers never land together on a phone — the spread between index
and little finger routinely runs to 150 ms, which is why `GROUP_WINDOW_MS` is as
generous as it is — and charging the first finger for that wait spent most of the
200 ms `tapMaxMs` budget before the hand had finished arriving. A four-finger tap
then silently did nothing at all, with the swipe and pinch paths declining it
too. For one finger the two values are identical, so single taps and double taps
are untouched.

The relaxation is only about the *landing*: the movement half of `quick` stays
per-finger, and a hand that stays down still fails on duration, so a deliberate
hold is not a tap.

**Judge multi-finger gestures on the *peak* finger count**, never on how many are
touching right now. One finger landing late or lifting early otherwise turns a
four-finger swipe into a three-finger one — and on a Mac that binds only the
four-finger gestures, that means nothing happens at all.

**A pinch needs opposing motion, not just a distance change.** Fingers report one
at a time, so mid-gesture the spacing changes even when nothing is pinching.
Judging on distance alone made every horizontal two-finger scroll a zoom.

**A multi-finger pinch must out-weigh the hand's travel.** Swiping four fingers
up curls and splays them enough to change the spread well past the threshold;
without this the swipe loses to Launchpad and vertical gestures feel random.

**Consume each pointer's delta exactly once.** A stationary finger must
contribute nothing when the *other* finger reports a sample. `Pointer::take()`
exists for this; reading a previous position instead double-counts and scroll
drifts.

**Fingers land raggedly on a phone.** `GROUP_WINDOW_MS` is 160 ms, and a finger
may join even later as long as nothing has moved yet — the hand is still
settling, not starting a second gesture.

## Tunables

In config (`gesture/config.rs`, hot-reloaded):

| Key | Meaning |
|---|---|
| `sensitivity` | Overall cursor gain |
| `accel.gain` | Acceleration strength; the curve itself is fixed (quadratic) |
| `tap.tapMaxMs`, `tap.tapMaxPx` | What still counts as a tap |
| `tap.doubleTapMs` | Double-click window (mirrored from macOS; default 500 ms) |
| `tap.pressMs` | Hold before press-and-drag |
| `scroll.*` | `enabled`, `natural`, `momentum`, `speed`, `accel`, `horizontal` |
| `zoom.*` | `enabled`, `backend`, `threshold` |
| `drag.*` | `pressAndDrag` (default on), `tapAndDrag` (default **off**) |
| `swipe.minPx` | Travel before a swipe counts |
| `bindings.*` | Gesture → action |

**Two sets of swipe bindings, on purpose.** `bindings.threeFingerVertSwipe` and
its four siblings bind a whole *axis*: one action the engine splits by direction.
That is the shape macOS's own trackpad preferences have, and therefore the only
shape `HostTrackpad::apply_to` can mirror into. `bindings.threeFingerSwipeUp` and
its nine siblings bind one *direction* each, which is what the settings page
edits and what `Bindings::directional` looks up.

A direction holds `"inherit"` until someone sets it, and `directional_shortcut`
then falls back to the axis — so an existing config, and everything the host
mirror writes, keep working with nothing migrated. `"none"` is not `"inherit"`:
it means deliberately unbound, and for two fingers sideways unbound means the
fingers scroll instead.

Compiled-in constants, when the config isn't the right home:

| Constant | Value | Why |
|---|---|---|
| `GROUP_WINDOW_MS` | 160 | Fingers landing together are one gesture |
| `MOMENTUM_DECAY` | 0.972 | Per 1/60 s coast decay |
| `MOMENTUM_MIN_LAUNCH` | 90 px/s | Below this a scroll just stops |
| `SCROLL_SPEED_REF` | 700 px/s | Where scroll acceleration knees |
| `SCROLL_BASE` / `SCROLL_MAX` | 0.85 / 6.0 | Scroll gain floor and ceiling |
| `accel::SPEED_REF` | 1000 px/s | Where the curve knees |
| `accel::BASE` / `MAX` | 0.55 / 3.5 | Gain floor and ceiling |

**Tap-and-drag ships off.** macOS's own "Enable dragging" is an Accessibility
option that is off until somebody turns it on, and v1 of the config file shipped
the opposite - so a tap followed by an ordinary move held the button down and
selected text, with nothing drawn on the pad to say a drag had armed. A host that
does have the setting turns it back on through `apply_to`; a file still at
version 1 is corrected once by `Config::migrate`.

**Long-press is not mirrored from the host.** `tapAndDrag` follows macOS, but
`pressAndDrag` substitutes for hardware the phone lacks — a physical button and
Force Click. Letting a host setting disable it leaves a Mac configured for
three-finger drag with no way to select text one-handed: that style turns
one-finger dragging off, and PadRemote has no three-finger drag to fall back on.

## Acceleration

A trackpad feels right when a slow finger gives fine control and a fast one
crosses the screen. That's a curve on *speed*, not a constant gain.

The speed estimate is smoothed (EMA), **the position never is** — smoothing
position adds latency you can feel. Sub-pixel remainders accumulate so slow
movement is not lost to rounding.

Scrolling has its own curve, for the same reason: strictly one-to-one scrolling
is a large part of what makes a touch surface feel unlike a trackpad, where a
quick flick covers a page. Momentum takes its launch velocity **after** the
curve, so the coast carries the speed the user actually saw.

## Testing it

```sh
cargo test --manifest-path desktop/Cargo.toml
```

Two suites:

- **`tests/fixtures.rs`** — eleven recorded gestures in `tests/fixtures/*.json`,
  generated by the Python prototype. Asserts both the specified behaviour *and*
  that the Rust engine still agrees action-for-action with the prototype the
  timings were tuned against.
- **`tests/swipes.rs`** — everything added after the prototype was retired:
  swipes, pinches, drag styles, smart zoom. Streams are built in Rust, and
  deliberately imperfect — fingers land 150 ms apart, lift early, splay
  mid-swipe. Perfect synthetic input hides the bugs that actually bite.

Watch a gesture decode without a phone:

```sh
cargo run --manifest-path desktop/Cargo.toml --bin replay -- \
  desktop/tests/fixtures/two_finger_scroll.json
```

Add `--inject` to drive the real cursor at the recorded pace.
