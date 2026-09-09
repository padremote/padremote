"""Generate synthetic touch streams for every v1 gesture, check the recognizer
against them, and write them out as shared fixtures for the Rust tests.

Synthetic rather than hand-recorded on purpose: these streams are exactly
reproducible, so the Python engine and its Rust port can be held to the same
expectations byte for byte. Hand-recorded streams from a real phone can be
dropped into the same directory later via `python -m proto --record`.
"""
from __future__ import annotations

import json
import math
import sys
from pathlib import Path

from proto.config import Config, load
from proto.recognizer import Act, Recognizer, TouchSample

ROOT = Path(__file__).resolve().parents[2]
FIXTURES = ROOT / "desktop" / "tests" / "fixtures"
W, H = 390.0, 716.0          # a typical phone surface, in CSS pixels
DT = 8                       # 125 Hz sampling

DOWN, MOVE, UP, CANCEL = 0, 1, 2, 3


class Stream:
    """Builds a touch stream in surface pixels, emitting normalized samples."""

    def __init__(self) -> None:
        self.samples: list[TouchSample] = []
        self.t = 0

    def add(self, pid: int, phase: int, px: float, py: float) -> None:
        self.samples.append(TouchSample(self.t, pid, phase, px / W, py / H))

    def wait(self, ms: int) -> "Stream":
        self.t += ms
        return self

    def down(self, pid: int, px: float, py: float) -> "Stream":
        self.add(pid, DOWN, px, py)
        return self

    def up(self, pid: int, px: float, py: float) -> "Stream":
        self.add(pid, UP, px, py)
        return self

    def cancel(self, pid: int, px: float, py: float) -> "Stream":
        self.add(pid, CANCEL, px, py)
        return self

    def glide(self, pid: int, x0: float, y0: float, x1: float, y1: float, steps: int) -> "Stream":
        """One finger travelling in a straight line, one sample per DT."""
        for i in range(1, steps + 1):
            self.t += DT
            self.add(pid, MOVE, x0 + (x1 - x0) * i / steps, y0 + (y1 - y0) * i / steps)
        return self

    def glide2(self, a: tuple, b: tuple, steps: int) -> "Stream":
        """Two fingers moving together; both report at each timestep.

        a and b are (pid, x0, y0, x1, y1).
        """
        for i in range(1, steps + 1):
            self.t += DT
            for pid, x0, y0, x1, y1 in (a, b):
                self.add(pid, MOVE, x0 + (x1 - x0) * i / steps, y0 + (y1 - y0) * i / steps)
        return self

    def jitter(self, pid: int, px: float, py: float, ms: int) -> "Stream":
        """A finger resting: sub-pixel noise, as a real digitiser reports."""
        n = max(1, ms // DT)
        for i in range(n):
            self.t += DT
            self.add(pid, MOVE, px + 0.3 * math.sin(i), py + 0.3 * math.cos(i))
        return self


def run(stream: Stream, cfg: Config | None = None) -> list:
    rec = Recognizer(cfg or Config(), surface_wpx=W, surface_hpx=H)
    return rec.feed(stream.samples)


def kinds(actions) -> list[str]:
    return [a.kind.value for a in actions]


# --------------------------------------------------------------------- gestures

def g_tap() -> Stream:
    return Stream().down(0, 200, 300).wait(70).up(0, 200, 300)


def g_double_tap() -> Stream:
    s = Stream().down(0, 200, 300).wait(70).up(0, 200, 300)
    return s.wait(90).down(0, 200, 300).wait(70).up(0, 200, 300)


def g_two_finger_tap() -> Stream:
    s = Stream().down(0, 180, 300)
    s.wait(12).down(1, 260, 300)
    return s.wait(70).up(0, 180, 300).up(1, 260, 300)


def g_three_finger_tap() -> Stream:
    s = Stream().down(0, 140, 300)
    s.wait(10).down(1, 200, 300)
    s.wait(10).down(2, 260, 300)
    return s.wait(70).up(0, 140, 300).up(1, 200, 300).up(2, 260, 300)


def g_move() -> Stream:
    s = Stream().down(0, 100, 400)
    s.glide(0, 100, 400, 300, 250, 25)
    return s.up(0, 300, 250)


def g_scroll() -> Stream:
    s = Stream().down(0, 180, 500)
    s.wait(12).down(1, 260, 500)
    s.glide2((0, 180, 500, 180, 260), (1, 260, 500, 260, 260), 24)
    return s.up(0, 180, 260).up(1, 260, 260)


def g_pinch_out() -> Stream:
    s = Stream().down(0, 170, 350)
    s.wait(12).down(1, 230, 350)
    s.glide2((0, 170, 350, 60, 350), (1, 230, 350, 340, 350), 24)
    return s.up(0, 60, 350).up(1, 340, 350)


def g_pinch_in() -> Stream:
    s = Stream().down(0, 60, 350)
    s.wait(12).down(1, 340, 350)
    s.glide2((0, 60, 350, 175, 350), (1, 340, 350, 225, 350), 24)
    return s.up(0, 175, 350).up(1, 225, 350)


def g_press_drag() -> Stream:
    s = Stream().down(0, 150, 400)
    s.jitter(0, 150, 400, 600)          # held past pressMs (500 ms)
    s.glide(0, 150, 400, 280, 400, 16)
    return s.up(0, 280, 400)


def g_tap_drag() -> Stream:
    s = Stream().down(0, 150, 400).wait(70).up(0, 150, 400)
    s.wait(90).down(0, 150, 400)        # second touch inside doubleTapMs
    s.glide(0, 150, 400, 280, 400, 16)
    return s.up(0, 280, 400)


def g_drag_cancelled() -> Stream:
    s = Stream().down(0, 150, 400)
    s.jitter(0, 150, 400, 600)          # held past pressMs (500 ms)
    s.glide(0, 150, 400, 240, 400, 12)
    return s.cancel(0, 240, 400)        # phone yanked away mid-drag


CASES = [
    ("tap_click", g_tap, "one finger down and up -> a single left click"),
    ("double_tap", g_double_tap, "two taps inside doubleTapMs -> click, then click with count 2"),
    ("two_finger_tap_rightclick", g_two_finger_tap, "two fingers tapped together -> right click"),
    ("three_finger_tap_middleclick", g_three_finger_tap, "three fingers tapped -> middle click"),
    ("one_finger_move", g_move, "one finger dragged -> relative cursor movement, no click"),
    ("two_finger_scroll", g_scroll, "two fingers moving in parallel -> pixel scroll, no zoom"),
    ("pinch_out_zoom_in", g_pinch_out, "fingers separating -> positive zoom steps"),
    ("pinch_in_zoom_out", g_pinch_in, "fingers converging -> negative zoom steps"),
    ("press_and_drag", g_press_drag, "held past pressMs then moved -> button down, moves, button up"),
    ("tap_and_drag", g_tap_drag, "tap then immediate press and move -> click then a held drag"),
    ("drag_cancelled", g_drag_cancelled, "cancel mid-drag -> the held button is still released"),
]


# ------------------------------------------------------------------ assertions

def check(name: str, actions: list) -> list[str]:
    """Return a list of failures for this case."""
    k = kinds(actions)
    bad: list[str] = []

    def want(cond: bool, msg: str) -> None:
        if not cond:
            bad.append(msg)

    if name == "tap_click":
        want(k == ["click"], f"expected exactly one click, got {actions}")
        want(actions[0].button == "left" and actions[0].count == 1, f"expected left x1, got {actions}")
    elif name == "double_tap":
        clicks = [a for a in actions if a.kind is Act.CLICK]
        want(len(clicks) == 2, f"expected two clicks, got {actions}")
        want([c.count for c in clicks] == [1, 2], f"expected counts [1,2], got {[c.count for c in clicks]}")
    elif name == "two_finger_tap_rightclick":
        want(k == ["click"] and actions[0].button == "right", f"expected one right click, got {actions}")
    elif name == "three_finger_tap_middleclick":
        want(k == ["click"] and actions[0].button == "middle", f"expected one middle click, got {actions}")
    elif name == "one_finger_move":
        want(all(x == "move" for x in k), f"expected only moves, got {set(k)}")
        want(len(actions) > 3, f"expected several move events, got {len(actions)}")
        want(sum(a.dx for a in actions) > 0 and sum(a.dy for a in actions) < 0,
             "expected net movement right and up (screen y grows downward)")
    elif name == "two_finger_scroll":
        want("scroll" in k, f"expected scroll actions, got {set(k)}")
        phases = [a.phase for a in actions if a.kind is Act.SCROLL]
        want(phases[0] == "begin", f"a scroll must open with a begin phase, got {phases[:1]}")
        want(phases[-1] == "end", f"a scroll must close with an end phase, got {phases[-1:]}")
        want(all(p == "continue" for p in phases[1:-1]),
             "every scroll between the edges must be a continue phase")
        want("zoom" not in k, "parallel two-finger movement must not be read as zoom")
        want("move" not in k, "two-finger movement must not move the cursor")
        want(sum(a.dy for a in actions if a.kind is Act.SCROLL) < 0,
             "fingers moving up the surface should scroll by a negative dy")
    elif name == "pinch_out_zoom_in":
        z = [a for a in actions if a.kind is Act.ZOOM]
        want(z, f"expected zoom actions, got {set(k)}")
        want(all(a.steps > 0 for a in z), "separating fingers must zoom in")
        want("scroll" not in k, "a pinch must not also scroll")
    elif name == "pinch_in_zoom_out":
        z = [a for a in actions if a.kind is Act.ZOOM]
        want(z, f"expected zoom actions, got {set(k)}")
        want(all(a.steps < 0 for a in z), "converging fingers must zoom out")
    elif name in ("press_and_drag", "tap_and_drag"):
        want(k.count("button_down") == 1, f"expected one button_down, got {k}")
        want(k.count("button_up") == 1, f"expected one button_up, got {k}")
        want(k.index("button_down") < k.index("button_up"), "button_up must follow button_down")
        want("move" in k[k.index("button_down"):], "the cursor must move while the button is held")
        if name == "tap_and_drag":
            want(k[0] == "click", f"tap-and-drag starts with the tap's click, got {k[:2]}")
    elif name == "drag_cancelled":
        want(k.count("button_down") == k.count("button_up") == 1,
             f"a cancelled drag must still release the button, got {k}")
        want(k[-1] == "button_up", f"the release must be the final action, got {k[-3:]}")
    return bad


def main() -> int:
    cfg = load()
    FIXTURES.mkdir(parents=True, exist_ok=True)
    failures = 0

    for name, build, description in CASES:
        stream = build()
        actions = run(stream, cfg)
        bad = check(name, actions)

        (FIXTURES / f"{name}.json").write_text(json.dumps({
            "name": name,
            "description": description,
            "surface": {"wpx": W, "hpx": H},
            "samples": [
                {"t_ms": s.t_ms, "pointer_id": s.pointer_id, "phase": s.phase,
                 "x": round(s.x, 6), "y": round(s.y, 6)} for s in stream.samples
            ],
            "expect": {
                "kinds": kinds(actions),
                "clicks": [{"button": a.button, "count": a.count}
                           for a in actions if a.kind is Act.CLICK],
                "net_move": [round(sum(a.dx for a in actions if a.kind is Act.MOVE), 3),
                             round(sum(a.dy for a in actions if a.kind is Act.MOVE), 3)],
                "net_scroll": [round(sum(a.dx for a in actions if a.kind is Act.SCROLL), 3),
                               round(sum(a.dy for a in actions if a.kind is Act.SCROLL), 3)],
                "net_zoom": sum(a.steps for a in actions if a.kind is Act.ZOOM),
                "scroll_phases": [a.phase for a in actions if a.kind is Act.SCROLL],
            },
        }, indent=1) + "\n")

        if bad:
            failures += 1
            print(f"FAIL  {name}")
            for b in bad:
                print(f"        {b}")
        else:
            summary = ", ".join(f"{k}x{kinds(actions).count(k)}" for k in dict.fromkeys(kinds(actions)))
            print(f"ok    {name:<30} {summary}")

    print(f"\n{len(CASES) - failures}/{len(CASES)} gestures behave as specified; "
          f"fixtures in {FIXTURES.relative_to(ROOT)}")
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())
