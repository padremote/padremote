r"""Gesture engine: touch samples in, InputActions out (plan section 9).

PURE AND DETERMINISTIC. No I/O, no Quartz, no clock of its own - every decision
is a function of the samples fed in and the timestamps they carry. That is what
makes it testable from recorded streams and portable to Rust unchanged.

State machine (section 9.8):

    IDLE -1 down-> PENDING -move-> MOVING ---------------------+
                          \-hold>pressMs-> DRAG                |
         -2 down-> TWO_PENDING -parallel move-> SCROLL         | all up
                              \-distance change-> ZOOM         |
                              \-quick up-> RIGHTCLICK          |
    IDLE -1 down/up quick-> TAP -> click                       |
    any <--------------------------------------------------------+
        (all fingers up -> release held buttons -> IDLE)
"""
from __future__ import annotations

import math
from dataclasses import dataclass, field
from enum import Enum
from typing import Iterable

from .config import Config

# Touch phases, matching the wire protocol.
DOWN, MOVE, UP, CANCEL = 0, 1, 2, 3

# Fingers landing within this window are one gesture, not a sequence.
GROUP_WINDOW_MS = 60

# Speed at which the acceleration curve reaches its knee.
ACCEL_SPEED_REF = 1000.0  # px/s
ACCEL_BASE = 0.55
ACCEL_MAX = 3.5

# Momentum scroll decay per second once the fingers lift.
MOMENTUM_DECAY = 0.972
MOMENTUM_MIN_PX = 0.4
MOMENTUM_MIN_LAUNCH = 90.0  # px/s below which we do not coast at all
MOMENTUM_STOP = 4.0

# Scroll acceleration, mirroring desktop/src/gesture/mod.rs. Kept in step only
# so the shared fixtures stay a true cross-check of the two implementations.
SCROLL_SPEED_REF = 700.0
SCROLL_BASE = 0.85
SCROLL_MAX = 6.0


@dataclass(frozen=True)
class TouchSample:
    t_ms: int
    pointer_id: int
    phase: int
    x: float  # normalized 0-1
    y: float  # normalized 0-1


class Act(str, Enum):
    MOVE = "move"
    CLICK = "click"
    BUTTON_DOWN = "button_down"
    BUTTON_UP = "button_up"
    SCROLL = "scroll"
    ZOOM = "zoom"


# Scroll gesture phases. macOS only does smooth scrolling and rubber-banding
# when it can see a gesture begin, continue and end - a wheel event without a
# phase is treated as an old mouse wheel, which is exactly what "not like a real
# trackpad" feels like.
PHASE_NONE, PHASE_BEGIN, PHASE_CONTINUE, PHASE_END = "", "begin", "continue", "end"
PHASE_MOMENTUM, PHASE_MOMENTUM_END = "momentum", "momentumEnd"


@dataclass(frozen=True)
class InputAction:
    kind: Act
    dx: float = 0.0
    dy: float = 0.0
    button: str = "left"
    count: int = 1
    steps: int = 0
    t_ms: int = 0
    phase: str = PHASE_NONE

    def __repr__(self) -> str:
        if self.kind is Act.MOVE:
            return f"move({self.dx:+.0f},{self.dy:+.0f})"
        if self.kind is Act.SCROLL:
            return f"scroll({self.dx:+.0f},{self.dy:+.0f},{self.phase})"
        if self.kind is Act.ZOOM:
            return f"zoom({self.steps:+d})"
        if self.kind is Act.CLICK:
            return f"click({self.button},x{self.count})"
        return f"{self.kind.value}({self.button})"


class State(str, Enum):
    IDLE = "idle"
    PENDING = "pending"
    MOVING = "move"
    DRAG = "drag"
    TWO_PENDING = "two_pending"
    SCROLL = "scroll"
    ZOOM = "zoom"
    MULTI_PENDING = "multi_pending"
    DEAD = "dead"  # intent spent; wait for all fingers to lift


@dataclass
class Pointer:
    start_t: int
    start_x: float
    start_y: float
    x: float
    y: float
    # Movement seen since a gesture handler last consumed it. Deltas are
    # consumed exactly once: a stationary finger must contribute nothing when
    # the *other* finger reports a sample, or two-finger scroll double-counts.
    dx: float = 0.0
    dy: float = 0.0
    path_px: float = 0.0
    last_t: int = 0

    def take(self) -> tuple[float, float]:
        d = (self.dx, self.dy)
        self.dx = self.dy = 0.0
        return d


