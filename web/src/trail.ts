/**
 * Finger trails on the touch surface.
 *
 * Beyond looking good, this is the most useful diagnostic the phone has: it is
 * drawn from **the very samples that were transmitted**, not from raw browser
 * events. So if the trail is smooth but the cursor is not, the problem is on the
 * desktop; if the trail itself is ragged, the phone is dropping or clumping
 * samples and the network is innocent.
 *
 * This module owns the canvas - its size, its device-pixel backing, the cached
 * glare and the animation frame - and draws the trails themselves. The
 * long-press indicator is a separate animation with its own state machine and
 * lives in [`./trail/hold`]; it is handed the canvas each frame rather than
 * given one of its own, because the two have to share a stacking order: trails
 * under, hold over, direction arrows on top.
 */

import type { TouchSample } from "./protocol";
import { Phase } from "./protocol";
import { buildGlare } from "./trail/glare";
import { HoldIndicator } from "./trail/hold";
import type { Insets, Stage } from "./trail/theme";
import { watchSize } from "./viewport";

/** How long a trail lingers. Long enough to see a flick, short enough to read. */
const TRAIL_MS = 420;
/** Fattest part of the stroke, in CSS pixels. */
const MAX_WIDTH = 22;

/** One colour per finger, so multi-finger gestures are legible at a glance. */
const COLORS = [
  "#2f6feb", // blue
  "#3fb950", // green
  "#d29922", // amber
  "#db61a2", // pink
  "#a371f7", // purple
];

interface Point {
  x: number; // CSS pixels on the surface
  y: number;
  t: number; // performance.now()
}

export class TrailRenderer {
  private readonly ctx: CanvasRenderingContext2D;
  private readonly points = new Map<number, Point[]>();
  /** Velocity per finger, in px/s, for the direction arrow. */
  private readonly velocity = new Map<number, { vx: number; vy: number }>();
  /** The long-press ring, the arming confirmation and the held-drag glow. */
  private readonly hold = new HoldIndicator();
  private width = 0;
  private height = 0;
  /** Device pixel ratio the backing store was built at. Changes when a window
   *  is dragged between a laptop screen and an external one. */
  private dpr = 0;
  /** Safe-area insets in CSS pixels, so the edge indicator clears the notch. */
  private safe: Insets = { top: 0, right: 0, bottom: 0, left: 0 };
  /**
   * The drag glare, drawn once and blitted every frame.
   *
   * Rebuilt only on resize, and owned here rather than by the hold indicator
   * because it is a property of the canvas: the size it was built for is the
   * size this module maintains.
   */
  private glare: HTMLCanvasElement | null = null;
  private running = false;

  constructor(private readonly canvas: HTMLCanvasElement) {
    const ctx = canvas.getContext("2d", { alpha: true });
    if (!ctx) throw new Error("2d canvas unavailable");
    this.ctx = ctx;
    this.resize();
    // Not `window.resize` alone: a phone browser has not settled on a size when
    // this runs, and Safari's address bar collapsing a moment later does not
    // reliably fire it. Measuring once here and never again is what left the
    // whole canvas drawn to a stale height on the first load of the page.
    watchSize(canvas, () => this.resize());
    this.start();
  }

  private resize(): void {
    const dpr = window.devicePixelRatio || 1;
    const r = this.canvas.getBoundingClientRect();
    // The watcher fires for anything that *might* have changed the size,
    // including a visual-viewport scroll - which on a phone can be continuous.
    // Rebuilding the glare bitmap for a size that has not moved would be a
    // stroke loop over the whole screen edge, every frame, for nothing.
    if (r.width === this.width && r.height === this.height && dpr === this.dpr) {
      return;
    }
    const cs = getComputedStyle(document.documentElement);
    const px = (name: string) => parseFloat(cs.getPropertyValue(name)) || 0;
    this.safe = {
      top: px("--sa-top"),
      right: px("--sa-right"),
      bottom: px("--sa-bottom"),
      left: px("--sa-left"),
    };
    this.dpr = dpr;
    this.width = r.width;
    this.height = r.height;
    // Back the canvas at device resolution so the strokes are not soft.
    this.canvas.width = Math.round(r.width * dpr);
    this.canvas.height = Math.round(r.height * dpr);
    this.ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    this.glare = buildGlare(this.width, this.height, dpr);
  }

