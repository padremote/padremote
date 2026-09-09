/**
 * Does the drag confirmation grow out of the hold, or interrupt it?
 *
 * The long press has two animations back to back: the ring charging up, and the
 * glow that stays lit for as long as the drag is held. They are drawn by
 * different code paths, so nothing stops them disagreeing — and when they do,
 * the moment the drag arms reads as a glitch: the ring jumps colour and width,
 * the screen border blinks out, and a green flash appears in front of a white
 * glare that was not there a frame earlier.
 *
 * That is invisible to a test that only asks "did it arm". This one records
 * what the renderer actually draws on the frames either side of the seam and
 * checks the two animations meet:
 *
 *   - the fingertip ring keeps its radius, width and colour across the seam
 *   - the colour then travels to the armed green rather than cutting to it
 *   - the edge glare arrives overbright and settles, with no dark frame
 *   - the arming beat is a scan crossing the screen, not a flat wash over it
 *   - nothing anywhere draws a rounded corner
 *
 *   npm run check:intro
 *
 * No test framework: the module needs a canvas and a clock, not a runner, and
 * recording those is the whole job.
 */

import { execFileSync } from "node:child_process";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

const W = 390;
const H = 716;
const DOWN = 0;
const MOVE = 1;
const PRESS_MS = 500;
const FRAME_MS = 16;
/** Must match `INTRO_MS` in trail.ts. */
const INTRO_MS = 380;
/** Must match `GLARE_ALPHA` and `GLARE_BREATH`. */
const GLARE_ALPHA = 0.72;
const GLARE_BREATH = 0.28;
/** Must match `GLARE_PERIOD_MS`. */
const GLARE_PERIOD_MS = 1800;

const out = mkdtempSync(join(tmpdir(), "hold-intro-"));
const bundle = join(out, "trail.mjs");
execFileSync(
  "npx",
  ["esbuild", "src/trail.ts", "--bundle", "--format=esm", `--outfile=${bundle}`, "--log-level=error"],
  { stdio: "inherit" },
);

let clock = 0;
const noop = () => {};

/**
 * A canvas context that remembers what it was asked to draw.
 *
 * Only the handful of calls this check reasons about are recorded; everything
 * else is accepted and dropped, exactly as the stub in `check-hold-ring.mjs`
 * does. `save`/`restore` are modelled properly because the renderer relies on
 * them, and an alpha that does not unwind would make every later reading wrong.
 */
class Recorder {
  constructor() {
    this.globalAlpha = 1;
    this.lineWidth = 1;
    this.strokeStyle = "#000";
    this.fillStyle = "#000";
    this.stack = [];
    this.frame = null;
    this.path = null;
  }
  begin() {
    this.frame = { rings: [], glare: 0, wash: 0, scan: null, rounded: 0 };
  }
  save() {
    this.stack.push([this.globalAlpha, this.lineWidth, this.strokeStyle, this.fillStyle]);
  }
  restore() {
    const s = this.stack.pop();
    if (s) [this.globalAlpha, this.lineWidth, this.strokeStyle, this.fillStyle] = s;
  }
  beginPath() {
    this.path = null;
  }
  arc(x, y, r) {
    this.path = { x, y, r };
  }
  stroke() {
    if (this.path) {
      this.frame.rings.push({
        r: this.path.r,
        lineWidth: this.lineWidth,
        color: String(this.strokeStyle),
        alpha: this.globalAlpha,
      });
    }
  }
  fill() {}
  // The two ways a canvas draws a rounded corner. Counting them is the whole
  // test for "the frame is not a rounded rectangle": the screen frame is four
  // chamfered brackets and the readout plate has its corners cut, so a single
  // call here means one of them has grown a radius back.
  arcTo() {
    this.frame.rounded++;
  }
  roundRect() {
    this.frame.rounded++;
  }
  fillRect(_x, y, w, h) {
    // A fill the size of the whole screen is the flat wash this design does
    // not have any more; a full-width band a few pixels tall is the scan.
    if (w === W && h === H) {
      const m = /rgba\([^)]*,\s*([\d.]+)\)/.exec(String(this.fillStyle));
      this.frame.wash = m ? Number(m[1]) : 1;
    } else if (w === W && h <= 4) {
      this.frame.scan = y;
    }
  }
  drawImage() {
    // The glare is blitted as four edge strips, all at the same alpha.
    this.frame.glare = Math.max(this.frame.glare, this.globalAlpha);
  }
  createRadialGradient() {
    return { addColorStop: noop };
  }
  createLinearGradient() {
    return { addColorStop: noop };
  }
  measureText() {
    return { width: 10 };
  }
}

