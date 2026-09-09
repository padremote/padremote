/**
 * A stand-in for the desktop app, for looking at the settings page.
 *
 * The settings page is useless without a computer to talk to: it renders itself
 * from the config the desktop sends, so a screenshot of it with no desktop
 * behind it is a screenshot of an error message. Building and installing the
 * real app to look at a change to a stylesheet is minutes per look.
 *
 * So this speaks the `/config` and `/devices` channels with a canned payload,
 * and never sends a challenge - there is no pairing secret to arrange, and no
 * credential in the browser's storage to depend on. It is for *looking* at the
 * page. Behaviour is checked by `check-settings.mjs`, which needs neither.
 *
 *   node scripts/mock-desktop.mjs                        # listens on :8788
 *   npm run dev                                          # serves the page
 *   open http://localhost:5173/config.html?h=localhost:8788
 *
 * The config it serves is `desktop/config.default.json`, so a setting added to
 * the Rust struct appears here too rather than being invisible in the mock.
 */
import { createServer } from "node:http";
import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";

const GUID = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";
const file = JSON.parse(
  readFileSync(new URL("../../desktop/config.default.json", import.meta.url)),
);

const CLICKS = ["none","leftClick","rightClick","middleClick","missionControl","appWindows",
  "showDesktop","launchpad","switchApps","spotlight","screenshot","lockScreen","mute","smartZoom",
  "copy","cut","paste","selectAll","save","find","newTab","closeWindow","minimiseWindow","quitApp",
  "fullScreen","calculator"];
const ACROSS = ["none","spaces","navigate","tabs","undoRedo"];
const UP_DOWN = ["none","missionControl","appWindows","volume","brightness","zoom"];
const DIRECTIONAL = ["inherit","none","desktopLeft","desktopRight","missionControl","appWindows",
  "showDesktop","switchApps","launchpad","back","forward","volumeUp","volumeDown","mute","brightnessUp","brightnessDown","zoomIn",
  "zoomOut","previousTab","nextTab","undo","redo","copy","paste","screenshot","lockScreen"];

const vocabulary = {
  "bindings.oneTap": CLICKS, "bindings.twoFingerTap": CLICKS, "bindings.threeFingerTap": CLICKS,
  "bindings.fourFingerTap": CLICKS,
  "bindings.twoFingerDoubleTap": CLICKS, "bindings.cornerSecondaryClick": CLICKS,
  "bindings.fourFingerPinch": CLICKS, "bindings.fiveFingerSpread": CLICKS,
  "bindings.twoFingerSwipeNavigate": ACROSS, "bindings.threeFingerHorizSwipe": ACROSS,
  "bindings.fourFingerHorizSwipe": ACROSS,
  "bindings.threeFingerVertSwipe": UP_DOWN, "bindings.fourFingerVertSwipe": UP_DOWN,
  "zoom.backend": ["appZoom", "systemPinch"],
};
for (const n of ["two", "three", "four"]) {
  for (const d of ["Left", "Right", "Up", "Down"]) {
    if (n === "two" && (d === "Up" || d === "Down")) continue;
    vocabulary[`bindings.${n}FingerSwipe${d}`] = DIRECTIONAL;
  }
}

const decidedBy = {
  "scroll.natural": "Scrolling direction: Natural",
  "scroll.enabled": "Use trackpad for scrolling",
  "scroll.momentum": "Use inertia when scrolling",
  "bindings.twoFingerTap": "Secondary click (two fingers)",
  "bindings.threeFingerVertSwipe": "Mission Control (three fingers)",
  "bindings.threeFingerHorizSwipe": "Swipe between pages (three fingers)",
  sensitivity: "Tracking speed",
};
const mirrorWrites = {
  "bindings.threeFingerVertSwipe": "missionControl",
  "bindings.threeFingerHorizSwipe": "spaces",
};
const host = [
  { setting: "Tracking speed", name: "com.apple.trackpad.scaling", value: "1.4", status: "mirrored", detail: "" },
  { setting: "Scrolling direction: Natural", name: "com.apple.swipescrolldirection", value: "natural", status: "mirrored", detail: "" },
  { setting: "Use trackpad for scrolling", name: "TrackpadScroll", value: "on", status: "mirrored", detail: "" },
  { setting: "Use inertia when scrolling", name: "TrackpadMomentumScroll", value: "on", status: "mirrored", detail: "" },
  { setting: "Secondary click (two fingers)", name: "TrackpadRightClick", value: "on", status: "mirrored", detail: "" },
  { setting: "Mission Control (three fingers)", name: "TrackpadThreeFingerVertSwipe", value: "on", status: "mirrored", detail: "" },
  { setting: "Swipe between pages (three fingers)", name: "TrackpadThreeFingerHorizSwipe", value: "on", status: "approximated", detail: "Sent as a keystroke; the page does not follow your fingers." },
  { setting: "Force Click", name: "ForceClick", value: "on", status: "not possible", detail: "A phone screen has no pressure to read." },
  { setting: "Tap to click", name: "Clicking", value: "on", status: "handled by the OS", detail: "" },
];

const effective = structuredClone(file);
effective.sensitivity = 1.4;
effective.bindings.threeFingerVertSwipe = "missionControl";
effective.bindings.threeFingerHorizSwipe = "spaces";

const configState = () => JSON.stringify({
  t: "config", computer: "Studio Mac", os: "macos",
  path: "/Users/an/Library/Application Support/PadRemote/config.json",
  file, effective, followSystem: file.followSystem, host, decidedBy, mirrorWrites, vocabulary,
});

const devicesState = () => JSON.stringify({
  t: "devices", connected: 2, manage: true, host: "192.168.1.42",
  devices: [
    { id: "a1", name: "An’s iPhone", connected: true, driving: true, since: 1738540800 },
    { id: "c3", name: "Studio iPad", connected: true, driving: false, since: 1729036800 },
    { id: "b2", name: "Pixel 8", connected: false, driving: false, since: 1717200000 },
  ],
});

function frame(text) {
  const body = Buffer.from(text);
  const head = body.length < 126
    ? Buffer.from([0x81, body.length])
    : Buffer.concat([Buffer.from([0x81, 126]), (() => { const b = Buffer.alloc(2); b.writeUInt16BE(body.length); return b; })()]);
  return Buffer.concat([head, body]);
}

createServer((_req, res) => res.end("mock desktop")).on("upgrade", (req, socket) => {
  const accept = createHash("sha1").update(req.headers["sec-websocket-key"] + GUID).digest("base64");
  socket.write("HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n"
    + `Sec-WebSocket-Accept: ${accept}\r\n\r\n`);
  const path = req.url.split("?")[0];
  socket.write(frame(path === "/devices" ? devicesState() : configState()));
  // Anything the page sends back is echoed as a fresh state, which is what the
  // real desktop does: a save is confirmed by the config coming back.
  socket.on("data", () => { if (path !== "/devices") socket.write(frame(configState())); });
  socket.on("error", () => {});
}).listen(8788, () => console.log("mock desktop on :8788"));
