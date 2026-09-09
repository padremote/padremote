/**
 * Does a touch reach the wire when it has to, and what gets dropped when it can't?
 *
 * Three rules live here, and all three are about latency rather than about what
 * a gesture means:
 *
 *   1. A finger-down goes out at once, without waiting for the next animation
 *      frame. That sample is the one the desktop decides *who is driving* on,
 *      so a frame spent queueing it is a frame on the front of every gesture
 *      and of every handover between two devices.
 *   2. The surface rect is read once per event, not once per sample. A single
 *      `pointermove` can carry a dozen coalesced points, and reading it per
 *      point meant a dozen forced layouts inside a handler the browser's own
 *      input pipeline is waiting on.
 *   3. When the socket is falling behind, stale positions are dropped and phase
 *      changes never are. A dropped `Up` is a stuck button.
 *
 * None of it is visible on screen, which is exactly why it is asserted here.
 *
 *   npm run check:surface
 */

import { execFileSync } from "node:child_process";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

const out = mkdtempSync(join(tmpdir(), "surface-"));
const bundle = join(out, "surface.mjs");
execFileSync(
  "npx",
  [
    "esbuild",
    "src/surface.ts",
    "--bundle",
    "--format=esm",
    `--outfile=${bundle}`,
    "--log-level=error",
  ],
  { stdio: "inherit" },
);

// ------------------------------------------------------------------ the stub

let rectReads = 0;
const listeners = new Map();
let rafs = [];

const el = {
  clientWidth: 390,
  clientHeight: 716,
  addEventListener: (type, fn) => listeners.set(type, fn),
  setPointerCapture: () => {},
  getBoundingClientRect: () => {
    rectReads++;
    return { left: 0, top: 0, width: 390, height: 716 };
  },
};

globalThis.window = {
  devicePixelRatio: 3,
  addEventListener: () => {},
  removeEventListener: () => {},
  visualViewport: null,
};
// Node 22 defines `navigator` as a getter-only global, so it has to be
// replaced rather than assigned.
Object.defineProperty(globalThis, "navigator", {
  value: { userAgent: "iPhone", platform: "iPhone", maxTouchPoints: 5 },
  configurable: true,
});
globalThis.location = { search: "" };
globalThis.document = { getElementById: () => null, addEventListener: () => {} };
globalThis.requestAnimationFrame = (fn) => {
  rafs.push(fn);
  return rafs.length;
};
globalThis.ResizeObserver = class {
  observe() {}
  disconnect() {}
};

const { Surface } = await import(bundle);

/** One pointer event, with however many coalesced points it carries. */
function ev(x, y, coalesced = []) {
  return {
    pointerId: 1,
    pointerType: "touch",
    clientX: x,
    clientY: y,
    timeStamp: 100,
    cancelable: true,
    preventDefault: () => {},
    getCoalescedEvents: () => coalesced,
  };
}

function point(x, y) {
  return { ...ev(x, y), getCoalescedEvents: () => [] };
}

/** A fresh surface, and the batches it hands out. */
function surface() {
  const batches = [];
  rectReads = 0;
  rafs = [];
  listeners.clear();
  new Surface(el, { onBatch: (s) => batches.push(s) });
  return {
    batches,
    down: (e) => listeners.get("pointerdown")(e),
    move: (e) => listeners.get("pointermove")(e),
    up: (e) => listeners.get("pointerup")(e),
    /** Run the surface's own animation frame, which is what flushes moves. */
    frame: () => {
      const pending = rafs;
      rafs = [];
      for (const fn of pending) fn(performance.now());
    },
  };
}

const checks = [];
const check = (name, ok, detail) => checks.push([name, ok, detail]);

// ------------------------------------------------------------------- 1. down

{
  const s = surface();
  s.down(ev(10, 10));
  check(
    "a finger-down is sent without waiting for a frame",
    s.batches.length === 1 && s.batches[0][0].phase === 0,
    `got ${s.batches.length} batches`,
  );
}

{
  const s = surface();
  s.down(ev(10, 10));
  s.batches.length = 0;
  s.move(ev(20, 20));
  check(
    "a move still waits for the frame, so one per frame goes out and not one per touch",
    s.batches.length === 0,
    `got ${s.batches.length} batches before the frame`,
  );
  s.frame();
  check("and arrives on it", s.batches.length === 1);
}

