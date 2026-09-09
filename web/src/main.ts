/**
 * PadRemote phone app - entry point.
 *
 * Wires the touch surface to the desktop link and the status UI. Everything
 * about *meaning* lives on the desktop; this page only reports touches
 * (plan.md section 3).
 */

import "./theme.css";
import "./style.css";
import { DesktopLink, type Status } from "./net";
import { resolveLink } from "./pairing";
import { blip, Ui } from "./ui";
import { Surface, suppressBrowserGestures } from "./surface";
import { trackViewport, watchSize } from "./viewport";
import { BUILD } from "./build";
import { hasFullscreen, isStandalone, toggleImmersive, watchFullscreenExit } from "./immersive";
import { requestShakePermission, shakeSupport, watchShake } from "./shake";
import { TrailRenderer } from "./trail";
import { buzz, hasVibration } from "./haptics";
import { clickDown, clickUp, primeClick } from "./sound";

suppressBrowserGestures();
// Before anything measures itself: this publishes *where* the visible area is
// and how big it is, which is what the whole page is laid out inside.
const stopViewport = trackViewport();

const surfaceEl = document.getElementById("surface")!;
const link = resolveLink();
let hostLabel = link.name ?? link.host;

// The settings page needs the same computer this pad is driving. It is served
// from the same place as this page, so only the address has to be carried over.
(document.getElementById("all-settings") as HTMLAnchorElement).href =
  `/config.html?h=${encodeURIComponent(link.host)}`;
(document.getElementById("gesture-guide") as HTMLAnchorElement).href =
  `/config.html?h=${encodeURIComponent(link.host)}#gestures`;

const ui = new Ui({
  onTakeControl: () => desktop.resume(),
  // iOS delivers no motion at all until this is asked for, and it may only be
  // asked from inside a user gesture - so it lives behind a button rather than
  // firing on load.
  onEnableShake: async () => {
    const granted = await requestShakePermission();
    if (granted) startShake();
    return granted;
  },
  onFullScreen: () => void toggleFullScreen(),
  // The name is only ever read by *other* devices, so it has to reach the
  // desktop the moment it changes rather than at the next reload.
  onNameChange: () => desktop.sendWelcome(),
  // Exactly what changed, and nothing else. The desktop mirrors the computer's
  // real trackpad settings, so anything this page sends is an *override* of
  // them - which makes sending a value the user never chose a bug, not a
  // default.
  onSettingsChange: (patch) => desktop.sendJson({ t: "settings", ...patch }),
});

// A guessed address means nobody has ever pointed this page at a computer, and
// the failure it is heading for needs different advice than a lost connection.
ui.setPaired(link.source !== "guess");
// Whether "full screen" can mean the whole screen here, said up front rather
// than discovered by asking for it and getting something else.
ui.setFullScreenLimit(!hasFullscreen() && !isStandalone());

