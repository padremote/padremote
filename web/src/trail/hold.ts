/**
 * The long-press indicator: the phone's stand-in for a trackpad click.
 *
 * This carries far more weight than it looks like it should. The hold is the
 * one gesture with no hardware analogue - on a real trackpad the click is
 * *felt* - and the phone cannot promise a substitute. iOS Safari has no
 * Vibration API at all, Chrome refuses to vibrate a page that has not yet seen
 * a completed tap, and plenty of tablets have no motor to begin with. So this
 * animation is not a garnish on the haptic; on most devices it is the only
 * feedback that ever arrives, and it is built accordingly.
 *
 * It is also a *preview of a decision the desktop makes*, which is why the
 * state machine here mirrors the engine's rule exactly - one finger, still,
 * for `pressMs`, judged from where it landed. A ring that charges when the
 * engine would not promises a drag that never happens.
 *
 * Two checks pin it down, and both drive this class directly:
 * `npm run check:hold` (it agrees with the engine) and `npm run check:intro`
 * (the confirmation grows out of the charge rather than interrupting it).
 */

import type { TouchSample } from "../protocol";
import { Phase } from "../protocol";
import { blitGlare, GLARE_ALPHA, GLARE_BREATH, GLARE_PERIOD_MS } from "./glare";
import {
  ARMED_COLOR,
  easeInOut,
  easeOut,
  mix,
  PRESS_COLOR,
  type Stage,
} from "./theme";

/** Radius the charging ring settles at, in CSS pixels. */
const RING_R = 38;
/** How long the completion burst plays for, in ms. */
const BURST_MS = 380;
/** How far the shockwave travels beyond the ring, in CSS pixels. */
const BURST_REACH = 42;
/**
 * How long the arming intro takes to settle into the held-drag glow, in ms.
 *
 * One number for the whole hand-over, and that is the point: the flash, the
 * colour change, the border and the glare all run off it, so they arrive and
 * settle together instead of three animations happening near each other.
 */
const INTRO_MS = 380;
/**
 * How far above its held level the glare blooms at the moment of arming.
 *
 * The intro used to be a green full-screen flash while the held state was a
 * white edge glare - two unrelated effects, so arming read as one animation
 * being interrupted by another. Now the *same* light simply arrives overbright
 * and settles: `GLARE_ALPHA + GLARE_INTRO_BOOST` is the peak, `GLARE_ALPHA` is
 * where it lands.
 */
const GLARE_INTRO_BOOST = 0.28;
/** How long the ring takes to turn from charging blue to armed green, in ms. */
const ARM_TINT_MS = 170;
/** How long the let-go ring lingers, in ms. */
const RELEASE_MS = 300;
/** Fraction of the charge after which the screen-edge frame appears. */
const BORDER_FROM = 0.5;
/**
 * The screen frame is four corner brackets, not a rectangle round the screen.
 *
 * A rounded rectangle was the obvious shape and the wrong one twice over. It
 * reads as *chrome* - a border the page has always had - which is exactly what
 * a state indicator must not look like; and its radius was a guess at the
 * phone's own display radius, so on any handset that guessed wrong the corners
 * either bulged past the glass or floated visibly inside it.
 *
 * Brackets solve both. Nothing is drawn where a display radius lives - the 45°
 * chamfer cuts the corner away entirely, so there is no arc to match and
 * nothing to clip - and four marks closing in on the edges read as an
 * instrument acquiring something rather than as a frame that was always there.
 *
 * `FRAME_CHAMFER` is deliberately larger than any current phone's corner radius
 * (iPhone's is around 45-55 device px), so the cut clears the glass on all of
 * them.
 */
const FRAME_CHAMFER = 46;
/** How far the frame sits inside the safe area, in CSS pixels. */
const FRAME_INSET = 4;
/**
 * The gap left at the middle of each edge when the brackets are fully grown.
 *
 * They must never meet. Four arms that join into a closed rectangle are a
 * rectangle again, and the whole point of the shape is that it is not one.
 */
