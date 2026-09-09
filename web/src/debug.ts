/**
 * Live debug view — the shared window onto what the phone is actually sending.
 *
 * Connects to the desktop app as an **observer** (`/observe`), which is a
 * read-only role: it never claims control of the cursor, so it can be left open
 * on the computer while someone drives from their phone.
 *
 * The point of this page is to separate the three things that all feel like
 * "it's not smooth":
 *   - the phone not capturing cleanly  → the surface view looks ragged
 *   - the network delivering unevenly  → the gap chart is spiky
 *   - the desktop mis-reading gestures → the events list disagrees with the hand
 */

import "./theme.css";
import { hapticsReport, TAP_PATTERN } from "./haptics";
import { authReply, socketUrl, storedLink, takeKeyFromFragment } from "./pairing";

/** The desktop challenges every connection, this read-only one included. */
interface Challenge {
  t: "challenge";
  nonce: string;
}

interface Batch {
  t: "batch";
  /** What the phone calls itself, so two hands can be told apart. */
  device: string;
  deviceId: number;
  /** False when this device is connected but another one is driving. */
  held: boolean;
  samples: number;
  gapMs: number | null;
  gesture: string;
  fingers: number;
  peakFingers: number;
  actions: string[];
  movePx: number;
  /** [pointerId, phase, x, y] per sample, normalized. */
  points: [number, number, number, number][];
}

interface Hello {
  t: "hello";
  computer: string;
  settings: { setting: string; value: string; status: string; detail: string }[];
}

const $ = (id: string) => document.getElementById(id)!;
// Keep the selected computer when moving between the supporting pages.
const connectionHost = new URLSearchParams(location.search).get("h");
for (const anchor of document.querySelectorAll<HTMLAnchorElement>("[data-app-link]")) {
  if (!connectionHost) continue;
  const target = new URL(anchor.href);
  target.searchParams.set("h", connectionHost);
  anchor.href = target.href;
}
const dot = $("dot");
const pad = $("pad") as HTMLCanvasElement;
const chart = $("chart") as HTMLCanvasElement;
const events = $("events") as HTMLUListElement;

const COLORS = ["#2f6feb", "#3fb950", "#d29922", "#db61a2", "#a371f7"];

// ------------------------------------------------------------------- haptics

/**
 * The vibration self-test.
 *
 * A silent phone says nothing about *why* it is silent, so this reports the two
 * facts that separate the cases: whether the API exists at all, and whether the
 * browser considers the page activated. Chrome refuses to vibrate an
 * unactivated page and says so nowhere the phone can see.
 */
function runHapticsTest(probe: boolean): void {
  const r = hapticsReport(probe);
  const rows: [string, string, boolean][] = [
    ["navigator.vibrate", r.api ? "present" : "missing (iOS Safari has none)", r.api],
    [
      "page activated",
      r.activated === null ? "unknown (no userActivation API)" : r.activated ? "yes" : "no — Chrome will refuse to vibrate",
      r.activated !== false,
    ],
    [
      `vibrate(${TAP_PATTERN})`,
      r.accepted === null ? "not tried yet" : r.accepted ? "accepted" : "refused",
      r.accepted !== false,
    ],
  ];
  $("haptics").innerHTML = rows
    .map(
      ([k, v, ok]) =>
        `<tr><td>${escapeHtml(k)}</td><td class="val"><span class="tag ${ok ? "mirrored" : "not-possible"}">${escapeHtml(v)}</span></td></tr>`,
    )
    .join("");
}

// Deliberately a `click` listener with no preventDefault anywhere near it: this
// is the control case that the real surface has to match.
$("buzz").addEventListener("click", () => runHapticsTest(true));
runHapticsTest(false);

// ---------------------------------------------------------------- connection
//
// Same resolution as the trackpad and the settings page: the desktop's own link
// carries `?h=`, and anything else falls back to the address the pad learned
// from the QR and remembered. Guessing `location.hostname:8787` was wrong the
// moment `--port` differed - and this is the page someone opens *because* the
// connection is not working, so it is the last place that should be guessing.
const host =
  new URLSearchParams(location.search).get("h") ??
  storedLink()?.host ??
  `${location.hostname}:8787`;
// This page mirrors every touch, so the desktop challenges it like any other
// connection. The key comes out of the fragment the app put in the link.
const key = takeKeyFromFragment();
let ws: WebSocket | null = null;
let backoff = 500;
// Set when the desktop challenged us and we had nothing to answer with.
// Reconnecting would fail exactly the same way, once a second, forever - and
// bury the one message that says what to do about it.
let unpaired = false;

