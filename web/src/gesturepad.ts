/**
 * A gesture, drawn as a trackpad with fingers on it.
 *
 * The list names a gesture in words - "Swipe up with three fingers" - and the
 * words are exact, but they are also the slowest way to answer "which one is
 * that?". A picture of three dots crossing a pad, blurring as they go, is read
 * at a glance, and the row beside it stops needing to be read at all.
 *
 * Drawn from `Gesture` and nothing else: fingers, motion, axis, direction and
 * where on the pad the hand lands. That is the whole contract - a gesture added
 * to `gestures.ts` gets a drawing without touching this file, and this file
 * cannot know anything about a gesture that the list does not also know.
 *
 * SVG rather than a canvas, and CSS keyframes rather than a frame loop. Every
 * drawing on the page moves, for as long as the page is open, and a
 * `requestAnimationFrame` per gesture would be twenty loops that each have to
 * be started, stopped when the gesture changes, and stopped again when the tab
 * is hidden - twenty chances to leak one. The browser already pauses CSS
 * animation off-screen, and `prefers-reduced-motion` is one media query away
 * rather than a flag threaded through the drawing code.
 *
 * The shapes are all here; which of them can be seen, and when, is the
 * stylesheet's business. There is no arrow: a gesture says which way it goes
 * with the blur it leaves behind it, and that blur is part of the still drawing
 * too, so the reader whose browser is told not to move anything is told the same
 * thing by the same shape rather than by a symbol drawn only for them. Nothing
 * draws a line it keeps, either. A stroke that stays at full strength from one
 * end of the journey to the other is a piece of string being towed along; what a
 * finger actually leaves is a smear that thins towards where it started, and
 * that is the one shape every drawing here uses.
 *
 * The thumbnail in the list and the demonstration beside it are now the *same*
 * drawing at two sizes - same shapes, same travel, same everything but scale.
 * A thumbnail that simplified itself was a picture of a different gesture than
 * the one it was standing in for, which is the one job a thumbnail has.
 */

import type { Gesture } from "./gestures";

const NS = "http://www.w3.org/2000/svg";

/** The pad, in the drawing's own units. Roughly a trackpad's proportions. */
const W = 100;
const H = 76;
/** How far the row of fingers bows away from the palm. */
const ARCH = 3.5;
const FINGER_R = 6.5;
/** The contact patch under the fingertip - the soft part of a finger. */
const HALO_R = 9.5;
/** The press ring, drawn clear of the contact patch it closes around. */
const RING_R = HALO_R + 3.5;

/**
 * Ids have to be unique across the page, and there are twenty of these drawings
 * on it: a gradient defined twice under one name is a gradient the second
 * drawing silently borrows from the first.
 */
let uid = 0;

/** Two decimal places is under a thousandth of a pad, and reads in a devtool. */
function round(n: number): number {
  return Math.round(n * 100) / 100;
}

function svg<K extends keyof SVGElementTagNameMap>(
  tag: K,
  attrs: Record<string, string | number>,
): SVGElementTagNameMap[K] {
  const node = document.createElementNS(NS, tag);
  // `className` on an SVG element is a read-only `SVGAnimatedString`, so every
  // attribute here - class included - goes through `setAttribute`.
  for (const [name, value] of Object.entries(attrs)) node.setAttribute(name, String(value));
  return node;
}

/**
 * A stroke that dissolves along its own length, from solid to nothing.
 *
 * This is what the motion blur is made of: where the finger has been, fading
 * with how long ago it was there. A hard edge anywhere on it turns the shape
 * into a second object being towed along behind the dot, which is what made the
 * old tapered wedge read as a fin - and what made the cursor's drawn-in trail,
 * solid from end to end, read as a piece of string.
 */