BUTTON_FOR_BINDING = {"leftClick": "left", "rightClick": "right", "middleClick": "middle"}


class Recognizer:
    """Feed it samples with `feed`; call `tick` on a timer for momentum scroll."""

    def __init__(self, cfg: Config, surface_wpx: float = 390.0, surface_hpx: float = 716.0):
        self.cfg = cfg
        self.surface_wpx = surface_wpx
        self.surface_hpx = surface_hpx
        self.reset(emit=None)
        # Click bookkeeping survives resets so double-click and tap-and-drag work.
        self._last_click_t = -10_000
        self._last_click_button = "left"
        self._click_state = 1
        self._tap_ended_t = -10_000

    # ---------------------------------------------------------------- lifecycle

    def set_surface(self, wpx: float, hpx: float) -> None:
        if wpx > 0 and hpx > 0:
            self.surface_wpx, self.surface_hpx = wpx, hpx

    def _to_px(self, x: float, y: float) -> tuple[float, float]:
        """Normalized surface coordinates -> surface pixels.

        The phone sends 0-1 so the desktop can scale by the real surface size
        reported in `welcome`; physical deltas are what the accel curve needs.
        """
        return x * self.surface_wpx, y * self.surface_hpx

    def reset(self, emit: list | None) -> None:
        """Drop all gesture state. Releases any held button into `emit`."""
        held = getattr(self, "_held_button", None)
        if held and emit is not None:
            emit.append(InputAction(Act.BUTTON_UP, button=held))
        self._held_button = None
        self.state = State.IDLE
        self.pointers: dict[int, Pointer] = {}
        self._gesture_start_t = 0
        self._armed_drag = False
        self._acc_x = 0.0
        self._acc_y = 0.0
        self._scroll_acc_x = 0.0
        self._scroll_acc_y = 0.0
        self._speed_ema = 0.0
        self._scroll_speed_ema = 0.0
        self._zoom_ref_dist = 0.0
        self._zoom_acc = 0.0
        self._momentum_vx = 0.0
        self._momentum_vy = 0.0
        self._momentum_t = 0
        self._scroll_vx = 0.0
        self._scroll_vy = 0.0
        self._scroll_started = False
        # A multi-finger tap is only judged once the LAST finger lifts, so the
        # peak finger count and the "still tap-like" flag have to outlive the
        # pointers themselves.
        self._max_fingers = 0
        self._tap_ok = True

    def release_all(self) -> list[InputAction]:
        """Connection lost / cancel: never leave a button or drag stuck."""
        out: list[InputAction] = []
        self.reset(out)
        return out

    @property
    def gesture_name(self) -> str:
        return {
            State.IDLE: "idle", State.PENDING: "idle", State.MOVING: "move",
            State.DRAG: "drag", State.TWO_PENDING: "idle", State.SCROLL: "scroll",
            State.ZOOM: "zoom", State.MULTI_PENDING: "idle", State.DEAD: "idle",
        }[self.state]

    # ------------------------------------------------------------------- input

    def feed(self, samples: Iterable[TouchSample]) -> list[InputAction]:
        out: list[InputAction] = []
        for s in samples:
            self._feed_one(s, out)
        return out

    def tick(self, t_ms: int) -> list[InputAction]:
        """Momentum scroll continuation (section 9.4). Safe to call at any rate."""
        out: list[InputAction] = []
        if self._momentum_vx == 0.0 and self._momentum_vy == 0.0:
            return out
        dt = max(0.0, (t_ms - self._momentum_t) / 1000.0)
        if dt <= 0.0:
            return out
        self._momentum_t = t_ms
        decay = MOMENTUM_DECAY ** (dt * 60.0)
        self._momentum_vx *= decay
        self._momentum_vy *= decay
        if abs(self._momentum_vx) < MOMENTUM_STOP and abs(self._momentum_vy) < MOMENTUM_STOP:
            self._momentum_vx = self._momentum_vy = 0.0
            out.append(InputAction(Act.SCROLL, t_ms=t_ms, phase=PHASE_MOMENTUM_END))
            return out
        self._emit_scroll(self._momentum_vx * dt, self._momentum_vy * dt, t_ms, out,
                          phase=PHASE_MOMENTUM)
        return out

    def _feed_one(self, s: TouchSample, out: list[InputAction]) -> None:
        if s.phase == DOWN:
            self._on_down(s, out)
        elif s.phase == MOVE:
            self._on_move(s, out)
        elif s.phase == UP:
            self._on_up(s, out)
        else:
            self._on_cancel(s, out)

    # -------------------------------------------------------------------- down

    def _on_down(self, s: TouchSample, out: list[InputAction]) -> None:
        # A new touch cancels any coasting scroll, like a real trackpad.
        self._momentum_vx = self._momentum_vy = 0.0

        px, py = self._to_px(s.x, s.y)
        self.pointers[s.pointer_id] = Pointer(
            start_t=s.t_ms, start_x=px, start_y=py, x=px, y=py, last_t=s.t_ms,
        )
        n = len(self.pointers)
        self._max_fingers = max(self._max_fingers, n)

        if self.state is State.IDLE:
            self._gesture_start_t = s.t_ms
            self.state = State.PENDING
            # Tap-and-drag ("tap-and-a-half"): a tap, then a finger down again
            # within doubleTapMs, arms a drag on the next movement.
            self._armed_drag = (s.t_ms - self._tap_ended_t) <= self.cfg.tap.doubleTapMs
            return

        # Additional fingers only change the intent while it is still forming.
        if self.state in (State.PENDING, State.TWO_PENDING, State.MULTI_PENDING):
            if s.t_ms - self._gesture_start_t <= GROUP_WINDOW_MS:
                if n == 2:
                    self.state = State.TWO_PENDING
                    self._zoom_ref_dist = self._pair_distance()
                    self._armed_drag = False
                elif n >= 3:
                    self.state = State.MULTI_PENDING
                    self._armed_drag = False
            else:
                # Landed too late to be part of this gesture; the intent is spent.
                self.state = State.DEAD
        # In a committed state (MOVING/DRAG/SCROLL/ZOOM) extra fingers are ignored
        # so the gesture cannot mutate mid-flight (section 9.1).

    # -------------------------------------------------------------------- move

    def _on_move(self, s: TouchSample, out: list[InputAction]) -> None:
        p = self.pointers.get(s.pointer_id)
        if p is None:
            return
        px, py = self._to_px(s.x, s.y)
        step = math.hypot(px - p.x, py - p.y)
        p.dx += px - p.x
        p.dy += py - p.y
        p.x, p.y = px, py
        p.path_px += step
        dt_ms = max(1, s.t_ms - p.last_t)
        p.last_t = s.t_ms

        if self.state is State.PENDING:
            self._pending_move(s, p, dt_ms, out)
        elif self.state in (State.MOVING, State.DRAG):
            self._cursor_move(p, dt_ms, s.t_ms, out)
        elif self.state is State.TWO_PENDING:
            self._two_pending_move(s, out)
        elif self.state is State.SCROLL:
            self._scroll_move(s, out)
        elif self.state is State.ZOOM:
            self._zoom_move(s, out)

    def _pending_move(self, s: TouchSample, p: Pointer, dt_ms: int, out: list[InputAction]) -> None:
        drift = math.hypot(p.x - p.start_x, p.y - p.start_y)
        held_ms = s.t_ms - p.start_t

        # Press-and-drag: stationary past pressMs, then moving. Checked before the
        # movement threshold so a long press that then moves becomes a drag, not a
        # cursor move.
        if held_ms >= self.cfg.tap.pressMs and drift <= self.cfg.tap.tapMaxPx:
            self._begin_drag(out)
            self._cursor_move(p, dt_ms, s.t_ms, out)
            return

        if drift > self.cfg.tap.tapMaxPx:
            if self._armed_drag:
                self._begin_drag(out)
            else:
                self.state = State.MOVING
            # Replay the whole drift so the cursor does not lag the finger, and
            # drop the unconsumed delta it already includes.
            p.take()
            self._acc_x += (p.x - p.start_x) * self.cfg.sensitivity
            self._acc_y += (p.y - p.start_y) * self.cfg.sensitivity
            self._flush_move(s.t_ms, out)

    def _begin_drag(self, out: list[InputAction]) -> None:
        self.state = State.DRAG
        self._held_button = "left"
        self._armed_drag = False
        out.append(InputAction(Act.BUTTON_DOWN, button="left"))

    def _cursor_move(self, p: Pointer, dt_ms: int, t_ms: int, out: list[InputAction]) -> None:
        dx, dy = p.take()
        if dx == 0.0 and dy == 0.0:
            return
        speed = math.hypot(dx, dy) / (dt_ms / 1000.0)
        # Smooth the speed estimate only - never the position - so acceleration is
        # stable without adding latency to the cursor itself (section 9.2).
        self._speed_ema = 0.5 * self._speed_ema + 0.5 * speed
        f = self._accel_factor(self._speed_ema) * self.cfg.sensitivity
        self._acc_x += dx * f
        self._acc_y += dy * f
        self._flush_move(t_ms, out)

    def _accel_factor(self, speed: float) -> float:
        s = speed / ACCEL_SPEED_REF
        if self.cfg.accel.curve == "linear":
            f = ACCEL_BASE + self.cfg.accel.gain * s
        else:  # quadratic (default): fine control when slow, reach when fast
            f = ACCEL_BASE + self.cfg.accel.gain * s * s * 2.2
        return min(ACCEL_MAX, max(0.15, f))

    def _flush_move(self, t_ms: int, out: list[InputAction]) -> None:
        # Emit whole pixels, carrying the remainder so slow movement is not lost.
        ix = math.trunc(self._acc_x)
        iy = math.trunc(self._acc_y)
        if ix == 0 and iy == 0:
            return
        self._acc_x -= ix
        self._acc_y -= iy
        out.append(InputAction(Act.MOVE, dx=float(ix), dy=float(iy), t_ms=t_ms))

    # ------------------------------------------------------- two-finger gestures

    def _two_pending_move(self, s: TouchSample, out: list[InputAction]) -> None:
        ps = list(self.pointers.values())
        if len(ps) < 2:
            return
        dist = self._pair_distance()
        ratio = dist / self._zoom_ref_dist if self._zoom_ref_dist > 1e-6 else 1.0
        avg_drift = sum(math.hypot(p.x - p.start_x, p.y - p.start_y) for p in ps) / len(ps)

        if self.cfg.zoom.enabled and abs(ratio - 1.0) > self.cfg.zoom.threshold:
            self.state = State.ZOOM
            self._zoom_ref_dist = dist
            self._zoom_acc = 0.0
        elif avg_drift > self.cfg.tap.tapMaxPx:
            self.state = State.SCROLL
            self._scroll_move(s, out)

    def _scroll_move(self, s: TouchSample, out: list[InputAction]) -> None:
        ps = list(self.pointers.values())
        if not ps:
            return
        taken = [p.take() for p in ps]
        dx = sum(t[0] for t in taken) / len(ps)
        dy = sum(t[1] for t in taken) / len(ps)
        if dx == 0.0 and dy == 0.0:
            return
        dt = max(1, s.t_ms - self._momentum_t) / 1000.0 if self._momentum_t else 0.016
        self._momentum_t = s.t_ms

        speed = math.hypot(dx, dy) / dt
        self._scroll_speed_ema = 0.6 * self._scroll_speed_ema + 0.4 * speed
        gain = self._scroll_accel(self._scroll_speed_ema)

        self._scroll_vx = dx * gain / dt
        self._scroll_vy = dy * gain / dt
        self._emit_scroll(dx * gain, dy * gain, s.t_ms, out)

    def _scroll_accel(self, speed: float) -> float:
        s = speed / SCROLL_SPEED_REF
        return min(SCROLL_MAX, max(0.2, SCROLL_BASE + self.cfg.scroll.accel * s * s * 2.5))

    def _emit_scroll(self, dx: float, dy: float, t_ms: int, out: list[InputAction],
                     phase: str | None = None) -> None:
        k = self.cfg.scroll.speed
        sign = 1.0 if self.cfg.scroll.natural else -1.0
        self._scroll_acc_x += dx * k * sign
        self._scroll_acc_y += dy * k * sign
        ix = math.trunc(self._scroll_acc_x)
        iy = math.trunc(self._scroll_acc_y)
        if ix == 0 and iy == 0:
            return
        self._scroll_acc_x -= ix
        self._scroll_acc_y -= iy
        if phase is None:
            phase = PHASE_CONTINUE if self._scroll_started else PHASE_BEGIN
            self._scroll_started = True
        out.append(InputAction(Act.SCROLL, dx=float(ix), dy=float(iy), t_ms=t_ms, phase=phase))

    def _zoom_move(self, s: TouchSample, out: list[InputAction]) -> None:
        if len(self.pointers) < 2:
            return
        for p in self.pointers.values():
            p.take()
        dist = self._pair_distance()
        if self._zoom_ref_dist <= 1e-6:
            self._zoom_ref_dist = dist
            return
        self._zoom_acc += math.log(max(dist, 1e-6) / self._zoom_ref_dist)
        self._zoom_ref_dist = dist
        # One zoom step per ~12% distance change; matches an app zoom increment.
        step_size = math.log(1.12)
        while abs(self._zoom_acc) >= step_size:
            direction = 1 if self._zoom_acc > 0 else -1
            self._zoom_acc -= direction * step_size
            out.append(InputAction(Act.ZOOM, steps=direction, t_ms=s.t_ms))

    def _pair_distance(self) -> float:
        ps = list(self.pointers.values())[:2]
        if len(ps) < 2:
            return 0.0
        return math.hypot(ps[0].x - ps[1].x, ps[0].y - ps[1].y)

    # ---------------------------------------------------------------------- up

    def _on_up(self, s: TouchSample, out: list[InputAction]) -> None:
        p = self.pointers.pop(s.pointer_id, None)
        if p is None:
            return

        quick = (s.t_ms - p.start_t) <= self.cfg.tap.tapMaxMs and p.path_px <= self.cfg.tap.tapMaxPx
        if not quick:
            self._tap_ok = False

        if self.pointers:
            # Not the last finger. Stay in the committed intent: a two-finger tap
            # lifts one finger fractionally before the other, and a scroll must
            # not turn into a cursor jerk because one finger left early
            # (section 9.1). A lone remaining finger cannot move the cursor
            # because the two-finger handlers require two pointers.
            return

        if self.state in (State.PENDING, State.TWO_PENDING, State.MULTI_PENDING) and self._tap_ok:
            binding = {
                1: self.cfg.bindings.get("oneTap", "leftClick"),
                2: self.cfg.bindings.get("twoFingerTap", "rightClick"),
                3: self.cfg.bindings.get("threeFingerTap", "middleClick"),
            }.get(self._max_fingers)
            if binding:
                self._emit_click(binding, s.t_ms, out)
                if self._max_fingers == 1:
                    # Opens the tap-and-drag window (section 9.5).
                    self._tap_ended_t = s.t_ms
        elif self.state is State.SCROLL:
            if self._scroll_started:
                out.append(InputAction(Act.SCROLL, t_ms=s.t_ms, phase=PHASE_END))
            self._launch_momentum(s.t_ms)

        self.reset(out)

    def _launch_momentum(self, t_ms: int) -> None:
        if not self.cfg.scroll.momentum:
            return
        if math.hypot(self._scroll_vx, self._scroll_vy) < MOMENTUM_MIN_LAUNCH:
            return
        self._momentum_vx = self._scroll_vx
        self._momentum_vy = self._scroll_vy
        self._momentum_t = t_ms

    def _emit_click(self, binding: str, t_ms: int, out: list[InputAction]) -> None:
        button = BUTTON_FOR_BINDING.get(binding)
        if button is None:  # binding "none"
            return
        if button == self._last_click_button and (t_ms - self._last_click_t) <= self.cfg.tap.doubleTapMs:
            self._click_state = min(3, self._click_state + 1)
        else:
            self._click_state = 1
        self._last_click_t = t_ms
        self._last_click_button = button
        out.append(InputAction(Act.CLICK, button=button, count=self._click_state, t_ms=t_ms))

    def _on_cancel(self, s: TouchSample, out: list[InputAction]) -> None:
        self.pointers.pop(s.pointer_id, None)
        if not self.pointers:
            self.reset(out)
        else:
            self.state = State.DEAD