const ctx = new Recorder();
for (const k of [
  "setTransform", "clearRect", "closePath", "moveTo", "lineTo", "rect",
  "setLineDash", "fillText", "strokeText", "clip", "translate", "scale",
]) {
  ctx[k] = noop;
}

const makeCanvas = () => ({
  getContext: () => ctx,
  getBoundingClientRect: () => ({ width: W, height: H }),
  width: 0,
  height: 0,
  style: {},
});
const canvas = makeCanvas();
Object.defineProperty(ctx, "canvas", { value: canvas });

globalThis.window = { devicePixelRatio: 2, addEventListener: noop };
globalThis.requestAnimationFrame = () => 0;
globalThis.cancelAnimationFrame = noop;
globalThis.getComputedStyle = () => ({ getPropertyValue: () => "0px" });
globalThis.document = {
  documentElement: {},
  hidden: false,
  addEventListener: noop,
  createElement: () => makeCanvas(),
};
globalThis.performance = { now: () => clock };

const { TrailRenderer } = await import(bundle);

const at = (phase, px, py) => ({ id: 0, phase, x: px / W, y: py / H, t: clock });

/**
 * Hold a finger still through the press and well past it, one frame at a time,
 * keeping what was drawn on each frame and when the drag armed.
 */
function holdAndRecord(totalMs) {
  clock = 0;
  const trail = new TrailRenderer(canvas);
  trail.setPressMs(PRESS_MS);
  trail.setTapMaxPx(10);
  let armedAt = null;
  trail.onPressArmed = () => (armedAt = frames.length);

  const frames = [];
  trail.push([at(DOWN, 200, 400)]);
  for (let t = 0; t <= totalMs; t += FRAME_MS) {
    clock = t;
    // The wobble a real hand has: enough to send samples, not enough to move.
    trail.push([at(MOVE, 200 + (t % 32 ? -0.3 : 0.3), 400)]);
    ctx.begin();
    trail["draw"]();
    frames.push({ t, ...ctx.frame });
  }
  return { frames, armedAt };
}

/** The fingertip ring: the bright stroke the eye actually follows. */
const ringOf = (f) => f.rings.filter((r) => r.alpha > 0.9 && r.r > 20).pop() ?? null;

const rgb = (c) => {
  const m = /rgb\((\d+),\s*(\d+),\s*(\d+)\)/.exec(c);
  if (m) return [+m[1], +m[2], +m[3]];
  const h = /^#([0-9a-f]{6})$/i.exec(c);
  return h ? [0, 2, 4].map((i) => parseInt(h[1].slice(i, i + 2), 16)) : null;
};
/** How green a colour is, relative to blue: 0 at the charge colour, 1 at armed. */
const greenness = (c) => {
  const v = rgb(c);
  return v ? v[1] / (v[1] + v[2]) : 0;
};

// Long enough to cover the press, the intro, and a full breath after it: the
// held level is only meaningful averaged over a whole cycle.
const { frames, armedAt } = holdAndRecord(PRESS_MS + INTRO_MS + GLARE_PERIOD_MS + 200);

const checks = [];
const check = (name, ok, detail = "") => checks.push([name, ok, detail]);

if (armedAt === null) {
  console.error("FAIL  the hold never armed — the seam cannot be checked");
  process.exit(1);
}

// `armedAt` is the frame index at which the ring completed, i.e. the first
// frame drawn in the armed state; the one before it is the last charging frame.
const before = ringOf(frames[armedAt - 1]);
const after = ringOf(frames[armedAt]);

check(
  "the ring keeps its radius across the seam",
  before && after && Math.abs(before.r - after.r) < 0.5,
  before && after ? `${before.r.toFixed(1)} -> ${after.r.toFixed(1)} px` : "no ring drawn",
);
check(
  "the ring keeps its stroke width across the seam",
  before && after && Math.abs(before.lineWidth - after.lineWidth) < 0.5,
  before && after ? `${before.lineWidth.toFixed(1)} -> ${after.lineWidth.toFixed(1)} px` : "",
);
check(
  "the ring does not jump colour on the arming frame",
  before && after && Math.abs(greenness(before.color) - greenness(after.color)) < 0.08,
  before && after ? `${before.color} -> ${after.color}` : "",
);

