/**
 * A standalone haptics probe, for a device that will not buzz.
 *
 * The Vibration API fails silently by design: `navigator.vibrate` returns a
 * boolean nobody checks, throws nothing, and logs nothing the phone can show.
 * So "it doesn't vibrate" covers three completely different faults - no API, no
 * user activation, or a phone with haptics switched off - and they need
 * separating before any of them can be fixed.
 *
 * The interesting axis is *what kind of touch* the browser will accept a
 * vibration from. Chrome grants a page its user activation from a completed
 * tap, and explicitly not from a finger going down, which is exactly the moment
 * a long-press has to buzz at. This page makes that difference visible.
 */

import "./theme.css";
import { clickDown, clickUp, primeClick, soundReport } from "./sound";

const $ = (id: string) => document.getElementById(id)!;
// Keep the selected computer when returning to its trackpad or settings.
const connectionHost = new URLSearchParams(location.search).get("h");
for (const anchor of document.querySelectorAll<HTMLAnchorElement>("[data-app-link]")) {
  if (!connectionHost) continue;
  const target = new URL(anchor.href);
  target.searchParams.set("h", connectionHost);
  anchor.href = target.href;
}

/** How long a hold must last to count, matching the app's own long press. */
const HOLD_MS = 500;
/** The app's real pulse width. */
const PULSE = 30;

interface Activation {
  hasBeenActive: boolean;
  isActive: boolean;
}

function activation(): Activation | null {
  const nav = navigator as Navigator & { userActivation?: Activation };
  const ua = nav.userActivation;
  return ua ? { hasBeenActive: ua.hasBeenActive, isActive: ua.isActive } : null;
}

// ------------------------------------------------------------------ environment

function renderEnv(): void {
  const ua = activation();
  const rows: [string, string, boolean][] = [
    [
      "navigator.vibrate",
      typeof navigator.vibrate === "function" ? "present" : "missing — this browser has no Vibration API",
      typeof navigator.vibrate === "function",
    ],
    [
      "sticky activation",
      ua === null ? "unknown (no userActivation API)" : ua.hasBeenActive ? "granted" : "not yet — vibrate will be refused",
      ua === null || ua.hasBeenActive,
    ],
    [
      "transient activation",
      ua === null ? "unknown" : ua.isActive ? "live" : "expired",
      true,
    ],
    ["secure context", isSecureContext ? "yes (https or localhost)" : "no (plain http)", true],
    [
      "click sound",
      // The fallback for everything above: worth reporting next to it, because
      // "this phone cannot buzz" and "this phone cannot signal at all" are very
      // different verdicts.
      soundReport().api ? (soundReport().running ? "ready" : "available (waiting for a touch)") : "no audio support",
      soundReport().api,
    ],
    [
      "display mode",
      matchMedia("(display-mode: standalone)").matches ? "installed / standalone" : "browser tab",
      true,
    ],
  ];
  $("env").innerHTML = rows
    .map(
      ([k, v, ok]) =>
        `<tr><td>${k}</td><td><span class="tag ${ok ? "ok" : "bad"}">${v}</span></td></tr>`,
    )
    .join("");
}

// ----------------------------------------------------------------------- tests

let n = 0;

/**
 * Fire one vibration and record everything that could explain the outcome.
 *
 * The activation state is sampled *at the moment of the call*, because that is
 * the only moment that decides anything.
 */
function probe(what: string, pattern: number | number[] = PULSE): void {
  const before = activation();
  let accepted: boolean | null = null;
  try {
    accepted = navigator.vibrate ? navigator.vibrate(pattern) : null;
  } catch (e) {
    accepted = null;
    what += ` (threw: ${String(e)})`;
  }

  const verdict =
    accepted === null
      ? ["no API", "bad"]
      : accepted
        ? ["accepted", "ok"]
        : ["REFUSED", "bad"];
  const sticky = before === null ? "?" : before.hasBeenActive ? "sticky ✓" : "sticky ✗";
  const transient = before === null ? "?" : before.isActive ? "transient ✓" : "transient ✗";

  const li = document.createElement("li");
  li.innerHTML =
    `<time>${String(++n).padStart(2, "0")}</time>` +
    `<b>${what}</b>` +
    `<span class="tag ${verdict[1]}">${verdict[0]}</span>` +
    `<i>${sticky} · ${transient}</i>`;
  $("log").prepend(li);
  renderEnv();
  showVerdict(accepted);
}