function connect(): void {
  ws = new WebSocket(`${socketUrl({ host })}/observe`);
  ws.onopen = () => {
    backoff = 500;
  };
  ws.onclose = () => {
    ws = null;
    if (unpaired) return;
    dot.className = "dot offline";
    $("connection-status").textContent = "Not connected · trying again…";
    setTimeout(connect, backoff);
    backoff = Math.min(backoff * 2, 5000);
  };
  ws.onerror = () => ws?.close();
  ws.onmessage = (ev) => {
    const msg = JSON.parse(ev.data as string) as Batch | Hello | Challenge;
    if (msg.t === "challenge") {
      const reply = authReply(msg.nonce, key);
      if (!reply) {
        unpaired = true;
        dot.className = "dot unpaired";
        $("connection-status").textContent =
          "This page has no pairing code · open it from PadRemote's menu";
        ws?.close();
        return;
      }
      ws?.send(JSON.stringify({ t: "auth", ...reply }));
      // `dot` is the shared indicator; the second class is the state.
      dot.className = "dot connected";
      $("connection-status").textContent = "Connected to your computer";
      return;
    }
    if (msg.t === "hello") onHello(msg);
    else onBatch(msg);
  };
}
connect();

function onHello(msg: Hello): void {
  $("title").textContent = `Connection check · ${msg.computer}`;
  const rows = msg.settings
    .map(
      (s) => `<tr>
        <td>${escapeHtml(s.setting)}</td>
        <td class="val">${escapeHtml(s.value)}</td>
        <td><span class="tag ${s.status.replace(/\s+/g, "-")}">${escapeHtml(s.status)}</span>
        ${s.detail ? `<div class="why">${escapeHtml(s.detail)}</div>` : ""}</td>
      </tr>`,
    )
    .join("");
  $("settings").innerHTML = rows;
}

// -------------------------------------------------------------------- state
/** How long a trail lingers on the pad view, matching the phone's own trails. */
const TRAIL_MS = 900;

interface TrailPoint {
  x: number;
  y: number;
  t: number;
}

interface Finger {
  x: number;
  y: number;
  /** The finger's own id on its device: two devices both have a finger 1. */
  id: number;
  device: number;
  trail: TrailPoint[];
  lifted: number | null;
}

/**
 * Live finger positions, so the pad view shows the hand, not just samples.
 *
 * Keyed by device *and* finger, because pointer ids are only unique within one
 * device: with a phone and a tablet connected, both send a finger 1, and a
 * single-keyed map drew them as one finger teleporting between two hands.
 */
const fingers = new Map<string, Finger>();
/** Everyone connected, in the order they first spoke, for the colour legend. */
const devices = new Map<number, string>();
const gaps: number[] = [];
let batchesThisSecond = 0;
let lastSecond = performance.now();

/** One colour per device, so a hand keeps its colour across the whole session. */
function deviceColor(id: number): string {
  const seen = [...devices.keys()];
  const i = seen.indexOf(id);
  return COLORS[(i < 0 ? id : i) % COLORS.length];
}

function onBatch(b: Batch): void {
  batchesThisSecond++;
  const device = b.deviceId ?? 0;
  if (!devices.has(device)) {
    devices.set(device, b.device ?? String(device));
    drawLegend();
  }

  // Only the driving device's cadence goes on the chart. Two devices' gaps
  // interleaved produce a sawtooth that says nothing about either link.
  if (b.gapMs !== null && b.held !== false) {
    gaps.push(b.gapMs);
    if (gaps.length > 240) gaps.shift();
  }

  // Likewise the headline numbers: they describe one gesture, so they follow
  // the device that is actually moving the cursor.
  if (b.held !== false) {
    $("driver").textContent = b.device ?? "—";
    $("gesture").textContent = b.gesture;
    $("fingers").textContent = String(b.fingers);
    // The count the gesture is actually judged on. If this reads 3 while four
    // fingers are down, the hand landed too raggedly to be grouped.
    $("peak").textContent = String(b.peakFingers ?? 0);
  }

  const now = performance.now();
  for (const [id, phase, x, y] of b.points) {
    const key = `${device}:${id}`;
    if (phase === 2 || phase === 3) {
      // Mark it lifted rather than deleting, so the tail can fade out instead
      // of vanishing the instant the finger leaves.
      const lifting = fingers.get(key);
      if (lifting) lifting.lifted = now;
      continue;
    }
    let f = fingers.get(key);
    if (!f || phase === 0 || f.lifted !== null) {
      f = { x, y, id, device, trail: [], lifted: null };
      fingers.set(key, f);
    }
    f.x = x;
    f.y = y;
    f.trail.push({ x, y, t: now });
  }

  // Only log things a person would care about, not every move.
  const notable = b.actions.filter((a) => a !== "move" && a !== "scroll");
  if (notable.length) {
    // A gesture that was read but not obeyed is the single most confusing thing
    // to watch, so it is labelled rather than left to look like a dropped input.
    const what = b.held === false ? `${notable.join(", ")} (ignored)` : notable.join(", ");
    log(what, b.gesture, devices.size > 1 ? (b.device ?? "") : "");
  }
}

function log(what: string, gesture: string, device: string): void {
  const li = document.createElement("li");
  const time = new Date().toLocaleTimeString([], { hour12: false });
  const who = device ? `<em>${escapeHtml(device)}</em>` : "";
  li.innerHTML = `<time>${time}</time>${who}<b>${escapeHtml(what)}</b><i>${escapeHtml(gesture)}</i>`;
  events.prepend(li);
  while (events.children.length > 40) events.lastChild!.remove();
}

