/** Exercise QR launch timing with the real bootstrap and a deterministic clock. */
import assert from "node:assert/strict";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { pathToFileURL } from "node:url";
import { build } from "esbuild";

const out = mkdtempSync(join(tmpdir(), "padremote-launch-"));
const iphone = "Mozilla/5.0 (iPhone; CPU iPhone OS 18_0 like Mac OS X) CriOS/130.0 Mobile";
const qr = "http://192.168.1.117:5173/?theme=dark#h=192.168.1.117:9100&n=Studio%20Mac";
let passed = 0;
try {
  await build({ entryPoints: ["src/launch.ts"], bundle: true, format: "esm", outfile: join(out, "launch.mjs") });
  const { launchPad } = await import(pathToFileURL(join(out, "launch.mjs")));
  function host({ url = qr, ua = iphone, type = "navigate", ready = "complete", visible = "visible" } = {}) {
    let now = 0, nextId = 0, starts = 0;
    const timers = new Map();
    const navigations = [];
    const windowHost = Object.assign(new EventTarget(), {
      location: { href: url, replace: (target) => navigations.push(target) },
      visualViewport: new EventTarget(),
      setTimeout: (callback, ms) => { const id = ++nextId; timers.set(id, { at: now + ms, callback }); return id; },
      clearTimeout: (id) => timers.delete(id),
    });
    const documentHost = Object.assign(new EventTarget(), { readyState: ready, visibilityState: visible });
    Object.assign(globalThis, { window: windowHost, document: documentHost });
    Object.defineProperty(globalThis, "navigator", { configurable: true, value: { userAgent: ua } });
    Object.defineProperty(globalThis, "performance", { configurable: true, value: { getEntriesByType: () => [{ type }] } });
    // Navigating must not depend on storage permissions, including loop prevention.
    for (const name of ["localStorage", "sessionStorage"]) {
      Object.defineProperty(globalThis, name, { configurable: true, get() { throw new Error("storage blocked"); } });
    }
    const stop = launchPad(() => starts++);
    return {
      window: windowHost, document: documentHost, navigations, stop,
      get starts() { return starts; },
      advance(ms) {
        const until = now + ms;
        while (true) {
          const due = [...timers].filter(([, timer]) => timer.at <= until).sort((a, b) => a[1].at - b[1].at)[0];
          if (!due) break;
          now = due[1].at;
          timers.delete(due[0]);
          due[1].callback();
        }
        now = until;
      },
    };
  }
  function check(name, test) { test(); console.log(`ok  ${name}`); passed++; }
  check("a QR launch waits for load, foregrounding and settled browser bars", () => {
    const h = host({ ready: "loading", visible: "hidden" });
    h.advance(5000);
    assert.equal(h.starts, 0);
    assert.equal(h.navigations.length, 0);
    h.document.readyState = "complete";
    h.window.dispatchEvent(new Event("load"));
    h.advance(5000);
    assert.equal(h.navigations.length, 0);
    h.document.visibilityState = "visible";
    h.document.dispatchEvent(new Event("visibilitychange"));
    h.advance(600);
    h.window.visualViewport.dispatchEvent(new Event("resize"));
    h.advance(600);
    assert.equal(h.navigations.length, 0);
    h.advance(100);
    assert.equal(h.navigations.length, 1);
    assert.equal(h.starts, 0, "the temporary landing must not start the trackpad");
    const target = new URL(h.navigations[0]);
    assert.equal(target.hash, new URL(qr).hash);
    assert.equal(target.searchParams.get("theme"), "dark");
    assert.equal(target.searchParams.get("__pad_launch"), "1");
    h.advance(10000);
    h.window.dispatchEvent(new Event("focus"));
    h.advance(10000);
    assert.equal(h.navigations.length, 1);
  });
  check("the second document starts exactly once with storage blocked", () => {
    const first = host();
    first.advance(700);
    const second = host({ url: first.navigations[0] });
    second.advance(10000);
    second.window.dispatchEvent(new Event("pageshow"));
    assert.equal(second.starts, 1);
    assert.equal(second.navigations.length, 0);
  });
  check("continuous viewport changes cannot delay launch beyond three seconds", () => {
    const h = host();
    for (let i = 0; i < 10; i++) {
      h.advance(300);
      h.window.visualViewport.dispatchEvent(new Event("resize"));
    }
    assert.equal(h.navigations.length, 1);
  });
  check("backgrounding cancels pending navigation until Chrome is visible again", () => {
    const h = host();
    h.advance(500);
    h.document.visibilityState = "hidden";
    h.document.dispatchEvent(new Event("visibilitychange"));
    h.advance(10000);
    assert.equal(h.navigations.length, 0);
    h.document.visibilityState = "visible";
    h.document.dispatchEvent(new Event("visibilitychange"));
    h.advance(700);
    assert.equal(h.navigations.length, 1);
  });
  check("direct URLs, reloads, history returns and other browsers start immediately", () => {
    for (const options of [
      { url: "http://192.168.1.117:5173/" },
      { type: "reload" }, { type: "back_forward" },
      { ua: "Mozilla/5.0 (iPhone) Version/18.0 Mobile Safari/604.1" },
      { ua: "Mozilla/5.0 (Linux; Android) Chrome/130.0 Mobile" },
      { ua: "Mozilla/5.0 (Macintosh) Chrome/130.0 Safari/537.36" },
    ]) {
      const h = host(options);
      assert.equal(h.starts, 1);
      h.advance(10000);
      assert.equal(h.navigations.length, 0);
    }
  });
  check("cleanup removes pending navigation and event listeners", () => {
    const h = host();
    h.stop();
    for (const event of ["load", "focus", "resize", "pageshow"]) h.window.dispatchEvent(new Event(event));
    h.document.dispatchEvent(new Event("visibilitychange"));
    h.window.visualViewport.dispatchEvent(new Event("resize"));
    h.advance(10000);
    assert.equal(h.navigations.length, 0);
    assert.equal(h.starts, 0);
  });
  console.log(`\n${passed} launch checks passed.`);
} finally {
  rmSync(out, { recursive: true, force: true });
}
