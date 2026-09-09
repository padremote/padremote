/**
 * The settings page.
 *
 * PadRemote's behaviour has always been editable in a JSON file with a text
 * editor, on the computer. That is a fine escape hatch and a poor front door,
 * so this is the same config over the same socket the trackpad uses - open it
 * on the phone in your hand, or on the computer, and both stay in step.
 *
 * Three tabs, because the settings are three different jobs:
 *
 * - **Basics** is what someone came here to change: how fast the pointer moves,
 *   which way scrolling goes, how a drag starts.
 * - **Gestures** is the only part that cannot be explained in words alone, so
 *   it lists each gesture and lets the user choose its action.
 * - **Advanced** is the timings and thresholds - and everything the desktop
 *   sends that this page has no opinion about.
 *
 * That last clause is load-bearing. The form is still **generated from the
 * config the desktop sends**, not written out by hand: a hand-written form is a
 * second copy of the schema, and the copy that drifts is always the one nobody
 * is looking at. A setting added to the Rust struct that nothing here knows
 * about lands in Advanced with a humanised name, rather than silently vanishing.
 * What this page adds on top is *judgement* the config file cannot carry: which
 * settings the computer's own trackpad is deciding, what each number means, and
 * what its units are.
 */

import "./theme.css";
import "./config.css";
import {
  ACTIONS,
  GESTURES,
  inheritedAction,
  actionName,
  groupActions,
  isPaired,
  claimedFields,
  gestureSections,
  forOs,
  type Gesture,
  type Os,
} from "./gestures";
import { gesturePad } from "./gesturepad";
import { authReply, socketUrl, storedLink, takeKeyFromFragment } from "./pairing";
import { startDevices } from "./devicespanel";

const $ = (id: string) => document.getElementById(id)!;

/** A config value, as it arrives. Booleans, numbers and short strings only. */
type Value = boolean | number | string;
type Group = Record<string, Value>;
/** The whole config: some scalars at the top, the rest in one level of groups. */
type Config = Record<string, Value | Group>;

interface HostRow {
  setting: string;
  name: string;
  value: string;
  status: string;
  detail: string;
}

interface State {
  t: "config";
  computer: string;
  /** Which system the desktop runs, so functions can be named as it names them. */
  os?: Os;
  path: string | null;
  file: Config;
  effective: Config;
  followSystem: boolean;
  host: HostRow[];
  /**
   * Which config field each mirrored setting decides, from the desktop.
   *
   * Sent rather than kept here: this page used to hold its own copy, and it
   * drifted the day a setting was renamed - a control that should have been
   * locked simply stopped saying why, silently, which is the exact class of
   * bug this page exists to make visible.
   */
  decidedBy: Record<string, string>;
  /** What the host mirror writes into each binding it owns. */
  mirrorWrites?: Record<string, string>;
  vocabulary: Record<string, string[]>;
}

// ------------------------------------------------------------------- wording

/** Where a known setting belongs, and under which heading. */
interface Panel {
  id: string;
  title: string;
  fields: string[];
}

/**
 * Basics, in the order the questions get asked.
 *
 * Only settings a person would go looking for. Everything else falls through
 * to Advanced by itself, so this list can stay short without hiding anything.
 *
 * A heading and no summary beneath it. "Pointer", over a row that says
 * "Pointer speed" and a slider labelled Slower and Faster, was already three
 * ways of saying the same thing before the sentence "How far the cursor
 * travels for the same movement of your finger" made it four.
 */
const BASICS: Panel[] = [
  {
    id: "trackpad",
    title: "Trackpad",
    fields: ["sensitivity", "scroll.speed", "scroll.natural", "drag.pressAndDrag"],
  },
];

/** What each Advanced group is called, in the user's language. */
const GROUPS: Record<string, { title: string }> = {
  "": { title: "Overall" },
  accel: { title: "Pointer acceleration" },
  tap: { title: "Taps and presses" },
  scroll: { title: "Scrolling" },
  drag: { title: "Dragging" },
  swipe: { title: "Swipes" },
  zoom: { title: "Zoom" },
};

/**
 * What a field means, for the fields whose name does not already say it.
 *
 * Deliberately partial, and deliberately short. A row is scanned by its label;
 * the hint under it is read only when the label was not enough, so a hint that
 * restates the label - "Side-to-side scrolling as well as up and down" under
 * "Side-to-side scrolling" - is a line of noise every reader pays for and
 * nobody needed. Those are absent here rather than reworded. What remains is a
 * fragment, not a sentence: the units moved to `UNITS`, beside the control
 * where the number being typed is, and the second clause went with them.
 */
const HINTS: Record<string, string> = {
  sensitivity: "1 matches your finger.",
  "accel.gain": "Faster on a quick flick. 0 is off.",
  "tap.tapMaxMs": "Longer, and it stops being a tap.",
  "tap.tapMaxPx": "Further, and the finger is moving.",
  "tap.doubleTapMs": "Allowed between two taps.",
  "tap.pressMs": "A still finger, before a drag begins.",
  "scroll.natural": "Content follows your fingers.",
  "scroll.accel": "How far a quick flick carries. 0 is one-to-one.",
  "swipe.minPx": "Travel before fingers count as a swipe.",
  "drag.pressAndDrag": "Rest a finger until the ring fills, then move.",
  "drag.tapAndDrag": "Tap, then move straight away.",
  "zoom.threshold": "How far fingers spread before a zoom step.",
  "zoom.backend": "appZoom sends the app's own zoom keystroke.",
};

