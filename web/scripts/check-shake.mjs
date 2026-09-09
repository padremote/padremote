/**
 * Does a shake read as a shake, and does everything else not?
 *
 * This is the hardest thing in the project to test by hand: it needs a phone,
 * two hands and a judgement about whether what just happened was "a shake". So
 * it is driven here instead, from synthetic accelerometer readings, against the
 * real detector.
 *
 * The failure modes are what the cases below are drawn from. A threshold that
 * is too eager turns a brisk scroll into a mode change; one that is too shy
 * makes the feature look broken; and either way the user is holding the phone
 * *while using it as a trackpad*, which is the worst possible place to get it
 * wrong.
 *
 *   npm run check:shake
 */

import { execFileSync } from "node:child_process";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

const out = mkdtempSync(join(tmpdir(), "shake-"));
const bundle = join(out, "shake.mjs");
execFileSync(
  "npx",
  ["esbuild", "src/shake.ts", "--bundle", "--format=esm", `--outfile=${bundle}`, "--log-level=error"],
  { stdio: "inherit" },
);

// The module talks to `window` and nothing else.
let listener = null;
globalThis.window = {
  addEventListener: (type, fn) => {
    if (type === "devicemotion") listener = fn;
  },
  removeEventListener: () => {
    listener = null;
  },
};
globalThis.isSecureContext = true;
globalThis.DeviceMotionEvent = function () {};
globalThis.localStorage = { getItem: () => null, setItem: () => {} };

const { watchShake } = await import(bundle);

/** Gravity, on the axis pointing down. */
const REST = 9.81;

/**
 * Drive the detector with one reading, as iOS reports them: the hand's own
 * acceleration along x, and the same thing again with gravity added in.
 *
 * `jolt` is signed - positive one way, negative the other - because direction
 * is the whole basis of the detection.
 */
function reading(t, jolt, { withGravityOnly = false } = {}) {
  listener?.({
    timeStamp: t,
    acceleration: withGravityOnly
      ? { x: null, y: null, z: null }
      : { x: jolt, y: 0, z: 0 },
    accelerationIncludingGravity: { x: jolt, y: REST, z: 0 },
  });
}

/** A hand shaking: several reversals of direction inside a second. */
function shake(from, { jolt = 18, reversals = 4, step = 90, ...rest } = {}) {
  for (let i = 0; i < reversals; i++) {
    reading(from + i * step, i % 2 ? -jolt : jolt, rest);
  }
}

const checks = [];
function check(name, ok, detail = "") {
  checks.push([name, ok, detail]);
}

function run(steps, { busy = false } = {}) {
  let fired = 0;
  const stop = watchShake({ busy: () => busy, onShake: () => fired++ });
  steps();
  stop();
  return fired;
}

// --------------------------------------------------------------- the cases

check("a deliberate shake fires once", run(() => shake(1000)) === 1);

check(
  "one sharp knock does not",
  run(() => {
    reading(1000, 30);
    reading(1100, 0);
  }) === 0,
  "a phone set down hard is a single spike, not a shake",
);

check(
  "ordinary handling does not",
  run(() => {
    // Walking about with the phone: real but gentle movement, well under the
    // threshold, for two seconds.
    for (let t = 0; t < 2000; t += 40) reading(t, Math.sin(t / 120) * 6);
  }) === 0,
);

check(
  "slow rocking does not",
  run(() => {
    // Big movements, but far too slow to be a shake: one reversal a second.
    for (let i = 0; i < 6; i++) reading(i * 1000, i % 2 ? -20 : 20);
  }) === 0,
  "reversals have to be inside one window, or turning round sets it off",
);

check(
  "nothing fires while a finger is on the pad",
  run(() => shake(1000), { busy: true }) === 0,
  "the phone is being used as a trackpad; a flick during a scroll must not count",
);

check(
  "one shake is never two",
  run(() => {
    shake(1000);
    // The tail of the same shake, still inside the cooldown.
    shake(1300);
  }) === 1,
);

check(
  "a phone that reports only gravity-inclusive readings still works",
  run(() => {
    // Android devices commonly leave `acceleration` null. The detector has to
    // find gravity itself and subtract it.
    for (let t = 0; t < 400; t += 40) reading(t, 0, { withGravityOnly: true });
    shake(500, { withGravityOnly: true, reversals: 6 });
  }) === 1,
);

check(
  "a second, separate shake does fire",
  run(() => {
    shake(1000);
    shake(4000);
  }) === 2,
  "after the cooldown, shaking again means what it says",
);

rmSync(out, { recursive: true, force: true });

let failed = 0;
for (const [name, ok, detail] of checks) {
  if (!ok) failed++;
  console.log(`${ok ? "ok  " : "FAIL"}  ${name}${detail ? `  (${detail})` : ""}`);
}
if (failed) {
  console.error(`\n${failed} of ${checks.length} failed: the shake detector disagrees with a hand.`);
  process.exit(1);
}
console.log(`\n${checks.length} passed: a shake reads as a shake, and nothing else does.`);