const FRAME_GAP = 26;
/** Length of the tick that caps a growing arm, in CSS pixels. */
const FRAME_TICK = 7;
/**
 * Longest an arm may grow, in CSS pixels, however long its edge is.
 *
 * Without a cap the arms scale with the edge, and on a phone's long side that
 * put them 280 px out from each corner - two marks separated by a 50 px gap,
 * which the eye reads as a rectangle someone has scratched a hole in rather
 * than as a pair of brackets. Capped, all four corners are the same size on
 * every screen, and the empty middle of each edge is plainly deliberate.
 */
const FRAME_ARM_MAX = 120;
/**
 * ...and no more than this share of the distance from the chamfer to the middle
 * of its edge.
 *
 * The absolute cap alone left a phone's short edges nearly closed - 52 px of
 * gap across the top against 376 down each side - which reads as a rectangle
 * with a nick in it at the top and brackets at the sides. Limiting the arm to a
 * fraction of its own edge as well keeps the corners recognisably corners
 * whatever the aspect ratio.
 */
const FRAME_ARM_SHARE = 0.62;
/**
 * How long the scan sweep takes to cross the screen at the moment of arming.
 *
 * Fast: it is a single beat that says *now*, and anything slower turns into an
 * animation the user waits out. It runs inside the intro, so the sweep, the
 * bracket snap and the glare bloom are all over together.
 */
const SCAN_MS = 260;

export class HoldIndicator {
  /**
   * The surface as it was on the most recent call.
   *
   * Passed in rather than held because the canvas can be resized - and the
   * glare rebuilt - between any two frames; a cached copy would keep drawing
   * the perimeter to a screen size that no longer exists.
   */
  private stage: Stage = {
    width: 0,
    height: 0,
    safe: { top: 0, right: 0, bottom: 0, left: 0 },
    glare: null,
  };
  /** How long a stationary finger must rest before a drag begins, in ms. */
  private pressMs = 500;
  /** How far a finger may stray and still count as still, in CSS pixels. */
  private tapMaxPx = 10;
  /**
   * The finger currently holding still, and since when.
   *
   * `startX`/`startY` are where it *landed*, not where it was last seen. The
   * engine judges stillness against the landing point, so anchoring to a
   * rolling position would let a slow drift stay "still" here while the desktop
   * had long since called it a move.
   */
  private holding:
    | { id: number; x: number; y: number; startX: number; startY: number; since: number }
    | null = null;
  /**
   * Fingers that have moved too far to arm a drag, until they lift.
   *
   * The engine gives each touch exactly one chance: once a finger has travelled
   * past `tapMaxPx` it is steering the cursor, and no amount of pausing turns it
   * back into a press. This set is the phone's copy of that verdict. Without it
   * the ring restarted its charge wherever the finger stopped, so pausing in the
   * middle of a move drew a full charge-up and buzzed - promising a drag the
   * desktop was never going to start.
   */
  private readonly spent = new Set<number>();
  /** Set once the ring completes, so it buzzes exactly once per press. */
  private armed = false;
  /** When the ring completed, for the burst and the held-drag pulse. */
  private armedAt = 0;
  /**
   * Fingers currently down, by protocol id.
   *
   * Kept separately from `points` because that map is a *rendering* structure:
   * it lags a batch behind, and a lifted finger stays in it while its trail
   * fades. Deciding "is exactly one finger down" from it was wrong in both
   * directions.
   */
  private readonly down = new Set<number>();
  /** When the armed state ended, so the release can be seen rather than blink out. */
  private releasedAt = 0;
  private releasedAtPos: { x: number; y: number } | null = null;
  /** Called the moment the hold completes. */
  onPressArmed: (() => void) | null = null;
  /**
   * Called when an armed drag ends.
   *
   * A physical trackpad clicks twice - once going down, once coming back up -
   * and the second one is what tells you the button let go without looking.
   */
  onPressReleased: (() => void) | null = null;

  /**
   * The desktop's own `pressMs`, so the ring finishes exactly when the drag
   * actually starts rather than at a guessed moment.
   */
  setPressMs(ms: number): void {
    if (ms > 0) this.pressMs = ms;
  }