/**
 * The unit a number is in, shown against the box rather than said in words.
 *
 * "200" and "ms" belong to each other; "ms — longer than this and a touch is
 * no longer a tap" put the unit at the head of a sentence three lines long,
 * where the one word that has to be read to type a number correctly was the
 * easiest word on the row to skip.
 */
const UNITS: Record<string, string> = {
  "tap.tapMaxMs": "ms",
  "tap.doubleTapMs": "ms",
  "tap.pressMs": "ms",
  "tap.tapMaxPx": "px",
  "swipe.minPx": "px",
};

/**
 * Names for the fields whose key does not survive being humanised.
 *
 * `scroll.accel` becomes "Accel", which says nothing; `accel.gain` becomes
 * "Gain", which says less. Everything not listed here reads perfectly well
 * from its key, and listing those too would be a second schema to keep.
 */
const LABELS: Record<string, string> = {
  sensitivity: "Pointer speed",
  "drag.pressAndDrag": "Press and drag",
  "drag.tapAndDrag": "Tap and drag",
  "scroll.enabled": "Scroll with two fingers",
  "scroll.natural": "Natural scrolling",
  "scroll.speed": "Scrolling speed",
  "scroll.horizontal": "Side-to-side scrolling",
  "scroll.momentum": "Coast after lifting",
  "scroll.accel": "Scroll acceleration",
  "zoom.enabled": "Pinch to zoom",
  "swipe.minPx": "Minimum travel",
  "accel.gain": "Acceleration",
  "tap.tapMaxMs": "Longest tap",
  "tap.tapMaxPx": "How far a tap may stray",
  "tap.doubleTapMs": "Double-tap gap",
  "tap.pressMs": "Hold before a drag starts",
  "zoom.threshold": "Zoom step",
  "zoom.backend": "How zoom is sent",
};

/**
 * The everyday numbers, as sliders with ends that say which way is which.
 *
 * A number box is right for a threshold in milliseconds and wrong for "how
 * fast should the pointer be?" - a question nobody answers with a number, and
 * everybody answers by trying it.
 */
const SLIDERS: Record<string, { min: number; max: number; step: number; low: string; high: string }> = {
  sensitivity: { min: 0.25, max: 3, step: 0.05, low: "Slower", high: "Faster" },
  "accel.gain": { min: 0, max: 2.5, step: 0.05, low: "Off", high: "Stronger" },
  "scroll.speed": { min: 0.25, max: 3, step: 0.05, low: "Slower", high: "Faster" },
};

/** `twoFingerTap` -> `Two finger tap`. */
function humanise(key: string): string {
  return key
    .replace(/([A-Z])/g, " $1")
    .replace(/^./, (c) => c.toUpperCase())
    .replace(/\bPx\b/, "distance")
    .replace(/\bMs\b/, "time");
}

// ------------------------------------------------------------------ the link

const params = new URLSearchParams(location.search);
/**
 * Where the computer is.
 *
 * The desktop's own links carry `?h=`. Everything else has to come from the
 * address the trackpad already learned and stored: the QR puts the computer on
 * `#h=<ip>:<ws port>`, the pad consumes it and hides it, so by the time someone
 * taps Gestures the fragment is long gone - and it is the *tab* by then anyway.
 *
 * The old fallback guessed `location.hostname:8787`, which is only right when
 * the desktop serves the page itself and nobody passed `--port`. Served from
 * padremote.com, as planned, it pointed the settings socket at the CDN.
 */
const remembered = storedLink();
// The desktop challenges this socket too - a settings page can rewrite how the
// whole app behaves, so it is not a lesser connection than the trackpad's. The
// key arrives in the fragment and is taken out of it before the tab router
// below ever sees it.
const key = takeKeyFromFragment();
/** Set once a challenge went unanswered; retrying would only repeat it. */
let unpaired = false;
const host = params.get("h") ?? remembered?.host ?? `${location.hostname}:8787`;
/** Did anything actually tell this page where the computer is? */
const paired = params.get("h") !== null || remembered?.host !== undefined;
let ws: WebSocket | null = null;
let state: State | null = null;
let backoff = 500;

function connect(): void {
  if (remembered?.name) {
    // The phone already knows whose computer this is; saying so beats a spinner
    // that resolves into the same name a moment later.
    $("computer").textContent = `Connecting to ${remembered.name}…`;
  }
  // `socketUrl` picks ws/wss from how this page was served: a page on https
  // may not open a plain ws://, and the browser blocks it outright.
  ws = new WebSocket(`${socketUrl({ host })}/config`);
  ws.onopen = () => {
    backoff = 500;
  };
  ws.onclose = () => {
    ws = null;
    if (unpaired) return;
    $("dot").className = "dot offline";
    // Same distinction the pad makes: a phone that has never been paired needs
    // to be told to scan, not to check whether the app is running.
    $("computer").textContent = paired
      ? "Not connected — keep PadRemote open on your computer"
      : "Not paired — scan the QR code on your computer";
    document.body.classList.add("offline");
    setTimeout(connect, backoff);
    backoff = Math.min(backoff * 2, 5000);
  };
  ws.onerror = () => ws?.close();
  ws.onmessage = (ev) => {
    const msg = JSON.parse(ev.data as string) as
      | State
      | { t: "error"; detail: string }
      | { t: "challenge"; nonce: string };
    if (msg.t === "challenge") {
      const reply = authReply(msg.nonce, key);
      if (!reply) {
        unpaired = true;
        $("dot").className = "dot unpaired";
        $("computer").textContent = "Not paired — open Settings from PadRemote's menu bar icon";
        document.body.classList.add("offline");
        ws?.close();
        return;
      }
      ws?.send(JSON.stringify({ t: "auth", ...reply }));
      $("dot").className = "dot connected";
      return;
    }
    if (msg.t === "error") {
      say(msg.detail, "bad");
      return;
    }
    arrived(msg);
  };
}

