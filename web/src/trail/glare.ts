/**
 * The light that spills in from the edges of the screen while a drag is held.
 *
 * Its own module because it is the one thing here that is *cached*: stroking
 * the profile below costs about as much as filling the screen, and a held drag
 * would pay that on every frame - exactly the frames where cursor movement must
 * not stutter. Built once per resize, composited per frame.
 */

/**
 * The glare's brightness profile, as a function of depth from the screen edge.
 *
 * Two components, because that is what light actually does. A **rim** carries
 * nearly all the brightness and falls off fast - it is the hot edge the eye
 * reads as "lit". A **bloom** reaches twice as far at a seventh the intensity,
 * and is what stops the effect ending at a boundary the eye can find. One curve
 * gives you only one of the two: tight enough to look bright and it ends in a
 * visible ring, wide enough to fade out and it looks like fog.
 *
 * Depths are in CSS pixels, measured inward from the edge of the glass, and the
 * frame is square: the glare runs corner to corner and lets the phone's own
 * display radius crop it. Rounding it to match would put a drawn shape back on
 * screen, and a shape has an outline however softly it is filled. Square, the
 * light simply reaches the end of the glass, and the corners read as pooled
 * light where two edges meet - which is what makes it cinematic rather than
 * decorative.
 *
 * The status line at the top of the screen sits inside the bloom and is dimmed
 * while a drag is held. That is the right trade: during a drag the user is
 * watching the other screen, and the one thing that must carry is the state of
 * the button.
 */
const GLARE_RIM_DEPTH = 26;
const GLARE_RIM_PEAK = 0.9;
const GLARE_RIM_FALLOFF = 1.6;
export const GLARE_BLOOM_DEPTH = 54;
const GLARE_BLOOM_PEAK = 0.14;
const GLARE_BLOOM_FALLOFF = 1.9;

/** Base opacity of the glare, before the breath. */
export const GLARE_ALPHA = 0.72;
/**
 * How much the breath swings it either side of the base.
 *
 * Wide enough to read as motion from the corner of the eye rather than to be
 * noticed only when looked at directly. The trough still leaves the rim plainly
 * white - the drag must never look as though it has ended.
 */
export const GLARE_BREATH = 0.28;
/**
 * Breath period in ms, over a full in-and-out.
 *
 * Brisk rather than restful. A slow swell is easy to miss entirely while the
 * eyes are on the other screen; at around two seconds the pulse registers in
 * peripheral vision without ever reading as an alarm.
 */
export const GLARE_PERIOD_MS = 1800;

/**
 * Render the edge glare into an offscreen canvas.
 *
 * Deliberately ignores the safe-area insets, unlike the charging border. That
 * border is a crisp line carrying a readable value, so it has to step clear of
 * the notch to stay unbroken. This is diffuse light: it wants the true edge of
 * the glass, and a notch simply occludes a little of it the way it occludes any
 * other lit pixel.
 */
export function buildGlare(w: number, h: number, dpr: number): HTMLCanvasElement | null {
  if (w <= 0 || h <= 0) return null;
  const off = document.createElement("canvas");
  off.width = Math.round(w * dpr);
  off.height = Math.round(h * dpr);
  const g = off.getContext("2d");
  if (!g) return null;
  g.setTransform(dpr, 0, 0, dpr, 0, 0);
  g.strokeStyle = "#ffffff";
  g.lineJoin = "round";
  g.lineWidth = 1;
  // One band per pixel of depth, each carrying the opacity the falloff curve
  // asks for at that distance. A handful of wide strokes is cheaper but sums
  // to a staircase - four visible nested outlines, which is four times the
  // border this is meant to replace. At one pixel a step the curve is simply
  // a gradient, and none of it costs anything per frame: this runs on resize.
  for (let d = 0; d < GLARE_BLOOM_DEPTH; d++) {
    const inset = d + 0.5;
    const bw = w - inset * 2;
    const bh = h - inset * 2;
    if (bw <= 0 || bh <= 0) break;
    const rim =
      d < GLARE_RIM_DEPTH
        ? GLARE_RIM_PEAK * Math.pow(1 - d / GLARE_RIM_DEPTH, GLARE_RIM_FALLOFF)
        : 0;
    const bloom =
      GLARE_BLOOM_PEAK * Math.pow(1 - d / GLARE_BLOOM_DEPTH, GLARE_BLOOM_FALLOFF);
    g.globalAlpha = Math.min(1, rim + bloom);
    g.beginPath();
    g.rect(inset, inset, bw, bh);
    g.stroke();
  }
  return off;
}

/**
 * Paint the cached glare at a given opacity.
 *
 * Four edge strips rather than one full-screen blit. The glare is transparent
 * across the whole middle of the screen, and compositing that emptiness would
 * cost the same as compositing light - about five times the pixels, on every
 * frame of a drag. Source coordinates are in the cached bitmap's own device
 * pixels, hence the scale.
 */
export function blitGlare(
  ctx: CanvasRenderingContext2D,
  g: HTMLCanvasElement | null,
  width: number,
  height: number,
  alpha: number,
): void {
  if (!g || alpha <= 0) return;
  const w = width;
  const h = height;
  const d = Math.min(GLARE_BLOOM_DEPTH, w / 2, h / 2);
  const k = g.width / w;
  ctx.globalCompositeOperation = "lighter";
  ctx.globalAlpha = alpha;
  const strip = (x: number, y: number, sw: number, sh: number) =>
    ctx.drawImage(g, x * k, y * k, sw * k, sh * k, x, y, sw, sh);
  strip(0, 0, w, d);
  strip(0, h - d, w, d);
  strip(0, d, d, h - d * 2);
  strip(w - d, d, d, h - d * 2);
}