/** Name each connected device in its own colour, under the pad view. */
function drawLegend(): void {
  const el = $("devices");
  el.innerHTML = [...devices]
    .map(
      ([id, name]) =>
        `<span><i style="background:${deviceColor(id)}"></i>${escapeHtml(name)}</span>`,
    )
    .join("");
}

// ------------------------------------------------------------------ drawing
function fit(c: HTMLCanvasElement): CanvasRenderingContext2D {
  const dpr = window.devicePixelRatio || 1;
  const r = c.getBoundingClientRect();
  if (c.width !== Math.round(r.width * dpr) || c.height !== Math.round(r.height * dpr)) {
    c.width = Math.round(r.width * dpr);
    c.height = Math.round(r.height * dpr);
  }
  const ctx = c.getContext("2d")!;
  ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
  return ctx;
}

function drawPad(): void {
  const ctx = fit(pad);
  const r = pad.getBoundingClientRect();
  ctx.clearRect(0, 0, r.width, r.height);

  ctx.strokeStyle = "#232830";
  ctx.lineWidth = 1;
  ctx.strokeRect(0.5, 0.5, r.width - 1, r.height - 1);

  const now = performance.now();
  for (const [key, f] of fingers) {
    // Retire points that have aged out, and the finger once its tail is gone.
    while (f.trail.length && now - f.trail[0].t > TRAIL_MS) f.trail.shift();
    if (!f.trail.length) {
      fingers.delete(key);
      continue;
    }

    const color = deviceColor(f.device);
    ctx.lineCap = "round";
    ctx.lineJoin = "round";
    ctx.strokeStyle = color;

    // Segment by segment, so both width and opacity can fall off with age —
    // that taper is what makes the direction of travel readable.
    for (let i = 1; i < f.trail.length; i++) {
      const a = f.trail[i - 1];
      const b2 = f.trail[i];
      const age = 1 - (now - b2.t) / TRAIL_MS;
      if (age <= 0) continue;
      ctx.globalAlpha = age * age * 0.8;
      ctx.lineWidth = Math.max(0.6, 5 * age);
      ctx.beginPath();
      ctx.moveTo(a.x * r.width, a.y * r.height);
      ctx.lineTo(b2.x * r.width, b2.y * r.height);
      ctx.stroke();
    }

    // The fingertip itself fades out once the finger lifts.
    const head = f.trail[f.trail.length - 1];
    const headAge = 1 - (now - head.t) / TRAIL_MS;
    ctx.globalAlpha = Math.max(0, headAge);
    ctx.fillStyle = color;
    ctx.beginPath();
    ctx.arc(f.x * r.width, f.y * r.height, 9, 0, Math.PI * 2);
    ctx.fill();
    if (headAge > 0.4) {
      ctx.fillStyle = "#0d0f13";
      ctx.font = "10px ui-monospace, monospace";
      ctx.textAlign = "center";
      ctx.textBaseline = "middle";
      ctx.fillText(String(f.id), f.x * r.width, f.y * r.height);
    }
    ctx.globalAlpha = 1;
  }
}

function drawChart(): void {
  const ctx = fit(chart);
  const r = chart.getBoundingClientRect();
  ctx.clearRect(0, 0, r.width, r.height);
  if (!gaps.length) return;

  // A fixed 100 ms ceiling keeps the shape comparable between sessions.
  const max = 100;
  const barW = r.width / 240;
  gaps.forEach((g, i) => {
    const h = Math.min(1, g / max) * (r.height - 4);
    ctx.fillStyle = g > 50 ? "#f85149" : g > 20 ? "#d29922" : "#3fb950";
    ctx.fillRect(i * barW, r.height - h, Math.max(1, barW - 0.5), h);
  });

  // The 16 ms line: one display frame, the cadence the phone aims for.
  const y = r.height - (16 / max) * (r.height - 4);
  ctx.strokeStyle = "#3a4149";
  ctx.setLineDash([3, 3]);
  ctx.beginPath();
  ctx.moveTo(0, y);
  ctx.lineTo(r.width, y);
  ctx.stroke();
  ctx.setLineDash([]);
}

function tick(now: number): void {
  drawPad();
  drawChart();

  if (now - lastSecond >= 1000) {
    $("hz").textContent = String(batchesThisSecond);
    batchesThisSecond = 0;
    lastSecond = now;

    const recent = gaps.slice(-120);
    if (recent.length > 2) {
      const avg = recent.reduce((a, b) => a + b, 0) / recent.length;
      $("gap").textContent = avg.toFixed(1);
      $("jitter").textContent = (Math.max(...recent) - Math.min(...recent)).toFixed(0);
    }
  }
  requestAnimationFrame(tick);
}
requestAnimationFrame(tick);

function escapeHtml(s: string): string {
  return s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
}