/** Apply an authenticated settings update from the computer. */
function arrived(msg: State): void {
  const first = state === null;
  state = msg;
  document.body.classList.remove("offline");
  $("dot").className = "dot connected";
  render(msg);
  // A save is confirmed by the config coming back, not by the send returning:
  // the desktop writes the file and every open page is told. Until then the
  // page says "Saving…", which is the truth.
  if (saving) {
    saving = false;
    say("Saved on your computer", "ok");
  } else if (first) {
    say("");
  }
}

/**
 * What just happened to the settings, in words.
 *
 * Saving and saved are different states and used to look identical - the page
 * said "saved" the instant it sent, which was a guess, and a wrong one whenever
 * the computer had gone away mid-edit.
 */
let saving = false;
let sayTimer: number | null = null;
const TONE = { ok: "text-ok", warn: "text-warn", bad: "text-bad" };
function say(text: string, tone: "ok" | "warn" | "bad" = "ok"): void {
  const el = $("saved");
  el.textContent = text;
  el.className =
    `text-[13px] font-[550] tabular-nums max-[620px]:not-empty:order-4 ${TONE[tone]}`;
  el.hidden = text === "";
  if (sayTimer !== null) clearTimeout(sayTimer);
  // "Saving…" stays put; a confirmation has said its piece after
  // a couple of seconds and then only adds noise.
  if (text && tone === "ok" && !saving) {
    sayTimer = window.setTimeout(() => (el.hidden = true), 2200);
  }
}

/**
 * Send the whole config back.
 *
 * Whole, not a patch: the file is the serialised form of one struct, and the
 * desktop rejects anything it cannot parse into that struct - so a partial
 * write would be refused, and a partial *merge* is how two writers corrupt one.
 */
function save(next: Config): void {
  if (!state) return;
  if (!ws || ws.readyState !== WebSocket.OPEN) {
    say("Not saved — your computer isn’t connected", "bad");
    return;
  }
  state = { ...state, file: next };
  ws.send(JSON.stringify({ t: "setConfig", config: next }));
  saving = true;
  say("Saving…", "warn");
}

/** Set one field and send the whole config. */
function commit(path: string, value: Value): void {
  if (!state) return;
  const next = structuredClone(state.file);
  const [group, key] = path.includes(".") ? path.split(".") : ["", path];
  if (group) (next[group] as Group)[key] = value;
  else next[key] = value;
  save(next);
}

// -------------------------------------------------------------------- pages
//
// Four sections, each a page of its own, reached from a hub that names them.
// The three tabs this replaced put every setting one tap away and all of them
// on the screen at once, which past a certain number of settings is the same
// thing as none of them being findable.
//
// Still one document and one socket, though: the pages are hash routes. A file
// per page would be a second connection, a second challenge and a second copy
// of the header, for navigation the browser already does without a reload -
// and `#gestures`, which the trackpad's own toolbar links to, keeps working.

type Page = "home" | "basics" | "gestures" | "advanced";

/** What each page is called, in the bar. */
const PAGES: Record<Page, { title: string }> = {
  home: { title: "Settings" },
  basics: { title: "Basic settings" },
  gestures: { title: "Gestures" },
  advanced: { title: "Advanced settings" },
};
/** Hub first, then the pages in the order the hub lists them. */
const ORDER = Object.keys(PAGES) as Page[];
let page: Page = "home";

function show(next: Page, viaHash = false): void {
  // Which way the page arrives. Deeper from the right, back towards the hub
  // from the left: the movement is the only thing saying which direction the
  // user just went, and a page that always slides in from the same side says
  // nothing at all.
  const forward = ORDER.indexOf(next) > ORDER.indexOf(page);
  page = next;
  for (const name of ORDER) $(`panel-${name}`).hidden = name !== next;
  document.body.dataset.page = next;
  document.body.dataset.direction = forward ? "in" : "out";
  $("page-title").textContent = PAGES[next].title;
  // The way out. From the hub that is the trackpad the user came from; from a
  // page it is the hub, because something reached from a list belongs back in
  // the list rather than two taps away from it.
  ($("back") as HTMLAnchorElement).href = next === "home" ? "/" : "#home";
  $("back-label").textContent = next === "home" ? "Trackpad" : "Settings";
  // A hash the browser set is already in the address bar; one this page chose
  // has to be put there, and replaced rather than pushed so that Back leaves
  // the settings page instead of walking the pages the user has already seen.
  if (!viaHash) history.replaceState(null, "", `#${next}`);
  window.scrollTo?.(0, 0);
}

function fromHash(): void {
  const name = location.hash.replace("#", "") as Page;
  show(ORDER.includes(name) ? name : "home", true);
}
window.addEventListener("hashchange", fromHash);

/**
 * What each card on the home page says underneath its name.
 *
 * A list of rooms is only better than one room if the list says what is in
 * them. These are the current values, not descriptions: "Pointer speed 1.20"
 * answers the question that would otherwise need the page opening to answer.
 *
 * The devices have no card, because they are not in another room - they are on
 * this page, and a card describing a list you can already read is a worse
 * version of the list.
 */
function updateHub(s: State): void {
  const speed = Number(valueAt(s.effective, "sensitivity") ?? 1).toFixed(2);
  const natural = valueAt(s.effective, "scroll.natural") !== false;
  $("nav-basics-note").textContent =
    `Pointer speed ${speed} · Natural scrolling ${natural ? "on" : "off"}`;
  const assignable = GESTURES.filter((g) => g.field);
  const set = assignable.filter((g) => {
    const value = gestureAction(s, g);
    return value !== "none" && value !== "inherit";
  }).length;
  $("nav-gestures-note").textContent =
    `${set} of ${assignable.length} gestures assigned an action`;
}

