/**
 * The touch surface (plan.md section 10).
 *
 * Two jobs, and the second is the hard one:
 *   1. report every touch point at the display's full rate;
 *   2. stop the mobile browser from treating this like a web page - no
 *      scrolling, zooming, selecting, callout menus or edge-swipe navigation.
 *
 * It decides nothing about gestures. Samples go out raw.
 */

import { Phase, type TouchSample } from "./protocol";
import { watchSize } from "./viewport";

export interface SurfaceOptions {
  /** Called once per animation frame with everything captured since the last. */
  onBatch: (samples: TouchSample[]) => void;
  /** Called on finger-down, for local feedback. */
  onTouchDown?: (x: number, y: number) => void;
  /** Reports the sample rate, for the debug readout. */
  onRate?: (hz: number) => void;
  /** Spread between the shortest and longest frame in the window, in ms. */
  onJitter?: (ms: number) => void;
  /**
   * The surface changed size, so the desktop's copy of its geometry is stale.
   *
   * Fires on a rotation, on a split-screen drag, and - the case that matters -
   * once shortly after the first load, when the phone browser finishes
   * collapsing its address bar and the page is finally the size it will stay.
   */
  onGeometry?: (geometry: { wpx: number; hpx: number; dpr: number }) => void;
}

/**
 * iOS, including iPadOS pretending to be a Mac.
 *
 * Only used to decide who needs the touch-event hammer below; nothing about the
 * trackpad itself branches on the browser.
 */
const IS_IOS =
  /iPad|iPhone|iPod/.test(navigator.userAgent) ||
  (navigator.platform === "MacIntel" && navigator.maxTouchPoints > 1);

/** Keep trackpad touches local while leaving buttons and settings native. */
export function suppressBrowserGestures(): void {
  const surface = document.getElementById("surface");
  if (!surface) return;
  const swallow = (e: Event) => {
    if (e.cancelable) e.preventDefault();
  };
  for (const type of [
    "contextmenu",
    "gesturestart",
    "gesturechange",
    "gestureend",
    "dblclick",
    "selectstart",
    "dragstart",
  ]) {
    surface.addEventListener(type, swallow, { passive: false });
  }
  // touch-action:none covers most of this, but iOS Safari still needs the
  // explicit preventDefault to suppress double-tap zoom and rubber-banding.
  //
  // It is applied *only* on iOS, and that restriction is load-bearing. Blink
  // grants a page its user activation from the tap gesture it synthesises out
  // of a touch sequence, and a `touchstart` that was preventDefault()ed
  // suppresses that gesture entirely. The page then never becomes "activated",
  // and Chrome silently refuses `navigator.vibrate` for the rest of its life -
  // which is exactly how a working long press ends up with no buzz behind it.
  // Android needs none of this anyway: `touch-action: none` already stops
  // scrolling, double-tap zoom and pull-to-refresh. These listeners belong on
  // the pad itself: cancelling a document-level touchstart also cancels the
  // click that would open Settings on iPhone, and prevents the sheet scrolling.
  if (!IS_IOS) return;
  for (const type of ["touchstart", "touchmove", "touchend", "touchcancel"]) {
    surface.addEventListener(type, swallow, { passive: false });
  }
}

/**
 * Which pointer types drive the trackpad.
 *
 * Mouse input is ignored by default, and that is not fussiness: when the page is
 * opened on the very computer it controls, the cursor it moves passes over the
 * page, the browser reports that as a mouse pointermove, and the page feeds it
 * straight back - a runaway loop that pins the renderer. On a phone there is no
 * mouse, so nothing is lost. `?mouse=1` re-enables it for debugging on a desktop
 * browser that is driving a *different* machine.
 */
function acceptedPointerTypes(): Set<string> {
  const types = new Set(["touch", "pen"]);
  if (new URLSearchParams(location.search).get("mouse") === "1") types.add("mouse");
  return types;
}

