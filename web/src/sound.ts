/**
 * The click you can hear, on every phone.
 *
 * A hold has to announce itself: on a real trackpad the click is *felt*, and
 * the phone has to substitute something. `haptics.ts` covers what vibration can
 * do, which is less than it looks. On iOS the honest answer is nothing at all -
 * Safari has never shipped `navigator.vibrate`, and no user gesture conjures
 * it. On Android the API exists but Chrome will not fire it without a user
 * activation from a completed tap, which a long press never provides. Sound is
 * the one feedback channel every phone actually has, so it is on by default
 * everywhere rather than only where vibration is missing outright.
 *
 * Three decisions worth keeping:
 *
 * - **Synthesised, not a file.** A click is ~8 ms of decaying noise; generating
 *   it costs a few hundred microseconds once and nothing thereafter, and it
 *   keeps the page free of an asset that has to load before the first press.
 * - **Two sounds, down and up**, because that is what a physical trackpad does.
 *   The release click is quieter and higher, exactly as a real one is, and it is
 *   what tells you the button let go without looking.
 * - **The `ambient` audio session**, so this never interrupts what the user is
 *   listening to and stays silenced by the phone's own mute switch. A click that
 *   pauses someone's music, or one that fires in a meeting because the page
 *   asked for playback priority, is worse than no click at all.
 */

/** Peak level of the press click. Feedback, not an alert. */
const DOWN_GAIN = 0.5;
/** The release is quieter, as it is on a real trackpad. */
const UP_GAIN = 0.32;

interface Clicks {
  ctx: AudioContext;
  down: AudioBuffer;
  up: AudioBuffer;
}

let clicks: Clicks | null = null;
/** Set once we know this browser has no usable AudioContext. */
let unavailable = false;

/**
 * One click, rendered into a buffer.
 *
 * A short noise burst under an exponential decay, plus a sine at the body
 * frequency: the noise is the *snap* of the mechanism and the sine is the
 * plastic it happens in. Either alone sounds synthetic - noise by itself is a
 * hiss, a sine by itself is a beep - and the mix is what reads as a click.
 *
 * The first fraction of a millisecond ramps up rather than starting at full
 * amplitude, because a hard edge at sample zero is a DC step: on a phone
 * speaker that is an audible pop in front of the click it is supposed to be.
 */
function renderClick(ctx: AudioContext, freq: number, decay: number, gain: number): AudioBuffer {
  const sr = ctx.sampleRate;
  const n = Math.max(1, Math.ceil(sr * decay * 6));
  const buf = ctx.createBuffer(1, n, sr);
  const data = buf.getChannelData(0);
  const attack = Math.max(1, Math.round(sr * 0.0004));
  for (let i = 0; i < n; i++) {
    const t = i / sr;
    const env = Math.exp(-t / decay) * Math.min(1, i / attack);
    const noise = Math.random() * 2 - 1;
    data[i] = env * gain * (0.62 * noise + 0.38 * Math.sin(2 * Math.PI * freq * t));
  }
  return buf;
}

/**
 * Wake the audio hardware, from inside a real user gesture.
 *
 * Mobile browsers refuse to start an `AudioContext` outside one, and a context
 * created anywhere else is born `suspended` - so the first click would be
 * swallowed and every later one would work, which is the most confusing
 * possible failure. Called from `touchstart` on the pad, where a gesture is
 * guaranteed, and cheap enough to call on every touch.
 */
export function primeClick(): void {
  if (unavailable) return;
  try {
    if (!clicks) {
      const Ctor: typeof AudioContext | undefined =
        window.AudioContext ??
        (window as unknown as { webkitAudioContext?: typeof AudioContext }).webkitAudioContext;
      if (!Ctor) {
        unavailable = true;
        return;
      }
      // Mix with whatever else is playing, and stay under the mute switch.
      // Safari 16.4+; ignored everywhere else.
      const session = (navigator as Navigator & { audioSession?: { type: string } }).audioSession;
      if (session) session.type = "ambient";

      const ctx = new Ctor();
      clicks = {
        ctx,
        // Low and short: the snap of a button bottoming out.
        down: renderClick(ctx, 1750, 0.0035, DOWN_GAIN),
        // Higher, shorter, quieter: the mechanism springing back.
        up: renderClick(ctx, 2600, 0.0022, UP_GAIN),
      };
    }
    // Autoplay policies suspend a context that was built too early, and one
    // that has been backgrounded comes back suspended too.
    if (clicks.ctx.state === "suspended") void clicks.ctx.resume();
  } catch {
    // No audio on this device, or the browser refused. Never fatal: the visual
    // feedback is what every device relies on anyway.
    unavailable = true;
    clicks = null;
  }
}

function play(which: "down" | "up"): boolean {
  if (!clicks || clicks.ctx.state !== "running") return false;
  try {
    const src = clicks.ctx.createBufferSource();
    src.buffer = clicks[which];
    src.connect(clicks.ctx.destination);
    src.start();
    return true;
  } catch {
    return false;
  }
}

/** The press: "the button is down, you are dragging". */
export function clickDown(): boolean {
  return play("down");
}

/** The release: "the button is back up". */
export function clickUp(): boolean {
  return play("up");
}

/**
 * What this device can do, for the debug page.
 *
 * `running` is the interesting field: a context that exists but is suspended is
 * the difference between "this phone has no audio" and "the browser is waiting
 * for a touch", which sound exactly the same from the user's side.
 */
export function soundReport(): { api: boolean; running: boolean } {
  return {
    api: !unavailable && (!!clicks || typeof window.AudioContext === "function"),
    running: clicks?.ctx.state === "running",
  };
}