// ---------------------------------------------------------------- the form
//
// Controls are built once and then updated in place. Rebuilding on every
// message was simpler and wrong: the desktop echoes the config back after each
// save, so a rebuild mid-drag tore the slider out from under the finger holding
// it.

interface Field {
  input: HTMLInputElement | HTMLSelectElement;
  hint: HTMLElement;
  output?: HTMLOutputElement;
  path: string;
}
const fields = new Map<string, Field>();
/** The shape the current controls were built for; a change means rebuild. */
let builtFor = "";

function render(s: State): void {
  $("computer").textContent = s.computer;
  $("path").textContent = s.path ?? "no config file";
  $("confirm-name").textContent = s.computer;
  $("forget-name").textContent = s.computer;

  const shape = paths(s.file).join("|");
  if (shape !== builtFor) {
    builtFor = shape;
    build(s);
  }
  sync(s);
  renderMirror(s);
  updateHub(s);
}

/** Every leaf in the config, as dotted paths, in the order it declares them. */
function paths(file: Config): string[] {
  const out: string[] = [];
  for (const [key, value] of Object.entries(file)) {
    if (value !== null && typeof value === "object") {
      for (const sub of Object.keys(value)) out.push(`${key}.${sub}`);
    } else {
      out.push(key);
    }
  }
  return out;
}

/** `version` is the file's own bookkeeping and `followSystem` has its own switch. */
const NOT_A_SETTING = new Set(["version", "followSystem"]);

function build(s: State): void {
  fields.clear();
  const all = paths(s.file).filter((p) => !NOT_A_SETTING.has(p));
  const placed = new Set<string>();

  const basics = $("basics-groups");
  basics.innerHTML = "";
  for (const panel of BASICS) {
    const present = panel.fields.filter((p) => all.includes(p));
    if (!present.length) continue;
    present.forEach((p) => placed.add(p));
    basics.append(section(panel.title, present, s));
  }

  // Only the bindings some feature on the Gestures tab actually offers. The
  // rest - Launchpad's four-finger pinch, Show Desktop's five-finger spread -
  // are gestures this app does not ask anyone to perform, so they fall through
  // to Advanced rather than vanishing.
  const claimed = claimedFields();
  all.filter((p) => claimed.has(p)).forEach((p) => placed.add(p));

  // Whatever is left, grouped as the config groups it - including any setting
  // added to the desktop that this page has never heard of.
  const advanced = $("advanced-groups");
  advanced.innerHTML = "";
  const rest = all.filter((p) => !placed.has(p));
  const byGroup = new Map<string, string[]>();
  for (const path of rest) {
    const group = path.includes(".") ? path.split(".")[0] : "";
    byGroup.set(group, [...(byGroup.get(group) ?? []), path]);
  }
  $("nav-advanced-note").textContent =
    `${rest.length} more settings, in ${byGroup.size} groups`;
  for (const [group, members] of byGroup) {
    const meta = GROUPS[group] ?? { title: humanise(group) };
    const disclosure = document.createElement("details");
    disclosure.className = "group rounded-2xl border border-line bg-panel " +
      "[&>.panel]:border-0 [&>.panel]:bg-transparent [&>.panel]:pt-1";
    const summary = document.createElement("summary");
    summary.className = "flex min-h-11 cursor-pointer list-none items-center gap-2 px-5 " +
      "py-[14px] font-[550] text-fg [&::-webkit-details-marker]:hidden " +
      "before:h-[7px] before:w-[7px] before:rotate-45 before:border-r-[1.5px] before:border-t-[1.5px] before:border-current before:transition-transform before:duration-150 before:content-[''] group-open:before:rotate-[135deg]";
    summary.textContent = meta.title;
    disclosure.append(summary, section(meta.title, members, s));
    advanced.append(disclosure);
  }

  buildGestures();
}

function section(title: string, members: string[], s: State): HTMLElement {
  const el = document.createElement("section");
  el.className = "panel grid gap-4 max-[540px]:rounded-2xl max-[540px]:p-[18px]";
  const h = document.createElement("h2");
  h.className = "hidden";
  h.textContent = title;
  el.append(h);
  for (const path of members) el.append(field(path, s));
  return el;
}

function field(path: string, s: State): HTMLElement {
  const value = valueAt(s.file, path);
  const slider = SLIDERS[path];
  const row = document.createElement(slider ? "div" : "label");
  row.className = slider
    ? "row-slider grid gap-0.5 [&_.row]:cursor-default"
    : "row cursor-pointer";

  const text = document.createElement("span");
  text.textContent = LABELS[path] ?? humanise(path.split(".").pop()!);
  const hint = document.createElement("em");
  text.append(hint);

  if (slider) {
    const head = document.createElement("label");
    head.className = "row";
    head.htmlFor = `set-${path}`;
    const out = document.createElement("output");
    out.className = "text-[15px] tabular-nums text-fg";
    head.append(text, out);
    const input = document.createElement("input");
    input.type = "range";
    input.id = `set-${path}`;
    Object.assign(input, { min: String(slider.min), max: String(slider.max), step: String(slider.step) });
    const ends = document.createElement("div");
    ends.className = "flex justify-between text-xs text-faint";
    const low = document.createElement("span");
    low.textContent = slider.low;
    const high = document.createElement("span");
    high.textContent = slider.high;
    ends.append(low, high);
    // Live while dragging so the pointer can be judged by feel, but not one
    // message per pixel: a config write is a file write on the other machine.
    const nudge = throttle(() => commit(path, Number(input.value)), 120);
    input.addEventListener("input", () => {
      out.textContent = Number(input.value).toFixed(2);
      nudge();
    });
    input.addEventListener("change", () => commit(path, Number(input.value)));
    row.append(head, input, ends);
    fields.set(path, { input, hint, output: out, path });
    return row;
  }

  row.append(text);
  const input = control(path, value, s);
  (row as HTMLLabelElement).htmlFor = input.id;
  // A number wears its unit. Everything else is wide enough on its own.
  const unit = UNITS[path];
  if (unit) {
    const box = document.createElement("span");
    box.className = "flex flex-none items-center gap-[7px]";
    const suffix = document.createElement("span");
    suffix.className = "w-5 text-[13px] text-faint";
    suffix.textContent = unit;
    box.append(input, suffix);
    row.append(box);
  } else {
    row.append(input);
  }
  fields.set(path, { input, hint, path });
  return row;
}