{
  const s = surface();
  s.down(ev(10, 10));
  s.batches.length = 0;
  s.up(ev(10, 10));
  check(
    "a finger-up is still sent at once - a late release is a stuck button",
    s.batches.length === 1 && s.batches[0][0].phase === 2,
  );
}

// ------------------------------------------------------------------- 2. rect

{
  const s = surface();
  s.down(ev(10, 10));
  rectReads = 0;
  // One event carrying five coalesced points, as a 120 Hz digitiser reports.
  s.move(ev(50, 50, [point(20, 20), point(30, 30), point(40, 40), point(45, 45), point(50, 50)]));
  check(
    "the surface rect is read once per event, not once per coalesced sample",
    rectReads === 1,
    `read it ${rectReads} times for 5 samples`,
  );
  s.frame();
  check(
    "and every coalesced point still reaches the wire",
    s.batches.at(-1).length === 5,
    `got ${s.batches.at(-1).length}`,
  );
}

// ----------------------------------------------------------- 3. backpressure

// The thinning lives in DesktopLink, so it is driven through the real one.
const netBundle = join(out, "net.mjs");
execFileSync(
  "npx",
  ["esbuild", "src/net.ts", "--bundle", "--format=esm", `--outfile=${netBundle}`, "--log-level=error"],
  { stdio: "inherit" },
);
globalThis.WebSocket = class {
  static CONNECTING = 0;
  static OPEN = 1;
};
globalThis.localStorage = { getItem: () => null, setItem: () => {} };
const { DesktopLink } = await import(netBundle);

/** A link that believes it is connected, over a socket that is `backlog` behind. */
function link(backlog) {
  const sent = [];
  const l = Object.create(DesktopLink.prototype);
  l.ws = { readyState: 1, bufferedAmount: backlog, send: (b) => sent.push(b) };
  l.authed = true;
  return { l, sent };
}

const stream = [
  { t: 0, id: 1, phase: 0, x: 0.1, y: 0.1 },
  { t: 1, id: 1, phase: 1, x: 0.2, y: 0.2 },
  { t: 2, id: 1, phase: 1, x: 0.3, y: 0.3 },
  { t: 3, id: 1, phase: 1, x: 0.4, y: 0.4 },
  { t: 4, id: 1, phase: 2, x: 0.4, y: 0.4 },
];

{
  const { l, sent } = link(0);
  l.sendSamples(stream);
  check(
    "a link that is keeping up sends every sample",
    sent[0].byteLength === 2 + 14 * 5,
    `${sent[0].byteLength} bytes`,
  );
}

{
  const { l, sent } = link(4096);
  l.sendSamples(stream);
  // Down, the newest Move, and Up: the two intermediate positions are stale.
  check(
    "a link that is behind drops stale positions",
    sent[0].byteLength === 2 + 14 * 3,
    `${sent[0].byteLength} bytes, expected ${2 + 14 * 3}`,
  );
}

{
  const { l, sent } = link(4096);
  l.sendSamples(stream);
  const view = new DataView(sent[0]);
  const phases = [];
  for (let i = 0; i < view.getUint8(1); i++) phases.push(view.getUint8(2 + i * 14 + 5));
  check(
    "and never a phase change - a dropped Up is a stuck button, a dropped Down is a lost handover",
    phases[0] === 0 && phases.at(-1) === 2,
    `phases ${phases.join(",")}`,
  );
}

{
  const { l, sent } = link(4096);
  l.sendSamples([
    { t: 0, id: 1, phase: 0, x: 0.1, y: 0.1 },
    { t: 1, id: 1, phase: 2, x: 0.1, y: 0.1 },
  ]);
  check(
    "a batch with no moves in it is untouched however far behind the link is",
    sent[0].byteLength === 2 + 14 * 2,
  );
}

rmSync(out, { recursive: true, force: true });

let failed = 0;
for (const [name, ok, detail] of checks) {
  if (!ok) failed++;
  console.log(`${ok ? "ok  " : "FAIL"}  ${name}${detail ? `  (${detail})` : ""}`);
}
if (failed) {
  console.error(`\n${failed} of ${checks.length} failed: the input path is not sending what it should.`);
  process.exit(1);
}
console.log(`\n${checks.length} passed: touches go out promptly, and only stale ones are dropped.`);