  /**
   * The desktop's own `tapMaxPx`, the distance that separates a press from a
   * move. Taken from the engine rather than assumed, so a host that retunes it
   * does not leave the ring disagreeing with what the cursor does.
   */
  setTapMaxPx(px: number): void {
    if (px > 0) this.tapMaxPx = px;
  }

  /**
   * Track whether a single finger is resting, which is what arms a drag.
   *
   * Mirrors the desktop's rule - one finger, still, for `pressMs` - so the
   * ring is an honest preview of what the engine is about to do rather than
   * decoration that might disagree with it.
   */
  update(samples: readonly TouchSample[], now: number, stage: Stage): void {
    this.stage = stage;
    for (const s of samples) {
      const x = s.x * this.stage.width;
      const y = s.y * this.stage.height;

      if (s.phase === Phase.Up || s.phase === Phase.Cancel) {
        this.down.delete(s.id);
        // Lifting is what clears the verdict: press again and you get a fresh
        // chance to hold, which is exactly the gesture the engine expects.
        this.spent.delete(s.id);
        if (!this.holding || this.holding.id === s.id) this.cancelHold();
        continue;
      }

      if (s.phase === Phase.Down) {
        this.down.add(s.id);
        // A reused id must not inherit the last finger's verdict.
        this.spent.delete(s.id);
      }

      // More than one finger is a scroll, a pinch or a swipe - never a drag.
      // Checked before the spent test so a second finger still clears a ring
      // that some other finger had started.
      if (this.down.size !== 1) {
        this.cancelHold();
        continue;
      }

      // This finger is already moving the cursor. It cannot arm a drag however
      // long it now rests, so it must not be shown charging one.
      if (this.spent.has(s.id)) continue;

      // Start the clock on the *Down* sample itself.
      //
      // This used to wait for a second sample, and that was the bug behind
      // "sometimes there is no ring at all": a finger resting perfectly still
      // emits no `pointermove`, `flush()` skips empty queues, and so no further
      // batch ever arrived to start it. The ring appeared only when the finger
      // happened to shake. From here the animation is driven purely by the
      // frame clock, and a motionless finger is the case it handles best.
      if (!this.holding || this.holding.id !== s.id) {
        this.holding = { id: s.id, x, y, startX: x, startY: y, since: now };
        this.armed = false;
        this.armedAt = 0;
        continue;
      }
      // Once the drag is armed, moving is the *point*: the desktop holds the
      // button down until the finger lifts, so the ring travels with the finger
      // and stays armed. Resetting here would have the phone show a fresh
      // charge-up for the whole of every drag, flatly contradicting what the
      // desktop is doing.
      if (this.armed) {
        this.holding.x = x;
        this.holding.y = y;
        continue;
      }
      // Before it arms, leaving the landing point spends the hold for good.
      //
      // This used to restart the clock at the new position instead, and that
      // was the bug: while you steered the cursor the ring kept re-charging
      // under your finger, so any pause mid-move drew a complete charge and
      // buzzed. The desktop meanwhile had committed to a cursor move the
      // instant the finger passed `tapMaxPx` and would never press the button.
      // Measured from where the finger landed, exactly as the engine does.
      if (Math.hypot(x - this.holding.startX, y - this.holding.startY) > this.tapMaxPx) {
        this.spent.add(s.id);
        this.cancelHold();
      }
    }
  }

  /**
   * Abandon the current hold, remembering where an *armed* one ended.
   *
   * A drag that simply blinks out gives no confirmation that the button was
   * released, which on a device that cannot buzz is the other half of the
   * gesture the user never gets told about.
   */
  private cancelHold(): void {
    if (this.armed && this.holding) {
      this.releasedAt = performance.now();
      this.releasedAtPos = { x: this.holding.x, y: this.holding.y };
      this.onPressReleased?.();
    }
    this.holding = null;
    this.armed = false;
    this.armedAt = 0;
  }