function control(path: string, value: Value, s: State): HTMLInputElement | HTMLSelectElement {
  const options = s.vocabulary[path];
  if (typeof value === "boolean") {
    const el = document.createElement("input");
    el.type = "checkbox";
    el.id = `set-${path}`;
    el.setAttribute("role", "switch");
    el.addEventListener("change", () => commit(path, el.checked));
    return el;
  }
  if (options) {
    const el = document.createElement("select");
    el.id = `set-${path}`;
    el.addEventListener("change", () => commit(path, el.value));
    return el;
  }
  const el = document.createElement("input");
  el.type = typeof value === "number" ? "number" : "text";
  el.id = `set-${path}`;
  if (typeof value === "number") el.step = Number.isInteger(value) ? "1" : "0.05";
  el.addEventListener("change", () => {
    if (typeof value !== "number") {
      commit(path, el.value);
      return;
    }
    // A number field renders in the browser's locale but reports a plain
    // number - and reports an *empty string* when what was typed is not one.
    // Sending `Number("")` would quietly write a zero.
    const n = Number(el.value);
    if (el.value.trim() === "" || !Number.isFinite(n)) {
      el.value = String(valueAt(state!.file, path));
      return;
    }
    commit(path, n);
  });
  return el;
}

/** Put current values into the controls, leaving whatever has focus alone. */
function sync(s: State): void {
  ($("follow") as HTMLInputElement).checked = s.followSystem;
  $("follow-panel").classList.toggle("following", s.followSystem);
  for (const f of fields.values()) {
    const value = valueAt(s.file, f.path);
    if (value === undefined) continue;
    const locked = lockedBy(s, f.path);
    f.input.disabled = locked !== null;
    f.hint.textContent = locked
      ? `Your computer: “${locked}”`
      : (HINTS[f.path] ?? "");
    f.hint.classList.toggle("locked", locked !== null);
    if (f.input === document.activeElement) continue;
    if (f.input instanceof HTMLInputElement && f.input.type === "checkbox") {
      f.input.checked = Boolean(value);
    } else if (f.input instanceof HTMLSelectElement) {
      setOptions(f.input, s.vocabulary[f.path] ?? [], String(value));
    } else {
      f.input.value = String(value);
    }
    // A slider shows the *effective* number where the computer is deciding it,
    // because a control that reads 1.00 while the pointer is plainly faster is
    // worse than no reading at all.
    if (f.output) {
      const shown = locked ? valueAt(s.effective, f.path) : value;
      f.output.textContent = Number(shown).toFixed(2);
      if (f.input instanceof HTMLInputElement) f.input.value = String(shown);
    }
  }
  syncGesture();
}

function setOptions(el: HTMLSelectElement, options: string[], value: string): void {
  const wanted = [...options];
  // A value the engine no longer accepts must still be visible rather than
  // silently becoming whatever happens to be first in the list.
  if (!wanted.includes(value)) wanted.unshift(value);
  const same = wanted.length === el.options.length && wanted.every((o, i) => el.options[i].value === o);
  if (!same) {
    el.innerHTML = "";
    for (const option of wanted) {
      const o = document.createElement("option");
      o.value = option;
      o.textContent = ACTIONS[option] ?? (options.includes(option) ? option : `${option} (unknown)`);
      el.append(o);
    }
  }
  el.value = value;
}

function valueAt(config: Config, path: string): Value {
  const [group, key] = path.includes(".") ? path.split(".") : ["", path];
  return (group ? (config[group] as Group)?.[key] : config[path]) as Value;
}

/**
 * The host setting deciding this field, if one is.
 *
 * "approximated" counts: the *value* still comes from the host, and only the
 * way PadRemote delivers it is a compromise. A control the host decides is a
 * control that does nothing here, however faithfully the action is sent.
 */
function lockedBy(s: State, path: string): string | null {
  if (!s.followSystem) return null;
  const setting = s.decidedBy[path];
  if (setting === undefined) return null;
  // A binding the mirror only *seeds* is not decided here. It is still locked -
  // see `seededBy`, which reports it separately - but the note it carries names
  // a different way out, so the two must not be collapsed into one.
  if (s.mirrorWrites?.[path] !== undefined) return null;
  const row = s.host.find((r) => r.setting === setting);
  if (!row || row.value.startsWith("not set")) return null;
  return row.status === "mirrored" || row.status === "approximated" ? setting : null;
}