function fade(
  id: string,
  from: { x: number; y: number },
  to: { x: number; y: number },
  peak: number,
  tone = "currentColor",
): SVGElement {
  const grad = svg("linearGradient", {
    id,
    // The endpoints are known exactly, so there is nothing to be gained from
    // making the browser measure a bounding box - and a straight line has a
    // flat one, which an object-space gradient cannot point along at all.
    gradientUnits: "userSpaceOnUse",
    x1: from.x, y1: from.y, x2: to.x, y2: to.y,
  });
  // `currentColor` so the whole drawing takes its hue from one `color`, and a
  // selected row lights its picture without restating it shape by shape.
  // Three stops rather than two. A straight ramp keeps too much of itself for
  // too long and then stops, which puts a visible band at the far end of a
  // blur; weighting the middle low makes the tail give out early and the shape
  // read as something dispersing instead of a bar that was cut off.
  grad.append(
    svg("stop", { offset: "0", "stop-color": tone, "stop-opacity": "0" }),
    svg("stop", { offset: ".55", "stop-color": tone, "stop-opacity": String(round(peak * 0.22)) }),
    svg("stop", { offset: "1", "stop-color": tone, "stop-opacity": String(peak) }),
  );
  return grad;
}

/**
 * How far apart the fingers sit. A wider hand needs each finger closer in.
 *
 * Closer, but not so close that the contact patches run together: four fingers
 * whose soft edges all overlap stop being four fingers and become one caterpillar,
 * and counting them is most of what the drawing is for.
 */
function spread(count: number): number {
  return count >= 4 ? 15.5 : count === 3 ? 17 : 19;
}

/** Where each finger lands: a row, bowed like a hand rather than ruled flat. */
function fingers(g: Gesture): { x: number; y: number }[] {
  const origin = g.origin ?? { x: 0.5, y: 0.5 };
  const cx = origin.x * W;
  const cy = origin.y * H;
  const gap = spread(g.fingers);
  const middle = (g.fingers - 1) / 2;
  return Array.from({ length: g.fingers }, (_, i) => {
    const offset = i - middle;
    // Flat for one finger; for more, the outer ones sit a little lower, which
    // is the difference between a row of dots and a hand.
    const bow = middle === 0 ? 0 : (1 - (offset / middle) ** 2) * ARCH;
    return { x: cx + offset * gap, y: cy - bow };
  });
}

/** Which way the fingers travel, as a unit vector in drawing space. */
function heading(g: Gesture): { dx: number; dy: number } {
  // The two one-finger gestures ride a curve, and the fingertip is turned to
  // face along it - `offset-rotate: auto` in the stylesheet. So their blur is
  // drawn in a frame that has already been pointed the right way, and "the way
  // the hand came" is simply back along the local x axis, wherever on the bend
  // the finger happens to be.
  if (g.motion === "hold" || g.motion === "move") return { dx: 1, dy: 0 };
  switch (g.direction) {
    case "left": return { dx: -1, dy: 0 };
    case "right": return { dx: 1, dy: 0 };
    case "up": return { dx: 0, dy: -1 };
    case "down": return { dx: 0, dy: 1 };
    default: return g.axis === "x" ? { dx: 1, dy: 0 } : { dx: 0, dy: 1 };
  }
}

/**
 * The curve both one-finger gestures travel.
 *
 * A swipe is a direction, and a straight line across the pad says so exactly.
 * The other two are not directions at all: the cursor goes wherever you send it,
 * and a drag goes wherever the thing being dragged has to end up. Drawn straight
 * they were a picture of a swipe with the wrong caption, so they curve.
 *
 * One easy S, and no more than that. A figure-eight said the same thing and said
 * it beautifully, but a loop is four times the distance of a swipe and the pad
 * gives it the same three seconds - so the finger tore round it at a speed no
 * hand moves at, and next to the swipes above it the whole page looked uneven.
 * Distance is what sets the speed here, and this is the shortest curve that is
 * still unmistakably a curve.
 *
 * Both ends are flat: the control points sit level with the anchors they belong
 * to, so the curve leaves and arrives horizontally. That is not decoration.
 * `offset-rotate: auto` turns the fingertip to face along the path, so at the
 * start of the journey it is turned by nothing at all - which is what lets the
 * press ring on a hold charge from twelve o'clock like the phone's own, instead
 * of from wherever the curve happened to be pointing.
 */
const SWEEP = { move: { x: 14, y: 8 }, hold: { x: 11, y: 6 } };