const desktop = new DesktopLink(link, {
  onStatus: (status: Status) => {
    ui.setStatus(status, hostLabel);
    if (status !== "connected") trail.clear();
    if (status === "connected") {
      // Re-assert only what this phone has actually taken over. This used to
      // send the sheet's stored values unconditionally, which overrode the
      // computer's own trackpad settings with whatever the sheet was left on -
      // a Mac with natural scrolling off scrolled the wrong way, and nothing in
      // the desktop's mirroring was wrong. A phone with no overrides now says
      // nothing at all and is told what it is being driven with instead.
      const patch: { sensitivity?: number; naturalScroll?: boolean } = {};
      if (ui.settings.sensitivity !== null) patch.sensitivity = ui.settings.sensitivity;
      if (ui.settings.naturalScroll !== null) patch.naturalScroll = ui.settings.naturalScroll;
      if (Object.keys(patch).length) desktop.sendJson({ t: "settings", ...patch });
      void keepAwake();
    }
  },
  onMessage: (msg) => {
    switch (msg.t) {
      case "state":
        ui.setGesture(msg.gesture);
        // The ring should finish exactly when the drag really starts, and die
        // exactly when the engine gives up on it, so take both numbers from the
        // desktop rather than assuming the defaults.
        if (msg.pressMs) trail.setPressMs(msg.pressMs);
        if (msg.tapMaxPx) trail.setTapMaxPx(msg.tapMaxPx);
        if (msg.name) {
          hostLabel = msg.name;
          ui.setStatus("connected", hostLabel);
        }
        break;
      case "echo":
        // Round trip from the sample that produced the last action, which is
        // the number the acceptance criterion is written against.
        ui.setLatency(performance.now() - msg.tMs);
        break;
      case "settings":
        // What the desktop is really driving this phone with, mirrored from the
        // computer's own trackpad. The sheet is filled in from this rather than
        // from anything stored here.
        ui.setEffective(msg);
        break;
      case "control":
        // Several devices can be connected at once and they take turns with the
        // one cursor. A device that is not driving is still reading every touch
        // - it simply is not obeyed - so it has to say so, or waiting for your
        // turn is indistinguishable from a broken link.
        ui.setControl(msg);
        break;
      case "error":
        // `superseded` only ever comes from a desktop older than multi-device
        // support, which evicts instead of sharing. Retrying would evict the
        // phone that just took over, and the two would trade control forever.
        if (msg.code === "superseded") {
          desktop.standDown();
        } else if (msg.code === "busy") {
          ui.setGesture("too many devices");
          desktop.standDown();
        } else if (msg.code === "replaced") {
          // Another tab on this same phone is now the trackpad. Reconnecting
          // would take the link straight back off it, and the two pages would
          // trade it forever - so this one stays down, and says where its
          // trackpad went.
          desktop.giveUp("replaced");
        } else if (msg.code === "unpaired" || msg.code === "badAuth") {
          // The computer has revoked this device - all of them at once, or just
          // this one from the menu bar. Either way the key this phone holds is
          // now worthless, so drop it rather than enrol a second id beside it,
          // stop reconnecting, and say the one thing that fixes it.
          desktop.forgetPairing();
          desktop.giveUp("unpaired");
        } else {
          ui.setGesture(`error: ${msg.code}`);
        }
        break;
    }
  },
});

const trail = new TrailRenderer(document.getElementById("trail") as HTMLCanvasElement);

// The long press is the phone's stand-in for holding a physical trackpad
// button, so it has to announce itself: on a trackpad you feel the click, and
// without a signal here you only discover the drag once something is selected.
trail.onPressArmed = () => {
  // A refusal here is not a broken phone: Chrome will not vibrate a page that
  // has not yet seen a completed tap, and a long press never lifts the finger.
  if (!buzz() && hasVibration()) ui.noteHapticsBlocked();
  // The other half of the substitute, and often the only half that arrives:
  // Safari has no Vibration API at all, and the `buzz()` above is exactly the
  // call Chrome refuses when the page holds no completed tap. So this is not an
  // iPhone consolation prize - it is what makes a hold announce itself at all.
  if (ui.settings.clickSound) clickDown();
};
// A real trackpad clicks on the way back up too, and that is the click that
// says the button has let go.
trail.onPressReleased = () => {
  if (ui.settings.clickSound) clickUp();
};

const surface = new Surface(surfaceEl, {
  onBatch: (samples) => {
    desktop.sendSamples(samples);
    // Drawn from the batch that was actually sent, so the trail is a picture of
    // the data the desktop received - not of the raw browser events.
    trail.push(samples);
  },
  // The desktop scales normalized touch deltas by this, so a stale copy makes
  // every gesture the wrong size. It goes stale on the first load of every
  // page: the browser is still collapsing its address bar when the surface is
  // first measured.
  onGeometry: (geometry) => {
    desktop.geometry = geometry;
    desktop.sendWelcome();
    showBuild();
  },
  onTouchDown: (x, y) => {
    document.body.classList.add("has-used-pad");
    blip(surfaceEl, x, y);
    // Audio may only be started from inside a real user gesture, and a context
    // created anywhere else is born suspended - so the *first* click would be
    // swallowed and every later one would work. Priming on every touch down is
    // cheap and makes the first hold sound like the tenth.
    if (ui.settings.clickSound) primeClick();
  },
  onRate: (hz) => ui.setRate(hz),
  onJitter: (ms) => ui.setJitter(ms),
});