/**
 * The host setting this gesture's action came from, while it still matches.
 *
 * Locked, like [`lockedBy`], but for a different reason and with a different
 * way out. The host does not *decide* this field - the mirror writes its own
 * action and leaves any other choice alone - but while the value is still the
 * one the computer gave, a control that moves is a control that lies about who
 * is in charge of it. So it locks, and the note says which switch releases it.
 *
 * This was once the other way round, and the reason is worth keeping: the
 * control stayed usable so that a value the mirror would not overwrite could be
 * chosen. What made that safe to reverse is that turning off Match my computer
 * now un-seeds every field at once - `followSystem` is the first thing both
 * this and `lockedBy` check - so there is a way out that does not depend on
 * guessing which values the mirror declines to write.
 */
function seededBy(s: State, path: string): string | null {
  if (!s.followSystem) return null;
  const owns = s.mirrorWrites?.[path];
  const setting = s.decidedBy[path];
  if (owns === undefined || setting === undefined) return null;
  const row = s.host.find((r) => r.setting === setting);
  if (!row || row.value.startsWith("not set")) return null;
  return String(valueAt(s.effective, path) ?? "") === owns ? setting : null;
}

/**
 * What PadRemote did with a host setting, in the words the computer's own
 * report uses.
 *
 * The desktop sends the engine's vocabulary - "mirrored", "approximated",
 * "not possible" - which is exactly right in a log and wrong on a page someone
 * opened to find out whether their trackpad settings arrived. The menu-bar
 * report already translates these; showing a different set of words for the
 * same three states, on two pages one menu apart, is how a user concludes the
 * two are measuring different things.
 */
const STATUS: Record<string, { tag: string; words: string }> = {
  mirrored: { tag: "mirrored", words: "Matches exactly" },
  approximated: { tag: "approximated", words: "Adapted for touch" },
  "not possible": { tag: "not-possible", words: "Not available" },
  "handled by the OS": { tag: "auto", words: "Handled by your computer" },
};

function renderMirror(s: State): void {
  // Only the settings this computer actually reports. A row saying "not set"
  // for a switch the OS does not have is noise, not information.
  const rows = s.host.filter((r) => !r.value.startsWith("not set"));
  $("mirror-rows").innerHTML = rows
    .map((r) => {
      // `setting` is what System Settings calls it; `name` is the preference
      // key underneath. The name of the row is the one a person would look for.
      const status = STATUS[r.status] ?? { tag: "", words: r.status };
      return `<tr>
        <td>${escape(r.setting)}</td>
        <td class="val tabular-nums">${escape(r.value)}</td>
        <td><span class="tag ${status.tag}">${escape(status.words)}</span>
          ${r.detail ? `<div class="why">${escape(r.detail)}</div>` : ""}</td>
      </tr>`;
    })
    .join("");
  const mirrored = s.host.filter((r) => r.status === "mirrored").length;
  $("mirror-count").textContent = s.host.length ? `${mirrored} of ${s.host.length} matched` : "";
  $("mirror-details").hidden = s.host.length === 0;
}

$("follow").addEventListener("change", (e) => {
  if (!state) return;
  const next = structuredClone(state.file);
  next.followSystem = (e.target as HTMLInputElement).checked;
  save(next);
});

// ------------------------------------------------------------------ gestures
//
// One gesture binds one config field, which is the shape the config has - so
// picking an action is a single write and no two rows can fight over a swipe.
// The action list is rendered inline rather than as a menu: the choices are
// whole sentences ("Mission Control up, app windows down"), and a native select
// shows one of them at a time and trims it.

let current: Gesture = GESTURES[0];

/** The wording for this computer; macOS phrasing until the desktop says. */
function os(): Os {
  return state?.os ?? "macos";
}

function buildGestures(): void {
  const list = $("gesture-list");
  const select = $("gesture-select") as HTMLSelectElement;
  if (list.children.length) return;

  for (const section of gestureSections()) {
    const heading = document.createElement("li");
    // Styled by the list it is in, which is where the rules that only apply
    // once the list is the navigation live.
    heading.className = "gesture-section";
    heading.setAttribute("role", "presentation");
    heading.textContent = section.title;
    list.append(heading);

    const optgroup = document.createElement("optgroup");
    optgroup.label = section.title;
    select.append(optgroup);

    for (const g of section.gestures) buildRow(g, list, optgroup);
  }

  function buildRow(g: Gesture, list: HTMLElement, optgroup: HTMLElement): void {
    const li = document.createElement("li");
    const button = document.createElement("button");
    button.type = "button";
    button.setAttribute("role", "tab");
    button.id = `gesture-tab-${g.id}`;
    button.setAttribute("aria-controls", "gesture-detail");
    button.dataset.gesture = g.id;
    // Picture first, then the words: the drawing is what the eye lands on, and
    // the name underneath confirms it.
    button.append(gesturePad(g));
    const words = document.createElement("span");
    words.className = "min-w-0";
    const title = document.createElement("b");
    title.dataset.name = g.id;
    const does = document.createElement("small");
    does.dataset.does = g.id;
    words.append(title, does);
    button.append(words);
    button.addEventListener("click", () => pick(g));
    li.append(button);
    list.append(li);

    const option = document.createElement("option");
    option.value = g.id;
    option.dataset.name = g.id;
    optgroup.append(option);
  }

  select.addEventListener("change", () => {
    const g = GESTURES.find((x) => x.id === select.value);
    if (g) pick(g);
  });
  list.addEventListener("keydown", (e) => {
    const key = (e as KeyboardEvent).key;
    const step = key === "ArrowDown" ? 1 : key === "ArrowUp" ? -1 : 0;
    if (!step) return;
    e.preventDefault();
    const i = GESTURES.findIndex((g) => g.id === current.id);
    const next = GESTURES[(i + step + GESTURES.length) % GESTURES.length];
    pick(next);
    (list.querySelector(`[data-gesture="${next.id}"]`) as HTMLElement)?.focus();
  });

  $("action-search").addEventListener("input", filterActions);
  $("action-search-clear").addEventListener("click", () => {
    const box = $("action-search") as HTMLInputElement;
    box.value = "";
    filterActions();
    box.focus();
  });
  pick(current);
}