  /** Everything the hold indicator needs to know about the canvas. */
  private stage(): Stage {
    return { width: this.width, height: this.height, safe: this.safe, glare: this.glare };
  }

  /** Called the moment a hold completes, i.e. when the drag really starts. */
  get onPressArmed(): (() => void) | null {
    return this.hold.onPressArmed;
  }
  set onPressArmed(fn: (() => void) | null) {
    this.hold.onPressArmed = fn;
  }

  /** Called when an armed drag ends. */
  get onPressReleased(): (() => void) | null {
    return this.hold.onPressReleased;
  }
  set onPressReleased(fn: (() => void) | null) {
    this.hold.onPressReleased = fn;
  }

  /**
   * The desktop's own `pressMs`, so the ring finishes exactly when the drag
   * actually starts rather than at a guessed moment.
   */
  setPressMs(ms: number): void {
    this.hold.setPressMs(ms);
  }

  /**
   * The desktop's own `tapMaxPx`, the distance that separates a press from a
   * move, so the ring gives up exactly when the engine does.
   */
  setTapMaxPx(px: number): void {
    this.hold.setTapMaxPx(px);
  }

  /**
   * Feed the batch that was just sent to the desktop.
   *
   * Coordinates arrive normalized, exactly as transmitted, and are scaled back
   * to the surface here.
   */
  push(samples: readonly TouchSample[]): void {
    const now = performance.now();
    this.hold.update(samples, now, this.stage());
    for (const s of samples) {
      const x = s.x * this.width;
      const y = s.y * this.height;

      if (s.phase === Phase.Up || s.phase === Phase.Cancel) {
        // Let the tail fade out rather than snapping away.
        this.velocity.delete(s.id);
        continue;
      }

      let pts = this.points.get(s.id);
      if (!pts || s.phase === Phase.Down) {
        pts = [];
        this.points.set(s.id, pts);
      }

      const prev = pts[pts.length - 1];
      if (prev) {
        const dt = Math.max(1, now - prev.t) / 1000;
        this.velocity.set(s.id, { vx: (x - prev.x) / dt, vy: (y - prev.y) / dt });
      }
      pts.push({ x, y, t: now });
    }
  }

  private start(): void {
    if (this.running) return;
    this.running = true;
    const frame = () => {
      this.draw();
      requestAnimationFrame(frame);
    };
    requestAnimationFrame(frame);
  }

  private draw(): void {
    const now = performance.now();
    this.ctx.clearRect(0, 0, this.width, this.height);

    for (const [id, pts] of this.points) {
      // Retire points that have aged out.
      while (pts.length && now - pts[0].t > TRAIL_MS) pts.shift();
      if (!pts.length) {
        this.points.delete(id);
        this.velocity.delete(id);
        continue;
      }
      const color = COLORS[id % COLORS.length];
      this.strokeTrail(pts, color, now);
    }

    this.hold.draw(this.ctx, now, this.stage());

    // Draw the arrows last so they sit above every trail.
    for (const [id, pts] of this.points) {
      const v = this.velocity.get(id);
      const head = pts[pts.length - 1];
      if (v && head && now - head.t < 90) {
        this.drawVector(head, v, COLORS[id % COLORS.length]);
      }
    }
  }

