/**
 * The phone's half of the pairing handshake, driven against a fake desktop.
 *
 * `src/net.ts` is the one place on the phone where a mistake is a security bug
 * rather than a glitch: a link that considers itself connected before it has
 * answered the challenge would stream touches at a desktop that is refusing
 * them, and - worse - the bug that motivated this file, a stale socket closing
 * after a newer one authenticated, silently stops every touch on a live link
 * whose status light still says "connected". Neither shows up as an error
 * anywhere. Both are one assertion each here.
 *
 *   npm run check:link
 */

import { execFileSync } from "node:child_process";
import { createHmac } from "node:crypto";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { pathToFileURL } from "node:url";
import assert from "node:assert/strict";

let failures = 0;
function check(name, fn) {
  try {
    fn();
    console.log(`ok  ${name}`);
  } catch (e) {
    failures++;
    console.error(`FAIL ${name}\n${e.stack}`);
  }
}

const out = mkdtempSync(join(tmpdir(), "link-"));
execFileSync(
  "npx",
  ["esbuild", "src/net.ts", "--bundle", "--format=esm", `--outfile=${join(out, "net.mjs")}`,
   "--log-level=error", "--define:import.meta.env.VITE_DESKTOP_WS=undefined",
   "--define:import.meta.env.VITE_DESKTOP_KEY=undefined"],
  { stdio: "inherit" },
);

// The browser surface `DesktopLink` actually touches.
const sockets = [];
class FakeSocket {
  static CONNECTING = 0;
  static OPEN = 1;
  static CLOSING = 2;
  static CLOSED = 3;
  constructor(url) {
    this.url = url;
    this.readyState = FakeSocket.CONNECTING;
    this.sent = [];
    sockets.push(this);
  }
  send(data) {
    this.sent.push(data);
  }
  close() {
    this.readyState = FakeSocket.CLOSED;
    this.onclose?.();
  }
  /** The desktop accepting the connection. */
  open() {
    this.readyState = FakeSocket.OPEN;
    this.onopen?.();
  }
  /** The desktop saying something. */
  say(msg) {
    this.onmessage?.({ data: JSON.stringify(msg) });
  }
}
globalThis.WebSocket = FakeSocket;
globalThis.window = {
  addEventListener: () => {},
  setTimeout: () => 0,
  clearTimeout: () => {},
};
// Node 22 defines `navigator` as a getter, so it has to be replaced rather
// than assigned to.
Object.defineProperty(globalThis, "navigator", {
  value: { userAgent: "iPhone", maxTouchPoints: 5 },
  configurable: true,
});
globalThis.localStorage = {
  store: new Map(),
  getItem(k) { return this.store.get(k) ?? null; },
  setItem(k, v) { this.store.set(k, v); },
  removeItem(k) { this.store.delete(k); },
};
globalThis.location = { protocol: "http:", hostname: "192.168.1.24", hash: "", search: "", pathname: "/" };
globalThis.history = {
  replaceState: (_s, _t, url) => {
    location.hash = url.includes("#") ? url.slice(url.indexOf("#")) : "";
  },
};

const { DesktopLink } = await import(pathToFileURL(join(out, "net.mjs")));

const KEY = "0123456789abcdef0123456789abcdef";
const NONCE = "9f2c00112233445566778899aabbccdd";
const rightAnswer = createHmac("sha256", Buffer.from(KEY, "hex")).update(NONCE).digest("hex");

function link(key) {
  const statuses = [];
  sockets.length = 0;
  localStorage.store.clear();
  location.hash = key ? `#h=192.168.1.24:8787&k=${key}` : "";
  const l = new DesktopLink(
    { host: "192.168.1.24:8787", key },
    { onStatus: (s) => statuses.push(s), onMessage: () => {} },
  );
  l.start();
  return { l, statuses };
}

// ------------------------------------------------------------------ the gate