function pick(g: Gesture): void {
  current = g;
  ($("action-search") as HTMLInputElement).value = "";
  $("action-search-clear").hidden = true;
  for (const button of $("gesture-list").querySelectorAll<HTMLElement>("[data-gesture]")) {
    const selected = button.dataset.gesture === g.id;
    button.setAttribute("aria-selected", String(selected));
    // Roving tabindex: a tablist is one stop, and the arrows move within it.
    button.setAttribute("tabindex", selected ? "0" : "-1");
    // The list is a column of twenty in its own scroller and the arrow keys
    // walk it, so the row being demonstrated has to be one that can be seen.
    if (selected) button.scrollIntoView?.({ block: "nearest" });
  }
  ($("gesture-select") as HTMLSelectElement).value = g.id;
  $("gesture-detail").setAttribute("aria-labelledby", `gesture-tab-${g.id}`);
  // Replaced rather than re-attributed: a fresh element restarts the keyframes,
  // so switching gestures shows the new one from the top of its loop instead of
  // halfway through the last one's.
  $("gesture-demo").replaceChildren(gesturePad(g, { animated: true }));
  syncGesture();
}

/**
 * What a gesture is actually set to.
 *
 * The file is what the user has chosen; the effective config is what the engine
 * is running, which differs wherever the host's own trackpad settings have been
 * mirrored on top. A locked control has to show the second: the whole point of
 * locking it is that the computer decided, and displaying the file's value
 * instead showed a gesture doing one thing while the computer did another - and
 * on a field where the two vocabularies differ, a vertical swipe could show a
 * sideways action.
 */
function shownValue(s: State, field: string, locked: boolean): string {
  const gesture = GESTURES.find((g) => g.field === field);
  if (gesture?.legacyField) {
    const explicit = valueAt(s.file, field);
    return explicit === undefined ? "inherit" : String(explicit);
  }
  const from = locked ? s.effective : s.file;
  return String(valueAt(from, field) ?? valueAt(s.file, field) ?? "none");
}

/** The name and actions for the selected gesture. */
function syncGesture(): void {
  if (!state) return;
  const g = current;
  const system = os();
  const actions = $("gesture-actions");
  const fixed = $("gesture-fixed");
  const locked = $("gesture-locked");

  $("gesture-name").textContent = forOs(g.name, system);
  const resolved = g.field ? gestureAction(state, g) : "";
  $("gesture-assignment").textContent = g.field ? `Assigned: ${actionName(resolved, system, false)}` : "";

  if (!g.field) {
    actions.hidden = true;
    fixed.hidden = false;
    fixed.textContent = g.fixed ?? "";
    locked.hidden = true;
  } else {
    fixed.hidden = true;
    actions.hidden = false;
    const by = g.legacyField ? null : lockedBy(state, g.field);
    const seeded = by === null && !g.legacyField ? seededBy(state, g.field) : null;
    const fromHost = by !== null || seeded !== null;
    const value = shownValue(state, g.field, fromHost);
    // Both cases lock the control: while a gesture is showing what the computer
    // says, the honest thing is a control you cannot move, not one that accepts
    // a choice. See the note on `seededBy` for why the *escape* differs.
    renderActions(g.field, state.vocabulary[g.field] ?? [value], value, system, fromHost, isPaired(g));
    locked.hidden = !fromHost;
    locked.classList.toggle("seeded", by === null && seeded !== null);
    locked.textContent = by
      ? `Your computer decides this: “${by}”. Turn off Match my computer to change it.`
      : seeded
        ? `Copied from your computer: “${seeded}”. Turn off Match my computer to change it.`
        : "";
    if (g.legacyField) {
      const supported = !!state.vocabulary[g.field];
      locked.hidden = false;
      locked.textContent = !supported
        ? "Update the desktop app to set directions separately."
        : value === "inherit"
          ? "Following your existing trackpad setting."
          : "Set for this direction only.";
      for (const button of actions.querySelectorAll<HTMLButtonElement>("[data-action]")) button.disabled = !supported;
    }
  }

  // Every row carries what it is currently set to, so "what do three fingers
  // do again?" does not need a tap per row to answer.
  for (const button of $("gesture-list").querySelectorAll<HTMLElement>("[data-name]")) {
    const gesture = GESTURES.find((x) => x.id === button.dataset.name)!;
    button.textContent = forOs(gesture.name, system);
  }
  for (const option of ($("gesture-select") as HTMLSelectElement).querySelectorAll<HTMLElement>("[data-name]")) {
    const gesture = GESTURES.find((x) => x.id === option.dataset.name)!;
    option.textContent = forOs(gesture.name, system);
  }
  for (const small of $("gesture-list").querySelectorAll<HTMLElement>("[data-does]")) {
    const gesture = GESTURES.find((x) => x.id === small.dataset.does)!;
    small.textContent = gesture.field
      ? actionName(
          gestureAction(state!, gesture),
          system,
          isPaired(gesture),
        )
      : (gesture.fixed ?? "");
  }
}

/**
 * The actions this gesture can perform, all of them visible at once.
 *
 * Every option the desktop says the engine accepts for this field, so nothing
 * here can offer an action that would be silently dropped - and nothing the
 * engine gains can go missing from the list.
 */