desktop.geometry = surface.geometry;

// Shown in the settings sheet, with the size the pad believes it has: between
// them they answer both halves of "why does it look wrong on this phone?".
const buildEl = document.getElementById("build")!;
const showBuild = () => {
  const g = surface.geometry;
  const vv = window.visualViewport;
  // The layout viewport beside the visible one, and the gap between them. When
  // the pad is drawn in the wrong place it is because those two disagree, and
  // this is the line that says by how much - readable from a photograph of the
  // phone, which is the only instrument available on someone else's device.
  const layout = `${document.documentElement.clientWidth}×${document.documentElement.clientHeight}`;
  const visible = vv ? `${Math.round(vv.width)}×${Math.round(vv.height)}` : "n/a";
  const offset = vv ? `+${Math.round(vv.offsetLeft)},${Math.round(vv.offsetTop)}` : "";
  const frame = surfaceEl.getBoundingClientRect();
  buildEl.textContent =
    `build ${BUILD} · surface ${Math.round(g.wpx)}×${Math.round(g.hpx)} @${g.dpr}× · ` +
    `visible ${visible}${offset} · layout ${layout} · ` +
    `scroll ${Math.round(window.scrollX)},${Math.round(window.scrollY)} · ` +
    `pad origin ${Math.round(frame.left)},${Math.round(frame.top)}`;
};
showBuild();
// The numbers move as the browser settles, so the line has to move with them.
watchSize(document.documentElement, showBuild);

// ------------------------------------------------------------------- shake
//
// Shake to give the pad the whole screen, shake again to give it back. What
// that can mean depends on the browser - see `immersive.ts`; on an iPhone
// there is no Fullscreen API for an element, so it hides what it can and says
// so once.
async function toggleFullScreen(): Promise<void> {
  const now = await toggleImmersive();
  if (now === "off") {
    ui.flash("exited full screen");
    return;
  }
  ui.flash(
    now === "fullscreen"
      ? "full screen — shake again to exit"
      : isStandalone()
        ? "full screen — shake again to exit"
        // The honest version, and browser-neutral: iOS Chrome is WebKit too,
        // so naming Safari told a Chrome user about a browser they were not
        // using. The sheet carries the full explanation and the way out.
        : "hid what this browser allows — add to Home Screen for true full screen",
  );
}

document.getElementById("focus-mode")!.addEventListener("click", () => void toggleFullScreen());
document.getElementById("exit-immersive")!.addEventListener("click", () => void toggleFullScreen());

function startShake(): void {
  watchShake({
    // Never mid-gesture: the phone is in a hand being used as a trackpad.
    busy: () => surface.touching,
    onShake: () => void toggleFullScreen(),
  });
}

if (shakeSupport() === "ready") startShake();
watchFullscreenExit(() => ui.flash("exited full screen"));

desktop.start();

// Vite replaces this module on every edit without unloading the old one, which
// would leave its socket alive and fighting the new one for control.
import.meta.hot?.dispose(() => {
  desktop.stop();
  stopViewport();
  ui.destroy();
});

/**
 * Keep the screen on while the trackpad is in use.
 *
 * Wake Lock needs a secure context, so over plain http on the LAN (milestone 2)
 * this is simply unavailable - it starts working once milestone 3 brings the
 * self-signed certificate. Failing here must stay silent.
 */
let wakeLock: WakeLockSentinel | null = null;
async function keepAwake(): Promise<void> {
  try {
    wakeLock = (await navigator.wakeLock?.request("screen")) ?? null;
  } catch {
    wakeLock = null;
  }
}
document.addEventListener("visibilitychange", () => {
  if (document.visibilityState === "visible" && desktop.connected) {
    void keepAwake();
  } else {
    void wakeLock?.release().catch(() => {});
    wakeLock = null;
  }
});
