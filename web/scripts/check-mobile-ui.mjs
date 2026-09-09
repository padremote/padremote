/** Behavior regressions for iPhone touch controls and delayed viewport layout.
 * Uses the real modules with a small event/geometry host; no desktop is needed.
 * Run from web/: node scripts/check-mobile-ui.mjs
 */
import assert from "node:assert/strict";
import { mkdtempSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { pathToFileURL } from "node:url";
import { build } from "esbuild";

const out = mkdtempSync(join(tmpdir(), "padremote-mobile-"));
let passed = 0;
function check(name, run) {
  run();
  passed++;
  console.log(`ok  ${name}`);
}

class ElementHost extends EventTarget {
  constructor(id = "") {
    super();
    this.id = id;
    this.hidden = false;
    this.disabled = false;
    this.inert = false;
    this.value = "";
    this.children = [];
    this.attributes = new Map();
    this.classes = new Set();
    this.classList = {
      toggle: (name, on) => on ? this.classes.add(name) : this.classes.delete(name),
      contains: (name) => this.classes.has(name),
    };
  }
  append(...elements) {
    for (const element of elements) {
      element.parentElement = this;
      this.children.push(element);
    }
  }
  setAttribute(name, value) { this.attributes.set(name, value); }
  getAttribute(name) { return this.attributes.get(name) ?? null; }
  removeAttribute(name) { this.attributes.delete(name); }
  focus() { document.activeElement = this; }
  contains(element) {
    return element === this || this.children.some((child) => child.contains(element));
  }
  querySelectorAll() { return this.controls ?? []; }
  getClientRects() { return this.hidden || this.disabled ? [] : [{}]; }
}

const elements = new Map();
function el(id) {
  if (!elements.has(id)) elements.set(id, new ElementHost(id));
  return elements.get(id);
}
const frame = el("app-frame");
const surface = el("surface");
const toolbar = el("toolbar");
const sheet = el("sheet");
frame.append(surface, toolbar, sheet);
toolbar.append(el("settings-open"));
sheet.append(...[
  "settings-close", "device-name", "sensitivity", "sensitivity-value", "natural", "click-sound",
  "enable-shake", "shake-note", "fullscreen-note", "go-full-screen", "sensitivity-note",
  "natural-note", "all-settings",
].map(el));
sheet.controls = [el("settings-close"), el("device-name"), el("all-settings")];

const root = el("html");
root.clientWidth = 390;
root.clientHeight = 844;
const css = new Map();
let writes = 0;
root.style = { setProperty: (name, value) => { css.set(name, value); writes++; } };
const documentHost = Object.assign(new EventTarget(), {
  documentElement: root,
  body: el("body"),
  visibilityState: "visible",
  activeElement: null,
  getElementById: el,
});
const visualViewport = Object.assign(new EventTarget(), {
  width: 390, height: 664, offsetTop: 0, offsetLeft: 0,
});
const timers = new Map();
const frames = new Map();
let nextId = 0;
const windowHost = Object.assign(new EventTarget(), {
  innerWidth: 390,
  innerHeight: 844,
  scrollX: 0,
  scrollY: 0,
  history: { scrollRestoration: "auto" },
  scrollTo: (x, y) => { windowHost.scrollX = x; windowHost.scrollY = y; },
  visualViewport,
  setTimeout: (callback) => { const id = ++nextId; timers.set(id, callback); return id; },
  clearTimeout: (id) => timers.delete(id),
});
Object.assign(globalThis, {
  document: documentHost,
  window: windowHost,
  HTMLElement: ElementHost,
  isSecureContext: false,
  requestAnimationFrame: (callback) => { const id = ++nextId; frames.set(id, callback); return id; },
  cancelAnimationFrame: (id) => frames.delete(id),
});
// Node defines navigator as a getter-only global, so it has to be redefined rather than assigned.
Object.defineProperty(globalThis, "navigator", {
  configurable: true,
  value: { userAgent: "Mozilla/5.0 (iPhone) CriOS/130.0 Mobile", maxTouchPoints: 5, platform: "iPhone" },
});
const stored = new Map();
globalThis.localStorage = {
  getItem: (key) => stored.get(key) ?? null,
  setItem: (key, value) => stored.set(key, value),
  removeItem: (key) => stored.delete(key),
};
function nextFrame() {
  const queued = [...frames.values()];
  frames.clear();
  queued.forEach((callback) => callback());
}
function settleTimers() {
  const queued = [...timers.values()];
  timers.clear();
  queued.forEach((callback) => callback());
  nextFrame();
}
function dispatch(element, type) {
  const event = new Event(type, { cancelable: true });
  for (let target = element; target; target = target.parentElement) target.dispatchEvent(event);
  document.dispatchEvent(event);
  return event;
}
function tap(element) {
  // Touch cancellation suppresses the compatibility click on mobile browsers.
  const down = dispatch(element, "touchstart");
  const up = dispatch(element, "touchend");
  if (!down.defaultPrevented && !up.defaultPrevented) element.dispatchEvent(new Event("click"));
}
function key(key, shiftKey = false) {
  const event = new Event("keydown", { cancelable: true });
  Object.assign(event, { key, shiftKey });
  document.dispatchEvent(event);
  return event;
}

try {
  await build({
    entryPoints: ["src/viewport.ts", "src/surface.ts", "src/ui.ts", "src/pairing.ts"],
    bundle: true, format: "esm", outdir: out, outExtension: { ".js": ".mjs" }, logLevel: "silent",
    define: { "import.meta.env.VITE_DESKTOP_WS": "undefined" },
  });
  const { trackViewport, watchSize } = await import(pathToFileURL(join(out, "viewport.mjs")));
  const { suppressBrowserGestures } = await import(pathToFileURL(join(out, "surface.mjs")));
  const { Ui, FOLLOWING } = await import(pathToFileURL(join(out, "ui.mjs")));

  const { resolveLink } = await import(pathToFileURL(join(out, "pairing.mjs")));
  // The QR carries the pairing secret as well as the address, and both have to
  // survive the round trip: a phone that resolves the host but drops the key
  // connects, fails the desktop's challenge, and reports "not paired" - which
  // looks like a pairing bug rather than the parsing bug it is.
  const qrKey = "0123456789abcdef0123456789abcdef";
  const qrHash = `#h=192.168.1.117:9100&n=Studio%20Mac&k=${qrKey}`;
  globalThis.location = {
    hash: qrHash, pathname: "/", search: "", hostname: "192.168.1.117",
  };
  let historyWrites = 0;
  globalThis.history = {
    replaceState: () => { historyWrites++; location.hash = ""; },
  };
  check("QR pairing preserves the scanned navigation while resolving the computer", () => {
    assert.deepEqual(resolveLink(), {
      host: "192.168.1.117:9100", name: "Studio Mac", key: qrKey, source: "fragment",
    });
    assert.equal(location.hash, qrHash);
    assert.equal(historyWrites, 0);
  });
  check("typing the base address reconnects to the same QR-paired computer", () => {
    location.hash = "";
    assert.deepEqual(resolveLink(), {
      host: "192.168.1.117:9100", name: "Studio Mac", key: qrKey, source: "stored",
    });
    assert.equal(historyWrites, 0);
  });
  check("QR pairing survives reload with storage blocked and a nondefault port", () => {
    const storage = globalThis.localStorage;
    globalThis.localStorage = {
      getItem() { throw new Error("storage blocked"); },
      setItem() { throw new Error("storage blocked"); },
    };
    location.hash = qrHash;
    try {
      assert.equal(resolveLink().host, "192.168.1.117:9100");
      assert.equal(resolveLink().key, qrKey, "the secret comes from the URL, not storage");
      // A reload retains the current URL but has no saved connection.
      assert.equal(resolveLink().host, "192.168.1.117:9100");
      assert.equal(location.hash, qrHash);
      assert.equal(historyWrites, 0);
    } finally {
      globalThis.localStorage = storage;
      location.hash = "";
      stored.clear();
    }
  });

  const stop = trackViewport();
  check("first load uses the visible phone viewport immediately", () => {
    assert.equal(css.get("--app-w"), "390px");
    assert.equal(css.get("--app-h"), "664px");
  });
  check("late browser-bar movement settles without requiring reload or an event", () => {
    visualViewport.height = 740;
    settleTimers();
    assert.equal(css.get("--app-h"), "740px");
  });
  check("unchanged measurements do not rewrite styles", () => {
    const before = writes;
    visualViewport.dispatchEvent(new Event("scroll"));
    nextFrame();
    assert.equal(writes, before);
  });
  check("keyboard visible area and offset stay in the same rectangle", () => {
    visualViewport.height = 320;
    visualViewport.offsetTop = 120;
    visualViewport.dispatchEvent(new Event("resize"));
    nextFrame();
    assert.equal(css.get("--app-top"), "120px");
    assert.equal(css.get("--app-h"), "320px");
    assert.equal(css.get("--app-bottom"), "404px");
  });
  check("zero-sized restored frames fall back to usable layout dimensions", () => {
    visualViewport.width = 0;
    visualViewport.height = 0;
    window.dispatchEvent(new Event("pageshow"));
    assert.equal(css.get("--app-h"), "844px");
    assert.equal(css.get("--app-top"), "0px");
    visualViewport.width = 390;
    visualViewport.height = 740;
    visualViewport.offsetTop = 0;
    settleTimers();
    assert.equal(css.get("--app-h"), "740px");
  });
  check("rotation settles even when the orientation event has stale dimensions", () => {
    window.dispatchEvent(new Event("orientationchange"));
    Object.assign(visualViewport, { width: 844, height: 320 });
    settleTimers();
    assert.equal(css.get("--app-w"), "844px");
    assert.equal(css.get("--app-h"), "320px");
  });
  check("returning from a background tab remeasures without a resize event", () => {
    visualViewport.height = 350;
    document.dispatchEvent(new Event("visibilitychange"));
    assert.equal(css.get("--app-h"), "350px");
  });
  check("browser bars settle after a resize with stale geometry long after startup", () => {
    settleTimers();
    visualViewport.dispatchEvent(new Event("resize"));
    nextFrame();
    visualViewport.height = 700;
    settleTimers();
    assert.equal(css.get("--app-h"), "700px");
  });
  check("keyboard dismissal settles even without a final viewport event", () => {
    visualViewport.height = 320;
    document.dispatchEvent(new Event("focusout"));
    visualViewport.height = 740;
    visualViewport.offsetTop = 0;
    settleTimers();
    assert.equal(css.get("--app-h"), "740px");
    assert.equal(css.get("--app-top"), "0px");
  });
  check("viewport cleanup cancels delayed work and listeners", () => {
    stop();
    const before = writes;
    visualViewport.height = 400;
    window.dispatchEvent(new Event("pageshow"));
    settleTimers();
    assert.equal(writes, before);
    assert.equal(timers.size, 0);
  });
  check("geometry watchers coalesce signals and cancel pending animation frames", () => {
    let count = 0;
    const unwatch = watchSize(surface, () => count++);
    window.dispatchEvent(new Event("resize"));
    window.dispatchEvent(new Event("scroll"));
    visualViewport.dispatchEvent(new Event("resize"));
    nextFrame();
    assert.equal(count, 1);
    window.dispatchEvent(new Event("resize"));
    unwatch();
    nextFrame();
    assert.equal(count, 1);
  });

  suppressBrowserGestures();
  check("iPhone pad touches still suppress browser gestures", () => {
    assert.equal(dispatch(surface, "touchstart").defaultPrevented, true);
    assert.equal(dispatch(surface, "touchmove").defaultPrevented, true);
  });
  check("iPhone button taps, text selection and sheet scrolling remain native", () => {
    assert.equal(dispatch(el("settings-open"), "touchstart").defaultPrevented, false);
    assert.equal(dispatch(el("device-name"), "selectstart").defaultPrevented, false);
    assert.equal(dispatch(sheet, "touchmove").defaultPrevented, false);
  });

  // A malformed old preference must not stop local settings initialization.
  stored.set("padremote.settings.v2", '{"sensitivity":"broken","naturalScroll":42}');
  const patches = [];
  let fullscreenCalls = 0;
  const ui = new Ui({
    onTakeControl() {}, onNameChange() {}, onEnableShake: async () => false,
    onFullScreen: () => fullscreenCalls++, onSettingsChange: (patch) => patches.push(patch),
  });
  check("stored invalid values cannot prevent the settings button working offline", () => {
    assert.equal(ui.settings.sensitivity, null);
    tap(el("settings-open"));
    assert.equal(sheet.hidden, false);
    assert.equal(sheet.classList.contains("open"), true);
    assert.equal(el("settings-open").getAttribute("aria-expanded"), "true");
    assert.equal(document.activeElement, el("settings-close"));
    assert.equal(surface.inert, true);
  });
  check("keyboard focus wraps within the open settings dialog", () => {
    el("all-settings").focus();
    assert.equal(key("Tab").defaultPrevented, true);
    assert.equal(document.activeElement, el("settings-close"));
    key("Tab", true);
    assert.equal(document.activeElement, el("all-settings"));
  });
  check("Escape closes settings and restores the pad and trigger focus", () => {
    key("Escape");
    assert.equal(sheet.hidden, true);
    assert.equal(surface.inert, false);
    assert.equal(document.activeElement, el("settings-open"));
  });
  check("tapping the settings backdrop closes the dialog", () => {
    tap(el("settings-open"));
    tap(sheet);
    assert.equal(sheet.hidden, true);
  });
  check("a phone that has never been paired is told to scan, not to check Wi-Fi", () => {
    // The page is a static site anyone can open directly, and it installs as a
    // PWA - so "opened without ever scanning" is a normal first run, and Wi-Fi
    // advice for it sends people to check something that was never the problem.
    ui.setStatus("offline", "");
    assert.match(el("hint").textContent, /scan the QR code/i);
    assert.doesNotMatch(el("hint").textContent, /same Wi-Fi/i);
  });
  check("and a phone that has been paired keeps the advice that fits it", () => {
    ui.setPaired(true);
    ui.setStatus("offline", "Studio Mac");
    assert.match(el("hint").textContent, /same Wi-Fi/i);
    ui.setStatus("connected", "Studio Mac");
  });
  check("a computer that cannot move its cursor is not shown as working", () => {
    // The failure this page cannot see on its own: the socket is up, the
    // gesture readout follows every finger, the latency figure is live, and
    // nothing moves. Green here is the lie that sends people to their Wi-Fi.
    ui.setControl({ active: true, devices: 1, blocked: "permission" });
    assert.equal(el("dot").className, "dot waiting");
    assert.match(el("hint").textContent, /Accessibility/);
    assert.match(el("hint").textContent, /Studio Mac/, "it must name the computer to go and fix");
    assert.equal(el("hint").hidden, false);
  });
  check("and being blocked outranks whose turn it is", () => {
    // No queue to be at the back of when the cursor is frozen for everybody:
    // "wait your turn" would send the user to the wrong computer entirely.
    ui.setControl({ active: false, holder: "iPad", devices: 2, blocked: "permission" });
    assert.doesNotMatch(el("hint").textContent, /take over/i);
    assert.match(el("hint").textContent, /Accessibility/);
  });
  check("a dry-run computer says that instead, because the fix is different", () => {
    ui.setControl({ active: true, devices: 1, blocked: "dryRun" });
    assert.match(el("hint").textContent, /dry-run/);
    assert.doesNotMatch(el("hint").textContent, /Accessibility/);
  });
  check("and the moment permission lands the pad goes quiet again", () => {
    // The desktop swaps its backend in without a restart, so this arrives on
    // the same connection. A page that kept apologising would be apologising
    // over a working trackpad.
    ui.setControl({ active: true, devices: 1 });
    assert.equal(el("dot").className, "dot connected");
    assert.equal(el("hint").textContent, "");
    assert.equal(el("hint").hidden, true);
  });
  check("a browser that cannot go full screen says so, and says what would", () => {
    // iOS gives no browser a Fullscreen API for an element, so asking for full
    // screen there returns something less. Saying that where the control lives
    // beats a message that fades after three seconds.
    ui.setFullScreenLimit(true);
    assert.equal(el("fullscreen-note").hidden, false);
    assert.match(el("fullscreen-note").textContent, /Add to Home Screen/i);
    assert.doesNotMatch(el("fullscreen-note").textContent, /Safari/, "it named one browser on all of them");
    ui.setFullScreenLimit(false);
    assert.equal(el("fullscreen-note").hidden, true);
  });
  check("the first paint and the running page use the same words for following", () => {
    // index.html paints these before any script runs; if the two disagree the
    // label silently rewrites itself a moment after the sheet opens.
    const page = readFileSync("index.html", "utf8");
    const painted = [...page.matchAll(/id="(?:sensitivity|natural)-note"[^>]*>([^<]*)</g)].map((m) => m[1]);
    assert.equal(painted.length, 2, "the override notes moved or were renamed");
    for (const text of painted) assert.equal(text, FOLLOWING);
    assert.equal(el("sensitivity-note").textContent, FOLLOWING);
  });
  check("sensitivity updates immediately while offline and can return to following", () => {
    el("sensitivity").value = "1.75";
    el("sensitivity").dispatchEvent(new Event("input"));
    assert.equal(el("sensitivity-value").textContent, "1.75");
    assert.equal(el("sensitivity-note").disabled, false);
    assert.deepEqual(patches.pop(), { sensitivity: 1.75 });
    assert.equal(el("sensitivity-note").textContent, "Use my computer\u2019s setting");
    tap(el("sensitivity-note"));
    assert.equal(el("sensitivity-note").disabled, true);
    assert.equal(el("sensitivity-note").textContent, FOLLOWING);
    assert.deepEqual(patches.pop(), { follow: ["sensitivity"] });
  });
  check("renaming twice does not make fullscreen toggle multiple times per tap", () => {
    for (const name of ["  Living room  ", "My phone"]) {
      el("device-name").value = name;
      el("device-name").dispatchEvent(new Event("change"));
      assert.equal(el("device-name").value, name.trim());
    }
    tap(el("settings-open"));
    tap(el("go-full-screen"));
    assert.equal(fullscreenCalls, 1);
    assert.equal(sheet.hidden, true);
  });
  check("the click sound is on by default, and follows what the user chose", () => {
    // It used to default to `!hasVibration()`, which left every Android phone
    // silent on a hold - the buzz that was supposed to cover it is the one
    // Chrome refuses without a completed tap. The default is now the same
    // everywhere; only a stored choice moves it.
    assert.equal(ui.settings.clickSound, true);
    assert.equal(el("click-sound").checked, true);
  });
  ui.destroy();

  check("a phone that can vibrate still gets the click, and can still turn it off", () => {
    stored.clear();
    // Android: the Vibration API is present, which is exactly the condition the
    // old default read as "this phone does not need sound".
    navigator.vibrate = () => true;
    try {
      const android = new Ui({
        onTakeControl() {}, onNameChange() {}, onEnableShake: async () => false,
        onFullScreen() {}, onSettingsChange() {},
      });
      assert.equal(android.settings.clickSound, true);
      assert.equal(el("click-sound").checked, true);

      el("click-sound").checked = false;
      el("click-sound").dispatchEvent(new Event("change"));
      assert.equal(android.settings.clickSound, false);
      android.destroy();

      // And the choice survives a reload, rather than the default arguing with it.
      const reloaded = new Ui({
        onTakeControl() {}, onNameChange() {}, onEnableShake: async () => false,
        onFullScreen() {}, onSettingsChange() {},
      });
      assert.equal(reloaded.settings.clickSound, false);
      assert.equal(el("click-sound").checked, false);
      reloaded.destroy();
    } finally {
      delete navigator.vibrate;
      stored.clear();
    }
  });
  console.log(`\n${passed} mobile UI checks passed.`);
} finally {
  rmSync(out, { recursive: true, force: true });
}