/**
 * Say what the result means, because the raw boolean is routinely misread.
 *
 * `true` is not "it vibrated" - it is only "Chrome handed the request to
 * Android". Every interesting failure left at that point is below the browser,
 * and a page that does not say so sends people back to edit JavaScript that was
 * already doing the right thing.
 */
function showVerdict(accepted: boolean | null): void {
  const el = $("verdict");
  el.hidden = false;
  if (accepted === null) {
    el.className = "bad";
    el.innerHTML = "<b>No Vibration API.</b> This browser cannot vibrate at all.";
  } else if (!accepted) {
    el.className = "bad";
    el.innerHTML =
      "<b>Chrome refused the call.</b> The page has no user activation yet - " +
      "tap test 1, then try again. This is a browser-side problem and the app can fix it.";
  } else {
    el.className = "ok";
    el.innerHTML =
      "<b>Chrome accepted the call</b> and passed it to Android. If you felt nothing, " +
      "the browser is not the problem - the vibration is being lost in the device. " +
      "See the checklist below.";
  }
}

// 1 — a plain click. This is what the Chrome sample page does, and it is the
// one kind of gesture every browser agrees grants activation.
$("t-click").addEventListener("click", () => probe("click"));

// 2 — touchstart. Chrome deliberately does *not* treat a finger going down as
// a gesture, so on a page that has had no tap yet this is expected to fail.
$("t-start").addEventListener(
  "touchstart",
  (e) => {
    e.preventDefault(); // exactly what the trackpad surface used to do everywhere
    probe("touchstart (default prevented)");
  },
  { passive: false },
);

// 3 — touchend: the completed tap, which is what actually grants activation.
$("t-end").addEventListener("touchend", () => probe("touchend"));

// 4 — the real thing: a timer that fires mid-gesture, finger still down, from a
// rAF callback rather than an event handler. This is the app's long press.
let holdStart = 0;
let holdRaf = 0;
const holdBtn = $("t-hold");

function holdFrame(): void {
  if (!holdStart) return;
  const held = performance.now() - holdStart;
  holdBtn.style.setProperty("--fill", `${Math.min(100, (held / HOLD_MS) * 100)}%`);
  if (held >= HOLD_MS) {
    holdStart = 0;
    holdBtn.style.removeProperty("--fill");
    probe(`hold ${HOLD_MS}ms, finger still down (rAF)`);
    return;
  }
  holdRaf = requestAnimationFrame(holdFrame);
}

holdBtn.addEventListener(
  "pointerdown",
  () => {
    holdStart = performance.now();
    cancelAnimationFrame(holdRaf);
    holdRaf = requestAnimationFrame(holdFrame);
  },
  { passive: true },
);
for (const type of ["pointerup", "pointercancel", "pointerleave"]) {
  holdBtn.addEventListener(type, () => {
    holdStart = 0;
    cancelAnimationFrame(holdRaf);
    holdBtn.style.removeProperty("--fill");
  });
}

// 5 - two seconds solid. A 30 ms pulse is genuinely imperceptible on a slab
// this size; this one is not missable on any device that has a motor at all,
// which makes "accepted but nothing felt" a conclusion rather than a guess.
$("t-long").addEventListener("click", () => probe("2000ms solid (click)", 2000));

// 6 - a pattern, because some vendors honour bursts while ignoring a steady
// pulse. Numbers alternate buzz/pause, starting with buzz.
$("t-pattern").addEventListener("click", () =>
  probe("pattern 300/150 x3 (click)", [300, 150, 300, 150, 300]),
);

// The fallback: what a phone that cannot vibrate can still do. A click is a
// real user gesture, which is exactly when audio is allowed to start, so this
// both unlocks the context and demonstrates it.
$("t-sound").addEventListener("click", () => {
  primeClick();
  const played = clickDown();
  // The release click, a beat later, so the pair is heard as a pair.
  window.setTimeout(() => clickUp(), 160);
  const li = document.createElement("li");
  li.innerHTML =
    `<time>${String(++n).padStart(2, "0")}</time><b>click sound</b>` +
    `<span class="tag ${played ? "ok" : "bad"}">${played ? "played" : "no audio"}</span>` +
    `<i>${played ? "silent? check the mute switch" : "this device has no usable audio"}</i>`;
  $("log").prepend(li);
  renderEnv();
});

$("reload").addEventListener("click", () => location.reload());

renderEnv();