  /**
   * The long-press indicator: "keep holding and this becomes a drag".
   *
   * This carries far more weight than it looks like it should. The hold is the
   * one gesture with no hardware analogue - on a real trackpad the click is
   * *felt* - and the phone cannot promise a substitute. iOS Safari has no
   * Vibration API at all, Chrome refuses to vibrate a page that has not yet
   * seen a completed tap, and plenty of tablets have no motor to begin with.
   * So this animation is not a garnish on the haptic; on most devices it is the
   * only feedback that ever arrives, and it is built accordingly.
   *
   * The governing constraint is that **a fingertip covers roughly 60-100 CSS
   * pixels** - it sits directly on top of anything drawn at the touch point.
   * Feedback drawn only there is feedback the user cannot see. So the signal
   * that has to be unmissable is put where the finger is not: a progress border
   * around the whole screen, and a full-screen flash at the moment it arms.
   */
  draw(ctx: CanvasRenderingContext2D, now: number, stage: Stage): void {
    this.stage = stage;
    // The release flash outlives the hold it belongs to, so it is drawn first
    // and independently of whether a finger is still down.
    this.drawRelease(ctx, now);
    if (!this.holding) return;

    const { x, y } = this.holding;
    const progress = Math.min(1, (now - this.holding.since) / this.pressMs);
    // Below this the finger has barely landed; drawing would flicker on every
    // ordinary pause mid-move.
    if (progress < 0.12 && !this.armed) return;

    ctx.save();

    if (progress >= 1 && !this.armed) {
      this.armed = true;
      this.armedAt = now;
      this.onPressArmed?.();
    }

    if (this.armed) this.drawArmed(ctx, x, y, now);
    else this.drawCharging(ctx, x, y, progress, now);

    ctx.restore();
  }

  /**
   * Four corner brackets closing in on the edges of the screen.
   *
   * The one part of the display a finger can never cover, and the only place a
   * progress signal can be trusted to be visible. `extent` runs 0 to 1: each
   * bracket grows out of its chamfered corner along both edges towards the
   * middle, and at 1 the four of them stop short of meeting.
   *
   * Straight lines only - no arc, no radius, nothing to match against the
   * phone's own corner. `push` moves the whole figure outward, which is what
   * lets the arming snap read as the frame *letting go* rather than as one
   * shape being swapped for another.
   */
  private strokeFrame(ctx: CanvasRenderingContext2D, extent: number, push = 0): void {
    // The canvas is full-bleed so that trails land where the finger did; the
    // frame is what steps inside the notch and the home indicator.
    const safe = this.stage.safe;
    const left = FRAME_INSET + safe.left - push;
    const top = FRAME_INSET + safe.top - push;
    const right = this.stage.width - FRAME_INSET - safe.right + push;
    const bottom = this.stage.height - FRAME_INSET - safe.bottom + push;
    const w = right - left;
    const h = bottom - top;
    if (w <= 0 || h <= 0) return;

    const c = Math.min(FRAME_CHAMFER, w / 2, h / 2);
    // Arms reach at most to the middle of their edge, less the gap that keeps
    // the brackets from closing into a plain rectangle.
    const reach = (half: number) =>
      Math.min(FRAME_ARM_MAX, (half - c) * FRAME_ARM_SHARE, Math.max(0, half - c - FRAME_GAP));
    const armX = Math.max(0, reach(w / 2)) * extent;
    const armY = Math.max(0, reach(h / 2)) * extent;

    // Every corner is the same L with a 45-degree bite out of it, mirrored.
    // Signs: (dx, dy) points from the corner into the screen.
    const corners: [number, number, number, number][] = [
      [left, top, 1, 1],
      [right, top, -1, 1],
      [right, bottom, -1, -1],
      [left, bottom, 1, -1],
    ];

    ctx.save();
    ctx.lineJoin = "miter";
    ctx.beginPath();
    for (const [cx, cy, dx, dy] of corners) {
      ctx.moveTo(cx + dx * (c + armX), cy);
      ctx.lineTo(cx + dx * c, cy);
      ctx.lineTo(cx, cy + dy * c);
      ctx.lineTo(cx, cy + dy * (c + armY));
    }
    ctx.stroke();

    // A tick across the leading end of each arm while it is still growing: the
    // eye tracks the moving mark, and a bare line end gives it nothing to hold.
    if (extent > 0 && extent < 1) {
      ctx.beginPath();
      for (const [cx, cy, dx, dy] of corners) {
        const ex = cx + dx * (c + armX);
        const ey = cy + dy * (c + armY);
        ctx.moveTo(ex, cy);
        ctx.lineTo(ex, cy + dy * FRAME_TICK);
        ctx.moveTo(cx, ey);
        ctx.lineTo(cx + dx * FRAME_TICK, ey);
      }
      ctx.stroke();
    }
    ctx.restore();
  }