function renderActions(
  field: string,
  options: string[],
  value: string,
  system: Os,
  disabled: boolean,
  paired: boolean,
): void {
  const box = $("action-choices");
  // A value the engine no longer accepts must still be visible rather than
  // silently reading as whatever happens to be first in the list.
  const wanted = options.includes(value) ? options : [value, ...options];
  const groups = groupActions(wanted, system, paired);
  const shape = `${field}|${groups.map((g) => `${g.title}:${g.actions.join(",")}`).join("|")}`;

  if (box.dataset.shape !== shape) {
    box.dataset.shape = shape;
    box.innerHTML = "";
    for (const group of groups) {
      if (group.title) {
        const heading = document.createElement("p");
        // More above than below, so a heading belongs to the group under it
        // rather than floating between two of them. It spans every column the
        // box has, so the group under it starts on a fresh row instead of the
        // first choice sliding up beside the words that name it.
        // `action-group` stays a name because the search filter and
        // `check-settings.mjs` both look these up by it.
        heading.className = "action-group col-span-full mb-1.5 mt-[18px] text-xs font-semibold " +
          "uppercase tracking-[.06em] text-muted first:mt-0";
        heading.textContent = group.title;
        box.append(heading);
      }
      for (const option of group.actions) {
        const choice = document.createElement("button");
        choice.type = "button";
        choice.className = "flex w-full min-h-11 cursor-pointer flex-col items-start " +
          "justify-center gap-[3px] rounded-xl border border-line bg-bg px-4 py-3 " +
          "text-left text-sm font-[450] text-muted disabled:opacity-55 " +
          "aria-checked:border-accent aria-checked:bg-accent-soft " +
          "aria-checked:font-[550] aria-checked:text-fg";
        choice.setAttribute("role", "radio");
        choice.dataset.action = option;
        // The *current* gesture, read when clicked rather than captured when
        // built. Two gestures with the same action list reuse these buttons, so
        // a captured field belonged to whichever gesture happened to build them
        // - and every click after switching went to the wrong one.
        choice.addEventListener("click", () => {
          if (current.field) commit(current.field, option);
        });
        box.append(choice);
      }
    }
  }
  for (const child of box.querySelectorAll<HTMLButtonElement>("[data-action]")) {
    if (child.dataset.action === "inherit") {
      // Two lines rather than one long one. "Use existing setting" on its own is
      // a value whose effect cannot be seen without choosing it, so the choice
      // has to name what following actually does - but the pair of them joined
      // by a separator is half again as wide as this column, and wrapped where
      // the words happened to run out. So the answer goes underneath, which is
      // the shape every other two-part control on this page already has.
      child.textContent = actionName("inherit", system, false);
      const now = document.createElement("small");
      now.className = "choice-now block text-[13px] font-normal text-muted";
      now.textContent = actionName(gestureAction(state!, current, true), system, false);
      child.append(now);
    } else {
      child.textContent = actionName(child.dataset.action!, system, paired);
    }
    child.setAttribute("aria-checked", String(child.dataset.action === value));
    child.disabled = disabled;
  }
  filterActions();
}

// -------------------------------------------------------------- destructive

$("defaults").addEventListener("click", () => {
  $("confirm").hidden = false;
  $("defaults").hidden = true;
  $("confirm-yes").focus();
});
$("confirm-no").addEventListener("click", closeConfirm);
$("confirm-yes").addEventListener("click", () => {
  closeConfirm();
  if (!ws || ws.readyState !== WebSocket.OPEN) {
    say("Not saved — your computer isn’t connected", "bad");
    return;
  }
  ws.send(JSON.stringify({ t: "reset" }));
  saving = true;
  say("Saving…", "warn");
});

/**
 * Asked in the page rather than with `confirm()`.
 *
 * A native dialog on the phone is a modal the page cannot style, cannot place
 * away from the thumb, and - on a browser that has decided the page is being
 * pushy - can decline to show at all, which would reset the config on a single
 * tap with no question asked.
 */
function closeConfirm(): void {
  $("confirm").hidden = true;
  $("defaults").hidden = false;
}

function throttle(run: () => void, ms: number): () => void {
  let last = 0;
  let timer: number | null = null;
  return () => {
    const now = Date.now();
    if (now - last >= ms) {
      last = now;
      run();
    } else if (timer === null) {
      timer = window.setTimeout(() => {
        timer = null;
        last = Date.now();
        run();
      }, ms - (now - last));
    }
  };
}

function escape(s: string): string {
  return s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
}

fromHash();
connect();
startDevices({ host, key });

function gestureAction(s: State, g: Gesture, inherited = false): string {
  if (!g.field) return "none";
  const value = shownValue(s, g.field, lockedBy(s, g.field) !== null || seededBy(s, g.field) !== null);
  if (g.legacyField && (inherited || value === "inherit")) {
    return inheritedAction(g, String(valueAt(s.followSystem ? s.effective : s.file, g.legacyField) ?? "none"),
      valueAt(s.effective, "scroll.natural") !== false);
  }
  return value;
}

function filterActions(): void {
  const box = $("action-search") as HTMLInputElement;
  const query = box.value.trim().toLowerCase();
  let count = 0;
  for (const button of $("action-choices").querySelectorAll<HTMLElement>("[data-action]")) {
    button.hidden = !button.textContent?.toLowerCase().includes(query);
    if (!button.hidden) count++;
  }
  for (const heading of $("action-choices").querySelectorAll<HTMLElement>(".action-group")) heading.hidden = !!query;
  $("action-empty").hidden = count > 0;
  $("action-search-clear").hidden = box.value === "";
}
