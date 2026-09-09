/**
 * Shake the phone to give the pad the whole screen, and shake again to give it
 * back.
 *
 * Two platform facts shape all of this, and neither is negotiable:
 *
 * **Motion needs permission on iOS.** Since iOS 13 `DeviceMotionEvent` delivers
 * nothing until `requestPermission()` has been called *from inside a user
 * gesture* and granted. There is no way to detect a shake before that, and no
 * way to ask without a tap. So the pad asks once, from the settings sheet, and
 * remembers the answer.
 *
 * **A shake must never fire mid-gesture.** The phone is in the user's hand
 * being used as a trackpad; a flick of the wrist while scrolling must not
 * throw the screen into another mode. Nothing here triggers while a finger is
 * down, and a deliberate shake is defined as several *reversals* of direction
 * rather than one big spike - which is what separates shaking a phone from
 * putting it down hard.
 */

/**
 * How hard the hand has to move for a reading to count, in m/s².
 *
 * This is acceleration with gravity removed, so it is purely what the arm did.
 * Ordinary handling - walking about, putting the phone down, using it as a
 * trackpad - stays under about 5; a deliberate shake peaks well past 15.
 */
const JOLT = 12;
/** Reversals of direction needed inside the window below. */
const REVERSALS = 3;
/** How long the reversals have to happen within, in ms. */
const WINDOW_MS = 700;
/** Quiet time after a shake is reported, so one shake is never two. */
const COOLDOWN_MS = 1500;

const STORAGE_KEY = "padremote.shake.v1";

export type ShakeSupport = "insecure" | "unavailable" | "needs-permission" | "ready";

/**
 * What this browser can do about motion, before anything is asked of it.
 *
 * `insecure` is the one that matters in practice today. Motion events are
 * gated behind a **secure context** in every browser, and PadRemote's page is
 * served over plain `http` on the LAN until the TLS work lands - so on a phone
 * `DeviceMotionEvent` is not merely unpermitted, it is *undefined*. Saying
 * "allow motion access" there would send the user hunting through iOS Settings
 * for a switch that would change nothing.
 */
export function shakeSupport(): ShakeSupport {
  if (typeof DeviceMotionEvent === "undefined") {
    return isSecureContext ? "unavailable" : "insecure";
  }
  // The typed permission gate exists on iOS only; everywhere else motion
  // events simply arrive.
  const gated = "requestPermission" in DeviceMotionEvent;
  if (!gated) return "ready";
  return remembered() ? "ready" : "needs-permission";
}

function remembered(): boolean {
  try {
    return localStorage.getItem(STORAGE_KEY) === "granted";
  } catch {
    return false;
  }
}

/**
 * Ask iOS for motion access. Must be called from inside a user gesture.
 *
 * Returns whether motion is now available. Remembers a grant, because iOS
 * re-asks on every page load otherwise and a permission dialog on startup is
 * exactly the kind of thing that gets an app closed.
 */
export async function requestShakePermission(): Promise<boolean> {
  const gate = DeviceMotionEvent as unknown as {
    requestPermission?: () => Promise<"granted" | "denied">;
  };
  if (typeof gate.requestPermission !== "function") return true;
  try {
    const granted = (await gate.requestPermission()) === "granted";
    try {
      localStorage.setItem(STORAGE_KEY, granted ? "granted" : "denied");
    } catch {
      /* storage unavailable; the permission still holds for this page */
    }
    return granted;
  } catch {
    // Called outside a user gesture, or the device has no motion hardware.
    return false;
  }
}

export interface ShakeOptions {
  /** True while a finger is on the pad - no shake is reported then. */
  busy: () => boolean;
  onShake: () => void;
}

/**
 * Watch for a deliberate shake.
 *
 * Detection is on *reversals*, not magnitude alone: a shake is a hand changing
 * direction several times in under a second, while a knock or a phone set down
 * hard is one spike. Counting sign changes on the axis that is moving most
 * tells the two apart with no calibration.
 */
export function watchShake({ busy, onShake }: ShakeOptions): () => void {
  let reversals = 0;
  let windowStarted = 0;
  let lastSign = 0;
  let quietUntil = 0;
  // A running estimate of which way is down, for devices that only report
  // acceleration *including* gravity. A slow low-pass converges on the
  // constant part - which is gravity - leaving the hand's contribution.
  let gravity: [number, number, number] | null = null;

  const onMotion = (e: DeviceMotionEvent) => {
    const now = e.timeStamp || performance.now();
    if (now < quietUntil) return;

    // Direction has to survive, so this works on the acceleration *vector*
    // rather than its magnitude. Magnitude is always positive: deviation from
    // rest can never be less than -9.81, so a threshold of 12 on it could
    // never see a negative reading at all, the sign could never flip, and no
    // shake could ever be detected. That version passed review and failed the
    // first synthetic shake put through it.
    const raw = e.acceleration;
    let x = raw?.x ?? null;
    let y = raw?.y ?? null;
    let z = raw?.z ?? null;

    if (x === null && y === null && z === null) {
      const g = e.accelerationIncludingGravity;
      if (!g) return;
      const v: [number, number, number] = [g.x ?? 0, g.y ?? 0, g.z ?? 0];
      gravity = gravity
        ? [
            gravity[0] * 0.9 + v[0] * 0.1,
            gravity[1] * 0.9 + v[1] * 0.1,
            gravity[2] * 0.9 + v[2] * 0.1,
          ]
        : v;
      [x, y, z] = [v[0] - gravity[0], v[1] - gravity[1], v[2] - gravity[2]];
    }

    const lin: [number, number, number] = [x ?? 0, y ?? 0, z ?? 0];
    const magnitude = Math.hypot(lin[0], lin[1], lin[2]);
    if (magnitude < JOLT) return;

    // Which way the hand is going, taken from the axis doing most of the work:
    // a shake is mostly along one axis, and the other two are noise.
    let axis = 0;
    for (let i = 1; i < 3; i++) {
      if (Math.abs(lin[i]) > Math.abs(lin[axis])) axis = i;
    }
    const sign = Math.sign(lin[axis]);

    if (now - windowStarted > WINDOW_MS) {
      // Too slow to be one shake: start counting again from this jolt.
      reversals = 0;
      windowStarted = now;
      lastSign = sign;
      return;
    }
    if (sign !== 0 && sign !== lastSign) {
      lastSign = sign;
      reversals++;
    }
    if (reversals < REVERSALS) return;

    reversals = 0;
    quietUntil = now + COOLDOWN_MS;
    // Never mid-gesture. The phone is in a hand being used as a trackpad, and
    // a flick of the wrist during a scroll must not change modes.
    if (busy()) return;
    onShake();
  };

  window.addEventListener("devicemotion", onMotion);
  return () => window.removeEventListener("devicemotion", onMotion);
}
