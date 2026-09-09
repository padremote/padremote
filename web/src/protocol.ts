/**
 * Wire format, mirroring `protocol/v1.schema.json`.
 *
 * The phone sends raw touch points and nothing else; every decision about what
 * a tap or a scroll means is made on the desktop (plan.md section 3).
 */

export const VERSION = 1;
/** u32 t_ms + u8 pointerId + u8 phase + f32 x + f32 y */
export const SAMPLE_BYTES = 14;

export const Phase = { Down: 0, Move: 1, Up: 2, Cancel: 3 } as const;
export type Phase = (typeof Phase)[keyof typeof Phase];

export interface TouchSample {
  /** performance.now() at the sample, in ms. */
  t: number;
  /** Stable per-finger id, already reduced to a byte. */
  id: number;
  phase: Phase;
  /** Normalized 0-1 across the surface. */
  x: number;
  y: number;
}

export type ServerMessage =
  | {
      t: "state";
      gesture: string;
      fingers: number;
      name?: string;
      pressMs?: number;
      tapMaxPx?: number;
    }
  | { t: "echo"; tMs: number }
  /**
   * Who is driving the cursor. Several devices can be connected at once, and
   * only one of them moves anything at a time; a page that is being read but
   * not obeyed has to be able to say so, or the hand-over reads as lag.
   */
  | {
      t: "control";
      active: boolean;
      holder?: string;
      devices: number;
      /**
       * Why the computer will move nothing at all, for anybody. Absent in the
       * normal case.
       *
       * This page has no other way to know. Everything it can see says the
       * link is healthy - the socket is up, the gesture readout follows every
       * finger, the latency figure is live - and the cursor never moves, which
       * reads as a broken app rather than as an unticked box.
       */
      blocked?: "permission" | "dryRun";
    }
  /**
   * The settings this device is actually being driven with, and which of them
   * are the computer's own values rather than this phone's overrides.
   *
   * The desktop mirrors the user's real trackpad, so the phone must not assume
   * it knows any of these - it asks, and shows what it is told.
   */
  | {
      t: "settings";
      sensitivity: number;
      naturalScroll: boolean;
      following: { sensitivity: boolean; naturalScroll: boolean };
    }
  /**
   * Prove you know the pairing secret. The first thing the desktop sends, on
   * every connection, before it will read anything at all.
   */
  | { t: "challenge"; nonce: string }
  | { t: "error"; code: string };

/** Pack a batch of samples into one binary frame. */
export function encodeFrame(samples: readonly TouchSample[]): ArrayBuffer {
  const buf = new ArrayBuffer(2 + SAMPLE_BYTES * samples.length);
  const dv = new DataView(buf);
  dv.setUint8(0, VERSION);
  dv.setUint8(1, samples.length);
  let o = 2;
  for (const s of samples) {
    dv.setUint32(o, s.t >>> 0, true);
    dv.setUint8(o + 4, s.id);
    dv.setUint8(o + 5, s.phase);
    dv.setFloat32(o + 6, s.x, true);
    dv.setFloat32(o + 10, s.y, true);
    o += SAMPLE_BYTES;
  }
  return buf;
}