  /**
   * The fill-up, while the finger is still earning its drag.
   *
   * Two readings of the same number: a border creeping round the screen, which
   * is visible from anywhere, and a ring tightening at the fingertip, which is
   * precise about *which* finger is doing it.
   */
  private drawCharging(
    ctx: CanvasRenderingContext2D,
    x: number,
    y: number,
    progress: number,
    now: number,
  ): void {
    const eased = easeOut(progress);

    // --- the screen border: the part that cannot be covered by a hand --------
    //
    // Held back until the hold is half earned, for two reasons that point the
    // same way. Positioning the cursor slowly and precisely keeps a finger
    // almost still, so an eager border would light up the whole screen during
    // ordinary use - and it is the most expensive thing drawn here, paid for on
    // every frame of exactly the movement that most needs to stay smooth.
    // Sweeping the full perimeter across the final stretch also reads better:
    // it arrives as "nearly there" rather than as constant background chatter.
    if (progress > BORDER_FROM) {
      const sweep = (progress - BORDER_FROM) / (1 - BORDER_FROM);
      ctx.strokeStyle = PRESS_COLOR;
      ctx.lineCap = "butt";
      // The rail: where the brackets are heading, at a tenth the brightness.
      // Without it the growth has no scale and could be anywhere in its run.
      ctx.globalAlpha = 0.1;
      ctx.lineWidth = 2;
      this.strokeFrame(ctx, 1);

      // Thin and precise rather than heavy: this is an instrument reading, and
      // a 7 px band around the screen reads as packaging. It also *closes in*
      // as the charge fills - the brackets start a little outside where they
      // will end up and settle onto their marks, which is the difference
      // between something acquiring a target and a bar filling up.
      ctx.globalAlpha = 0.85;
      ctx.lineWidth = 3 + sweep * 2;
      this.strokeFrame(ctx, sweep, (1 - sweep) * 10);
    }

    // --- the ring at the fingertip ------------------------------------------
    // Starts wide and closes on RING_R, so the gesture reads as gathering.
    const r = RING_R + (1 - eased) * 18;

    ctx.globalAlpha = 0.16;
    ctx.lineWidth = 3;
    ctx.beginPath();
    ctx.arc(x, y, r, 0, Math.PI * 2);
    ctx.stroke();

    const halo = ctx.createRadialGradient(x, y, r * 0.3, x, y, r * 1.7);
    halo.addColorStop(0, `rgba(47, 111, 235, ${0.26 * progress})`);
    halo.addColorStop(1, "rgba(47, 111, 235, 0)");
    ctx.globalAlpha = 1;
    ctx.fillStyle = halo;
    ctx.beginPath();
    ctx.arc(x, y, r * 1.7, 0, Math.PI * 2);
    ctx.fill();

    ctx.globalAlpha = 0.95;
    ctx.strokeStyle = PRESS_COLOR;
    ctx.lineWidth = 4 + progress * 3;
    ctx.lineCap = "round";
    ctx.beginPath();
    ctx.arc(x, y, r, -Math.PI / 2, -Math.PI / 2 + progress * Math.PI * 2);
    ctx.stroke();

    // A bright head on the leading edge - the part the eye actually tracks.
    const head = -Math.PI / 2 + progress * Math.PI * 2;
    ctx.globalAlpha = 1;
    ctx.fillStyle = "#8ab4ff";
    ctx.beginPath();
    ctx.arc(x + Math.cos(head) * r, y + Math.sin(head) * r, 3 + progress * 2, 0, Math.PI * 2);
    ctx.fill();

    // Motes drawn inward past the halfway mark: the "charging" read. Held back
    // until then so an ordinary pause mid-move stays quiet.
    if (progress > 0.45) {
      const pull = (progress - 0.45) / 0.55;
      for (let i = 0; i < 3; i++) {
        const phase = (now / 620 + i / 3) % 1;
        const dist = r + 30 - easeOut(phase) * 30;
        const angle = head + (i - 1) * 2.1;
        ctx.globalAlpha = Math.sin(phase * Math.PI) * 0.55 * pull;
        ctx.beginPath();
        ctx.arc(x + Math.cos(angle) * dist, y + Math.sin(angle) * dist, 2.5, 0, Math.PI * 2);
        ctx.fill();
      }
    }
  }