  /**
   * A tapered ribbon, fat at the fingertip and vanishing at the tail.
   *
   * Built as a single filled polygon — one edge down each side of the path —
   * rather than as a series of strokes. Stroking segment by segment gives every
   * segment its own round cap, which reads as a string of beads; one polygon
   * gives a continuous blade.
   */
  private strokeTrail(pts: Point[], color: string, now: number): void {
    const ctx = this.ctx;
    if (pts.length < 2) return;

    // Half-width at each point: newest is fattest.
    const half = pts.map((p) => {
      const age = Math.max(0, 1 - (now - p.t) / TRAIL_MS);
      return (MAX_WIDTH / 2) * age * age;
    });

    // Perpendicular at each point, taken from its neighbours so the ribbon
    // turns smoothly through a curve.
    const normals = pts.map((_p, i) => {
      const prev = pts[Math.max(0, i - 1)];
      const next = pts[Math.min(pts.length - 1, i + 1)];
      const dx = next.x - prev.x;
      const dy = next.y - prev.y;
      const len = Math.hypot(dx, dy) || 1;
      return { x: -dy / len, y: dx / len };
    });

    ctx.beginPath();
    // Up one side...
    for (let i = 0; i < pts.length; i++) {
      const x = pts[i].x + normals[i].x * half[i];
      const y = pts[i].y + normals[i].y * half[i];
      i ? ctx.lineTo(x, y) : ctx.moveTo(x, y);
    }
    // ...and back down the other.
    for (let i = pts.length - 1; i >= 0; i--) {
      ctx.lineTo(pts[i].x - normals[i].x * half[i], pts[i].y - normals[i].y * half[i]);
    }
    ctx.closePath();

    ctx.globalAlpha = 0.8;
    ctx.fillStyle = color;
    // A soft glow, which is what makes it read as a blade rather than a smear.
    ctx.shadowColor = color;
    ctx.shadowBlur = 14;
    ctx.fill();
    ctx.shadowBlur = 0;

    // A bright head, so the current fingertip is unmistakable.
    const head = pts[pts.length - 1];
    ctx.globalAlpha = 1;
    ctx.fillStyle = "#fff";
    ctx.beginPath();
    ctx.arc(head.x, head.y, 5, 0, Math.PI * 2);
    ctx.fill();
    ctx.fillStyle = color;
    ctx.beginPath();
    ctx.arc(head.x, head.y, 9, 0, Math.PI * 2);
    ctx.globalAlpha = 0.35;
    ctx.fill();
    ctx.globalAlpha = 1;
  }

  /** An arrow showing where the finger is heading and how fast. */
  private drawVector(head: Point, v: { vx: number; vy: number }, color: string): void {
    const speed = Math.hypot(v.vx, v.vy);
    if (speed < 60) return; // a resting finger has no meaningful direction

    const ctx = this.ctx;
    // Compress the length so a fast flick stays on screen.
    const len = Math.min(90, 14 + speed * 0.05);
    const ux = v.vx / speed;
    const uy = v.vy / speed;
    const tipX = head.x + ux * len;
    const tipY = head.y + uy * len;

    ctx.globalAlpha = 0.55;
    ctx.strokeStyle = color;
    ctx.lineWidth = 2;
    ctx.beginPath();
    ctx.moveTo(head.x, head.y);
    ctx.lineTo(tipX, tipY);
    ctx.stroke();

    // Arrowhead.
    const wing = 7;
    ctx.beginPath();
    ctx.moveTo(tipX, tipY);
    ctx.lineTo(tipX - ux * wing + uy * wing * 0.6, tipY - uy * wing - ux * wing * 0.6);
    ctx.lineTo(tipX - ux * wing - uy * wing * 0.6, tipY - uy * wing + ux * wing * 0.6);
    ctx.closePath();
    ctx.fillStyle = color;
    ctx.fill();
    ctx.globalAlpha = 1;
  }

  /** Wipe every trail, e.g. when the link drops. */
  clear(): void {
    this.points.clear();
    this.velocity.clear();
    this.hold.clear();
  }
}