function sweepPath(spot: { x: number; y: number }, motion: "move" | "hold"): string {
  const { x: a, y: b } = SWEEP[motion];
  // Away from the bottom left, up, and settling flat into the top right.
  return `M${round(spot.x - a)} ${round(spot.y + b)}`
    + `C${round(spot.x - a / 3)} ${round(spot.y + b)}`
    + ` ${round(spot.x + a / 3)} ${round(spot.y - b)}`
    + ` ${round(spot.x + a)} ${round(spot.y - b)}`;
}

/**
 * How far the blur reaches back from a fingertip, in drawing units.
 *
 * A little shorter than the swipe's own travel, so at the moment the hand is
 * moving fastest the smear covers most of the ground already crossed and never
 * arrives somewhere the finger has not been.
 */
const SMEAR = 21;

/**
 * How far it reaches on a hold, which needs more.
 *
 * The blur is drawn *from* the fingertip outwards and is hidden by whatever is
 * drawn on top of it. On a swipe that is the contact patch, nine units of it; on
 * a hold it is the press ring, which is fourteen - so a smear of the ordinary
 * length arrived from behind the ring with three units to spare and read as a
 * smudge on the edge of a circle. Longer, so that the part of it anyone can see
 * is as long as the part they can see on a swipe - and no longer than that,
 * because this one's journey is short, and a tail as long as the journey claims
 * ground the finger has not covered yet.
 */
const SMEAR_HELD = 22;

/**
 * The motion blur, as two capsules rather than a filter.
 *
 * A shutter open while something moves does not record an edge, it records the
 * whole path smeared out and thinning towards where the thing was longest ago.
 * That is exactly the shape `fade` draws: one wide faint capsule for the spill
 * and one at the fingertip's own width for the core, both reaching back along
 * the way the hand came. Every gesture that goes anywhere is drawn this way -
 * one finger or four, a swipe, a cursor move or the drag out of a press - so the
 * page has one idea of what movement looks like rather than three.
 *
 * Two vector shapes rather than `filter: blur()` because there are twenty
 * drawings on the page, each with up to four fingers, all moving all the time -
 * eighty filtered layers being re-rasterised every frame is a phone getting
 * warm to soften an edge that a gradient softens for nothing.
 */
const BLUR: { cls: string; width: number; peak: number }[] = [
  { cls: "pad-blur", width: HALO_R * 2, peak: 0.2 },
  { cls: "pad-blur-core", width: FINGER_R * 2, peak: 0.5 },
];

/**
 * The press indicator, as the phone actually draws it.
 *
 * This is a miniature of `trail/hold.ts`, and deliberately so: the panel is
 * explaining a gesture whose whole difficulty is that nothing about it is
 * obvious until you have done it once, and a demonstration that invents its own
 * idea of what a press looks like teaches the wrong thing. So the beats are the
 * phone's beats - the ring arrives wide and gathers in, a bright head runs the
 * charging edge, completion bursts and turns the ring the armed green, and the
 * green ring then *travels with the finger* for as long as the drag lasts.
 *
 * That last part is what the old drawing got most wrong. It left the ring
 * behind at the point of the press and faded it out, which says the press is
 * something that happened and is now over. On the phone the ring rides your
 * fingertip the entire time the button is held down, and its still being there
 * is the only thing telling you the drag is live.
 *
 * Which is why it is built into the finger's own group: the group is what
 * moves, so anything belonging to the fingertip cannot be left behind by
 * accident.
 */
function press(finger: SVGElement): void {
  // Outside the gathering ring, so the burst is not scaled by the closing.
  finger.append(svg("circle", { class: "pad-burst", cx: 0, cy: 0, r: RING_R }));
  // The ring and its track close inward together, so the gathering is one
  // movement rather than a ring shrinking against a fixed circle.
  const gather = svg("g", { class: "pad-press" });
  gather.append(svg("circle", { class: "pad-ring-track", cx: 0, cy: 0, r: RING_R }));
  // `pathLength` 1 makes the dash arithmetic readable; the dash itself is a
  // hair longer than the path so that a finished ring closes over its own
  // start instead of leaving a butt-capped seam at twelve o'clock.
  gather.append(svg("circle", { class: "pad-ring", cx: 0, cy: 0, r: RING_R, pathLength: 1 }));
  // The head is a dot parked at twelve o'clock in a group that turns a full
  // circle, which is the cheapest way to keep it exactly on the end of an arc
  // that is being drawn by a dash offset - the two run off the same clock and
  // the same easing, so it cannot drift off the edge it is supposed to be on.
  const head = svg("g", { class: "pad-ring-head" });
  head.append(svg("circle", { class: "pad-head", cx: 0, cy: -RING_R, r: 2.4 }));
  gather.append(head);
  finger.append(gather);
}

