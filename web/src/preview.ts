/**
 * A bench for the press animation, driven by the real renderer.
 *
 * The hold indicator is the one part of the phone page that cannot be judged
 * from code or from a screenshot: it is four animations meeting over about a
 * second, and the only question that matters - does it look like one thing
 * happening? - needs eyes on it. Until now the only way to see it was to pair a
 * phone and hold a finger on the glass.
 *
 * This is the same `TrailRenderer` the phone runs, on the same canvas element,
 * fed the same sample stream. Nothing here re-implements any of the drawing, so
 * what you see is what the phone does - at whatever size the window happens to
 * be, which is also the fastest way to check the frame still sits right on a
 * different aspect ratio.
 */

import "./theme.css";
import { Phase, type TouchSample } from "./protocol";
import { TrailRenderer } from "./trail";
import { trackViewport } from "./viewport";

// The bench shares the pad's stylesheet, so it needs the pad's viewport height
// published too - otherwise it falls back to `100dvh` and stops being a fair
// picture of what the phone does.
trackViewport();

const canvas = document.getElementById("trail") as HTMLCanvasElement;
const trail = new TrailRenderer(canvas);
const status = document.getElementById("state")!;

// The desktop's real defaults, so the ring finishes when a drag really would.
trail.setPressMs(500);
trail.setTapMaxPx(10);
trail.onPressArmed = () => say("armed - the drag has started");
trail.onPressReleased = () => say("released");

function say(what: string): void {
  status.textContent = what;
}

const sample = (phase: Phase, x: number, y: number): TouchSample => ({
  t: performance.now(),
  id: 1,
  phase,
  x: x / window.innerWidth,
  y: y / window.innerHeight,
});

// ------------------------------------------------------------- by hand
//
// Press and hold anywhere. Deliberately wired straight to the renderer rather
// than through `Surface`: this page is not a trackpad and has no desktop to
// talk to, and the mouse-pointer guard that protects the real surface would
// leave nothing to test with here.
let drawing = false;
canvas.addEventListener("pointerdown", (e) => {
  drawing = true;
  // A synthetic pointer (this page is also driven from a script) has no real
  // capture to take, and the throw would abandon the press half-started.
  try {
    canvas.setPointerCapture(e.pointerId);
  } catch {
    /* nothing to capture; the listeners below still see the whole gesture */
  }
  trail.push([sample(Phase.Down, e.clientX, e.clientY)]);
  say("holding…");
});
canvas.addEventListener("pointermove", (e) => {
  if (!drawing) return;
  trail.push([sample(Phase.Move, e.clientX, e.clientY)]);
});
const lift = (e: PointerEvent) => {
  if (!drawing) return;
  drawing = false;
  trail.push([sample(Phase.Up, e.clientX, e.clientY)]);
  say("press and hold anywhere");
};
canvas.addEventListener("pointerup", lift);
canvas.addEventListener("pointercancel", lift);

// ------------------------------------------------------------ scripted
//
// The whole gesture at its intended pace, for judging the timing rather than
// the shapes: land, charge, arm, drag, release.
let script: number[] = [];
function play(): void {
  script.forEach(clearTimeout);
  drawing = false;
  const cx = window.innerWidth / 2;
  const cy = window.innerHeight / 2;
  const at = (ms: number, fn: () => void) => script.push(window.setTimeout(fn, ms));

  say("landing…");
  trail.push([sample(Phase.Down, cx, cy)]);
  // Nothing is sent while it charges: a finger resting perfectly still emits no
  // events, and the animation has to hold up on the frame clock alone.
  at(1200, () => {
    say("dragging - the finger moves and the button stays down");
    // A slow arc, so the held state is seen travelling rather than parked.
    for (let i = 0; i <= 40; i++) {
      at(1200 + i * 25, () => {
        const a = (i / 40) * Math.PI;
        trail.push([sample(Phase.Move, cx + Math.cos(a) * 90, cy - Math.sin(a) * 60)]);
      });
    }
  });
  at(2400, () => {
    trail.push([sample(Phase.Up, cx - 90, cy)]);
    say("released - press and hold anywhere, or replay");
  });
}

document.getElementById("play")!.addEventListener("click", play);

// --------------------------------------------------------- frame by frame
//
// A design review needs to stop on an exact millisecond - the arming beat is
// 380 ms long and its first frame is the one that matters most - and a live
// animation cannot be caught there by hand. Worse, a browser pauses
// `requestAnimationFrame` outright in a background tab, so "wait 500 ms and
// look" is not even reliable.
//
// `padremotePreview.at(600)` replays the press from scratch, one 16 ms frame at
// a time, and leaves the clock stopped exactly 600 ms in - so the canvas holds
// that frame for as long as it takes to look at it, or to photograph it.
const realNow = performance.now.bind(performance);
function freeze(t: number): void {
  performance.now = () => t;
}

function at(ms: number): void {
  const base = realNow();
  const cx = window.innerWidth / 2;
  const cy = window.innerHeight / 2;
  freeze(base);
  trail.clear();
  trail.push([sample(Phase.Down, cx, cy)]);
  // Stepping matters: the hold arms *inside* a draw, so the frames between
  // landing and `ms` have to actually happen for the state to be right.
  for (let t = 0; t <= ms; t += 16) {
    freeze(base + t);
    (trail as unknown as { draw(): void }).draw();
  }
  say(`stopped ${ms} ms after the finger landed`);
}

function live(): void {
  performance.now = realNow;
  trail.clear();
  say("press and hold anywhere");
}

declare global {
  interface Window {
    padremotePreview: { at(ms: number): void; live(): void };
  }
}
window.padremotePreview = { at, live };

say("press and hold anywhere");
