"""Milestone-0 runtime: static page + WebSocket + recognizer + injection.

One process, two listeners:
  - http://127.0.0.1:8080  serves page.html (stdlib, on a daemon thread)
  - ws://127.0.0.1:8787    receives binary touch frames

This is the throwaway prototype from plan section 5. Its job is to make the
gesture engine real enough to tune, and to record touch streams that become the
Rust test fixtures.
"""
from __future__ import annotations

import argparse
import asyncio
import json
import struct
import threading
import time
from functools import partial
from http.server import SimpleHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

import websockets

from . import config as cfgmod
from .inject_macos import MacInjector, accessibility_trusted, permission_help
from .recognizer import Act, Recognizer, TouchSample

HERE = Path(__file__).resolve().parent
SAMPLE = struct.Struct("<IBBff")  # t_ms, pointerId, phase, x, y  -> 14 bytes


def decode_frame(data: bytes) -> list[TouchSample]:
    """Parse a binary touch frame (protocol/v1.schema.json)."""
    if len(data) < 2 or data[0] != 1:
        return []
    count = data[1]
    if len(data) != 2 + SAMPLE.size * count:
        return []
    out = []
    for i in range(count):
        t, pid, phase, x, y = SAMPLE.unpack_from(data, 2 + i * SAMPLE.size)
        out.append(TouchSample(t_ms=t, pointer_id=pid, phase=phase, x=x, y=y))
    return out


class Runtime:
    """Owns the recognizer, the injector and the (optional) recording."""

    def __init__(self, dry_run: bool = False, record_path: Path | None = None):
        self.config_path = cfgmod.seed_user_config()
        self.cfg = cfgmod.load(self.config_path)
        self._cfg_mtime = cfgmod.mtime(self.config_path)
        self.rec = Recognizer(self.cfg)
        self.dry_run = dry_run
        self.injector = None if dry_run else MacInjector()
        self.record_path = record_path
        self.recording: list[dict] = []
        self.last_t_ms = 0

    # ------------------------------------------------------------------ config

    def maybe_reload_config(self) -> bool:
        m = cfgmod.mtime(self.config_path)
        if m and m != self._cfg_mtime:
            self._cfg_mtime = m
            self.cfg = cfgmod.load(self.config_path)
            self.rec.cfg = self.cfg
            print(f"[padremote] config reloaded: sensitivity={self.cfg.sensitivity} "
                  f"natural={self.cfg.scroll.natural} accel_gain={self.cfg.accel.gain}")
            return True
        return False

    def apply_settings(self, msg: dict) -> None:
        if "sensitivity" in msg:
            self.cfg.sensitivity = float(msg["sensitivity"])
        if "naturalScroll" in msg:
            self.cfg.scroll.natural = bool(msg["naturalScroll"])

    # ------------------------------------------------------------------ samples

    def handle_samples(self, samples: list[TouchSample]) -> None:
        if self.record_path:
            self.recording.extend(
                {"t_ms": s.t_ms, "pointer_id": s.pointer_id, "phase": s.phase,
                 "x": round(s.x, 6), "y": round(s.y, 6)} for s in samples
            )
        if samples:
            self.last_t_ms = samples[-1].t_ms
        self.apply(self.rec.feed(samples))

    def apply(self, actions) -> None:
        if not actions:
            return
        if self.dry_run:
            for a in actions:
                print("  ", a)
            return
        inj = self.injector
        for a in actions:
            if a.kind is Act.MOVE:
                inj.move_by(a.dx, a.dy)
            elif a.kind is Act.CLICK:
                inj.click(a.button, a.count)
            elif a.kind is Act.BUTTON_DOWN:
                inj.button_down(a.button)
            elif a.kind is Act.BUTTON_UP:
                inj.button_up(a.button)
            elif a.kind is Act.SCROLL:
                inj.scroll_by(a.dx, a.dy)
            elif a.kind is Act.ZOOM:
                inj.zoom(a.steps)

    def release_all(self) -> None:
        self.apply(self.rec.release_all())
        if self.injector:
            self.injector.release_all()

    def save_recording(self) -> None:
        if not self.record_path or not self.recording:
            return
        self.record_path.parent.mkdir(parents=True, exist_ok=True)
        self.record_path.write_text(json.dumps(
            {"version": 1, "surface": {"wpx": self.rec.surface_wpx, "hpx": self.rec.surface_hpx},
             "samples": self.recording}, indent=1))
        print(f"[padremote] wrote {len(self.recording)} samples to {self.record_path}")