  /**
   * Armed: the drag is live.
   *
   * Three simultaneous signals, because this is the moment the user must not
   * miss and the one a buzz would normally carry: the whole screen flashes, the
   * border snaps green and stays lit for as long as the drag lasts, and a word
   * appears clear of the finger saying so in plain language.
   */
  private drawArmed(ctx: CanvasRenderingContext2D, x: number, y: number, now: number): void {
    const since = now - this.armedAt;
    // One clock for the whole hand-over. Everything below is a function of it,
    // so the wash, the border, the colour and the glare settle together.
    const intro = Math.min(1, since / INTRO_MS);
    const settle = easeOut(intro);
    // The ring turns green rather than cutting to it, which is what makes the
    // charge and the confirmation read as one object.
    const color = mix(PRESS_COLOR, ARMED_COLOR, since / ARM_TINT_MS);

    // --- the lock: the brackets snap outward and hand over to the glare -----
    //
    // The charge ends with the four brackets fully grown, and the held state
    // lights the same edges. They snap outward and dissolve into that light
    // across the intro, which is what makes arming read as the frame *opening*
    // rather than as one animation being replaced by another. Without the
    // hand-over the brackets blinked off on the arming frame and the glare
    // appeared from nowhere in their place.
    if (intro < 1) {
      ctx.save();
      ctx.globalAlpha = 0.9 * (1 - settle);
      ctx.strokeStyle = color;
      // Picks up exactly where the charge left off - 5 px, on its marks - and
      // thickens as it is thrown outward.
      ctx.lineWidth = 5 + 3 * settle;
      ctx.lineCap = "butt";
      // Outward, and further the more of the intro has passed: the shape is
      // being thrown off the screen, not fading where it stood.
      this.strokeFrame(ctx, 1, settle * 14);
      ctx.restore();
    }

    // --- the scan: one pass down the screen, the beat that says "now" -------
    //
    // This replaced a flat green wash over the whole screen. The wash was
    // unmissable, which was its job, but at any speed slower than real time it
    // is plainly a rectangle of colour laid over the page - the one moment in
    // the gesture with no structure to it at all. A line crossing the screen
    // covers the same ground, reads as something *happening*, and costs a
    // fraction as much: a 2 px band and a short gradient trail against a fill
    // of every pixel on the display, on the frames right after a drag begins
    // when cursor movement matters more than anything drawn here.
    if (since < SCAN_MS) {
      const t = easeOut(since / SCAN_MS);
      const yScan = t * this.stage.height;
      const trail = 54;
      const fade = 1 - t;
      ctx.save();
      ctx.globalCompositeOperation = "lighter";
      const g = ctx.createLinearGradient(0, yScan - trail, 0, yScan);
      g.addColorStop(0, "rgba(63, 185, 80, 0)");
      g.addColorStop(1, `rgba(63, 185, 80, ${0.3 * fade})`);
      ctx.fillStyle = g;
      ctx.fillRect(0, yScan - trail, this.stage.width, trail);
      ctx.globalAlpha = 0.75 * fade;
      ctx.fillStyle = "#eafff0";
      ctx.fillRect(0, yScan - 1, this.stage.width, 2);
      ctx.restore();
    }

    // --- the glare: lit for the whole drag, and never under a hand ----------
    //
    // White, and light rather than paint. A drawn outline reads as chrome -
    // something the page has around it - while a glare spilling in from the
    // edges reads as the screen itself being live, which is what a held drag
    // is. Composited additively over a dark UI, so it brightens what is under
    // it instead of covering it.
    //
    // It arrives overbright and settles to its held level, and the breath fades
    // in behind it: the moment of arming is this same light blooming, not a
    // separate flash that then hands over to it. The breath starts from its
    // neutral point and swells - starting anywhere else would step the
    // brightness on the very first frame - and its swing is scaled by the
    // intro, so the glow can never dip *below* its held level while the bloom
    // is fading. A dip there is the artefact that made the confirmation look
    // like two animations fighting.
    const breath = easeInOut((Math.sin((since / GLARE_PERIOD_MS) * Math.PI * 2) + 1) / 2);
    ctx.save();
    blitGlare(
      ctx,
      this.stage.glare,
      this.stage.width,
      this.stage.height,
      GLARE_ALPHA +
        (breath - 0.5) * 2 * GLARE_BREATH * settle +
        GLARE_INTRO_BOOST * (1 - settle),
    );
    ctx.restore();

    // --- the shockwave: two rings, staggered --------------------------------
    for (const delay of [0, 90]) {
      const st = since - delay;
      if (st < 0 || st >= BURST_MS) continue;
      const t = easeOut(st / BURST_MS);
      ctx.globalAlpha = (1 - t) * 0.7;
      // Leaves the ring in whatever colour the ring is at that instant, so the
      // wave looks thrown off by it rather than painted over it.
      ctx.strokeStyle = mix(PRESS_COLOR, ARMED_COLOR, (since - delay) / ARM_TINT_MS);
      ctx.lineWidth = 5 * (1 - t) + 1;
      ctx.beginPath();
      ctx.arc(x, y, RING_R + t * BURST_REACH, 0, Math.PI * 2);
      ctx.stroke();
    }

    // --- the disc at the fingertip, breathing so it never looks frozen ------
    //
    // Starts at exactly the radius the charge ring closed on, and at exactly
    // the stroke width it had, then eases to the held look. The seam between
    // the two animations is the one frame the user is looking straight at.
    const r = RING_R + breath * 3 * settle;
    ctx.globalAlpha = 0.22 + breath * 0.08;
    ctx.fillStyle = color;
    ctx.beginPath();
    ctx.arc(x, y, r, 0, Math.PI * 2);
    ctx.fill();

    ctx.globalAlpha = 0.95;
    ctx.strokeStyle = color;
    ctx.lineWidth = 3.5 + 3.5 * (1 - settle);
    ctx.beginPath();
    ctx.arc(x, y, r, 0, Math.PI * 2);
    ctx.stroke();

    // A bracket box around the fingertip: the screen frame in miniature.
    //
    // These used to be chevrons, which said "you can move now" but belonged to
    // no other part of the gesture. The same four corners the edges are drawn
    // with tie the two together - what is happening at the rim is happening
    // under your finger - and a box closing on a point is what every targeting
    // instrument has ever looked like. They grow in with the intro rather than
    // appearing at full size on the arming frame.
    ctx.globalAlpha = (0.55 + breath * 0.35) * settle;
    ctx.lineWidth = 2.5;
    ctx.lineCap = "butt";
    ctx.lineJoin = "miter";
    const reach = r + (10 + breath * 5) * settle;
    const arm = 9;
    ctx.beginPath();
    for (const [sx, sy] of [
      [-1, -1],
      [1, -1],
      [1, 1],
      [-1, 1],
    ]) {
      const cx = x + sx * reach;
      const cy = y + sy * reach;
      ctx.moveTo(cx - sx * arm, cy);
      ctx.lineTo(cx, cy);
      ctx.lineTo(cx, cy - sy * arm);
    }
    ctx.stroke();

    this.drawLabel(ctx, x, y, "DRAGGING", color, (0.85 + breath * 0.15) * settle);
  }