// ...and then travels to the armed colour rather than staying blue.
const settled = ringOf(frames[Math.min(frames.length - 1, armedAt + Math.ceil(300 / FRAME_MS))]);
check(
  "the ring has turned green shortly after",
  settled && before && greenness(settled.color) - greenness(before.color) > 0.3,
  settled && before
    ? `${greenness(before.color).toFixed(2)} -> ${greenness(settled.color).toFixed(2)} green`
    : "",
);

// The glare must be lit on the very first armed frame — brighter than it will
// settle at — and must never drop out on the way down.
const glare = frames.slice(armedAt).map((f) => f.glare);
check(
  "the glare arrives overbright, not from nothing",
  glare[0] > GLARE_ALPHA,
  `first armed frame at ${glare[0].toFixed(2)} vs held ${GLARE_ALPHA}`,
);
const introFrames = Math.ceil(INTRO_MS / FRAME_MS);
check(
  "the glare stays lit for the whole intro",
  glare.slice(0, introFrames).every((a) => a >= GLARE_ALPHA - 0.01),
  `min ${Math.min(...glare.slice(0, introFrames)).toFixed(2)}`,
);
// Once the intro is over the bloom must be spent: from there the glare is the
// breathing hold and nothing else, so it has to sit inside the breath's own
// envelope and average out at the held level over a full period.
const held = glare.slice(introFrames, introFrames + Math.ceil(GLARE_PERIOD_MS / FRAME_MS));
const mean = held.reduce((a, b) => a + b, 0) / held.length;
check(
  "the bloom is spent by the end of the intro",
  Math.max(...held) <= GLARE_ALPHA + GLARE_BREATH + 0.01,
  `peak ${Math.max(...held).toFixed(2)} vs ceiling ${(GLARE_ALPHA + GLARE_BREATH).toFixed(2)}`,
);
check(
  "the glare settles on its held level",
  Math.abs(mean - GLARE_ALPHA) < 0.06,
  `mean ${mean.toFixed(2)} over one breath`,
);

// The wash is a tint over that bloom, not a second event: it must start with
// the intro and be gone by the end of it.
// The arming beat: a line crossing the screen, not a rectangle of colour laid
// over it. A flat wash is unmissable and structureless - the one moment in the
// gesture with nothing happening in it - and it costs a fill of every pixel on
// the display on exactly the frames where cursor movement matters most.
const wash = frames.slice(armedAt).map((f) => f.wash);
check("no flat wash over the screen", Math.max(...wash) === 0, `peak ${Math.max(...wash)}`);

const scans = frames.slice(armedAt).map((f) => f.scan);
const travelled = scans.filter((y) => y !== null);
check(
  "the scan crosses the whole screen",
  travelled.length > 3 &&
    travelled[0] < H * 0.25 &&
    travelled[travelled.length - 1] > H * 0.7,
  `${travelled[0]?.toFixed(0)} -> ${travelled[travelled.length - 1]?.toFixed(0)} of ${H}px`,
);
check(
  "and is over within the intro",
  scans.slice(introFrames).every((y) => y === null),
  `${travelled.length} frames`,
);

// The screen frame reads as an instrument acquiring something, not as chrome
// the page has always had - and a rounded rectangle reads as chrome however it
// is animated. It is also a guess at the phone's own corner radius, which is
// wrong on most phones. Nothing in the whole gesture may draw one.
const rounded = frames.reduce((n, f) => n + f.rounded, 0);
check("nothing draws a rounded corner", rounded === 0, `${rounded} arcTo/roundRect calls`);

rmSync(out, { recursive: true, force: true });

let failed = 0;
for (const [name, ok, detail] of checks) {
  if (!ok) failed++;
  console.log(`${ok ? "ok  " : "FAIL"}  ${name}${detail ? `  (${detail})` : ""}`);
}
if (failed) {
  console.error(`\n${failed} of ${checks.length} failed: the confirmation does not grow out of the hold.`);
  process.exit(1);
}
console.log(`\n${checks.length} passed: the drag confirmation grows out of the hold.`);
