/** Behaviour regressions for the settings page and gesture assignments.
 * Uses the real modules with a small DOM/socket host; no desktop is needed.
 * Run from web/: node scripts/check-settings.mjs
 */
import assert from "node:assert/strict";
import { mkdtempSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { pathToFileURL } from "node:url";
import { build } from "esbuild";

const out = mkdtempSync(join(tmpdir(), "padremote-settings-"));
let passed = 0;
function check(name, run) {
  run();
  passed++;
  console.log(`ok  ${name}`);
}

// ------------------------------------------------------------------ the DOM
//
// Hand-rolled rather than jsdom: the page touches a small, boring part of the
// platform, and a stub that small is easier to trust than a dependency.

class ElementHost extends EventTarget {
  constructor(tag = "div", id = "") {
    super();
    this.tag = tag;
    this.id = id;
    this.hidden = false;
    this.disabled = false;
    this.checked = false;
    this.value = "";
    this.textContent = "";
    this.children = [];
    this.dataset = {};
    this.attributes = new Map();
    this.classes = new Set();
    this.classList = {
      toggle: (name, on) => (on ? this.classes.add(name) : this.classes.delete(name)),
      contains: (name) => this.classes.has(name),
      add: (name) => this.classes.add(name),
      remove: (name) => this.classes.delete(name),
    };
  }
  // The page creates most of its controls and gives them ids; a lookup by id
  // has to find those, not mint a second empty element with the same name.
  set id(value) {
    this._id = value;
    if (value) byId.set(value, this);
  }
  get id() { return this._id ?? ""; }
  set className(value) { this.classes = new Set(String(value).split(/\s+/).filter(Boolean)); }
  get className() { return [...this.classes].join(" "); }
  set innerHTML(html) { this.html = html; this.children = []; }
  get innerHTML() { return this.html ?? ""; }
  get options() { return this.children.filter((c) => c.tag === "option"); }
  append(...nodes) {
    for (const node of nodes) {
      if (!(node instanceof ElementHost)) continue;
      node.parentElement = this;
      this.children.push(node);
    }
  }
  replaceChildren(...nodes) {
    this.children = [];
    this.append(...nodes);
  }
  setAttribute(name, value) {
    // SVG elements have a read-only `className`, so the drawings set their
    // class through here - and a selector has to find them either way.
    if (name === "class") this.className = value;
    else this.attributes.set(name, String(value));
  }
  getAttribute(name) { return this.attributes.get(name) ?? null; }
  removeAttribute(name) { this.attributes.delete(name); }
  focus() { document.activeElement = this; }
  getContext() { return null; }
  get clientWidth() { return 300; }
  get clientHeight() { return 210; }
  /** Enough selector engine for `[data-x]`, `[data-x="y"]` and `.class`. */
  matches(selector) {
    let m = /^\[([\w-]+)(?:="([^"]*)")?\]$/.exec(selector);
    if (m) {
      const key = m[1].replace(/^data-/, "").replace(/-(\w)/g, (_, c) => c.toUpperCase());
      const value = this.dataset[key];
      return value !== undefined && (m[2] === undefined || value === m[2]);
    }
    m = /^\.([\w-]+)$/.exec(selector);
    return m ? this.classes.has(m[1]) : false;
  }
  querySelectorAll(selector) {
    const found = [];
    for (const child of this.children) {
      if (child.matches(selector)) found.push(child);
      found.push(...child.querySelectorAll(selector));
    }
    return found;
  }
  querySelector(selector) { return this.querySelectorAll(selector)[0] ?? null; }
}
class InputHost extends ElementHost {}
class SelectHost extends ElementHost {}
const byId = new Map();
function el(id) {
  if (!byId.has(id)) byId.set(id, new ElementHost("div", id));
  return byId.get(id);
}
function make(tag) {
  if (tag === "input") return new InputHost(tag);
  if (tag === "select") return new SelectHost(tag);
  return new ElementHost(tag);
}

// The parts of `config.html` these checks walk: which page a control ends up
// on is a claim about the page, so the containers have to really contain.
// The devices are on the home page, not behind a card: a short list that
// answers its question by being read is not worth a tap.
el("panel-home").append(el("device-count"), el("device-rows"), el("device-empty"),
  el("device-note"), el("forget-panel"),
  el("nav-basics-note"), el("nav-gestures-note"), el("nav-advanced-note"));
el("forget-panel").append(el("forget-all"), el("forget-confirm"), el("forget-name"));
el("panel-basics").append(el("follow-panel"), el("basics-groups"));
// Three columns now: the actions on one side, the gestures on the other, and
// between them only the gesture being demonstrated. So the action buttons are
// no longer inside the detail panel - they are a column of their own.
el("panel-gestures").append(el("gesture-list"), el("gesture-detail"), el("gesture-actions"));
el("gesture-detail").append(el("gesture-name"), el("gesture-assignment"),
  el("gesture-fixed"), el("gesture-locked"));
// The action buttons live inside the fieldset, which is what lets the page
// disable the lot of them in one pass over their container.
el("gesture-actions").append(el("action-search"), el("action-empty"), el("action-choices"));
el("panel-advanced").append(el("advanced-groups"), el("defaults"), el("confirm"), el("path"));
el("follow-panel").append(el("follow"), el("mirror-details"));
el("mirror-details").append(el("mirror-count"), el("mirror-rows"));
el("back").append(el("back-label"));

const body = new ElementHost("body", "body");
const documentHost = Object.assign(new EventTarget(), {
  body,
  activeElement: null,
  getElementById: el,
  createElement: make,
  // The gesture drawings are SVG; the stub does not care about namespaces, but
  // the page cannot build an `<svg>` without this.
  createElementNS: (_ns, tag) => make(tag),
  createTextNode: (text) => ({ nodeValue: text }),
});
const locationHost = { search: "", hash: "", hostname: "127.0.0.1", protocol: "http:", pathname: "/config.html" };
// The address the trackpad learned from the QR and remembered. The port is
// deliberately not the default: the settings page used to hard-code 8787.
const stored = new Map([[
  "padremote.link.v1",
  JSON.stringify({ host: "192.168.1.42:9100", name: "Studio Mac" }),
]]);
globalThis.localStorage = {
  getItem: (k) => stored.get(k) ?? null,
  setItem: (k, v) => stored.set(k, v),
  removeItem: (k) => stored.delete(k),
};
const windowHost = Object.assign(new EventTarget(), {
  matchMedia: () => ({ matches: false }),
  setTimeout: (fn, ms) => setTimeout(fn, ms),
  clearTimeout: (id) => clearTimeout(id),
  devicePixelRatio: 2,
});

/**
 * The sockets, kept so a test can play the desktop's side of the conversation.
 *
 * Two of them: the settings themselves on `/config`, and who is paired on
 * `/devices`. They are different channels with different rules about who may
 * write to them, so a check has to be able to say which one it is talking to -
 * a single `socket` global silently became whichever opened last.
 */
const sockets = [];
let socket = null;
const sent = [];
const socketOn = (path) => sockets.find((s) => s.url.endsWith(path));
class SocketHost {
  static OPEN = 1;
  constructor(url) {
    this.url = url;
    this.readyState = 1;
    sockets.push(this);
    if (url.endsWith("/config")) socket = this;
  }
  send(text) { sent.push(JSON.parse(text)); }
  close() { this.readyState = 3; this.onclose?.(); }
}

const frames = [];
Object.assign(globalThis, {
  document: documentHost,
  window: windowHost,
  location: locationHost,
  history: { replaceState: (_a, _b, url) => (locationHost.hash = String(url)) },
  HTMLElement: ElementHost,
  HTMLInputElement: InputHost,
  HTMLSelectElement: SelectHost,
  WebSocket: SocketHost,
  requestAnimationFrame: (fn) => frames.push(fn),
  cancelAnimationFrame: () => {},
  getComputedStyle: () => ({ getPropertyValue: () => "" }),
});
Object.defineProperty(globalThis, "navigator", {
  configurable: true,
  value: { userAgent: "Mozilla/5.0 (iPhone) Mobile", maxTouchPoints: 5 },
});

function fire(element, type) {
  const event = new Event(type, { cancelable: true });
  Object.defineProperty(event, "target", { value: element });
  element.dispatchEvent(event);
  return event;
}
const tap = (element) => fire(element, "click");
/**
 * Open one of the settings pages.
 *
 * The tabs are gone: each section is a page of its own, reached from a hub, and
 * navigating to one is what a browser does to a link with a hash in it. So this
 * is what a tap on a hub card actually is.
 */
function go(page) {
  locationHost.hash = `#${page}`;
  windowHost.dispatchEvent(new Event("hashchange"));
}
function press(element, key) {
  const event = new Event("keydown", { cancelable: true });
  Object.defineProperty(event, "key", { value: key });
  Object.defineProperty(event, "target", { value: element });
  element.dispatchEvent(event);
}
const wait = () => new Promise((resolve) => setTimeout(resolve, 0));

// The desktop's side of the conversation, near enough to the real thing.
const CONFIG = {
  version: 1,
  followSystem: true,
  sensitivity: 1,
  accel: { gain: 1 },
  // `tap.newThing` is not in any list this page keeps: it stands for a setting
  // added to the desktop that the page has never heard of.
  tap: { tapMaxMs: 200, pressMs: 500, newThing: 7 },
  scroll: { enabled: true, natural: true, speed: 1, horizontal: true, momentum: true, accel: 1 },
  zoom: { enabled: true, backend: "appZoom", threshold: 0.05 },
  drag: { pressAndDrag: true, tapAndDrag: true },
  swipe: { minPx: 50 },
  // Every binding the desktop actually sends, so a field the page chooses not
  // to rewrite is still present with its old value.
  bindings: {
    oneTap: "leftClick", twoFingerTap: "rightClick", threeFingerTap: "middleClick",
    fourFingerTap: "none",
    twoFingerDoubleTap: "none", cornerSecondaryClick: "none", twoFingerSwipeNavigate: "none",
    threeFingerHorizSwipe: "spaces", threeFingerVertSwipe: "missionControl",
    fourFingerHorizSwipe: "none", fourFingerVertSwipe: "none",
    fourFingerPinch: "none", fiveFingerSpread: "none",
    // One field per direction, each starting at "inherit". A direction nobody
    // has touched follows the paired setting above it, so an existing setup -
    // and everything the host's own trackpad settings are mirrored into -
    // keeps working, and only the direction someone edits stops following.
    twoFingerSwipeLeft: "inherit", twoFingerSwipeRight: "inherit",
    threeFingerSwipeLeft: "inherit", threeFingerSwipeRight: "inherit",
    threeFingerSwipeUp: "inherit", threeFingerSwipeDown: "inherit",
    fourFingerSwipeLeft: "inherit", fourFingerSwipeRight: "inherit",
    fourFingerSwipeUp: "inherit", fourFingerSwipeDown: "inherit",
  },
};

/** The ten directions the desktop binds one at a time. */
const DIRECTIONS = [
  "bindings.twoFingerSwipeLeft", "bindings.twoFingerSwipeRight",
  "bindings.threeFingerSwipeLeft", "bindings.threeFingerSwipeRight",
  "bindings.threeFingerSwipeUp", "bindings.threeFingerSwipeDown",
  "bindings.fourFingerSwipeLeft", "bindings.fourFingerSwipeRight",
  "bindings.fourFingerSwipeUp", "bindings.fourFingerSwipeDown",
];
/**
 * What `Config::vocabulary` offers one direction: every action that means
 * something on its own, plus "inherit" for a direction still following the
 * paired setting it came from. Nothing here is a pair - "Volume up and down"
 * needs two directions, and this is one.
 */
const DIRECTIONAL = [
  "inherit", "none", "desktopLeft", "desktopRight", "missionControl",
  "showDesktop", "switchApps", "launchpad", "back", "forward",
  "volumeUp", "volumeDown", "mute", "brightnessUp", "brightnessDown", "zoomIn", "zoomOut",
  "previousTab", "nextTab", "undo", "redo", "copy", "paste",
  "screenshot", "lockScreen", "appWindows",
];
function state(overrides = {}) {
  const file = structuredClone(CONFIG);
  Object.assign(file, overrides.file ?? {});
  return {
    t: "config",
    computer: "Studio Mac",
    path: "/Users/x/config.json",
    file,
    effective: { ...file, sensitivity: 1.4 },
    followSystem: file.followSystem,
    // `setting` is what System Settings calls it, `name` the preference key
    // underneath - two different strings on purpose, because showing the key
    // where the name belongs is a mistake that reads as plausible.
    host: [
      { setting: "Scrolling direction", name: "com.apple.swipescrolldirection", value: "natural", status: "mirrored", detail: "" },
      { setting: "Tracking speed", name: "com.apple.trackpad.scaling", value: "1.4", status: "approximated", detail: "Apple's curve is private" },
      { setting: "Double-click speed", name: "com.apple.mouse.doubleClickThreshold", value: "0.5", status: "handled by the OS", detail: "your computer applies it" },
      { setting: "Force Click", name: "com.apple.trackpad.forceClick", value: "not set", status: "not possible", detail: "no pressure sensor" },
      { setting: "Mission Control (three fingers)", name: "TrackpadThreeFingerVertSwipeGesture", value: "on", status: "mirrored", detail: "" },
    ],
    decidedBy: {
      "scroll.natural": "Scrolling direction",
      sensitivity: "Tracking speed",
      "bindings.threeFingerVertSwipe": "Mission Control (three fingers)",
    },
    // What the mirror would write there, so the page can tell a host-decided
    // control from one the user has taken outside the host's vocabulary.
    mirrorWrites: {
      "bindings.threeFingerVertSwipe": "missionControl",
      "bindings.threeFingerHorizSwipe": "spaces",
    },
    vocabulary: {
      "bindings.twoFingerTap": ["none", "leftClick", "rightClick", "middleClick"],
      "bindings.threeFingerTap": ["none", "leftClick", "rightClick", "middleClick"],
      // No trackpad setting behind this one, so nothing ever locks it - which
      // is the point of the check below.
      "bindings.fourFingerTap": ["none", "leftClick", "rightClick", "middleClick", "missionControl"],
      ...Object.fromEntries(DIRECTIONS.map((path) => [path, DIRECTIONAL])),
      // Split by axis, as `Config::vocabulary` sends it: "spaces" does
      // nothing when swiped upward, so it must not be offered here.
      "bindings.threeFingerVertSwipe": ["none", "missionControl", "appWindows", "volume", "brightness", "zoom"],
      "bindings.threeFingerHorizSwipe": ["none", "spaces", "navigate", "tabs", "undoRedo"],
      // The four-finger pair shares its vocabulary with the three-finger one,
      // which is exactly the case where the action list gets reused.
      "bindings.fourFingerHorizSwipe": ["none", "spaces", "navigate", "tabs", "undoRedo"],
      "bindings.fourFingerVertSwipe": ["none", "missionControl", "appWindows", "volume", "brightness", "zoom"],
      // The full list `Config::vocabulary` sends for a tap.
      "bindings.oneTap": ["none", "leftClick", "rightClick", "middleClick", "missionControl",
        "appWindows", "showDesktop", "launchpad", "switchApps", "spotlight", "screenshot",
        "lockScreen", "mute", "smartZoom", "copy", "cut", "paste", "selectAll", "save", "find",
        "newTab", "closeWindow", "minimiseWindow", "quitApp", "fullScreen", "calculator"],
    },
    ...(overrides.rest ?? {}),
  };
}
const deliver = (s) => socket.onmessage({ data: JSON.stringify(s) });
const field = (path) => el(`set-${path}`);

try {
  await build({
    entryPoints: ["src/config.ts", "src/gestures.ts"],
    bundle: true, format: "esm", outdir: out, outExtension: { ".js": ".mjs" }, logLevel: "silent",
    // The page pulls in its stylesheet, which is Tailwind's entry point and
    // needs Tailwind to resolve. Nothing below asks what anything looks like,
    // so the import is dropped rather than built.
    loader: { ".css": "empty" },
  });
  const { GESTURES, actionName } =
    await import(pathToFileURL(join(out, "gestures.mjs")));

  // ------------------------------------------------- what the dots are doing

  const choices = () => el("action-choices").querySelectorAll("[data-action]");
  const actionLabels = () => [...choices()].map((c) => c.textContent);
  /* The second line on the "use existing setting" choice, which is a `small`
     inside the button rather than more of its text. */
  const inheritNote = () => [...choices()]
    .find((c) => c.dataset.action === "inherit")
    .querySelector(".choice-now");
  const chosen = () => [...choices()].find((c) => c.getAttribute("aria-checked") === "true");
  // ------------------------------------------------------------- the page

  locationHost.search = "?demo=1"; // Obsolete links must still connect to the real computer.
  locationHost.hash = "#gestures";
  await import(pathToFileURL(join(out, "config.mjs")));
  await wait();

  const pages = ["home", "basics", "gestures", "advanced"];
  const showing = () => pages.filter((name) => !el(`panel-${name}`).hidden);

  check("the trackpad's Gestures button opens the Gestures page", () => {
    // `/config.html#gestures` is the link in the trackpad's own toolbar, and it
    // has to keep landing on the gestures rather than on the hub in front of
    // them - the sections became pages, not a maze.
    assert.deepEqual(showing(), ["gestures"], "one page at a time");
    assert.equal(el("page-title").textContent, "Gestures");
  });
  check("every page is one page, and the way back is to the list of them", () => {
    // From a page, Back is the hub: something reached from a list belongs back
    // in the list. From the hub it is the trackpad the user came from.
    assert.equal(el("back").href, "#home");
    assert.equal(el("back-label").textContent, "Settings");
    go("home");
    assert.deepEqual(showing(), ["home"]);
    assert.equal(el("page-title").textContent, "Settings");
    assert.equal(el("back").href, "/");
    assert.equal(el("back-label").textContent, "Trackpad");
    // Deeper slides in from the right, back towards the hub from the left.
    go("advanced");
    assert.equal(body.dataset.direction, "in");
    go("home");
    assert.equal(body.dataset.direction, "out");
    go("gestures");
  });
  check("an address with nothing in it opens the hub, not a settings page", () => {
    locationHost.hash = "";
    windowHost.dispatchEvent(new Event("hashchange"));
    assert.deepEqual(showing(), ["home"]);
    go("gestures");
  });
  check("the settings page connects where the trackpad already connects", () => {
    // Not "ws://127.0.0.1:8787" - the page is served on one port and the
    // socket listens on another, and the QR's address is the one that works.
    assert.equal(socket.url, "ws://192.168.1.42:9100/config");
  });
  check("and names the computer it remembers while it is still connecting", () => {
    assert.match(el("computer").textContent, /Connecting to Studio Mac/);
  });
  check("nothing is claimed to be saved before a computer has answered", () => {
    assert.equal(el("saved").textContent, "");
  });

  deliver(state());

  check("the computer's name and config file are shown once it answers", () => {
    assert.equal(el("computer").textContent, "Studio Mac");
    assert.equal(el("path").textContent, "/Users/x/config.json");
  });
  check("each card on the hub says what is behind it", () => {
    // A list of four rooms is only better than one room if the list says what
    // is in them - so the cards carry the current values, not descriptions.
    // The *effective* values: 1.40 is what the mirror is running, and a card
    // reading 1.00 beside a pointer that is plainly faster is worse than a
    // card that said nothing.
    assert.match(el("nav-basics-note").textContent, /Pointer speed 1\.40/);
    assert.match(el("nav-basics-note").textContent, /Natural scrolling on/);
    assert.match(el("nav-gestures-note").textContent, /of \d+ gestures assigned/);
    assert.match(el("nav-advanced-note").textContent, /more settings/);
  });
  check("a setting the page has never heard of still appears, under Advanced", () => {
    assert.ok(field("tap.newThing"), "an unknown setting went missing");
    assert.equal(field("tap.newThing").value, "7");
  });
  check("everyday settings are on Basics and timings are not", () => {
    const on = (id, path) => {
      for (let e = field(path); e; e = e.parentElement) if (e.id === id) return true;
      return false;
    };
    assert.ok(on("panel-basics", "sensitivity"), "pointer speed was not in Basics");
    assert.ok(on("panel-basics", "scroll.natural"), "scrolling direction was not in Basics");
    assert.ok(on("panel-advanced", "tap.pressMs"), "a timing escaped into Basics");
    for (const path of ["drag.tapAndDrag", "accel.gain", "scroll.horizontal", "scroll.momentum", "zoom.enabled"]) {
      assert.ok(on("panel-advanced", path), `${path} should be in Advanced`);
    }
  });
  check("a setting the computer decides is locked, and says which setting", () => {
    assert.equal(field("scroll.natural").disabled, true);
    const row = field("scroll.natural").parentElement;
    assert.match(row.children[0].children[0].textContent, /Scrolling direction/);
  });
  check("the computer's settings are listed by the name System Settings uses", () => {
    const html = el("mirror-rows").innerHTML;
    assert.match(html, /Scrolling direction/);
    assert.doesNotMatch(html, /com\.apple\./, "a preference key leaked into the table");
  });
  check("and say what became of each one in words, not the engine's", () => {
    const html = el("mirror-rows").innerHTML;
    // The menu-bar report calls these "Matches exactly" and "Adapted for
    // touch"; two names for one state, one menu apart, reads as two states.
    assert.match(html, /Matches exactly/);
    assert.match(html, /Adapted for touch/);
    assert.match(html, /Handled by your computer/);
    // The class names stay the engine's - they key the tag colours - so this
    // looks at what is actually rendered between the tags.
    assert.doesNotMatch(html, />(mirrored|approximated|handled by the OS)</);
  });
  check("a setting this computer does not report is left out of the table", () => {
    // A row reading "Force Click: not set" answers a question nobody asked.
    assert.doesNotMatch(el("mirror-rows").innerHTML, /Force Click/);
    assert.match(el("mirror-count").textContent, /2 of 5 matched/);
  });
  check("a locked slider reads the speed the computer is actually using", () => {
    // 1.4 is the effective value; the file still says 1, and showing that would
    // be a control disagreeing with the pointer under the user's finger.
    assert.equal(field("sensitivity").value, "1.4");
  });

  const off = structuredClone(CONFIG);
  off.followSystem = false;
  deliver(state({ file: off }));

  check("turning off Match my computer hands the controls back", () => {
    assert.equal(field("scroll.natural").disabled, false);
    assert.equal(field("sensitivity").value, "1");
  });

  check("a change says Saving… until the computer confirms it", () => {
    field("scroll.speed").value = "2";
    fire(field("scroll.speed"), "change");
    assert.deepEqual(sent.at(-1).t, "setConfig");
    assert.equal(sent.at(-1).config.scroll.speed, 2);
    assert.equal(el("saved").textContent, "Saving…");
  });
  check("and Saved on your computer once the config comes back", () => {
    const saved = structuredClone(off);
    saved.scroll.speed = 2;
    deliver(state({ file: saved }));
    assert.equal(el("saved").textContent, "Saved on your computer");
  });
  check("a change made while the computer is away is not called saved", () => {
    socket.readyState = 3;
    fire(field("scroll.momentum"), "change");
    assert.match(el("saved").textContent, /Not saved/);
    socket.readyState = 1;
  });

  check("each row is a gesture, showing the action it is set to", () => {
    const does = (id) => el("gesture-list").querySelectorAll("[data-does]")
      .find((r) => r.dataset.does === id).textContent;
    // Nobody has assigned these two directions, so each row shows the action
    // it inherits from the paired setting it came from - never the word
    // "inherit", which describes the plumbing rather than the gesture.
    assert.equal(does("threeFingerSwipeUp"), "Mission Control");
    assert.equal(does("threeFingerSwipeDown"), "App windows");
    assert.match(does("scrollup"), /Scrolls the window/);
  });
  check("every row is pictured, with a finger on the pad for each one it takes", () => {
    const thumb = (id) => el("gesture-list")
      .querySelectorAll("[data-gesture]")
      .find((r) => r.dataset.gesture === id)
      .querySelector(".gesture-thumb");
    for (const g of GESTURES) {
      const drawing = thumb(g.id);
      assert.ok(drawing, `${g.id} has no drawing`);
      assert.equal(drawing.querySelectorAll(".finger").length, g.fingers,
        `${g.id} should be drawn with ${g.fingers} fingers`);
      // What the stylesheet animates on, and what tells two rows apart when
      // they take the same number of fingers in different directions.
      assert.equal(drawing.getAttribute("data-motion"), g.motion);
      assert.equal(drawing.getAttribute("data-direction"), g.direction ?? null);
    }
  });
  const thumb = (id) => el("gesture-list")
    .querySelectorAll("[data-gesture]")
    .find((r) => r.dataset.gesture === id)
    .querySelector(".gesture-thumb");

  check("a swipe is drawn with the blur it leaves, a tap with the contact under it", () => {
    // No arrows, at either size. A swipe says which way it goes with the smear
    // behind its fingers, and that smear is in the still drawing too - so the
    // reader whose browser is told not to animate is told the same thing by the
    // same shape rather than by a symbol drawn only for them.
    for (const g of GESTURES) {
      assert.equal(thumb(g.id).querySelectorAll(".pad-arrow").length, 0,
        `${g.id} is still drawn with an arrow`);
    }
    // One blur per finger, in both layers: the wide faint spill and the core at
    // the fingertip's own width.
    assert.equal(thumb("threeFingerSwipeUp").querySelectorAll(".pad-blur").length, 3);
    assert.equal(thumb("threeFingerSwipeUp").querySelectorAll(".pad-blur-core").length, 3);
    assert.equal(thumb("threeFingerSwipeUp").querySelectorAll(".pad-ripple").length, 0);
    // A tap goes nowhere, so there is nothing about it to blur.
    assert.equal(thumb("twoFingerTap").querySelectorAll(".pad-blur").length, 0);
    // One ring per finger: a two-finger tap is two contacts, not one.
    assert.equal(thumb("twoFingerTap").querySelectorAll(".pad-ripple").length, 2);
    // The hold is the only one that shows the press ring, because it is the
    // only one where waiting is the gesture - and it blurs on the drag after it.
    assert.equal(thumb("pressAndDrag").querySelectorAll(".pad-ring").length, 1);
    assert.equal(thumb("pressAndDrag").querySelectorAll(".pad-blur").length, 1);
    // The cursor moves the way every other travelling gesture does. It used to
    // ride a curve and draw the curve in under itself, and the drag out of a
    // press drew a straight green line the same way - a stroke at full strength
    // from one end of the journey to the other, which is a piece of string being
    // towed rather than what a finger leaves behind. Both smear now, so the page
    // has one idea of what movement looks like.
    assert.equal(thumb("move").querySelectorAll(".pad-blur").length, 1);
    assert.equal(thumb("move").querySelectorAll(".pad-blur-core").length, 1);
    for (const id of ["move", "pressAndDrag"]) {
      assert.equal(thumb(id).querySelectorAll(".pad-trail").length, 0,
        `${id} is still drawn with a line it keeps`);
      // Neither of them is a direction, so neither crosses the pad in a line.
      // They curve instead, handed to the stylesheet as `--path` - the same
      // curve in the row and in the panel, because it is the same drawing.
      assert.match(thumb(id).getAttribute("style") ?? "", /--path: path\('M[-\d. ]+C/,
        `${id} should be flown along a curve`);
    }
    for (const id of ["twoFingerTap", "threeFingerSwipeUp"]) {
      assert.doesNotMatch(thumb(id).getAttribute("style") ?? "", /--path/,
        `${id} has a direction of its own and should not be given a path`);
    }
    // Nothing a finger does happens off the side of a trackpad, so every shape
    // that moves is inside one clipped group. The frame is outside it: clipping
    // a stroke to its own path shaves half of that stroke away.
    for (const id of ["move", "pressAndDrag", "twoFingerTap", "threeFingerSwipeUp"]) {
      const inside = thumb(id).querySelector(".pad-inside");
      assert.ok(inside, `${id} draws outside the pad`);
      assert.equal(inside.querySelectorAll(".pad-frame").length, 0,
        `${id} clipped its own frame`);
    }
  });
  check("the thumbnail is the demonstration, only smaller", () => {
    // The two used to be different drawings - the row travelled less than half
    // as far and kept an arrow the panel dropped - so a gesture was pictured
    // one way in the list and another beside it, which is the one thing a
    // thumbnail must not do. Same shapes now; the stylesheet does the rest.
    tap(el("gesture-list").querySelector('[data-gesture="fourFingerSwipeLeft"]'));
    const shapes = (root) =>
      ["finger", "pad-halo", "pad-blur", "pad-blur-core", "pad-ripple", "pad-arrow",
       "pad-trail", "pad-ring", "pad-frame"]
        .map((name) => `${name}:${root.querySelectorAll(`.${name}`).length}`);
    assert.deepEqual(shapes(thumb("fourFingerSwipeLeft")),
      shapes(el("gesture-demo").children[0]));
  });
  check("the panel demonstrates the gesture picked, and only that one", () => {
    tap(el("gesture-list").querySelector('[data-gesture="fourFingerSwipeLeft"]'));
    const demo = el("gesture-demo").children;
    assert.equal(demo.length, 1, "one drawing at a time");
    assert.ok(demo[0].classList.contains("is-animated"), "the panel's copy moves");
    assert.equal(demo[0].getAttribute("data-direction"), "left");
    assert.equal(demo[0].querySelectorAll(".finger").length, 4);
    // Switching replaces it rather than adding to it - and a replaced element
    // is what restarts the keyframes on the new gesture.
    tap(el("gesture-list").querySelector('[data-gesture="oneTap"]'));
    assert.equal(el("gesture-demo").children.length, 1);
    assert.equal(el("gesture-demo").children[0].getAttribute("data-motion"), "tap");
  });
  check("every thumbnail moves, out of step with the rest, on no frame loop", () => {
    // A gesture is a thing that happens, so every drawing of one moves - the
    // thumbnails included. What keeps twenty of them from reading as a strobe
    // is that they are all somewhere else in the same loop, which is the phase
    // the drawing carries. Two rows landing on the same phase is fine; the
    // whole list landing on one is the failure this guards against.
    const phases = new Set();
    for (const row of el("gesture-list").querySelectorAll("[data-gesture]")) {
      const drawing = row.querySelector(".gesture-thumb");
      // `is-animated` is the panel's copy: bigger and lit, not the only one
      // that moves.
      assert.equal(drawing.classList.contains("is-animated"), false);
      const phase = /--phase:\s*([\d.]+)s/.exec(drawing.getAttribute("style") ?? "");
      assert.ok(phase, `${row.dataset.gesture} has no phase to start from`);
      phases.add(phase[1]);
    }
    assert.ok(phases.size > 6, `only ${phases.size} phases across the list`);
    // The demonstration starts its gesture from the top of the loop, because it
    // is replaced the moment a different gesture is picked.
    assert.match(el("gesture-demo").children[0].getAttribute("style"), /--phase:\s*0s/);
    // CSS keyframes, not `requestAnimationFrame`: nothing here has a loop to
    // leave running when the gesture changes or the tab is hidden.
    assert.equal(frames.length, 0);
    assert.doesNotMatch(readFileSync("config.html", "utf8"), /<canvas|demo-toggle/);
  });
  check("the actions panel names itself from inside its own box", () => {
    // A `<legend>` is drawn into the top border of its fieldset, so on a panel
    // with a radius and a background of its own the heading sat *above* the box
    // it names, with the border notched around it. Two CSS ways out of that -
    // making the fieldset a flex column, then floating the legend - both fixed
    // Chrome and neither fixed iOS Safari, which is the browser the phone reads
    // this page in. The element is what had to change, so this is checked in the
    // markup: no fieldset, no legend, and the heading inside the panel it names.
    const page = readFileSync("config.html", "utf8");
    const panel = /<section id="gesture-actions"[\s\S]*?<\/section>/.exec(page);
    assert.ok(panel, "the actions panel should be a section");
    assert.doesNotMatch(panel[0], /<legend|<fieldset/, "a legend cannot sit inside its own panel");
    assert.match(panel[0], /id="assign-title"/, "the panel needs a heading of its own");
    assert.match(panel[0], /aria-labelledby="assign-title"/, "and has to be named by it");
  });
  check("the gesture list is grouped by how many fingers land", () => {
    const sections = el("gesture-list").querySelectorAll(".gesture-section").map((h) => h.textContent);
    assert.deepEqual(sections,
      ["Pointer & taps", "2-finger swipes", "3-finger swipes", "4-finger swipes"]);
  });
  check("a gesture is named by its fingers and the one direction it goes", () => {
    tap(el("gesture-list").querySelector('[data-gesture="threeFingerSwipeUp"]'));
    assert.equal(el("gesture-name").textContent, "Swipe up with three fingers");

  });
  check("every direction of a hand is its own row, in the order a hand goes", () => {
    // A direction is its own setting, so it is its own row - there is no
    // second control repeating the four directions above the demonstration.
    const rows = el("gesture-list").querySelectorAll("[data-gesture]")
      .map((r) => r.dataset.gesture)
      .filter((id) => id.startsWith("threeFingerSwipe"));
    assert.deepEqual(rows, ["threeFingerSwipeLeft", "threeFingerSwipeRight",
      "threeFingerSwipeUp", "threeFingerSwipeDown"]);
  });
  check("two fingers up and down is scrolling, and is listed as itself", () => {
    tap(el("gesture-list").querySelector('[data-gesture="scrollup"]'));
    assert.equal(el("gesture-actions").hidden, true, "scrolling was offered an action to bind");
    assert.match(el("gesture-fixed").textContent, /Scrolls the window/);
  });
  check("the arrow keys walk the list, and take the panel with them", () => {
    tap(el("gesture-list").querySelector('[data-gesture="threeFingerSwipeUp"]'));
    press(el("gesture-list"), "ArrowDown");
    assert.equal(el("gesture-name").textContent, "Swipe down with three fingers");
    assert.equal(document.activeElement.dataset.gesture, "threeFingerSwipeDown");
    press(el("gesture-list"), "ArrowUp");
    assert.equal(el("gesture-name").textContent, "Swipe up with three fingers");
  });
  check("the phone-sized picker lists the same gestures and drives the same panel", () => {
    // A twenty-row list beside the demonstration does not fit a phone, so the
    // rows collapse to a select. It has to be the same list: a picker built
    // from a second copy of the gestures is a picker that drifts.
    const select = el("gesture-select");
    assert.equal(select.querySelectorAll("[data-name]").length,
      el("gesture-list").querySelectorAll("[data-gesture]").length);
    select.value = "fourFingerSwipeUp";
    fire(select, "change");
    assert.equal(el("gesture-name").textContent, "Swipe up with four fingers");
    assert.equal(el("gesture-list").querySelector('[data-gesture="fourFingerSwipeUp"]')
      .getAttribute("aria-selected"), "true");
  });
  check("a direction nobody has set follows the paired setting, and says so", () => {
    tap(el("gesture-list").querySelector('[data-gesture="threeFingerSwipeUp"]'));
    assert.equal(chosen().dataset.action, "inherit");
    assert.match(el("gesture-locked").textContent, /Following your existing trackpad setting/);
    // "Use existing setting" on its own is a value whose effect cannot be seen
    // without choosing it, so the choice names what following actually does -
    // on a second line, because the two of them side by side are half again as
    // wide as the column they sit in.
    assert.equal(actionLabels()[1], "Use existing setting");
    assert.equal(inheritNote().textContent, "Mission Control",
      "the inherit choice has to say what following currently does");
    assert.match(el("gesture-assignment").textContent, /Assigned: Mission Control/);
  });
  check("every action the direction can perform is shown at once, not in a menu", () => {
    // Recommended first, then the rest alphabetically - "Calculator" above
    // "Mission Control" is true and useless.
    assert.deepEqual(actionLabels().slice(0, 8), [
      "Do nothing",
      "Use existing setting",
      "Desktop left",
      "Desktop right",
      "App windows",
      "Mission Control",
      "Show Desktop",
      "Screenshot",
    ]);
    const headings = el("action-choices").querySelectorAll(".action-group").map((h) => h.textContent);
    assert.deepEqual(headings, ["Recommended", "Other actions"]);
    const marks = [...el("action-choices").children];
    const otherAt = marks.findIndex((m) => m.textContent === "Other actions");
    const rest = marks.slice(otherAt + 1).map((m) => m.textContent);
    assert.ok(rest.length > 8, `only ${rest.length} actions under Other`);
    assert.deepEqual(rest, [...rest].sort((a, b) => a.localeCompare(b)), "Other actions are unsorted");
    // One direction has no up and down for an action to split across, so the
    // paired wording - "Mission Control up, app windows down" - must not follow
    // it here: it describes a gesture the user is not being offered.
    assert.ok(!actionLabels().some((l) => /up,.*down/.test(l)), `paired wording: ${actionLabels()}`);
  });
  check("the actions sit under headings once there are more than a handful", () => {
    tap(el("gesture-list").querySelector('[data-gesture="oneTap"]'));
    const headings = el("action-choices").querySelectorAll(".action-group").map((h) => h.textContent);
    assert.deepEqual(headings, ["Recommended", "Other actions"]);
    const marks = [...el("action-choices").children];
    const otherAt = marks.findIndex((m) => m.textContent === "Other actions");
    const rest = marks.slice(otherAt + 1).map((m) => m.textContent);
    assert.ok(rest.length > 8, `only ${rest.length} actions under Other`);
    assert.deepEqual(rest, [...rest].sort((a, b) => a.localeCompare(b)), "Other actions are unsorted");
    assert.ok(actionLabels().includes("Mission Control"), "a tap was offered a direction it has none of");
    assert.ok(actionLabels().length > 8, "a tap should offer more than a click");
    assert.equal(actionLabels()[0], "Do nothing", "Doing nothing belongs at the top, ungrouped");
  });
  check("a heading takes the whole row of the grid its group is laid out in", () => {
    // The box of choices is as many columns wide as fits, so a heading that
    // does not span leaves the first choice of its group sitting up beside the
    // words naming the group above it. Nothing about that fails to render: the
    // words are there, the buttons are there, and the reading is wrong. So the
    // class is asserted here rather than being left to be noticed.
    tap(el("gesture-list").querySelector('[data-gesture="oneTap"]'));
    for (const heading of el("action-choices").querySelectorAll(".action-group")) {
      assert.ok(
        heading.classList.contains("col-span-full"),
        `"${heading.textContent}" does not span the columns`,
      );
    }
  });
  check("a long list of actions can be searched down to the one wanted", () => {
    tap(el("gesture-list").querySelector('[data-gesture="threeFingerSwipeUp"]'));
    const search = (text) => {
      el("action-search").value = text;
      fire(el("action-search"), "input");
      return [...choices()].filter((c) => !c.hidden).map((c) => c.textContent);
    };
    assert.deepEqual(search("volume"), ["Volume down", "Volume up"]);
    assert.equal(el("action-empty").hidden, true);
    assert.deepEqual(search("teleport"), []);
    assert.equal(el("action-empty").hidden, false, "an empty list said nothing about being empty");
    search("");
    assert.equal(el("action-empty").hidden, true);
  });
  check("choosing an action sets that one direction and leaves the rest alone", () => {
    tap(el("gesture-list").querySelector('[data-gesture="threeFingerSwipeUp"]'));
    [...choices()].find((c) => c.dataset.action === "showDesktop").dispatchEvent(new Event("click"));
    const bindings = sent.at(-1).config.bindings;
    assert.equal(bindings.threeFingerSwipeUp, "showDesktop");
    assert.equal(bindings.threeFingerSwipeDown, "inherit", "the opposite direction stopped following");
    assert.equal(bindings.threeFingerVertSwipe, "missionControl", "the paired setting was rewritten");
  });
  check("and the direction left alone keeps following the old setting", () => {
    const custom = structuredClone(CONFIG);
    custom.followSystem = false;
    custom.bindings.threeFingerSwipeUp = "showDesktop";
    deliver(state({ file: custom }));
    const does = (id) => el("gesture-list").querySelectorAll("[data-does]")
      .find((r) => r.dataset.does === id).textContent;
    assert.equal(does("threeFingerSwipeUp"), "Show Desktop");
    assert.equal(does("threeFingerSwipeDown"), "App windows");
    tap(el("gesture-list").querySelector('[data-gesture="threeFingerSwipeUp"]'));
    assert.match(el("gesture-locked").textContent, /Set for this direction only/);
  });
  check("brightness splits into up and down the way volume does", () => {
    // Up is brighter, the same way up is louder - and a vertical pair that got
    // this backwards would dim the screen when asked to brighten it, which is
    // the one mistake nobody would think to check for.
    const does = (id) => el("gesture-list").querySelectorAll("[data-does]")
      .find((r) => r.dataset.does === id).textContent;
    const bright = structuredClone(CONFIG);
    bright.followSystem = false;
    bright.bindings.threeFingerVertSwipe = "brightness";
    bright.bindings.threeFingerSwipeUp = "inherit";
    bright.bindings.threeFingerSwipeDown = "inherit";
    deliver(state({ file: bright }));
    assert.equal(does("threeFingerSwipeUp"), "Brightness up");
    assert.equal(does("threeFingerSwipeDown"), "Brightness down");

    // And both directions are offered by name, so a swipe can take one alone.
    tap(el("gesture-list").querySelector('[data-gesture="threeFingerSwipeUp"]'));
    el("action-search").value = "bright";
    fire(el("action-search"), "input");
    assert.deepEqual(
      [...choices()].filter((c) => !c.hidden).map((c) => c.textContent),
      ["Brightness down", "Brightness up"],
    );
    el("action-search").value = "";
    fire(el("action-search"), "input");
  });
  check("what a sideways swipe inherits depends on which way scrolling runs", () => {
    // The paired setting says "spaces"; which desktop a swipe *left* reaches
    // is then the host's scrolling direction, and getting it backwards binds
    // both sideways swipes to the wrong neighbour.
    const does = (id) => el("gesture-list").querySelectorAll("[data-does]")
      .find((r) => r.dataset.does === id).textContent;
    const natural = structuredClone(CONFIG);
    natural.followSystem = false;
    deliver(state({ file: natural }));
    assert.equal(does("threeFingerSwipeLeft"), "Desktop right");
    assert.equal(does("threeFingerSwipeRight"), "Desktop left");

    const reversed = structuredClone(natural);
    reversed.scroll.natural = false;
    deliver(state({ file: reversed }));
    assert.equal(does("threeFingerSwipeLeft"), "Desktop left");
    assert.equal(does("threeFingerSwipeRight"), "Desktop right");
  });
  check("a desktop too old to bind one direction says so instead of pretending", () => {
    // The vocabulary is the desktop's, and an older one sends no per-direction
    // fields at all. Offering the choices anyway would accept a setting that
    // the desktop drops on the floor.
    const old = state({ file: { ...structuredClone(CONFIG), followSystem: false } });
    for (const path of DIRECTIONS) delete old.vocabulary[path];
    deliver(old);
    tap(el("gesture-list").querySelector('[data-gesture="threeFingerSwipeUp"]'));
    assert.match(el("gesture-locked").textContent, /Update the desktop app/);
    assert.ok([...choices()].every((c) => c.disabled), "it offered what the desktop cannot store");
    // And the row still says what the swipe does on that desktop today.
    assert.equal(el("gesture-list").querySelectorAll("[data-does]")
      .find((r) => r.dataset.does === "threeFingerSwipeUp").textContent, "Mission Control");
  });
  check("a tap the computer decides is locked, and says which setting decides it", () => {
    const s2 = state({ file: { ...structuredClone(CONFIG), followSystem: true } });
    s2.decidedBy["bindings.twoFingerTap"] = "Secondary click (two fingers)";
    s2.host = [...s2.host, { setting: "Secondary click (two fingers)", name: "TrackpadRightClick",
      value: "on", status: "mirrored", detail: "" }];
    deliver(s2);
    tap(el("gesture-list").querySelector('[data-gesture="twoFingerTap"]'));
    assert.equal(chosen().dataset.action, "rightClick", "it showed the file, not the computer");
    assert.ok([...choices()].every((c) => c.disabled), "a gesture the computer owns stayed selectable");
    assert.match(el("gesture-locked").textContent, /Your computer decides this/);
    assert.match(el("gesture-locked").textContent, /Secondary click/);
  });
  check("and one the mirror only copied is locked with a gentler way out", () => {
    // Seeded rather than decided: the mirror writes its own action and leaves
    // any other choice alone, so the way out is turning matching off rather
    // than changing the setting on the computer. The two notes must not read
    // as the same sentence, and the milder case is styled as the milder case.
    const s2 = state({ file: { ...structuredClone(CONFIG), followSystem: true } });
    s2.decidedBy["bindings.threeFingerTap"] = "Three finger tap";
    s2.mirrorWrites["bindings.threeFingerTap"] = "middleClick";
    s2.host = [...s2.host, { setting: "Three finger tap", name: "TrackpadThreeFingerTapGesture",
      value: "on", status: "mirrored", detail: "" }];
    deliver(s2);
    tap(el("gesture-list").querySelector('[data-gesture="threeFingerTap"]'));
    assert.match(el("gesture-locked").textContent, /Copied from your computer/);
    assert.ok(el("gesture-locked").classList.contains("seeded"));
    assert.doesNotMatch(el("gesture-locked").textContent, /Your computer decides this/);
    // The note has to name a way out that actually works. "Choose anything
    // else" did not: the mirror re-seeds a binding sitting at none, so picking
    // Nothing sprang straight back.
    assert.match(el("gesture-locked").textContent, /Turn off Match my computer/);
    assert.ok([...choices()].every((c) => c.disabled), "a seeded gesture was still selectable");
  });
  check("a four-finger tap is offered, and nothing on the Mac can claim it", () => {
    // The one tap with no trackpad setting behind it: macOS has no four-finger
    // tap, so the mirror never writes it and it can never be locked. That is
    // what makes it worth a check of its own - every other tap in this list
    // spends most of its life owned by the computer, and a bug that locked all
    // of them would be invisible here without a gesture that must stay free.
    const s2 = state({ file: { ...structuredClone(CONFIG), followSystem: true } });
    deliver(s2);
    const row = el("gesture-list").querySelector('[data-gesture="fourFingerTap"]');
    assert.ok(row, "the gesture list never offered a four-finger tap");
    tap(row);
    assert.equal(el("gesture-locked").hidden, true, "something claimed to decide a four-finger tap");
    assert.ok([...choices()].some((c) => !c.disabled), "a gesture nothing owns was not settable");
    assert.equal(chosen().dataset.action, "none", "it should start out unbound");
    // Its drawing is covered by "every row is pictured" above, which walks
    // GESTURES and counts fingers on the pad - so a four-finger tap drawn with
    // three is caught there rather than asserted twice here.
  });
  check("turning off Match my computer hands the gesture back", () => {
    const free = structuredClone(CONFIG);
    free.followSystem = false;
    deliver(state({ file: free }));
    tap(el("gesture-list").querySelector('[data-gesture="twoFingerTap"]'));
    assert.equal(el("gesture-locked").hidden, true, "it still claimed the computer owned it");
    assert.ok([...choices()].every((c) => !c.disabled), "the gesture stayed locked with matching off");
    [...choices()].find((c) => c.dataset.action === "middleClick").dispatchEvent(new Event("click"));
    assert.equal(sent.at(-1).config.bindings.twoFingerTap, "middleClick");
  });
  check("switching between gestures rebinds the actions to the new one", () => {
    // Two gestures with the same action list can reuse the same buttons. Each
    // button captured the gesture it was built for, so after switching, a click
    // committed to the *previous* gesture - and the new one looked like it
    // refused to accept anything.
    const free = structuredClone(CONFIG);
    free.followSystem = false;
    deliver(state({ file: free }));

    tap(el("gesture-list").querySelector('[data-gesture="threeFingerSwipeLeft"]'));
    tap(el("gesture-list").querySelector('[data-gesture="fourFingerSwipeLeft"]'));
    [...choices()].find((c) => c.dataset.action === "desktopRight").dispatchEvent(new Event("click"));
    const bindings = sent.at(-1).config.bindings;
    assert.equal(bindings.fourFingerSwipeLeft, "desktopRight", "the click went to the wrong gesture");
    assert.equal(bindings.threeFingerSwipeLeft, "inherit", "it changed the one left behind");
  });
  check("and switching back and forth keeps working", () => {
    const free = structuredClone(CONFIG);
    free.followSystem = false;
    for (const id of ["threeFingerSwipeUp", "threeFingerSwipeLeft", "threeFingerSwipeUp"]) {
      deliver(state({ file: free }));
      tap(el("gesture-list").querySelector(`[data-gesture="${id}"]`));
    }
    [...choices()].find((c) => c.dataset.action === "showDesktop").dispatchEvent(new Event("click"));
    assert.equal(sent.at(-1).config.bindings.threeFingerSwipeUp, "showDesktop");
  });
  check("a setting the computer genuinely forces is still locked", () => {
    // Pointer speed is written straight over by the mirror, not seeded, so it
    // really is out of the user's hands while Match my computer is on.
    // Delivered here rather than inherited: the checks above turn matching off.
    deliver(state({ file: { ...structuredClone(CONFIG), followSystem: true } }));
    go("basics");
    assert.equal(field("sensitivity").disabled, true);
    go("gestures");
  });
  check("a gesture with nothing to choose states what it does instead", () => {
    tap(el("gesture-list").querySelector('[data-gesture="scrollup"]'));
    assert.equal(el("gesture-actions").hidden, true);
    assert.equal(el("gesture-fixed").hidden, false);
    assert.match(el("gesture-fixed").textContent, /Scrolls the window/i);
  });
  check("a gesture this app never asks anyone to perform still lives in Advanced", () => {
    assert.ok(field("bindings.fourFingerPinch"), "the pinch binding vanished entirely");
    assert.ok(field("bindings.twoFingerDoubleTap"), "the double-tap binding vanished entirely");
    assert.ok(field("zoom.enabled"), "pinch zoom vanished entirely");
  });
  check("the paired setting a direction inherits is not also editable in Advanced", () => {
    // It is the Gestures tab's business now: two controls for one behaviour,
    // one of them worded for a trackpad this app does not have, is how a swipe
    // ends up set twice and running as neither.
    const inPanel = (id, path) => {
      for (let e = field(path); e; e = e.parentElement) if (e.id === id) return true;
      return false;
    };
    assert.ok(!inPanel("panel-advanced", "bindings.threeFingerVertSwipe"),
      "the old paired swipe setting is offered twice");
    assert.ok(inPanel("panel-advanced", "bindings.fourFingerPinch"),
      "a binding no gesture claims went missing");
  });
  check("the gesture list is one tab stop, wired to the panel it drives", () => {
    const tabs = el("gesture-list").querySelectorAll("[data-gesture]");
    assert.ok(tabs.length > 6, "the gesture list is empty");
    const tabbable = tabs.filter((t) => t.getAttribute("tabindex") === "0");
    assert.equal(tabbable.length, 1, "every row was its own tab stop");
    assert.equal(tabbable[0].getAttribute("aria-selected"), "true");
    for (const t of tabs) assert.equal(t.getAttribute("aria-controls"), "gesture-detail");
    assert.equal(el("gesture-detail").getAttribute("aria-labelledby"), tabbable[0].id);
  });
  go("advanced");
  check("Restore defaults asks before it does anything", () => {
    const before = sent.length;
    tap(el("defaults"));
    assert.equal(el("confirm").hidden, false);
    assert.equal(sent.length, before, "it reset the config without asking");
    assert.match(el("confirm-name").textContent, /Studio Mac/);
  });
  check("cancelling leaves the config alone", () => {
    const before = sent.length;
    tap(el("confirm-no"));
    assert.equal(el("confirm").hidden, true);
    assert.equal(sent.length, before);
  });
  check("confirming resets, and says so", () => {
    tap(el("defaults"));
    tap(el("confirm-yes"));
    assert.equal(sent.at(-1).t, "reset");
    assert.equal(el("saved").textContent, "Saving…");
  });

  // ----------------------------------------------------- connected devices
  //
  // Its own channel, on its own socket: reading the list is for anyone who has
  // answered the challenge, and revoking a pairing is for the computer alone.
  // Which of the two a caller may do is decided on the desktop, from the TCP
  // peer - so the page is told in `manage` rather than guessing from the fact
  // that it happens to be running on a phone.

  const deliverDevices = (msg) =>
    socketOn("/devices").onmessage({ data: JSON.stringify(msg) });
  const rows = () => el("device-rows").children;
  const words = (i) => rows()[i].querySelectorAll(".device-words")[0].children.map((c) => c.textContent);
  const forgets = () => el("device-rows").querySelectorAll("[data-forget]");
  const PHONE = { id: "a1", name: "An’s iPhone", connected: true, driving: true, since: 1738540800 };
  const IPAD = { id: "b2", name: "iPad", connected: false, driving: false, since: 1717200000 };
  // Connected with no credential: the replay tool and the tests authenticate
  // with the QR secret and never enrol.
  const REPLAY = { id: null, name: "replay", connected: true, driving: false, since: null };
  const list = (manage) =>
    ({ t: "devices", connected: 2, manage, devices: [PHONE, IPAD, REPLAY] });

  go("home");
  check("the devices have their own socket, not the settings one", () => {
    assert.equal(socketOn("/devices").url, "ws://192.168.1.42:9100/devices");
  });
  check("the home page lists who is paired, and which of them is here now", () => {
    deliverDevices(list(false));
    assert.equal(rows().length, 3);
    assert.equal(words(0)[0], "An’s iPhone");
    // Driving beats connected: the phone in your hand is the one you are
    // looking for, and "connected" is true of it either way.
    assert.match(words(0)[1], /^Moving the cursor now · Paired /);
    assert.equal(words(1)[0], "iPad");
    assert.doesNotMatch(words(1)[1], /Connected/);
    assert.equal(el("device-count").textContent, "2 of 3 connected");
    assert.equal(el("device-empty").hidden, true);
  });
  check("the count is a caption on the list rather than a card to tap", () => {
    // It used to be a card's subtitle standing in for a list one tap away. The
    // list is on this page now, so the number is a caption beside it - and
    // there is no devices page left to open.
    assert.equal(el("device-count").textContent, "2 of 3 connected");
    assert.equal(el("panel-home").hidden, false);
    assert.ok(el("panel-home").querySelectorAll("[data-forget]").length >= 0,
      "the device list is not on the home page");
  });
  check("a phone may read the list and not revoke anything", () => {
    // Said once, under the list, rather than as a disabled button per row: a
    // row of greyed-out buttons is a page asking to be tapped before it will
    // explain itself.
    assert.equal(forgets().length, 0, "a phone was offered a Forget button");
    assert.equal(el("device-note").hidden, false);
    assert.equal(el("forget-panel").hidden, true);
  });
  check("the computer may, and forgets the device it was asked to", () => {
    deliverDevices(list(true));
    // Two of the three: the connection with no credential has nothing to
    // revoke, so offering to forget it would be offering nothing.
    assert.deepEqual(forgets().map((b) => b.dataset.forget), ["a1", "b2"]);
    assert.equal(el("device-note").hidden, true);
    tap(forgets()[1]);
    assert.deepEqual(sent.at(-1), { t: "forget", id: "b2" });
  });
  check("forgetting everything asks first", () => {
    const before = sent.length;
    tap(el("forget-all"));
    assert.equal(el("forget-confirm").hidden, false);
    assert.equal(sent.length, before, "it un-paired every device without asking");
    assert.match(el("forget-name").textContent, /Studio Mac/);
    tap(el("forget-no"));
    assert.equal(el("forget-confirm").hidden, true);
    assert.equal(sent.length, before);
    tap(el("forget-all"));
    tap(el("forget-yes"));
    assert.deepEqual(sent.at(-1), { t: "forgetAll" });
  });
  check("a computer nothing is paired with says that, rather than nothing", () => {
    deliverDevices({ t: "devices", connected: 0, manage: true, devices: [] });
    assert.equal(rows().length, 0);
    assert.equal(el("device-empty").hidden, false);
    assert.equal(el("forget-panel").hidden, true, "offered to forget nothing");
    // The "nothing is paired yet" line is in the markup rather than written by
    // the page, so what is asserted here is that it is shown - and that the
    // count says nothing rather than "0 of 0 connected".
    assert.equal(el("device-empty").hidden, false);
    assert.equal(el("device-count").textContent, "", "counted nothing out loud");
  });

  console.log(`\n${passed} settings checks passed.`);
} finally {
  rmSync(out, { recursive: true, force: true });
}