export class Surface {
  private readonly accepted = acceptedPointerTypes();
  private queue: TouchSample[] = [];
  /** pointerId can be any integer; the protocol allows one byte. */
  private readonly ids = new Map<number, number>();
  private nextId = 0;
  private sampleCount = 0;
  private lastRateAt = performance.now();
  private lastFrameAt = performance.now();
  private frameGaps: number[] = [];
  /**
   * Whether the page has the user activation Chrome wants before it will
   * vibrate. Defaults to *true* where `navigator.userActivation` is missing -
   * on iOS there is no Vibration API to win, so nothing is worth deviating from
   * the proven input path for.
   */
  private activationDone =
    (navigator as Navigator & { userActivation?: { hasBeenActive: boolean } }).userActivation
      ?.hasBeenActive ?? true;

  constructor(
    private readonly el: HTMLElement,
    private readonly opts: SurfaceOptions,
  ) {
    el.addEventListener("pointerdown", this.onDown, { passive: false });
    el.addEventListener("pointermove", this.onMove, { passive: false });
    el.addEventListener("pointerup", this.onEnd(Phase.Up), { passive: false });
    el.addEventListener("pointercancel", this.onEnd(Phase.Cancel), { passive: false });
    // If capture is lost some other way, retire the id so it can be reused.
    el.addEventListener("lostpointercapture", (e) => this.ids.delete(e.pointerId));
    requestAnimationFrame(this.frame);
    // Only when the numbers really moved: the watcher also fires on a
    // visual-viewport scroll, and re-announcing an unchanged surface would put
    // a message on the socket for every one of them.
    let last = "";
    watchSize(el, () => {
      const geometry = this.geometry;
      const key = `${geometry.wpx}x${geometry.hpx}@${geometry.dpr}`;
      if (key === last) return;
      last = key;
      this.opts.onGeometry?.(geometry);
    });
  }

  /**
   * Is anything touching the pad right now?
   *
   * `ids` holds one entry per live pointer - it is how a browser pointer id
   * becomes the short id on the wire - so it is already the answer. Asked by
   * the shake detector, which must never change modes mid-gesture.
   */
  get touching(): boolean {
    return this.ids.size > 0;
  }

  /** Surface geometry, sent to the desktop so it can scale normalized deltas. */
  get geometry(): { wpx: number; hpx: number; dpr: number } {
    return {
      wpx: this.el.clientWidth,
      hpx: this.el.clientHeight,
      dpr: window.devicePixelRatio || 1,
    };
  }

  private shortId(pointerId: number): number {
    let id = this.ids.get(pointerId);
    if (id === undefined) {
      id = this.nextId++ & 0xff;
      this.ids.set(pointerId, id);
    }
    return id;
  }

  /**
   * Queue one sample, in coordinates normalized against `r`.
   *
   * The caller passes the rect rather than this reading it, because one
   * `pointermove` can carry a dozen coalesced samples and reading it per sample
   * meant a dozen forced layout flushes inside a `{passive: false}` handler -
   * which the browser's own input pipeline is waiting on before it can go any
   * further. The rect cannot change part-way through a single handler, so once
   * per event is both cheaper and exactly as correct.
   */
  private push(ev: PointerEvent, phase: Phase, r: DOMRect): void {
    this.queue.push({
      t: ev.timeStamp || performance.now(),
      id: this.shortId(ev.pointerId),
      phase,
      x: (ev.clientX - r.left) / r.width,
      y: (ev.clientY - r.top) / r.height,
    });
    this.sampleCount++;
  }

  private accepts(ev: PointerEvent): boolean {
    return this.accepted.has(ev.pointerType);
  }