  /**
   * A word, placed clear of the hand.
   *
   * Above the finger by default, below it near the top of the screen, and
   * always inset from the edges - a label half off-screen says nothing. Plain
   * language beats any glyph here: it is read once and understood, and it needs
   * no legend in the docs.
   *
   * Drawn on a chamfered plate rather than a rounded pill, and set in tracked
   * monospace: the same cut corner as the screen frame, so the readout looks
   * like part of the instrument rather than like a notification from the page.
   */
  private drawLabel(
    ctx: CanvasRenderingContext2D,
    x: number,
    y: number,
    text: string,
    color: string,
    alpha: number,
  ): void {
    const above = y > 150;
    // Clear of the reticle brackets, which reach further from the fingertip
    // than the old chevrons did - the readout was sitting on top of them.
    const ly = above ? y - 92 : y + 100;
    const lx = Math.min(Math.max(x, 70), this.stage.width - 70);

    ctx.save();
    ctx.globalAlpha = alpha;
    // Letter spacing is Chrome 99+ and Safari 17.4+; assigning it anywhere else
    // is an inert property set, never an error, so it needs no guard.
    ctx.letterSpacing = "2.5px";
    ctx.font = "600 13px ui-monospace, SFMono-Regular, Menlo, monospace";
    ctx.textAlign = "center";
    ctx.textBaseline = "middle";

    const w = ctx.measureText(text).width + 30;
    const h = 28;
    const cut = 9;
    const x0 = lx - w / 2;
    const y0 = ly - h / 2;
    // A plate with two corners cut away on the diagonal, the same 45 degrees
    // the screen frame is cut at.
    ctx.beginPath();
    ctx.moveTo(x0 + cut, y0);
    ctx.lineTo(x0 + w, y0);
    ctx.lineTo(x0 + w, y0 + h - cut);
    ctx.lineTo(x0 + w - cut, y0 + h);
    ctx.lineTo(x0, y0 + h);
    ctx.lineTo(x0, y0 + cut);
    ctx.closePath();
    ctx.fillStyle = "rgba(6, 20, 10, 0.82)";
    ctx.fill();
    ctx.strokeStyle = color;
    ctx.lineWidth = 1.5;
    ctx.lineJoin = "miter";
    ctx.stroke();

    ctx.fillStyle = color;
    // The tracking pushes the glyphs right by half a space; pull them back so
    // the word still sits centred on the plate.
    ctx.fillText(text, lx + 1.25, ly + 0.5);
    ctx.restore();
  }

