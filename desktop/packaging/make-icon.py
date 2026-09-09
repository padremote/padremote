#!/usr/bin/env python3
"""Generate PadRemote's app icon as a PNG, with no image library.

A tiny hand-rolled PNG writer keeps the build free of dependencies: the icon is
a few rounded rectangles, which is not worth a toolchain.
"""
from __future__ import annotations

import struct
import sys
import zlib

SIZE = 1024
BG = (13, 15, 19)        # the app's near-black
ACCENT = (47, 111, 235)  # the app's blue


def rounded_rect(x0, y0, x1, y1, r):
    """Predicate: is (x, y) inside this rounded rectangle?"""
    def inside(x, y):
        if not (x0 <= x <= x1 and y0 <= y <= y1):
            return False
        for cx, cy in ((x0 + r, y0 + r), (x1 - r, y0 + r), (x0 + r, y1 - r), (x1 - r, y1 - r)):
            # Only the corner squares need the distance test.
            if (x < x0 + r or x > x1 - r) and (y < y0 + r or y > y1 - r):
                if abs(x - cx) <= r and abs(y - cy) <= r:
                    return (x - cx) ** 2 + (y - cy) ** 2 <= r * r
        return True
    return inside


def build() -> bytearray:
    s = SIZE
    outer = rounded_rect(0, 0, s - 1, s - 1, int(s * 0.22))
    pad_outer = rounded_rect(int(s * 0.23), int(s * 0.17), int(s * 0.77), int(s * 0.83), int(s * 0.08))
    pad_inner = rounded_rect(int(s * 0.28), int(s * 0.22), int(s * 0.72), int(s * 0.78), int(s * 0.055))

    # Two contact dots, as if two fingers rested on the pad.
    dots = [(int(s * 0.41), int(s * 0.48), int(s * 0.062), 255),
            (int(s * 0.60), int(s * 0.58), int(s * 0.062), 140)]

    rows = bytearray()
    for y in range(s):
        rows.append(0)  # PNG filter type 0 for this scanline
        for x in range(s):
            if not outer(x, y):
                rows += bytes((0, 0, 0, 0))
                continue
            r, g, b = BG
            a = 255
            if pad_outer(x, y) and not pad_inner(x, y):
                r, g, b = ACCENT
            for cx, cy, rad, alpha in dots:
                if (x - cx) ** 2 + (y - cy) ** 2 <= rad * rad:
                    r, g, b = ACCENT
                    a = alpha
            rows += bytes((r, g, b, a))
    return rows


def chunk(tag: bytes, data: bytes) -> bytes:
    return (struct.pack(">I", len(data)) + tag + data
            + struct.pack(">I", zlib.crc32(tag + data) & 0xFFFFFFFF))


def main() -> int:
    out = sys.argv[1] if len(sys.argv) > 1 else "icon.png"
    header = struct.pack(">IIBBBBB", SIZE, SIZE, 8, 6, 0, 0, 0)  # 8-bit RGBA
    png = (b"\x89PNG\r\n\x1a\n"
           + chunk(b"IHDR", header)
           + chunk(b"IDAT", zlib.compress(bytes(build()), 9))
           + chunk(b"IEND", b""))
    with open(out, "wb") as f:
        f.write(png)
    print(f"wrote {out} ({len(png)} bytes)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