check("an open socket is not a connected one until the challenge is answered", () => {
  const { l, statuses } = link(KEY);
  sockets[0].open();
  assert.equal(l.connected, false, "connected before any challenge");
  assert.ok(!statuses.includes("connected"), `said connected too early: ${statuses}`);

  // And touches sent in that window go nowhere rather than to a desktop that
  // is about to refuse them.
  l.sendSamples([{ t: 0, id: 1, phase: 0, x: 0.5, y: 0.5 }]);
  assert.equal(sockets[0].sent.length, 0, "sent touches before authenticating");
});

check("the challenge is answered with the right HMAC, and only then a welcome", () => {
  const { l, statuses } = link(KEY);
  sockets[0].open();
  sockets[0].say({ t: "challenge", nonce: NONCE });

  const sent = sockets[0].sent.map((s) => JSON.parse(s));
  assert.equal(sent[0].t, "auth", "auth must be the first thing sent");
  assert.equal(sent[0].hmac, rightAnswer);
  assert.equal(sent[1].t, "welcome", "the welcome comes after the answer");
  assert.equal(l.connected, true);
  assert.ok(statuses.includes("connected"));
});

check("a page with no pairing key gives up instead of retrying forever", () => {
  const { l, statuses } = link(undefined);
  sockets[0].open();
  sockets[0].say({ t: "challenge", nonce: NONCE });

  assert.equal(sockets[0].sent.length, 0, "answered a challenge it could not answer");
  assert.equal(statuses.at(-1), "unpaired");
  assert.equal(l.connected, false);
  // Retrying would be refused identically every time and bury the one message
  // that tells the user to scan the QR.
  assert.equal(sockets.length, 1, "reconnected after being told it is not paired");
});

check("a malformed key is treated as no key, not as a wrong answer", () => {
  const { statuses } = link("not-hex");
  sockets[0].open();
  sockets[0].say({ t: "challenge", nonce: NONCE });
  assert.equal(sockets[0].sent.length, 0);
  assert.equal(statuses.at(-1), "unpaired");
});

// ------------------------------------------------------- the stale socket bug

check("a stale socket closing does not disarm the live link", () => {
  const { l } = link(KEY);
  const first = sockets[0];
  first.open();

  // A new QR, or a reconnect: the old socket is closed and a new one opens
  // without waiting for the old one's close event.
  l.retarget({ host: "192.168.1.99:8787", key: KEY });
  const second = sockets.at(-1);
  assert.notEqual(second, first, "retarget should open a new socket");
  second.open();
  second.say({ t: "challenge", nonce: NONCE });
  assert.equal(l.connected, true);

  // Now the old socket's close finally lands. It must change nothing.
  first.onclose?.();
  assert.equal(l.connected, true, "a stale close silently stopped every touch");

  l.sendSamples([{ t: 0, id: 1, phase: 0, x: 0.5, y: 0.5 }]);
  assert.ok(second.sent.length >= 3, "touches stopped reaching the live socket");
});

check("a challenge on a stale socket is ignored", () => {
  const { l } = link(KEY);
  const first = sockets[0];
  first.open();
  l.retarget({ host: "192.168.1.99:8787", key: KEY });
  const second = sockets.at(-1);
  second.open();
  second.say({ t: "challenge", nonce: NONCE });

  // The old desktop challenges after we have moved on. Answering would
  // authenticate a link nobody is using.
  const before = first.sent.length;
  first.say({ t: "challenge", nonce: NONCE });
  assert.equal(first.sent.length, before, "answered a challenge on a dead socket");
  assert.equal(l.connected, true, "the live link survived");
});

// ------------------------------------------------------------ losing the link

check("a dropped connection goes offline and does not stay authenticated", () => {
  const { l, statuses } = link(KEY);
  sockets[0].open();
  sockets[0].say({ t: "challenge", nonce: NONCE });
  assert.equal(l.connected, true);

  sockets[0].close();
  assert.equal(l.connected, false);
  assert.equal(statuses.at(-1), "offline");
});

// ------------------------------------------------------------- enrolment

const deviceKey = (secretHex, id) =>
  createHmac("sha256", Buffer.from(secretHex, "hex"))
    .update(`padremote:device:${id}`)
    .digest("hex")
    .slice(0, 32);

