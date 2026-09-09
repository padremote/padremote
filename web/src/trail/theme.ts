/**
 * The visual vocabulary the surface is drawn in.
 *
 * Colours, easings and the two geometry helpers that more than one renderer
 * needs. Kept apart from the code that draws, so that changing what the phone
 * *looks* like never means reading the code that decides what it *does*.
 */

/** Colour of the long-press ring: matches the phone's accent. */
export const PRESS_COLOR = "#2f6feb";
/** Colour the ring settles on once the drag is armed. */
export const ARMED_COLOR = "#3fb950";

/** Ease-out cubic: fast to start, gentle to land. */
export const easeOut = (t: number): number => 1 - Math.pow(1 - t, 3);
/** Ease-in-out, for the breathing pulse of a held drag. */
export const easeInOut = (t: number): number =>
  t < 0.5 ? 2 * t * t : 1 - Math.pow(-2 * t + 2, 2) / 2;

/**
 * Blend two hex colours.
 *
 * The charge ring used to *cut* from blue to green on the frame it armed, next
 * to a burst and a flash that were themselves fading in - three things changing
 * at once, which reads as a glitch rather than as a confirmation. Turning the
 * ring through the intermediate colours makes the same instant read as one
 * object changing state.
 */
export function mix(a: string, b: string, t: number): string {
  const k = Math.min(1, Math.max(0, t));
  const parse = (hex: string) => [
    parseInt(hex.slice(1, 3), 16),
    parseInt(hex.slice(3, 5), 16),
    parseInt(hex.slice(5, 7), 16),
  ];
  const [r1, g1, b1] = parse(a);
  const [r2, g2, b2] = parse(b);
  const c = (x: number, y: number) => Math.round(x + (y - x) * k);
  return `rgb(${c(r1, r2)},${c(g1, g2)},${c(b1, b2)})`;
}

/** Safe-area insets in CSS pixels, so an edge indicator clears the notch. */
export interface Insets {
  top: number;
  right: number;
  bottom: number;
  left: number;
}

/**
 * What a renderer needs to know about the surface it is drawing on.
 *
 * Passed in on every frame rather than held: the canvas can be resized and the
 * glare rebuilt at any moment, and a renderer that cached these would keep
 * drawing to the old geometry until the next touch.
 */
export interface Stage {
  width: number;
  height: number;
  safe: Insets;
  /** The cached edge glare, or null before the first resize. */
  glare: HTMLCanvasElement | null;
}