/**
 * Build the drawing.
 *
 * `animated` is the only difference between the thumbnail in the list and the
 * demonstration beside it: same shapes, same markup, same movement, and one
 * class that lets the stylesheet draw the second one bigger and brighter.
 */
export function gesturePad(g: Gesture, options: { animated?: boolean } = {}): SVGSVGElement {
  const id = `pad${++uid}`;
  const spots = fingers(g);
  // The two one-finger gestures curve instead of crossing the pad in a line,
  // and the stylesheet flies the fingertip along that curve.
  const curves = g.motion === "move" || g.motion === "hold";
  // Everything that crosses the pad blurs as it goes, and a tap crosses
  // nothing. The cursor and the drag used to be exceptions: each drew a line
  // under itself that stayed at full strength from end to end, which is not what
  // a moving finger leaves behind - it is a piece of string being towed. They
  // now smear like every swipe does, which is the same movement drawn honestly
  // and the same picture the rest of the page is drawn in.
  const smears = g.motion !== "tap";

  const root = svg("svg", {
    viewBox: `0 0 ${W} ${H}`,
    class: `gesture-pad${options.animated ? " is-animated" : " gesture-thumb"}`,
    // The words beside it already say what this is; a screen reader that also
    // reads the picture reads the gesture twice.
    "aria-hidden": "true",
    focusable: "false",
    // What the stylesheet animates on. Kept as data rather than as classes so
    // a selector reads like the gesture it matches.
    "data-motion": g.motion,
    "data-fingers": String(g.fingers),
    ...(g.direction ? { "data-direction": g.direction } : {}),
    ...(g.axis ? { "data-axis": g.axis } : {}),
    // Every drawing runs the same three-second loop, so left alone a list of
    // them beats in unison - which is a strobe, not a page. Each thumbnail
    // starts somewhere else in the loop instead, and the wall of movement
    // settles into something closer to a room with people in it. The
    // demonstration keeps phase zero: it is replaced when the gesture changes,
    // and it should start that gesture from the top.
    style: `--phase: ${options.animated ? 0 : phase(g.id)}s`
      + (curves ? `; --path: path('${sweepPath(spots[0], g.motion as "move" | "hold")}')` : ""),
  });

  const defs = svg("defs", {});
  root.append(defs);

  const frame = { x: 1.5, y: 1.5, width: W - 3, height: H - 3, rx: 13 };
  root.append(svg("rect", { class: "pad-frame", ...frame }));

  /*
   * Everything that happens on the pad, clipped to the pad.
   *
   * Nothing a finger does can happen off the side of a trackpad, and until this
   * was here things did: a blur reaching back from a fingertip at the top of the
   * loop hung over the frame and was then cut off square by the edge of the
   * drawing, and the shockwave off a press left the pad entirely. The frame
   * itself stays outside the clip - it is drawn *on* the boundary, and clipping
   * a stroke to its own path shaves the outer half of it off.
   */
  const clip = svg("clipPath", { id: `${id}-pad` });
  clip.append(svg("rect", frame));
  defs.append(clip);
  const inside = svg("g", { class: "pad-inside", "clip-path": `url(#${id}-pad)` });
  root.append(inside);

  // The blur every finger of this gesture drags behind it. One pair of
  // gradients for the whole drawing rather than one per finger: each fingertip
  // is drawn about its own origin, so the smear behind every one of them is the
  // same shape in the same local coordinates.
  const tail = { x: 0, y: 0 };
  if (smears) {
    const { dx, dy } = heading(g);
    const reach = g.motion === "hold" ? SMEAR_HELD : SMEAR;
    tail.x = round(-dx * reach);
    tail.y = round(-dy * reach);
    for (const layer of BLUR) {
      // Green on a hold, because by the time that finger is moving the button
      // is down and everything about a live drag on the phone is green.
      //
      // Mixed towards white first. A blur is a faint thing by construction -
      // a fifth of an opacity for the spill, half for the core - and the armed
      // green is dark enough that at those strengths it disappeared into the
      // pad, leaving the one gesture whose point is that it *drags* with nothing
      // to show for the dragging. The same trick as the charging head, and for
      // the same reason: the colour has to survive being drawn faintly.
      defs.append(fade(`${id}-${layer.cls}`, tail, { x: 0, y: 0 }, layer.peak,
        g.motion === "hold" ? "color-mix(in srgb, var(--armed) 55%, white)" : "currentColor"));
    }
  }

  // What a tap leaves, drawn behind the fingers so nothing crosses a fingertip.
  // Everything else says what it did with the blur it dragged, which belongs to
  // the fingertip and travels with it.
  if (g.motion === "tap") {
    // One ring per finger: a tap is the fingers landing, not travelling. Drawn
    // clear of the fingertip rather than under it, so that a tap still reads as
    // a tap when the animation is turned off and the ring never expands - the
    // keyframes start it scaled back inside the dot and push it out past this.
    for (const [i, spot] of spots.entries()) {
      inside.append(svg("circle", {
        class: "pad-ripple", cx: spot.x, cy: spot.y, r: FINGER_R * 2, style: `--finger: ${i}`,
      }));
    }
  }

  for (const spot of spots) {
    // Each finger is a group so everything belonging to that fingertip moves as
    // one thing. It is all drawn about the origin and the group is put in place
    // by `--x` and `--y`, so a keyframe can move or scale a fingertip without
    // any of them needing a bounding box measured for it.
    //
    // Nothing here is staggered. A hand is one object: the fingers of a real
    // three-finger swipe land together, travel together and leave together, and
    // a drawing that sets them off a beat apart is drawing three hands. The row
    // is bowed instead - a spatial offset, which is the thing that was actually
    // true about a hand all along.
    const finger = svg("g", {
      class: "pad-finger",
      style: `--x: ${round(spot.x)}; --y: ${round(spot.y)}`,
    });
    // A second group inside the first, carrying size while the outer one
    // carries position.
    //
    // They cannot share an element. CSS applies the individual `scale` property
    // *outside* the `transform` property, so a scale on the same group as the
    // translate multiplies the position it had just been given: a fingertip at
    // x=59.5 scaled to .82 arrives at x=48.8, and a two-finger tap dipped by
    // sliding its fingers toward the corner of the pad. Splitting them puts the
    // scale on a group whose own origin is the fingertip and which has no
    // translate of its own left to distort.
    const touch = svg("g", { class: "pad-touch" });
    // Drawn from the fingertip *outwards*, so a dash walked along it grows the
    // smear backwards out of the finger rather than forwards out of the empty
    // pad behind it. `pathLength` 1 makes an offset of 1 "no blur at all" and 0
    // "the whole reach", the same arithmetic as every other drawn-in line here.
    for (const layer of smears ? BLUR : []) {
      touch.append(svg("path", {
        class: layer.cls,
        d: `M0 0L${tail.x} ${tail.y}`,
        stroke: `url(#${id}-${layer.cls})`,
        "stroke-width": layer.width,
        pathLength: 1,
      }));
    }
    if (g.motion === "hold") press(touch);
    // The soft contact patch, then the fingertip inside it. Two circles rather
    // than one because a flat disc is a bullet hole; a finger presses.
    touch.append(svg("circle", { class: "pad-halo", cx: 0, cy: 0, r: HALO_R }));
    touch.append(svg("circle", { class: "finger", cx: 0, cy: 0, r: FINGER_R }));
    finger.append(touch);
    inside.append(finger);
  }

  return root;
}

/**
 * Where in the shared loop this gesture's thumbnail starts, in seconds.
 *
 * Hashed from the id rather than taken from the row's position, so a gesture
 * keeps its phase when the list is regrouped and two neighbours cannot end up
 * in step because they happen to be adjacent.
 */
function phase(id: string): number {
  let h = 0;
  for (const ch of id) h = (h * 31 + ch.charCodeAt(0)) % 1000;
  return Math.round((h % 30)) / 10;
}