check("the first connection enrols: a new id, signed with the QR secret", () => {
  const { l } = link(KEY);
  sockets[0].open();
  sockets[0].say({ t: "challenge", nonce: NONCE });

  const auth = JSON.parse(sockets[0].sent[0]);
  assert.equal(auth.t, "auth");
  assert.match(auth.device, /^[0-9a-f]{32}$/, "a well-formed device id");
  assert.equal(auth.hmac, rightAnswer, "enrolment signs with the QR secret");
  assert.equal(l.connected, true);
});

check("the credential is kept only once the desktop has accepted it", () => {
  link(KEY);
  sockets[0].open();
  sockets[0].say({ t: "challenge", nonce: NONCE });
  const id = JSON.parse(sockets[0].sent[0]).device;

  // Nothing stored yet: the desktop has not said anything but the challenge.
  assert.equal(localStorage.getItem("padremote.credential.v1"), null);

  // Refused. Storing here - and throwing the QR secret away with it - would
  // strand the phone holding a key the computer does not know.
  sockets[0].say({ t: "error", code: "badAuth" });
  assert.equal(localStorage.getItem("padremote.credential.v1"), null,
    "a refused enrolment was saved anyway");

  // Now the accepting case.
  link(KEY);
  sockets[0].open();
  sockets[0].say({ t: "challenge", nonce: NONCE });
  const good = JSON.parse(sockets[0].sent[0]).device;
  sockets[0].say({ t: "state", gesture: "idle", fingers: 0, name: "jarvis" });

  const cred = JSON.parse(localStorage.getItem("padremote.credential.v1"));
  assert.equal(cred.id, good);
  assert.equal(cred.key, deviceKey(KEY, good), "the key the desktop will derive");
  assert.notEqual(id, good, "each enrolment mints its own id");
});

check("the QR secret is thrown away once enrolment sticks", () => {
  link(KEY);
  sockets[0].open();
  sockets[0].say({ t: "challenge", nonce: NONCE });
  sockets[0].say({ t: "state", gesture: "idle", fingers: 0, name: "jarvis" });

  // This is what makes revoking a device final: while the phone still holds
  // the secret, being forgotten is undone by enrolling again under a new id.
  const stored = JSON.parse(localStorage.getItem("padremote.link.v1") ?? "{}");
  assert.equal(stored.key, undefined, "the QR secret is still in storage");
  assert.ok(!location.hash.includes("k="), `the secret is still in the URL: ${location.hash}`);
});

check("later connections sign with the device key, not the QR secret", () => {
  const { l } = link(KEY);
  sockets[0].open();
  sockets[0].say({ t: "challenge", nonce: NONCE });
  sockets[0].say({ t: "state", gesture: "idle", fingers: 0, name: "jarvis" });
  const id = JSON.parse(sockets[0].sent[0]).device;

  l.retarget({ host: "192.168.1.24:8787" });
  const next = sockets.at(-1);
  next.open();
  next.say({ t: "challenge", nonce: NONCE });

  const auth = JSON.parse(next.sent[0]);
  assert.equal(auth.device, id, "the same device comes back as itself");
  const expected = createHmac("sha256", Buffer.from(deviceKey(KEY, id), "hex"))
    .update(NONCE)
    .digest("hex");
  assert.equal(auth.hmac, expected, "signed with this device's own key");
  assert.notEqual(auth.hmac, rightAnswer, "and not with the QR secret");
});

check("being revoked drops the credential rather than enrolling beside it", () => {
  const { l, statuses } = link(KEY);
  sockets[0].open();
  sockets[0].say({ t: "challenge", nonce: NONCE });
  sockets[0].say({ t: "state", gesture: "idle", fingers: 0, name: "jarvis" });
  assert.ok(localStorage.getItem("padremote.credential.v1"));

  // What "Forget a device" looks like from here.
  l.forgetPairing();
  l.giveUp("unpaired");
  assert.equal(localStorage.getItem("padremote.credential.v1"), null,
    "a key known not to work was kept");
  assert.equal(statuses.at(-1), "unpaired");
});

rmSync(out, { recursive: true, force: true });
if (failures) {
  console.error(`\n${failures} link check(s) failed.`);
  process.exit(1);
}
console.log(`\nAll link checks passed.`);
