"""macOS input injection via Quartz CGEvent (plan section 9.10).

Everything is posted to `kCGHIDEventTap` so events look like they came from real
hardware and reach every application.

This module is the one piece of the prototype that is deliberately platform
specific; the Rust app mirrors it 1:1 in src/input/macos.rs.
"""
from __future__ import annotations

import Quartz
from ApplicationServices import AXIsProcessTrustedWithOptions
from CoreFoundation import CFDictionaryCreate, kCFTypeDictionaryKeyCallBacks, kCFTypeDictionaryValueCallBacks

ACCESSIBILITY_URL = (
    "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility"
)

_BUTTONS = {
    "left": (Quartz.kCGEventLeftMouseDown, Quartz.kCGEventLeftMouseUp,
             Quartz.kCGEventLeftMouseDragged, Quartz.kCGMouseButtonLeft),
    "right": (Quartz.kCGEventRightMouseDown, Quartz.kCGEventRightMouseUp,
              Quartz.kCGEventRightMouseDragged, Quartz.kCGMouseButtonRight),
    "middle": (Quartz.kCGEventOtherMouseDown, Quartz.kCGEventOtherMouseUp,
               Quartz.kCGEventOtherMouseDragged, Quartz.kCGMouseButtonCenter),
}

KEY_EQUAL = 24  # '=' -> Cmd+= zooms in
KEY_MINUS = 27  # '-' -> Cmd+- zooms out


def accessibility_trusted(prompt: bool = False) -> bool:
    """True when this process may inject input.

    On macOS the grant attaches to the *binary that runs us* - during development
    that is the terminal app, not this script. The shipped app gets its own entry
    (plan section 11.1).
    """
    if not prompt:
        return bool(AXIsProcessTrustedWithOptions(None))
    key = "AXTrustedCheckOptionPrompt"
    opts = CFDictionaryCreate(
        None, [key], [True], 1, kCFTypeDictionaryKeyCallBacks, kCFTypeDictionaryValueCallBacks
    )
    return bool(AXIsProcessTrustedWithOptions(opts))


def permission_help() -> str:
    return (
        "\n  PadRemote needs Accessibility permission to move the cursor.\n"
        "  Open:  System Settings -> Privacy & Security -> Accessibility\n"
        "  and enable the app running this script (Terminal / iTerm / your editor).\n"
        f"  Deep link:  open '{ACCESSIBILITY_URL}'\n"
        "  Then run this again - no restart needed once the switch is on.\n"
    )


class MacInjector:
    """Holds the virtual cursor position and turns InputActions into CGEvents."""

    def __init__(self) -> None:
        self.x, self.y = self._current_location()
        self.bounds = self._screen_bounds()
        self._down: set[str] = set()

    # ------------------------------------------------------------------ helpers

    @staticmethod
    def _current_location() -> tuple[float, float]:
        loc = Quartz.CGEventGetLocation(Quartz.CGEventCreate(None))
        return float(loc.x), float(loc.y)

    @staticmethod
    def _screen_bounds() -> tuple[float, float, float, float]:
        """Union of every display, so the cursor can cross monitors."""
        err, ids, count = Quartz.CGGetActiveDisplayList(16, None, None)
        if err != 0 or not count:
            return (0.0, 0.0, 1920.0, 1080.0)
        x0 = y0 = float("inf")
        x1 = y1 = float("-inf")
        for did in ids[:count]:
            r = Quartz.CGDisplayBounds(did)
            x0 = min(x0, r.origin.x)
            y0 = min(y0, r.origin.y)
            x1 = max(x1, r.origin.x + r.size.width)
            y1 = max(y1, r.origin.y + r.size.height)
        return (x0, y0, x1, y1)

    def _post(self, event) -> None:
        if event is not None:
            Quartz.CGEventPost(Quartz.kCGHIDEventTap, event)

    def _clamp(self) -> None:
        x0, y0, x1, y1 = self.bounds
        self.x = min(max(self.x, x0), x1 - 1)
        self.y = min(max(self.y, y0), y1 - 1)

    def sync_from_system(self) -> None:
        """Re-seed from the real cursor, in case the user touched the trackpad."""
        self.x, self.y = self._current_location()

    # ------------------------------------------------------------------ actions

    def move_by(self, dx: float, dy: float) -> None:
        self.x += dx
        self.y += dy
        self._clamp()
        # While a button is held the event type must be *Dragged*, or apps will
        # not track the drag.
        held = next(iter(self._down), None)
        etype = _BUTTONS[held][2] if held else Quartz.kCGEventMouseMoved
        button = _BUTTONS[held][3] if held else Quartz.kCGMouseButtonLeft
        ev = Quartz.CGEventCreateMouseEvent(None, etype, (self.x, self.y), button)
        # Delta fields matter to apps that read relative motion (games, 3D views).
        Quartz.CGEventSetIntegerValueField(ev, Quartz.kCGMouseEventDeltaX, int(dx))
        Quartz.CGEventSetIntegerValueField(ev, Quartz.kCGMouseEventDeltaY, int(dy))
        self._post(ev)

    def button_down(self, button: str) -> None:
        down, _up, _drag, btn = _BUTTONS[button]
        ev = Quartz.CGEventCreateMouseEvent(None, down, (self.x, self.y), btn)
        Quartz.CGEventSetIntegerValueField(ev, Quartz.kCGMouseEventClickState, 1)
        self._post(ev)
        self._down.add(button)

    def button_up(self, button: str) -> None:
        _down, up, _drag, btn = _BUTTONS[button]
        ev = Quartz.CGEventCreateMouseEvent(None, up, (self.x, self.y), btn)
        Quartz.CGEventSetIntegerValueField(ev, Quartz.kCGMouseEventClickState, 1)
        self._post(ev)
        self._down.discard(button)

    def click(self, button: str, count: int = 1) -> None:
        """A click with the right clickState so double-click is recognised."""
        down, up, _drag, btn = _BUTTONS[button]
        for etype in (down, up):
            ev = Quartz.CGEventCreateMouseEvent(None, etype, (self.x, self.y), btn)
            Quartz.CGEventSetIntegerValueField(ev, Quartz.kCGMouseEventClickState, count)
            self._post(ev)

    def scroll_by(self, dx: float, dy: float) -> None:
        # Pixel units keep scrolling smooth instead of line-quantised.
        # pyobjc's variadic binding wants exactly `5 + wheelCount` arguments:
        # (source, units, wheelCount, wheel1..wheelN, pad, pad). Wheel 1 is the
        # vertical axis on macOS, wheel 2 the horizontal.
        ev = Quartz.CGEventCreateScrollWheelEvent2(
            None, Quartz.kCGScrollEventUnitPixel, 2, int(dy), int(dx), 0, 0
        )
        self._post(ev)

    def zoom(self, steps: int) -> None:
        """App-level zoom: Cmd+= / Cmd+- (plan section 5 - the reliable backend).

        No OS exposes a universal synthetic magnify event, so v1 approximates.
        """
        key = KEY_EQUAL if steps > 0 else KEY_MINUS
        for _ in range(abs(steps)):
            for is_down in (True, False):
                ev = Quartz.CGEventCreateKeyboardEvent(None, key, is_down)
                Quartz.CGEventSetFlags(ev, Quartz.kCGEventFlagMaskCommand)
                self._post(ev)

    def release_all(self) -> None:
        for button in list(self._down):
            self.button_up(button)