async def handle_client(ws, rt: Runtime) -> None:
    peer = getattr(ws, "remote_address", ("?",))[0]
    print(f"[padremote] phone connected from {peer}")
    last_state = None
    try:
        async for msg in ws:
            if isinstance(msg, bytes):
                rt.handle_samples(decode_frame(msg))
            else:
                try:
                    m = json.loads(msg)
                except json.JSONDecodeError:
                    continue
                if m.get("t") == "welcome":
                    s = m.get("surface") or {}
                    rt.rec.set_surface(float(s.get("wpx", 390)), float(s.get("hpx", 716)))
                    print(f"[padremote] surface {s.get('wpx')}x{s.get('hpx')} @{s.get('dpr')}x")
                elif m.get("t") == "settings":
                    rt.apply_settings(m)

            state = rt.rec.gesture_name
            if state != last_state:
                last_state = state
                await ws.send(json.dumps({"t": "state", "gesture": state,
                                          "fingers": len(rt.rec.pointers)}))
            if isinstance(msg, bytes) and rt.last_t_ms:
                await ws.send(json.dumps({"t": "echo", "tMs": rt.last_t_ms}))
    except websockets.exceptions.ConnectionClosed:
        pass
    finally:
        # Never leave a button or drag stuck when the phone goes away.
        rt.release_all()
        print("[padremote] phone disconnected")


async def momentum_loop(rt: Runtime) -> None:
    """Drives momentum scroll and config hot-reload off a steady clock."""
    while True:
        await asyncio.sleep(1 / 60)
        rt.apply(rt.rec.tick(int(time.monotonic() * 1000)))
        rt.maybe_reload_config()


class PageHandler(SimpleHTTPRequestHandler):
    """Serves page.html from this package, quietly."""

    def log_message(self, *args) -> None:  # keep the console for gesture output
        pass


def serve_page(port: int) -> None:
    handler = partial(PageHandler, directory=str(HERE))
    ThreadingHTTPServer(("0.0.0.0", port), handler).serve_forever()


async def amain(args) -> None:
    rt = Runtime(dry_run=args.dry_run, record_path=Path(args.record) if args.record else None)

    if not args.dry_run and not accessibility_trusted():
        print(permission_help())
        accessibility_trusted(prompt=True)
        return

    threading.Thread(target=serve_page, args=(args.http_port,), daemon=True).start()

    print(f"[padremote] config: {rt.config_path}")
    print(f"[padremote] open  http://localhost:{args.http_port}/page.html")
    print(f"[padremote] ws    ws://0.0.0.0:{args.ws_port}"
          + ("   (dry run: printing actions, not injecting)" if args.dry_run else ""))

    async with websockets.serve(lambda ws: handle_client(ws, rt), "0.0.0.0", args.ws_port,
                                max_queue=64, ping_interval=20):
        try:
            await momentum_loop(rt)
        finally:
            rt.save_recording()


def main() -> None:
    ap = argparse.ArgumentParser(prog="proto", description="PadRemote milestone-0 prototype")
    ap.add_argument("--http-port", type=int, default=8080)
    ap.add_argument("--ws-port", type=int, default=8787)
    ap.add_argument("--dry-run", action="store_true",
                    help="print InputActions instead of injecting them")
    ap.add_argument("--record", metavar="OUT.json",
                    help="record the raw touch stream as a Rust test fixture")
    args = ap.parse_args()
    try:
        asyncio.run(amain(args))
    except KeyboardInterrupt:
        print("\n[padremote] bye")


if __name__ == "__main__":
    main()