  /**
   * The let-go: a brief outward ring where an armed drag ended.
   *
   * Without it the whole display vanishes between two frames, and "did I just
   * drop that where I meant to?" has no answer on screen.
   */
  private drawRelease(ctx: CanvasRenderingContext2D, now: number): void {
    if (!this.releasedAtPos) return;
    const since = now - this.releasedAt;
    if (since >= RELEASE_MS) {
      this.releasedAtPos = null;
      return;
    }
    const t = easeOut(since / RELEASE_MS);
    const { x, y } = this.releasedAtPos;
    ctx.save();
    ctx.globalAlpha = (1 - t) * 0.55;
    ctx.strokeStyle = ARMED_COLOR;
    ctx.lineWidth = 3 * (1 - t) + 1;
    ctx.beginPath();
    ctx.arc(x, y, RING_R + t * 30, 0, Math.PI * 2);
    ctx.stroke();
    // The glare fades with it, rather than snapping off. Same light, dimming -
    // a different shape here would read as a second event rather than the end
    // of the one that was running.
    blitGlare(ctx, this.stage.glare, this.stage.width, this.stage.height, (1 - t) * GLARE_ALPHA);
    ctx.restore();
  }

  /**
   * Forget everything, e.g. when the link drops.
   *
   * `down` must be emptied with the rest. A link that drops mid-touch never
   * delivers the Up that would have removed the finger, and a single stale id
   * left behind would make `down.size !== 1` true forever - no ring, ever
   * again, until the page was reloaded.
   */
  clear(): void {
    this.down.clear();
    this.spent.clear();
    this.holding = null;
    this.armed = false;
    this.armedAt = 0;
    this.releasedAt = 0;
    this.releasedAtPos = null;
  }
}