  /**
   * Cancel the browser's default handling - for every touch but the first.
   *
   * Cancelling a touch `pointerdown` suppresses the compatibility mouse events
   * it would have produced, and on Chrome for Android that takes the
   * synthesised `click` with it. That click is how a page earns its sticky user
   * activation, and without activation Chrome refuses `navigator.vibrate` for
   * the document's whole life - so the long-press can never buzz.
   *
   * Leaving touch uncancelled outright is not the answer either, and that was a
   * real regression: the compatibility stream is a `mousedown`, a `mousemove`
   * *every frame*, a `mouseup` and a `click`, each costing a hit-test and a
   * page-wide `:hover` recalculation on exactly the frames that have to be
   * delivering touch samples. Cursor movement went visibly rough.
   *
   * Activation is sticky, so it only has to be earned once. Exactly one touch
   * is allowed through; every touch after it is swallowed as before, and the
   * input path is bit-for-bit the one that was already smooth.
   */
  private swallow(ev: PointerEvent): void {
    if (ev.pointerType === "mouse" || this.activationDone) ev.preventDefault();
  }

  private refreshActivation(): void {
    if (this.activationDone) return;
    const ua = (navigator as Navigator & { userActivation?: { hasBeenActive: boolean } })
      .userActivation;
    if (ua?.hasBeenActive) this.activationDone = true;
  }

  private onDown = (ev: PointerEvent): void => {
    if (!this.accepts(ev)) return;
    this.swallow(ev);
    // Capture keeps the finger bound to the surface even if it strays. It
    // throws if the pointer is already gone by the time we handle the event -
    // which must not cost us the touch sample.
    try {
      this.el.setPointerCapture(ev.pointerId);
    } catch {
      /* capture is an optimisation, not a requirement */
    }
    this.push(ev, Phase.Down, this.el.getBoundingClientRect());
    // A finger-down must not wait for the next frame either, and for a reason
    // the release above does not share: this is the sample the desktop decides
    // *who is driving* on. Control only ever changes hands on a batch that
    // opens a gesture, so a `DOWN` sitting in the queue for a frame is a frame
    // added to the front of every gesture and to every handover between two
    // devices - which is exactly where it is felt.
    this.flush();
    this.opts.onTouchDown?.(ev.clientX, ev.clientY);
  };

  private onMove = (ev: PointerEvent): void => {
    if (!this.accepts(ev)) return;
    this.swallow(ev);
    // Coalesced events recover the samples the browser batched into this single
    // callback; without them a 120 Hz digitiser reports like a 60 Hz one.
    const coalesced = ev.getCoalescedEvents?.() ?? [];
    const r = this.el.getBoundingClientRect();
    for (const e of coalesced.length ? coalesced : [ev]) this.push(e, Phase.Move, r);
  };

  private onEnd(phase: Phase) {
    return (ev: PointerEvent): void => {
      if (!this.accepts(ev)) return;
      this.swallow(ev);
      // A completed tap is what grants activation; re-read it here so the very
      // next touch goes back to being fully swallowed.
      this.refreshActivation();
      this.push(ev, phase, this.el.getBoundingClientRect());
      this.ids.delete(ev.pointerId);
      // A finger-up must never wait for the next frame: a late release is a
      // stuck button or a missed click.
      this.flush();
    };
  }

  private flush(): void {
    if (!this.queue.length) return;
    const batch = this.queue.splice(0, 255);
    this.queue.length = 0;
    this.opts.onBatch(batch);
  }

  private frame = (now: number): void => {
    this.flush();

    this.frameGaps.push(now - this.lastFrameAt);
    this.lastFrameAt = now;

    const elapsed = now - this.lastRateAt;
    if (elapsed >= 500) {
      this.opts.onRate?.(Math.round((this.sampleCount * 1000) / elapsed));
      if (this.frameGaps.length > 2) {
        // Ignore the first gap, which spans the reporting boundary.
        const gaps = this.frameGaps.slice(1);
        this.opts.onJitter?.(Math.max(...gaps) - Math.min(...gaps));
      }
      this.frameGaps = [];
      this.sampleCount = 0;
      this.lastRateAt = now;
    }
    requestAnimationFrame(this.frame);
  };
}
