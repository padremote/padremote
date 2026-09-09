/**
 * Does the long-press ring tell the truth?
 *
 * The ring on the phone is a *preview* of a decision the desktop makes. When
 * the two disagree the phone promises a drag that never happens - and the bug
 * is invisible, because both halves look right on their own. This drives the
 * real `TrailRenderer` through the three touches that pin the rule down:
 *
 *   - a finger that has moved cannot arm a drag by pausing
 *   - a finger that lands and rests still can
 *   - lifting clears the verdict, so press-again works
 *
 * The engine's side of the same rule is pinned by
 * `desktop/tests/press_after_move.rs`. Run both when either changes.
 *
 *   npm run check:hold
 *
 * No test framework: the module needs a canvas and a clock, not a runner, and
 * stubbing those is the whole job.
 */

import { execFileSync } from "node:child_process";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

const W = 390;
const H = 716;
const DOWN = 0;
const MOVE = 1;
const UP = 2;
/** Matches the desktop's `tap.pressMs` and `tap.tapMaxPx` defaults. */
const PRESS_MS = 500;
const TAP_MAX_PX = 10;

// Bundle to plain JS so node can load it. esbuild ships inside vite.
const out = mkdtempSync(join(tmpdir(), "hold-ring-"));
const bundle = join(out, "trail.mjs");
execFileSync(
  "npx",
  ["esbuild", "src/trail.ts", "--bundle", "--format=esm", `--outfile=${bundle}`, "--log-level=error"],
  { stdio: "inherit" },
);

let clock = 0;
const noop = () => {};
const gradient = { addColorStop: noop };
const makeCanvas = () => ({
  getContext: () => ctx,
  getBoundingClientRect: () => ({ width: W, height: H }),
  width: 0,
  height: 0,
  style: {},
});
const canvas = makeCanvas();
const ctx = new Proxy(
  {},
  {
    get: (_t, k) => {
      if (k === "canvas") return canvas;
      if (typeof k === "string" && k.startsWith("create")) return () => gradient;
      if (k === "measureText") return () => ({ width: 10 });
      return noop;
    },
    set: () => true,
  },
);
globalThis.window = { devicePixelRatio: 2, addEventListener: noop };
globalThis.requestAnimationFrame = () => 0;
globalThis.cancelAnimationFrame = noop;
globalThis.getComputedStyle = () => ({ getPropertyValue: () => "0px" });
globalThis.document = {
  documentElement: {},
  hidden: false,
  addEventListener: noop,
  // The renderer builds its edge glare into an offscreen canvas on resize.
  createElement: () => makeCanvas(),
};
globalThis.performance = { now: () => clock };

const { TrailRenderer } = await import(bundle);

const at = (phase, px, py) => ({ id: 0, phase, x: px / W, y: py / H, t: clock });

/** Feed one touch and report whether the ring ever completed. */
function armsRing(steps) {
  clock = 0;
  const trail = new TrailRenderer(canvas);
  trail.setPressMs(PRESS_MS);
  trail.setTapMaxPx(TAP_MAX_PX);
  let armed = 0;
  trail.onPressArmed = () => armed++;
  for (const [advance, samples] of steps) {
    clock += advance;
    trail.push(samples);
    // The ring completes while drawing; the page does this every frame.
    trail["draw"]();
  }
  return armed > 0;
}

/** Steer 120 px, then stop dead and rest for two seconds without lifting. */
function moveThenRest() {
  const s = [[0, [at(DOWN, 100, 400)]]];
  for (let i = 1; i <= 15; i++) s.push([8, [at(MOVE, 100 + i * 8, 400)]]);
  for (let i = 0; i < 125; i++) s.push([16, [at(MOVE, 220, 400)]]);
  return s;
}

/** Land and stay put, with only the wobble a real hand has. */
function rest(fromX) {
  const s = [];
  for (let i = 0; i < 60; i++) s.push([16, [at(MOVE, fromX + (i % 2 ? -0.4 : 0.4), 400)]]);
  return s;
}

const cases = [
  ["a pause mid-move must not charge the ring", moveThenRest(), false],
  ["landing and resting still must charge it", [[0, [at(DOWN, 200, 400)]], ...rest(200)], true],
  [
    "lifting clears it, so press-again works",
    [...moveThenRest().slice(0, 17), [8, [at(UP, 220, 400)]], [800, [at(DOWN, 220, 400)]], ...rest(220)],
    true,
  ],
];

let failed = 0;
for (const [name, steps, want] of cases) {
  const got = armsRing(steps);
  const ok = got === want;
  if (!ok) failed++;
  console.log(`${ok ? "ok  " : "FAIL"}  ${name}${ok ? "" : ` (ring ${got ? "armed" : "did not arm"})`}`);
}

rmSync(out, { recursive: true, force: true });
if (failed) {
  console.error(`\n${failed} of ${cases.length} failed: the ring disagrees with the engine.`);
  process.exit(1);
}
console.log(`\n${cases.length} passed: the ring agrees with the engine.`);
